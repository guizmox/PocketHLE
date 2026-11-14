//! Original Explorer subscribes itself to time-change notifications during boot.
use crate::{CallCtx, WinCeDispatcher};
use pocket_kernel::{DispatchOutcome, KernelError};
pub fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", "CeRunAppAtEvent", register_event);
}
fn register_event(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let pointer = ctx.arg_u32(0)?;
    let event = ctx.arg_u32(1)?;
    let request = (|| -> Result<(), u32> {
        if event > 15 {
            return Err(87);
        }
        if event != 0 && !ctx.kernel.process_launch_enabled {
            return Err(50);
        }
        let name = crate::ce_shortcut::text(ctx, pointer, 1024)?;
        if name.is_empty() {
            return Err(87);
        }
        ctx.kernel.object_handles.register_app_event(name, event)
    })();
    match request {
        Ok(()) => Ok(DispatchOutcome::ReturnedR0(1)),
        Err(error) => {
            crate::coredll::set_thread_error(ctx, error);
            Ok(DispatchOutcome::ReturnedR0(0))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth::tests::{call, setup};
    use pocket_cpu::Cpu;
    #[test]
    fn shell_event_subscription_survives_process_exit_and_cancels_by_normalized_name() {
        let (mut cpu, mut k, mut d) = setup();
        k.process_launch_enabled = true;
        cpu.write_mem(
            0x1000,
            &r"\Windows\explorer.exe"
                .encode_utf16()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
        for event in [0, 1, 1] {
            assert_eq!(
                call(
                    &mut cpu,
                    &mut k,
                    &mut d,
                    "coredll.dll",
                    "CeRunAppAtEvent",
                    &[0x1000, event]
                ),
                DispatchOutcome::ReturnedR0(1)
            );
        }
        let (_, child) = k.object_handles.new_child().unwrap();
        k.object_handles.mark_inactive();
        child.emit_app_event(1);
        child.emit_app_event(1);
        assert_eq!(
            child.take_app_event(),
            Some((
                r"\Windows\explorer.exe".into(),
                "AppRunAfterTimeChange".into()
            ))
        );
        assert!(child.take_app_event().is_none());
        child
            .register_app_event("/windows/EXPLORER.exe".into(), 0)
            .unwrap();
        child.emit_app_event(1);
        assert!(child.take_app_event().is_none());
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "CeRunAppAtEvent",
                &[0x1000, 16]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
    }
    #[test]
    fn device_clock_change_is_shared_and_emits_one_time_notification_without_changing_host_clock() {
        let (mut cpu, mut k, mut d) = setup();
        k.object_handles
            .register_app_event(r"\Windows\explorer.exe".into(), 1)
            .unwrap();
        let (_, child) = k.object_handles.new_child().unwrap();
        let st = [1969u16, 12, 3, 31, 12, 0, 0, 0];
        cpu.write_mem(
            0x1000,
            &st.into_iter()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let host_before = std::time::SystemTime::now();
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "SetLocalTime",
                &[0x1000]
            ),
            DispatchOutcome::ReturnedR0(1)
        );
        let shared = crate::datetime::now(child.utc_now());
        assert_eq!(&shared[..6], &st[..6]);
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "GetLocalTime",
            &[0x1100],
        );
        assert_eq!(
            cpu.read_mem(0x1100, 12).unwrap(),
            st[..6]
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>()
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "GetCurrentFT",
            &[0x1200],
        );
        let file_time = u64::from_le_bytes(cpu.read_mem(0x1200, 8).unwrap().try_into().unwrap());
        let expected = (11_644_473_600u64 - 43_200) * 10_000_000;
        assert!((expected..expected + 10_000_000).contains(&file_time));
        assert_eq!(
            call(&mut cpu, &mut k, &mut d, "coredll.dll", "time", &[0x1300]),
            DispatchOutcome::ReturnedR0((-43_200i32) as u32)
        );
        assert_eq!(cpu.read_u32_le(0x1300).unwrap(), (-43_200i32) as u32);
        assert!(std::time::SystemTime::now()
            .duration_since(host_before)
            .is_ok());
        assert_eq!(child.take_app_event().unwrap().1, "AppRunAfterTimeChange");
        assert!(child.take_app_event().is_none());
        cpu.write_mem(0x1006, &32u16.to_le_bytes()).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "SetSystemTime",
                &[0x1000]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(crate::datetime::now(child.utc_now())[3], 31);
        assert!(child.take_app_event().is_none());
    }
}
