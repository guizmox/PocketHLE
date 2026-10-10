//! Native Android vibrator bridge. The last shared device owner releases it.
use jni::{
    objects::{GlobalRef, JClass, JValue},
    JNIEnv, JavaVM,
};
use pocket_core::kernel::vibration::{self, Backend, Motor, Result};
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
        let mut env = self.vm.attach_current_thread().map_err(|_| 21u32)?;
        let value = env.with_local_frame(16, |e| f(e, &self.class));
        value.map_err(|_| {
            let _ = env.exception_clear();
            21
        })
    }
    fn close(&self, id: i32) {
        let _ = self.call(|e, c| {
            e.call_static_method(c, "vibClose", "(I)V", &[JValue::Int(id)])
                .map(|_| ())
        });
    }
}
pub fn install(env: &mut JNIEnv<'_>, class: &JClass<'_>) -> jni::errors::Result<()> {
    vibration::install_host(Arc::new(Bridge {
        vm: env.get_java_vm()?,
        class: env.new_global_ref(class)?,
    }));
    Ok(())
}
impl Backend for Bridge {
    fn open(&self) -> Result<Box<dyn Motor>> {
        let vm = self.call(|e, _| e.get_java_vm())?;
        let id = self.call(|e, c| e.call_static_method(c, "vibOpen", "()I", &[])?.i())?;
        if id <= 0 {
            return Err(if id < 0 { (-id) as u32 } else { 21 });
        }
        Ok(Box::new(JavaMotor {
            bridge: Bridge {
                vm,
                class: self.class.clone(),
            },
            id,
        }))
    }
}
struct JavaMotor {
    bridge: Bridge,
    id: i32,
}
impl Motor for JavaMotor {
    fn apply(&mut self, timings: &[i64]) -> Result<()> {
        let code = self.bridge.call(|e, c| {
            let a = e.new_long_array(timings.len() as i32)?;
            e.set_long_array_region(&a, 0, timings)?;
            e.call_static_method(
                c,
                "vibApply",
                "(I[J)I",
                &[JValue::Int(self.id), JValue::Object(a.as_ref())],
            )?
            .i()
        })?;
        if code == 0 {
            Ok(())
        } else {
            Err(code as u32)
        }
    }
}
impl Drop for JavaMotor {
    fn drop(&mut self) {
        self.bridge.close(self.id);
    }
}
