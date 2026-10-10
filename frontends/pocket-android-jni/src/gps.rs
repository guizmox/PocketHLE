//! Foreground Android location mailbox bridge, fixed 64-byte LE packet.
use jni::{
    objects::{GlobalRef, JByteArray, JClass, JValue},
    JNIEnv, JavaVM,
};
use pocket_core::kernel::gps::{self, Backend, Capture, Position, Result};
use std::sync::Arc;
pub fn install(env: &mut JNIEnv<'_>, class: &JClass<'_>) -> jni::errors::Result<()> {
    gps::install_host(Arc::new(Bridge {
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
    ) -> Result<T> {
        let mut env = self.vm.attach_current_thread().map_err(|_| 21u32)?;
        let result = env.with_local_frame(16, |env| f(env, &self.class));
        result.map_err(|_| {
            let exception = env.exception_occurred().ok();
            let _ = env.exception_clear();
            if exception.is_some_and(|e| {
                env.is_instance_of(e, "java/lang/SecurityException")
                    .unwrap_or(false)
            }) {
                5
            } else {
                21
            }
        })
    }
}
impl Backend for Bridge {
    fn start(&self) -> Result<Box<dyn Capture>> {
        let id =
            self.call(|env, class| env.call_static_method(class, "gpsOpen", "()I", &[])?.i())?;
        if id <= 0 {
            return Err(21);
        }
        // JavaVM is a shared VM handle; GlobalRef clones retain the Java class.
        let vm = match self.call(|env, _| env.get_java_vm()) {
            Ok(vm) => vm,
            Err(e) => {
                let _ = self.call(|env, class| {
                    env.call_static_method(class, "gpsClose", "(I)V", &[JValue::Int(id)])
                        .map(|_| ())
                });
                return Err(e);
            }
        };
        Ok(Box::new(JavaCapture {
            bridge: Bridge {
                vm,
                class: self.class.clone(),
            },
            id,
        }))
    }
}
struct JavaCapture {
    bridge: Bridge,
    id: i32,
}
impl Capture for JavaCapture {
    fn latest(&mut self) -> Result<Option<Position>> {
        let data = self.bridge.call(|env, class| {
            let a = JByteArray::from(
                env.call_static_method(class, "gpsRead", "(I)[B", &[JValue::Int(self.id)])?
                    .l()?,
            );
            env.convert_byte_array(a)
        })?;
        if data.is_empty() {
            return Ok(None);
        }
        if data.len() == 4 {
            return Err(u32::from_le_bytes(data[..4].try_into().unwrap()));
        }
        if data.len() != 64 {
            return Err(13);
        }
        let value = |i: usize| f64::from_le_bytes(data[i..i + 8].try_into().unwrap());
        let optional = |i| {
            let v = value(i);
            if v.is_nan() {
                None
            } else {
                Some(v)
            }
        };
        Ok(Some(Position {
            unix_ms: u64::from_le_bytes(data[0..8].try_into().unwrap()),
            latitude: value(8),
            longitude: value(16),
            altitude_msl: optional(24),
            speed: optional(32),
            course: optional(40),
            horizontal_error: value(48),
            vertical_error: optional(56),
        }))
    }
}
impl Drop for JavaCapture {
    fn drop(&mut self) {
        let _ = self.bridge.call(|env, class| {
            env.call_static_method(class, "gpsClose", "(I)V", &[JValue::Int(self.id)])
                .map(|_| ())
        });
    }
}
