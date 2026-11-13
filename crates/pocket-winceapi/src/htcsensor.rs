//! HTC G-sensor SDK ABI used by Windows Mobile sensor wrappers.
use crate::coredll::set_thread_error;
use crate::{CallCtx, WinCeDispatcher};
use pocket_cpu::Prot;
use pocket_kernel::{
    accelerometer, handles::HandleObject, vfs::AccelerometerStream, DispatchOutcome, KernelError,
};

const DLL: &str = "htcsensorsdk.dll";
const SENSOR: HandleObject = HandleObject::Device(pocket_kernel::HTCSENSOR_MODULE_HANDLE);

pub(crate) fn register(d: &mut WinCeDispatcher) {
    d.register_handler(DLL, "HTCSensorOpen", open);
    d.register_handler(DLL, "HTCSensorGetDataOutput", read);
    d.register_handler(DLL, "HTCSensorClose", close);
}

fn available(kernel: &pocket_kernel::KernelState) -> bool {
    kernel.virtual_tilt_axes.is_some() || accelerometer::available()
}

fn sample_counts(kernel: &mut pocket_kernel::KernelState) -> Option<[i16; 3]> {
    // The HTC sensor reports gravity in device axes (X right, Y down,
    // Z toward the screen). Host sensors report proper acceleration in
    // display axes (X right, Y up, Z toward the screen).
    let [x, y, z] = if let Some(axes) = kernel.virtual_tilt_axes {
        let [x, y, z] = accelerometer::simulated_sample(kernel, axes);
        [x, -y, -z]
    } else {
        kernel.async_keys.sync(
            &kernel.pending_input,
            kernel.gapi_action_key != pocket_kernel::gapi::VK_RETURN,
        );
        let [x, y, z] = accelerometer::sample()?;
        [-x, y, -z]
    };
    // Undo the presentation rotation before exposing the raw device axes.
    let axes = accelerometer::undo_display_rotation(
        [x, y, z],
        usize::from(kernel.async_keys.display_turns),
    );
    Some(axes.map(|value| (value / 0.0098).round() as i16))
}

pub(crate) fn open_stream(
    ctx: &mut CallCtx<'_>,
    disposition: u32,
    stream: AccelerometerStream,
) -> DispatchOutcome {
    let error = if disposition != 3 {
        87 // ERROR_INVALID_PARAMETER
    } else if !available(ctx.kernel) {
        1167 // ERROR_DEVICE_NOT_CONNECTED
    } else if let Some(handle) = ctx.kernel.vfs.open_htc_sensor(stream) {
        return DispatchOutcome::ReturnedR0(handle);
    } else {
        8 // ERROR_NOT_ENOUGH_MEMORY
    };
    set_thread_error(ctx, error);
    DispatchOutcome::ReturnedR0(u32::MAX)
}

pub(crate) fn stream_control(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let handle = ctx.arg_u32(0)?;
    let stream = ctx.kernel.vfs.htc_sensor_stream(handle);
    let code = ctx.arg_u32(1)?;
    const AXES_BYTES: u32 = 6;
    let fail = |ctx: &mut CallCtx<'_>, error| {
        set_thread_error(ctx, error);
        Ok(DispatchOutcome::ReturnedR0(0))
    };
    if !matches!(
        (stream, code),
        (Some(AccelerometerStream::Sen0), 0x0100_0001)
            | (Some(AccelerometerStream::Acc1), 0x8012_2ef0)
    ) {
        return fail(ctx, 50); // ERROR_NOT_SUPPORTED
    }
    let output = ctx.arg_u32(4)?;
    let capacity = ctx.arg_u32(5)?;
    let written = ctx.arg_u32(6)?;
    // The HTC stream ABI also accepts a zero length for its fixed-size packet.
    if output == 0
        || (capacity != 0 && capacity < AXES_BYTES)
        || ctx
            .cpu
            .check_guest_access(output, AXES_BYTES, Prot::WRITE)
            .is_err()
        || (written != 0 && ctx.cpu.check_guest_access(written, 4, Prot::WRITE).is_err())
    {
        return fail(ctx, 87);
    }
    let Some(axes) = sample_counts(ctx.kernel) else {
        return fail(ctx, if available(ctx.kernel) { 21 } else { 1167 });
    };
    let axes = if stream == Some(AccelerometerStream::Acc1) {
        // The SDK decodes ACC1 as (raw X - 0x800, raw Y - 0x800, 0x800 - raw Z).
        // Invert that wire encoding after normalizing the device axes.
        let [x, y, z] = axes.map(i32::from);
        [x, y, -z].map(|value| (value + 0x800).clamp(0, 0xfff) as i16)
    } else {
        axes
    };
    let mut data = [0; AXES_BYTES as usize];
    for (value, field) in axes.into_iter().zip(data.chunks_exact_mut(2)) {
        field.copy_from_slice(&value.to_le_bytes());
    }
    ctx.cpu.write_mem(output, &data)?;
    if written != 0 {
        ctx.cpu.write_mem(written, &AXES_BYTES.to_le_bytes())?;
    }
    Ok(DispatchOutcome::ReturnedR0(1))
}

