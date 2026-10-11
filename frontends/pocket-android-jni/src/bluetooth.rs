//! Java BluetoothSocket transport behind the shared CE Bluetooth contracts.
use jni::{
    objects::{GlobalRef, JByteArray, JClass, JString, JValue},
    JNIEnv, JavaVM,
};
use pocket_core::kernel::bluetooth::{
    self, Backend, BtResult, Device, PortParams, Stream, WOULD_BLOCK,
};
use std::sync::Arc;

pub fn install(env: &mut JNIEnv<'_>, class: &JClass<'_>) -> jni::errors::Result<()> {
    bluetooth::install_host(Arc::new(Bridge {
        vm: env.get_java_vm()?,
        class: env.new_global_ref(class)?,
    }));
    Ok(())
}
struct Bridge {
    vm: JavaVM,
    class: GlobalRef,
}
impl Bridge {
    fn call<T>(
        &self,
        f: impl FnOnce(&mut JNIEnv<'_>, &GlobalRef) -> jni::errors::Result<T>,
    ) -> BtResult<T> {
        let mut env = self.vm.attach_current_thread().map_err(|_| 10091u32)?;
        let result = env.with_local_frame(32, |env| f(env, &self.class));
        match result {
            Ok(v) => Ok(v),
            Err(_) => {
                let exception = env.exception_occurred().ok();
                let _ = env.exception_clear();
                let security = exception.is_some_and(|e| {
                    env.is_instance_of(e, "java/lang/SecurityException")
                        .unwrap_or(false)
                });
                Err(if security { 10013 } else { 10091 })
            }
        }
    }
}
impl Backend for Bridge {
    fn ready(&self) -> BtResult<()> {
        let code =
            self.call(|env, class| env.call_static_method(class, "btReady", "()I", &[])?.i())?;
        if code == 0 {
            Ok(())
        } else {
            Err(code.unsigned_abs())
        }
    }
    fn socket(&self) -> BtResult<Box<dyn bluetooth::RfcommSocket>> {
        let bridge = self.duplicate()?;
        let v = self.socket_call(serde_json::json!({"op": "new"}))?;
        let id = v["id"].as_i64().ok_or(10091u32)? as i32;
        Ok(Box::new(JavaSocket {
            bridge,
            id,
            options: Default::default(),
        }))
    }
    fn hostname(&self) -> BtResult<String> {
        self.call(|env, class| {
            let s = JString::from(
                env.call_static_method(class, "btHostname", "()Ljava/lang/String;", &[])?
                    .l()?,
            );
            let text: String = env.get_string(&s)?.into();
            Ok(text)
        })
    }
    fn scan(&self) -> BtResult<Vec<Device>> {
        let json: String = self.call(|env, class| {
            let s = JString::from(
                env.call_static_method(class, "btScan", "()Ljava/lang/String;", &[])?
                    .l()?,
            );
            let text: String = env.get_string(&s)?.into();
            Ok(text)
        })?;
        serde_json::from_str(&json).map_err(|_| 10022)
    }
    fn open(&self, params: &PortParams) -> BtResult<Box<dyn Stream>> {
        let b = params.service_uuid();
        let uuid = format!(
            "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            u32::from_le_bytes(b[..4].try_into().unwrap()),
            u16::from_le_bytes(b[4..6].try_into().unwrap()),
            u16::from_le_bytes(b[6..8].try_into().unwrap()),
            b[8],
            b[9],
            b[10],
            b[11],
            b[12],
            b[13],
            b[14],
            b[15]
        );
        let id = self.call(|env, class| {
            let uuid = env.new_string(uuid)?;
            env.call_static_method(
                class,
                "btOpen",
                "(ZJLjava/lang/String;)I",
                &[
                    JValue::Bool(params.server as u8),
                    JValue::Long(params.address as i64),
                    JValue::Object(uuid.as_ref()),
                ],
            )?
            .i()
        })?;
        if id <= 0 {
            return Err(if id < 0 { id.unsigned_abs() } else { 10091 });
        }
        let vm = self.call(|env, _| env.get_java_vm())?;
        let class = self.call(|env, class| env.new_global_ref(class.as_obj()))?;
        Ok(Box::new(JavaStream {
            bridge: Bridge { vm, class },
            id,
        }))
    }
}
struct JavaStream {
    bridge: Bridge,
    id: i32,
}
impl Stream for JavaStream {
    fn read(&mut self, bytes: &mut [u8]) -> BtResult<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        let status = self.bridge.call(|env, class| {
            env.call_static_method(class, "btStatus", "(I)I", &[JValue::Int(self.id)])?
                .i()
        })?;
        if status == -1 {
            return Ok(0);
        }
        if status != 0 {
            return Err(status as u32);
        }
        let data = self.bridge.call(|env, class| {
            let array = JByteArray::from(
                env.call_static_method(
                    class,
                    "btRead",
                    "(II)[B",
                    &[
                        JValue::Int(self.id),
                        JValue::Int(bytes.len().min(65536) as i32),
                    ],
                )?
                .l()?,
            );
            env.convert_byte_array(array)
        })?;
        if data.is_empty() {
            return Err(WOULD_BLOCK);
        }
        bytes[..data.len()].copy_from_slice(&data);
        Ok(data.len())
    }
    fn write(&mut self, bytes: &[u8]) -> BtResult<usize> {
        let count = self.bridge.call(|env, class| {
            let array = env.byte_array_from_slice(&bytes[..bytes.len().min(4096)])?;
            env.call_static_method(
                class,
                "btWrite",
                "(I[B)I",
                &[JValue::Int(self.id), JValue::Object(array.as_ref())],
            )?
            .i()
        })?;
        if count < 0 {
            Err(count.unsigned_abs())
        } else {
            Ok(count as usize)
        }
    }
}
impl Drop for JavaStream {
    fn drop(&mut self) {
        let _ = self.bridge.call(|env, class| {
            env.call_static_method(class, "btClose", "(I)V", &[JValue::Int(self.id)])
                .map(|_| ())
        });
    }
}

