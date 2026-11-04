//! CE text shortcuts created through the same VFS contracts as CreateFileW.
use crate::{CallCtx, WinCeDispatcher};
use pocket_cpu::Prot;
use pocket_kernel::{DispatchOutcome, KernelError};

pub fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", "SHCreateShortcut", create);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth::tests::{call, setup};
    use pocket_cpu::Cpu;
    #[test]
    fn accented_command_uses_guest_ansi_and_unrepresentable_text_fails() {
        let encoded = encode_command("\\Jeux\\Été.exe").unwrap();
        let split = encoded.iter().position(|b| *b == b'#').unwrap();
        assert_eq!(
            std::str::from_utf8(&encoded[..split])
                .unwrap()
                .parse::<usize>()
                .unwrap(),
            encoded.len() - split - 1
        );
        assert!(encoded.contains(&0xe9));
        assert!(!encoded.windows(2).any(|b| b == [0xc3, 0xa9]));
        assert_eq!(encode_command("\\新.exe"), Err(1113));
    }
    #[test]
    fn ce_shortcut_creates_real_file_without_overwriting_or_creating_parents() {
        let (mut cpu, mut k, mut d) = setup();
        let dir = tempfile::tempdir().unwrap();
        k.vfs.mount("\\", dir.path());
        let put = |cpu: &mut pocket_cpu::stub::StubCpu, p, s: &str| {
            cpu.write_mem(
                p,
                &s.encode_utf16()
                    .chain(Some(0))
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>(),
            )
            .unwrap()
        };
        put(&mut cpu, 0x1000, "\\App.lnk");
        put(&mut cpu, 0x1200, "\"\\Flash Disk\\Test.exe\" -run");
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "SHCreateShortcut",
                &[0x1000, 0x1200]
            ),
            DispatchOutcome::ReturnedR0(1)
        );
        let bytes = std::fs::read(dir.path().join("App.lnk")).unwrap();
        assert!(String::from_utf8(bytes.clone())
            .unwrap()
            .ends_with("#\"\\Flash Disk\\Test.exe\" -run"));
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "SHCreateShortcut",
                &[0x1000, 0x1200]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(std::fs::read(dir.path().join("App.lnk")).unwrap(), bytes);
        put(&mut cpu, 0x1000, "\\missing\\App.lnk");
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "SHCreateShortcut",
                &[0x1000, 0x1200]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert!(!dir.path().join("missing").exists());
    }
}
pub(crate) fn text(ctx: &mut CallCtx<'_>, p: u32, limit: u32) -> Result<String, u32> {
    if p == 0 {
        return Err(87);
    }
    let mut units = Vec::new();
    for i in 0..limit {
        let a = p.checked_add(i * 2).ok_or(87u32)?;
        if ctx.cpu.check_guest_access(a, 2, Prot::READ).is_err() {
            return Err(87);
        }
        let b = ctx.cpu.read_mem(a, 2).map_err(|_| 87u32)?;
        let c = u16::from_le_bytes([b[0], b[1]]);
        if c == 0 {
            return String::from_utf16(&units).map_err(|_| 87);
        }
        units.push(c);
    }
    Err(87)
}
fn create(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let path = ctx.arg_u32(0)?;
    let target = ctx.arg_u32(1)?;
    let result = (|| {
        let path = text(ctx, path, 260)?;
        let target = text(ctx, target, 256)?;
        if path.is_empty() || target.is_empty() {
            return Err(87);
        }
        // CE .lnk files contain a decimal command length, '#', and the command.
        // Preserve the caller's quotes and arguments; do not execute on the host.
        let bytes = encode_command(&target)?;
        let file = ctx.kernel.vfs.open_file(&path, 2, 0, 1, false, 0x80)?;
        let written = ctx.kernel.vfs.write(file.handle, &bytes);
        let error = ctx.kernel.vfs.write_failure_error(file.handle);
        ctx.kernel.vfs.close(file.handle);
        if written != Some(bytes.len()) {
            // Only this newly created file is eligible for cleanup.
            let _ = ctx.kernel.vfs.delete(&path);
            return Err(error);
        }
        Ok(())
    })();
    match result {
        Ok(()) => Ok(DispatchOutcome::ReturnedR0(1)),
        Err(e) => {
            crate::coredll::set_thread_error(ctx, e);
            Ok(DispatchOutcome::ReturnedR0(0))
        }
    }
}

fn encode_command(command: &str) -> Result<Vec<u8>, u32> {
    // CE shortcuts are narrow text files, not UTF-8 or UTF-16 files. Match
    // the emulator's CP_ACP conversion (Latin-1) without lossy '?' replacement.
    let body: Vec<u8> = command
        .chars()
        .map(|c| u8::try_from(u32::from(c)).map_err(|_| 1113u32))
        .collect::<Result<_, _>>()?;
    let mut result = format!("{}#", body.len()).into_bytes();
    result.extend(body);
    Ok(result)
}