fn open(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    if ctx.arg_u32(0)? != 1 || !available(ctx.kernel) {
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
    let Some(axes) = sample_counts(ctx.kernel) else {
        return Ok(DispatchOutcome::ReturnedR0(21)); // ERROR_NOT_READY
    };
    // TiltX/Y/Z are signed 16-bit counts, approximately 1000 per g.
    // Leave the SDK's unknown and auxiliary angle fields at zero.
    let mut data = [0u8; 20];
    for (value, field) in axes.into_iter().zip(data.chunks_exact_mut(2)) {
        field.copy_from_slice(&value.to_le_bytes());
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
    fn acc1_stream_returns_biased_samples_for_the_native_sdk() {
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        kernel.virtual_tilt_axes = Some(accelerometer::TiltAxes::Both);
        let name: Vec<_> = "ACC1:"
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        cpu.write_mem(0x1000, &name).unwrap();
        let DispatchOutcome::ReturnedR0(handle) = call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "coredll.dll",
            "CreateFileW",
            &[0x1000, 0, 0, 0, 3, 0, 0],
        ) else {
            panic!("expected an ACC1 stream handle");
        };
        assert_eq!(
            kernel.vfs.htc_sensor_stream(handle),
            Some(AccelerometerStream::Acc1)
        );
        for (code, expected) in [(0x0100_0001, 0), (0x8012_2ef0, 1)] {
            assert_eq!(
                call(
                    &mut cpu,
                    &mut kernel,
                    &mut dispatcher,
                    "coredll.dll",
                    "DeviceIoControl",
                    &[handle, code, 0, 0, 0x1100, 6, 0x1200, 0]
                ),
                DispatchOutcome::ReturnedR0(expected)
            );
        }
        assert_eq!(cpu.read_mem(0x1100, 6).unwrap(), [0, 8, 0, 8, 0xe8, 0x0b]);
        kernel.pending_input.extend([
            pocket_kernel::InputEvent::KeyboardRotation { turns: 1 },
            pocket_kernel::InputEvent::KeyDown {
                vk: pocket_kernel::gapi::VK_UP,
            },
        ]);
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "DeviceIoControl",
                &[handle, 0x8012_2ef0, 0, 0, 0x1100, 6, 0x1200, 0]
            ),
            DispatchOutcome::ReturnedR0(1)
        );
        assert_eq!(
            cpu.read_mem(0x1100, 6).unwrap(),
            [0, 8, 0x0c, 6, 0x62, 0x0b]
        );
        assert_eq!(cpu.read_u32_le(0x1200).unwrap(), 6);
    }

    #[test]
    fn sen0_stream_uses_arrow_tilt_with_the_legacy_zero_length_request() {
        use pocket_kernel::{gapi, InputEvent};

        let (mut cpu, mut kernel, mut dispatcher) = setup();
        kernel.gapi_action_key = gapi::VK_RETURN;
        kernel.virtual_tilt_axes = Some(accelerometer::TiltAxes::Horizontal);
        let name: Vec<_> = "SEN0:"
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        cpu.write_mem(0x1000, &name).unwrap();
        let DispatchOutcome::ReturnedR0(handle) = call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "coredll.dll",
            "CreateFileW",
            &[0x1000, 0, 0, 0, 3, 0x80, 0],
        ) else {
            panic!("expected a SEN0 stream handle");
        };
        assert!(kernel.vfs.is_htc_sensor(handle));
        kernel
            .pending_input
            .push_back(InputEvent::KeyDown { vk: gapi::VK_RIGHT });
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "DeviceIoControl",
                &[handle, 0x0100_0001, 0, 0, 0x1100, 0, 0x1200, 0],
            ),
            DispatchOutcome::ReturnedR0(1)
        );
        assert_eq!(cpu.read_mem(0x1100, 2).unwrap(), 500i16.to_le_bytes());
        assert_eq!(cpu.read_u32_le(0x1200).unwrap(), 6);
        kernel.pending_input.extend([
            InputEvent::KeyUp { vk: gapi::VK_RIGHT },
            InputEvent::KeyboardRotation { turns: 3 },
            // Counter-clockwise display rotation maps display Right to guest Down.
            InputEvent::KeyDown { vk: gapi::VK_DOWN },
        ]);
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "DeviceIoControl",
                &[handle, 0x0100_0001, 0, 0, 0x1100, 0, 0x1200, 0],
            ),
            DispatchOutcome::ReturnedR0(1)
        );
        assert_eq!(
            cpu.read_mem(0x1100, 6).unwrap(),
            [0, 0, 0xf4, 1, 0x9e, 0xfc]
        );
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "CloseHandle",
                &[handle]
            ),
            DispatchOutcome::ReturnedR0(1)
        );
        assert!(!kernel.vfs.is_htc_sensor(handle));
    }

    #[test]
    fn virtual_tilt_tracks_display_direction_and_centers_on_release() {
        use pocket_kernel::{gapi, InputEvent};

        let (mut cpu, mut kernel, mut dispatcher) = setup();
        kernel.virtual_tilt_axes = Some(accelerometer::TiltAxes::Horizontal);
        let DispatchOutcome::ReturnedR0(handle) = call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            DLL,
            "HTCSensorOpen",
            &[1],
        ) else {
            panic!("expected sensor handle");
        };
        assert_ne!(handle, 0);
        let mut read = |kernel: &mut pocket_kernel::KernelState| {
            assert_eq!(
                call(
                    &mut cpu,
                    kernel,
                    &mut dispatcher,
                    DLL,
                    "HTCSensorGetDataOutput",
                    &[handle, 0x1100]
                ),
                DispatchOutcome::ReturnedR0(0)
            );
            let bytes = cpu.read_mem(0x1100, 6).unwrap();
            std::array::from_fn::<_, 3, _>(|i| i16::from_le_bytes([bytes[i * 2], bytes[i * 2 + 1]]))
        };
        assert_eq!(read(&mut kernel), [0, 0, -1000]);
        kernel
            .pending_input
            .push_back(InputEvent::KeyboardRotation { turns: 1 });
        // At 90 degrees clockwise, display Left reaches the guest as Down.
        kernel
            .pending_input
            .push_back(InputEvent::KeyDown { vk: gapi::VK_DOWN });
        assert_eq!(read(&mut kernel), [0, 500, -866]);
        while kernel.pop_pending_input().is_some() {}
        assert_eq!(read(&mut kernel), [0, 500, -866]);
        kernel
            .pending_input
            .push_back(InputEvent::KeyDown { vk: gapi::VK_UP });
        assert_eq!(read(&mut kernel), [0, 0, -1000]);
        kernel
            .pending_input
            .push_back(InputEvent::KeyUp { vk: gapi::VK_DOWN });
        assert_eq!(read(&mut kernel), [0, -500, -866]);
        kernel.reset_foreground_input();
        assert_eq!(read(&mut kernel), [0, 0, -1000]);
        kernel
            .pending_input
            .push_back(InputEvent::KeyDown { vk: gapi::VK_UP });
        assert_eq!(read(&mut kernel), [0, -500, -866]);
        kernel
            .pending_input
            .push_back(InputEvent::KeyUp { vk: gapi::VK_UP });
        assert_eq!(read(&mut kernel), [0, 0, -1000]);

        kernel
            .pending_input
            .push_back(InputEvent::TouchSteeringPosition { x: 560, y: -560 });
        assert_eq!(read(&mut kernel), [0, -250, -968]);
        kernel.virtual_tilt_axes = Some(accelerometer::TiltAxes::Both);
        assert_eq!(read(&mut kernel), [-250, -250, -935]);

        // In the rotated view Up reaches the guest as Left. It overrides
        // the stick vertically without affecting horizontal travel.
        kernel
            .pending_input
            .push_back(InputEvent::KeyDown { vk: gapi::VK_LEFT });
        assert_eq!(read(&mut kernel), [-500, -250, -829]);
        kernel
            .pending_input
            .push_back(InputEvent::KeyUp { vk: gapi::VK_LEFT });
        kernel
            .pending_input
            .push_back(InputEvent::TouchSteeringPosition { x: -1000, y: 1000 });
        assert_eq!(read(&mut kernel), [500, 500, -707]);
        kernel
            .pending_input
            .push_back(InputEvent::TouchSteeringPosition { x: 0, y: 0 });
        assert_eq!(read(&mut kernel), [0, 0, -1000]);
    }

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
        assert_eq!(
            cpu.read_mem(0x1100, 6).unwrap(),
            [0x0c, 0xfe, 0x18, 0xfc, 0, 0]
        );
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
