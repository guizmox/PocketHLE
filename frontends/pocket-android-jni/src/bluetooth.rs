//! Java BluetoothSocket transport behind the shared CE Bluetooth contracts.
use jni::{JNIEnv, JavaVM, objects::{GlobalRef, JByteArray, JClass, JString, JValue}};
use pocket_core::kernel::bluetooth::{self, Backend, BtResult, Device, PortParams, Stream, WOULD_BLOCK};
use std::sync::Arc;

pub fn install(env: &mut JNIEnv<'_>, class: &JClass<'_>) -> jni::errors::Result<()> {
    bluetooth::install_host(Arc::new(Bridge { vm: env.get_java_vm()?, class: env.new_global_ref(class)? }));
    Ok(())
}
struct Bridge { vm: JavaVM, class: GlobalRef }
impl Bridge {
    fn call<T>(&self, f: impl FnOnce(&mut JNIEnv<'_>, &GlobalRef) -> jni::errors::Result<T>) -> BtResult<T> {
        let mut env = self.vm.attach_current_thread().map_err(|_| 10091u32)?;
        let result = env.with_local_frame(32, |env| f(env, &self.class));
        match result {
            Ok(v) => Ok(v),
            Err(_) => {
                let exception = env.exception_occurred().ok(); let _ = env.exception_clear();
                let security = exception.is_some_and(|e| env.is_instance_of(e, "java/lang/SecurityException").unwrap_or(false));
                Err(if security { 10013 } else { 10091 })
            }
        }
    }
}
impl Backend for Bridge {
    fn hostname(&self) -> BtResult<String> { self.call(|env, class| {
        let s = JString::from(env.call_static_method(class, "btHostname", "()Ljava/lang/String;", &[])?.l()?);
        let text: String = env.get_string(&s)?.into(); Ok(text)
    }) }
    fn scan(&self) -> BtResult<Vec<Device>> {
        let json: String = self.call(|env, class| {
            let s = JString::from(env.call_static_method(class, "btScan", "()Ljava/lang/String;", &[])?.l()?);
            let text: String = env.get_string(&s)?.into(); Ok(text)
        })?;
        serde_json::from_str(&json).map_err(|_| 10022)
    }
    fn open(&self, params: &PortParams) -> BtResult<Box<dyn Stream>> {
        let b = params.service_uuid();
        let uuid = format!("{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            u32::from_le_bytes(b[..4].try_into().unwrap()), u16::from_le_bytes(b[4..6].try_into().unwrap()),
            u16::from_le_bytes(b[6..8].try_into().unwrap()), b[8],b[9],b[10],b[11],b[12],b[13],b[14],b[15]);
        let id = self.call(|env, class| {
            let uuid = env.new_string(uuid)?;
            env.call_static_method(class, "btOpen", "(ZJLjava/lang/String;)I",
                &[JValue::Bool(params.server as u8), JValue::Long(params.address as i64), JValue::Object(uuid.as_ref())])?.i()
        })?;
        if id <= 0 { return Err(if id < 0 { id.unsigned_abs() } else { 10091 }); }
        let vm = self.call(|env, _| env.get_java_vm())?;
        let class = self.call(|env, class| env.new_global_ref(class.as_obj()))?;
        Ok(Box::new(JavaStream { bridge: Bridge { vm, class }, id }))
    }
}
struct JavaStream { bridge: Bridge, id: i32 }
impl Stream for JavaStream {
    fn read(&mut self, bytes: &mut [u8]) -> BtResult<usize> {
        if bytes.is_empty() { return Ok(0); }
        let status = self.bridge.call(|env, class| env.call_static_method(class, "btStatus", "(I)I", &[JValue::Int(self.id)])?.i())?;
        if status == -1 { return Ok(0); } if status != 0 { return Err(status as u32); }
        let data = self.bridge.call(|env, class| {
            let array = JByteArray::from(env.call_static_method(class, "btRead", "(II)[B", &[JValue::Int(self.id), JValue::Int(bytes.len().min(65536) as i32)])?.l()?);
            env.convert_byte_array(array)
        })?;
        if data.is_empty() { return Err(WOULD_BLOCK); }
        bytes[..data.len()].copy_from_slice(&data); Ok(data.len())
    }
    fn write(&mut self, bytes: &[u8]) -> BtResult<usize> {
        let count = self.bridge.call(|env, class| {
            let array = env.byte_array_from_slice(&bytes[..bytes.len().min(4096)])?;
            env.call_static_method(class, "btWrite", "(I[B)I", &[JValue::Int(self.id), JValue::Object(array.as_ref())])?.i()
        })?;
        if count < 0 { Err(count.unsigned_abs()) } else { Ok(count as usize) }
    }
}
impl Drop for JavaStream { fn drop(&mut self) {
    let _ = self.bridge.call(|env, class| env.call_static_method(class, "btClose", "(I)V", &[JValue::Int(self.id)]).map(|_| ()));
} }
