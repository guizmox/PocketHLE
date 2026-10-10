//! Android HTTP transport; Java owns cancellable workers and bounded streams.
use jni::{
    objects::{GlobalRef, JByteArray, JClass, JString, JValue},
    JNIEnv, JavaVM,
};
use pocket_core::kernel::internet::{
    self, Backend, Client, RequestSpec, ResponseHead, Result, SessionSpec, Transfer,
};
use std::sync::Arc;
struct Bridge {
    vm: JavaVM,
    class: GlobalRef,
}
impl Bridge {
    fn call<T>(
        &self,
        f: impl FnOnce(&mut JNIEnv<'_>, &GlobalRef) -> jni::errors::Result<T>,
    ) -> Result<T> {
        let mut env = self.vm.attach_current_thread().map_err(|_| 12030u32)?;
        let result = env.with_local_frame(16, |e| f(e, &self.class));
        result.map_err(|_| {
            let _ = env.exception_clear();
            12030
        })
    }
    fn duplicate(&self) -> Result<Arc<Self>> {
        self.call(|e, _| e.get_java_vm()).map(|vm| {
            Arc::new(Self {
                vm,
                class: self.class.clone(),
            })
        })
    }
    fn close(&self, name: &str, id: i32) {
        let _ = self.call(|e, c| {
            e.call_static_method(c, name, "(I)V", &[JValue::Int(id)])
                .map(|_| ())
        });
    }
}
pub fn install(env: &mut JNIEnv<'_>, class: &JClass<'_>) -> jni::errors::Result<()> {
    internet::install_host(Arc::new(Bridge {
        vm: env.get_java_vm()?,
        class: env.new_global_ref(class)?,
    }));
    Ok(())
}
struct Session {
    bridge: Arc<Bridge>,
    id: i32,
}
struct JavaClient {
    session: Arc<Session>,
}
impl Backend for Bridge {
    fn open(&self, spec: &SessionSpec) -> Result<Arc<dyn Client>> {
        let bridge = self.duplicate()?;
        let json = serde_json::to_string(spec).map_err(|_| 87u32)?;
        let id = self.call(|e, c| {
            let s = e.new_string(json)?;
            e.call_static_method(
                c,
                "httpSessionOpen",
                "(Ljava/lang/String;)I",
                &[JValue::Object(s.as_ref())],
            )?
            .i()
        })?;
        if id <= 0 {
            return Err(if id < 0 { (-id) as u32 } else { 12030 });
        }
        Ok(Arc::new(JavaClient {
            session: Arc::new(Session { bridge, id }),
        }))
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        self.bridge.close("httpSessionClose", self.id);
    }
}
struct JavaTransfer {
    session: Arc<Session>,
    id: i32,
}
impl Client for JavaClient {
    fn start(&self, spec: RequestSpec, body: Vec<u8>) -> Result<Box<dyn Transfer>> {
        let json = serde_json::to_string(&spec).map_err(|_| 87u32)?;
        let id = self.session.bridge.call(|e, c| {
            let s = e.new_string(json)?;
            let b = e.byte_array_from_slice(&body)?;
            e.call_static_method(
                c,
                "httpStart",
                "(ILjava/lang/String;[B)I",
                &[
                    JValue::Int(self.session.id),
                    JValue::Object(s.as_ref()),
                    JValue::Object(b.as_ref()),
                ],
            )?
            .i()
        })?;
        if id <= 0 {
            return Err(if id < 0 { (-id) as u32 } else { 12030 });
        }
        // The session stays alive until its last outstanding transfer is closed.
        Ok(Box::new(JavaTransfer {
            session: self.session.clone(),
            id,
        }))
    }
}
impl Transfer for JavaTransfer {
    fn head(&mut self) -> Result<Option<ResponseHead>> {
        let json = self.session.bridge.call(|e, c| {
            let s = JString::from(
                e.call_static_method(
                    c,
                    "httpHead",
                    "(I)Ljava/lang/String;",
                    &[JValue::Int(self.id)],
                )?
                .l()?,
            );
            let value: String = e.get_string(&s)?.into();
            Ok(value)
        })?;
        if json.is_empty() {
            return Ok(None);
        }
        let value: serde_json::Value = serde_json::from_str(&json).map_err(|_| 12030u32)?;
        if let Some(error) = value.get("error").and_then(|v| v.as_u64()) {
            return Err(error as u32);
        }
        serde_json::from_value(value).map(Some).map_err(|_| 12030)
    }
    fn available(&mut self) -> Result<(usize, bool)> {
        let v = self.session.bridge.call(|e, c| {
            e.call_static_method(c, "httpAvailable", "(I)J", &[JValue::Int(self.id)])?
                .j()
        })?;
        if v < 0 {
            Err((-v) as u32)
        } else {
            Ok(((v as u64 & 0xffffffff) as usize, v as u64 & (1 << 32) != 0))
        }
    }
    fn read(&mut self, out: &mut [u8]) -> Result<Option<usize>> {
        let data = self.session.bridge.call(|e, c| {
            let a = JByteArray::from(
                e.call_static_method(
                    c,
                    "httpRead",
                    "(II)[B",
                    &[
                        JValue::Int(self.id),
                        JValue::Int(out.len().min(65536) as i32),
                    ],
                )?
                .l()?,
            );
            e.convert_byte_array(a)
        })?;
        if data.len() < 4 {
            return Err(12030);
        }
        let status = i32::from_le_bytes(data[..4].try_into().unwrap());
        if status < 0 {
            return Err((-status) as u32);
        }
        let n = data.len() - 4;
        if n > out.len() {
            return Err(12030);
        }
        out[..n].copy_from_slice(&data[4..]);
        if n != 0 || status == 1 || out.is_empty() {
            Ok(Some(n))
        } else {
            Ok(None)
        }
    }
}
impl Drop for JavaTransfer {
    fn drop(&mut self) {
        self.session.bridge.close("httpClose", self.id);
    }
}
