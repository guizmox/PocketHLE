//! Battery bridge: one process-wide class reference, no Activity or device handles.
use jni::{objects::{GlobalRef, JClass}, JNIEnv, JavaVM};
use pocket_core::kernel::power::{self, Backend, Status};
use std::sync::Arc;
pub fn install(env: &mut JNIEnv<'_>, class: &JClass<'_>) -> jni::errors::Result<()> {
    power::install_host(Arc::new(Bridge { vm: env.get_java_vm()?, class: env.new_global_ref(class)? }));
    Ok(())
}
struct Bridge { vm: JavaVM, class: GlobalRef }
impl Backend for Bridge {
    fn read(&self) -> Option<Status> {
        let mut env = self.vm.attach_current_thread().ok()?;
        let value = env.call_static_method(&self.class, "batteryStatus", "()I", &[]).and_then(|v| v.i());
        match value {
            Ok(v) => Some(Status { ac: v as u8, flags: (v >> 8) as u8,
                percent: (v >> 16) as u8, remaining: u32::MAX, full: u32::MAX }),
            Err(_) => { let _ = env.exception_clear(); None }
        }
    }
}
