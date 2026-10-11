//! Battery bridge: one process-wide class reference, no Activity or device handles.
use jni::{
    objects::{GlobalRef, JClass},
    JNIEnv, JavaVM,
};
use pocket_core::kernel::power::{self, Backend, Status};
use std::sync::Arc;
pub fn install(env: &mut JNIEnv<'_>, class: &JClass<'_>) -> jni::errors::Result<()> {
    let bridge = Arc::new(Bridge {
        vm: env.get_java_vm()?,
        class: env.new_global_ref(class)?,
    });
    power::install_host(bridge.clone());
    pocket_core::kernel::device_status::install_host(bridge);
    Ok(())
}
struct Bridge {
    vm: JavaVM,
    class: GlobalRef,
}
impl Backend for Bridge {
    fn read(&self) -> Option<Status> {
        let mut env = self.vm.attach_current_thread().ok()?;
        let value = env
            .call_static_method(&self.class, "batteryStatus", "()I", &[])
            .and_then(|v| v.i());
        match value {
            Ok(v) => Some(Status {
                ac: v as u8,
                flags: (v >> 8) as u8,
                percent: (v >> 16) as u8,
                remaining: u32::MAX,
                full: u32::MAX,
            }),
            Err(_) => {
                let _ = env.exception_clear();
                None
            }
        }
    }
}

impl pocket_core::kernel::device_status::Backend for Bridge {
    fn read(&self) -> Option<pocket_core::kernel::device_status::Status> {
        let mut env = self.vm.attach_current_thread().ok()?;
        match env
            .call_static_method(&self.class, "deviceStatus", "()I", &[])
            .and_then(|v| v.i())
        {
            Ok(v) => Some(pocket_core::kernel::device_status::Status::from_packed(
                v as u32,
            )),
            Err(_) => {
                let _ = env.exception_clear();
                None
            }
        }
    }
}
