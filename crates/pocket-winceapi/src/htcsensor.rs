//! HTC G-sensor SDK ABI used by Windows Mobile sensor wrappers.
use crate::coredll::set_thread_error;
use crate::{CallCtx, WinCeDispatcher};
use pocket_cpu::Prot;
use pocket_kernel::{accelerometer, handles::HandleObject, DispatchOutcome, KernelError};

const DLL: &str = "htcsensorsdk.dll";
const SENSOR: HandleObject = HandleObject::Device(pocket_kernel::HTCSENSOR_MODULE_HANDLE);

pub(crate) fn register(d: &mut WinCeDispatcher) {
    d.register_handler(DLL, "HTCSensorOpen", open);
    d.register_handler(DLL, "HTCSensorGetDataOutput", read);
    d.register_handler(DLL, "HTCSensorClose", close);
}

fn open(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    if ctx.arg_u32(0)? != 1 || !accelerometer::available() {
        set_thread_error(ctx, 1167); // ERROR_DEVICE_NOT_CONNECTED
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    let handle = ctx.kernel.object_handles.duplicate(SENSOR).unwrap_or(0);
    if handle == 0 {
        set_thread_error(ctx, 8);
    }
    Ok(DispatchOutcome::ReturnedR0(handle))
}

fn read(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let handle = ctx.arg_u32(0)?;
    let output = ctx.arg_u32(1)?;
    if ctx.kernel.object_handles.get(handle) != Some(SENSOR) {
        return Ok(DispatchOutcome::ReturnedR0(6));
    }
    if output == 0 || ctx.cpu.check_guest_access(output, 20, Prot::WRITE).is_err() {
        return Ok(DispatchOutcome::ReturnedR0(87));
    }
    let Some(axes) = accelerometer::sample() else {
        return Ok(DispatchOutcome::ReturnedR0(21)); // ERROR_NOT_READY
    };
    // TiltX/Y/Z are signed 16-bit counts, approximately 1000 per g.
    // Leave the SDK's unknown and auxiliary angle fields at zero.
    let mut data = [0u8; 20];
    for (value, field) in axes.into_iter().zip(data.chunks_exact_mut(2)) {
        field.copy_from_slice(&((value / 0.0098).round() as i16).to_le_bytes());
    }
    ctx.cpu.write_mem(output, &data)?;
    Ok(DispatchOutcome::ReturnedR0(0))
}

fn close(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let handle = ctx.arg_u32(0)?;
    if ctx.kernel.object_handles.get(handle) == Some(SENSOR) {
        ctx.kernel.object_handles.close(handle);
    }
    Ok(DispatchOutcome::ReturnedR0(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth::tests::{call, setup};
    use pocket_cpu::Cpu;

    #[test]
    fn physical_samples_use_htc_units_and_closed_handles_are_rejected() {
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                accelerometer::publish(false, None);
            }
        }
        let _reset = Reset;
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        macro_rules! api {
            ($name:literal, $args:expr) => {
                call(&mut cpu, &mut kernel, &mut dispatcher, DLL, $name, $args)
            };
        }
        accelerometer::publish(false, None);
        assert_eq!(api!("HTCSensorOpen", &[1]), DispatchOutcome::ReturnedR0(0));
        accelerometer::publish(true, Some([4.9, -9.8, 0.0]));
        let DispatchOutcome::ReturnedR0(handle) = api!("HTCSensorOpen", &[1]) else {
            panic!("expected sensor handle");
        };
        assert_ne!(handle, 0);
        assert_eq!(
            api!("HTCSensorGetDataOutput", &[handle, 0x1100]),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(cpu.read_mem(0x1100, 6).unwrap(), [0xf4, 0x01, 0x18, 0xfc, 0, 0]);
        assert_eq!(
            api!("HTCSensorGetDataOutput", &[handle, 0x4ff8]),
            DispatchOutcome::ReturnedR0(87)
        );
        accelerometer::publish(false, None);
        assert_eq!(
            api!("HTCSensorGetDataOutput", &[handle, 0x1100]),
            DispatchOutcome::ReturnedR0(21)
        );
        api!("HTCSensorClose", &[handle]);
        assert_eq!(
            api!("HTCSensorGetDataOutput", &[handle, 0x1100]),
            DispatchOutcome::ReturnedR0(6)
        );
    }
}