/// Android RFCOMM endpoint. Control calls are bounded JSON, payloads JNI arrays.
struct JavaSocket {
    bridge: Bridge,
    id: i32,
    options: std::collections::HashMap<i32, Vec<u8>>,
}
fn uuid_text(b: &[u8; 16]) -> String {
    format!(
        "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        u32::from_le_bytes(b[..4].try_into().unwrap()),
        u16::from_le_bytes(b[4..6].try_into().unwrap()),
        u16::from_le_bytes(b[6..8].try_into().unwrap()),
        b[8],
        b[9],
        b[10],
        b[11],
        b[12],
        b[13],
        b[14],
        b[15]
    )
}
fn socket_address(a: &bluetooth::SocketAddress) -> serde_json::Value {
    serde_json::json!({"address": a.address, "uuid": uuid_text(&a.uuid), "port": a.port})
}
fn decode_address(v: &serde_json::Value) -> BtResult<bluetooth::SocketAddress> {
    let text = v["uuid"].as_str().unwrap_or("");
    let mut uuid = [0u8; 16];
    if !text.is_empty() {
        let hex = text.replace('-', "");
        if hex.len() != 32 || !hex.is_ascii() {
            return Err(10022);
        }
        for (i, b) in uuid.iter_mut().enumerate() {
            *b = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).map_err(|_| 10022u32)?;
        }
        uuid[..4].reverse();
        uuid[4..6].reverse();
        uuid[6..8].reverse();
    }
    Ok(bluetooth::SocketAddress {
        address: v["address"].as_u64().unwrap_or(0),
        uuid,
        port: v["port"]
            .as_u64()
            .unwrap_or(0)
            .try_into()
            .map_err(|_| 10022u32)?,
    })
}
impl Bridge {
    fn duplicate(&self) -> BtResult<Self> {
        Ok(Self {
            vm: self.call(|env, _| env.get_java_vm())?,
            class: self.call(|env, class| env.new_global_ref(class.as_obj()))?,
        })
    }
    fn socket_call(&self, value: serde_json::Value) -> BtResult<serde_json::Value> {
        let text: String = self.call(|env, class| {
            let request = env.new_string(value.to_string())?;
            let reply = JString::from(
                env.call_static_method(
                    class,
                    "btSocketCall",
                    "(Ljava/lang/String;)Ljava/lang/String;",
                    &[JValue::Object(request.as_ref())],
                )?
                .l()?,
            );
            let text: String = env.get_string(&reply)?.into();
            Ok(text)
        })?;
        let v: serde_json::Value = serde_json::from_str(&text).map_err(|_| 10091u32)?;
        if let Some(e) = v["error_code"].as_u64() {
            return Err(e as u32);
        }
        Ok(v)
    }
}
impl JavaSocket {
    fn control(&self, op: &str, mut args: serde_json::Value) -> BtResult<serde_json::Value> {
        args["op"] = op.into();
        args["id"] = self.id.into();
        self.bridge.socket_call(args)
    }
}
impl bluetooth::RfcommSocket for JavaSocket {
    fn bind(&mut self, a: &bluetooth::SocketAddress) -> BtResult<()> {
        self.control("bind", serde_json::json!({"addr": socket_address(a)}))
            .map(|_| ())
    }
    fn listen(&mut self, backlog: i32) -> BtResult<()> {
        self.control("listen", serde_json::json!({"backlog": backlog}))
            .map(|_| ())
    }
    fn connect(&mut self, a: &bluetooth::SocketAddress) -> BtResult<()> {
        self.control("connect", serde_json::json!({"addr": socket_address(a)}))
            .map(|_| ())
    }
    fn accept(&mut self) -> BtResult<(Box<dyn bluetooth::RfcommSocket>, bluetooth::SocketAddress)> {
        // Acquire JNI ownership before accepting, so failures cannot leak a child.
        let bridge = self.bridge.duplicate()?;
        let v = self.control("accept", serde_json::json!({}))?;
        let id = v["id"].as_i64().ok_or(10091u32)? as i32;
        let socket = JavaSocket {
            bridge,
            id,
            options: self.options.clone(),
        };
        let address = decode_address(&v["addr"])?;
        Ok((Box::new(socket), address))
    }
    fn read(&mut self, bytes: &mut [u8], flags: i32) -> BtResult<usize> {
        if flags & !2 != 0 {
            return Err(10045);
        }
        if bytes.is_empty() {
            return Ok(0);
        }
        let status = self.bridge.call(|env, class| {
            env.call_static_method(class, "btSocketStatus", "(I)I", &[JValue::Int(self.id)])?
                .i()
        })?;
        if status == -1 {
            return Ok(0);
        }
        if status != 0 {
            return Err(status as u32);
        }
        let data = self.bridge.call(|env, class| {
            let array = JByteArray::from(
                env.call_static_method(
                    class,
                    "btSocketRead",
                    "(IIZ)[B",
                    &[
                        JValue::Int(self.id),
                        JValue::Int(bytes.len().min(65536) as i32),
                        JValue::Bool((flags == 2) as u8),
                    ],
                )?
                .l()?,
            );
            env.convert_byte_array(array)
        })?;
        if data.is_empty() {
            return Err(WOULD_BLOCK);
        }
        if data.len() > bytes.len() {
            return Err(10014);
        }
        bytes[..data.len()].copy_from_slice(&data);
        Ok(data.len())
    }
    fn write(&mut self, bytes: &[u8], flags: i32) -> BtResult<usize> {
        if flags != 0 {
            return Err(10045);
        }
        let count = self.bridge.call(|env, class| {
            let array = env.byte_array_from_slice(&bytes[..bytes.len().min(4096)])?;
            env.call_static_method(
                class,
                "btSocketWrite",
                "(I[B)I",
                &[JValue::Int(self.id), JValue::Object(array.as_ref())],
            )?
            .i()
        })?;
        if count < 0 {
            Err(count.unsigned_abs())
        } else {
            Ok(count as usize)
        }
    }
    fn readiness(&mut self) -> BtResult<(bool, bool, bool)> {
        let v = self.control("ready", serde_json::json!({}))?;
        Ok((
            v["read"].as_bool().unwrap_or(false),
            v["write"].as_bool().unwrap_or(false),
            v["error"].as_bool().unwrap_or(false),
        ))
    }
    fn available(&mut self) -> BtResult<u32> {
        let v = self.control("available", serde_json::json!({}))?;
        Ok(v["count"].as_u64().unwrap_or(0) as u32)
    }
    fn set_option(&mut self, level: i32, name: i32, value: &[u8]) -> BtResult<()> {
        // Android has no TCP_NODELAY for Bluetooth; report unsupported honestly.
        // Winsock buffer sizes are advisory; transport queues remain bounded.
        if level != 0xffff || !matches!(name, 0x1001 | 0x1002) {
            return Err(10042);
        }
        if value.len() != 4 {
            return Err(10014);
        }
        let size = i32::from_le_bytes(value.try_into().unwrap());
        if size <= 0 {
            return Err(10022);
        }
        self.options
            .insert(name, size.min(65536).to_le_bytes().to_vec());
        Ok(())
    }
    fn get_option(&mut self, level: i32, name: i32, size: usize) -> BtResult<Vec<u8>> {
        if level != 0xffff {
            return Err(10042);
        }
        if size < 4 {
            return Err(10014);
        }
        let value = match name {
            0x1007 => self.control("error", serde_json::json!({}))?["code"]
                .as_u64()
                .unwrap_or(0) as u32,
            0x1008 => 1, // SOCK_STREAM
            0x1001 | 0x1002 => {
                return Ok(self
                    .options
                    .get(&name)
                    .cloned()
                    .unwrap_or_else(|| 65536u32.to_le_bytes().to_vec()))
            }
            _ => return Err(10042),
        };
        Ok(value.to_le_bytes().to_vec())
    }
    fn address(&mut self, peer: bool) -> BtResult<bluetooth::SocketAddress> {
        decode_address(&self.control("address", serde_json::json!({"peer": peer}))?["addr"])
    }
    fn shutdown(&mut self, how: i32) -> BtResult<()> {
        self.control("shutdown", serde_json::json!({"how": how}))
            .map(|_| ())
    }
}
impl Drop for JavaSocket {
    fn drop(&mut self) {
        let _ = self.control("close", serde_json::json!({}));
    }
}

#[cfg(test)]
mod socket_tests {
    use super::*;
    #[test]
    fn guid_memory_order_and_arm_address_roundtrip() {
        let a = bluetooth::SocketAddress {
            address: 0x123456789abc,
            uuid: [
                0x50, 0x48, 0x4c, 0x45, 0x42, 0x54, 0x41, 0x42, 0x91, 0x21, 0x47, 0x49, 0x5a, 0, 0,
                2,
            ],
            port: 2,
        };
        assert_eq!(uuid_text(&a.uuid), "454c4850-5442-4241-9121-47495a000002");
        assert_eq!(decode_address(&socket_address(&a)).unwrap(), a);
        assert_eq!(
            decode_address(&serde_json::json!({"uuid":"bad"})),
            Err(10022)
        );
        assert_eq!(
            decode_address(&serde_json::json!({"port":4294967296u64})),
            Err(10022)
        );
    }
}
