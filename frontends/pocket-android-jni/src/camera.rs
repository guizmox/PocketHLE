//! Camera2 mailbox bridge. Wire packet: LE width/height/serial, followed by I420.
use jni::{
    objects::{GlobalRef, JByteArray, JClass, JValue},
    JNIEnv, JavaVM,
};
use pocket_core::kernel::camera::{self, Backend, Capture, Frame, Result};
use std::sync::Arc;
pub fn install(env: &mut JNIEnv<'_>, class: &JClass<'_>) -> jni::errors::Result<()> {
    camera::install_host(Arc::new(Bridge {
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
            self.call(|env, class| env.call_static_method(class, "camOpen", "()I", &[])?.i())?;
        if id <= 0 {
            return Err(21);
        }
        // JavaVM is a shared VM handle; GlobalRef clones retain the Java class.
        let vm = match self.call(|env, _| env.get_java_vm()) {
            Ok(vm) => vm,
            Err(e) => {
                let _ = self.call(|env, class| {
                    env.call_static_method(class, "camClose", "(I)V", &[JValue::Int(id)])
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
            cached: None,
        }))
    }
}
struct JavaCapture {
    bridge: Bridge,
    id: i32,
    cached: Option<Arc<Frame>>,
}
impl Capture for JavaCapture {
    fn latest(&mut self) -> Result<Option<Arc<Frame>>> {
        let data = self.bridge.call(|env, class| {
            let a = JByteArray::from(
                env.call_static_method(class, "camRead", "(I)[B", &[JValue::Int(self.id)])?
                    .l()?,
            );
            env.convert_byte_array(a)
        })?;
        if data.is_empty() {
            self.cached = None;
            return Ok(None);
        }
        if data.len() == 4 {
            return Err(u32::from_le_bytes(data[..4].try_into().unwrap()));
        }
        if data.len() < 16 {
            return Err(13);
        }
        let width = u32::from_le_bytes(data[0..4].try_into().unwrap());
        let height = u32::from_le_bytes(data[4..8].try_into().unwrap());
        let serial = u64::from_le_bytes(data[8..16].try_into().unwrap());
        if let Some(frame) = self.cached.as_ref().filter(|f| f.serial == serial) {
            return Ok(Some(frame.clone()));
        }
        let frame = Arc::new(camera::from_i420(width, height, serial, &data[16..])?);
        self.cached = Some(frame.clone());
        Ok(Some(frame))
    }
}
impl Drop for JavaCapture {
    fn drop(&mut self) {
        let _ = self.bridge.call(|env, class| {
            env.call_static_method(class, "camClose", "(I)V", &[JValue::Int(self.id)])
                .map(|_| ())
        });
    }
}
