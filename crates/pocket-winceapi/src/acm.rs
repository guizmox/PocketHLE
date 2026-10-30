//! Synchronous PCM ACM conversion, independent of waveOut and its voice budget.
use crate::{CallCtx, WinCeDispatcher};
use pocket_cpu::{Cpu, Prot};
use pocket_kernel::audio::conversion::{Format, Prepared, Stream};
use pocket_kernel::{DispatchOutcome, KernelError};

const INVALID_HANDLE: u32 = 5;
const NO_MEMORY: u32 = 7;
const NOT_SUPPORTED: u32 = 8;
const INVALID_FLAGS: u32 = 10;
const INVALID_PARAMETER: u32 = 11;
const NOT_POSSIBLE: u32 = 512;
const BUSY: u32 = 513;
const UNPREPARED: u32 = 514;
const PREPARED: u32 = 0x20000;
const DONE: u32 = 0x10000;
const INQUEUE: u32 = 0x100000;
const HEADER_SIZE: u32 = 84; // CE's 32-bit ACMSTREAMHEADER, including reserved DWORDs.
const BUFFER_LIMIT: u32 = 64 * 1024 * 1024;

pub(crate) fn register(dispatcher: &mut WinCeDispatcher) {
    for dll in ["coredll.dll", "msacm32.dll"] {
        dispatcher.register_handler(dll, "acmStreamOpen", open);
        dispatcher.register_handler(dll, "acmStreamSize", size);
        dispatcher.register_handler(dll, "acmStreamPrepareHeader", prepare);
        dispatcher.register_handler(dll, "acmStreamConvert", convert);
        dispatcher.register_handler(dll, "acmStreamUnprepareHeader", unprepare);
        dispatcher.register_handler(dll, "acmStreamClose", close);
    }
}
fn outcome(result: Result<(), u32>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(result.err().unwrap_or(0)))
}
fn readable(cpu: &mut dyn Cpu, pointer: u32, length: u32) -> Result<Vec<u8>, u32> {
    if pointer == 0 || pointer.checked_add(length).is_none() {
        return Err(INVALID_PARAMETER);
    }
    cpu.check_guest_access(pointer, length, Prot::READ)
        .map_err(|_| INVALID_PARAMETER)?;
    cpu.read_mem(pointer, length).map_err(|_| INVALID_PARAMETER)
}
fn writable(cpu: &mut dyn Cpu, pointer: u32, length: u32) -> Result<(), u32> {
    if pointer == 0 || pointer.checked_add(length).is_none() {
        return Err(INVALID_PARAMETER);
    }
    cpu.check_guest_access(pointer, length, Prot::WRITE)
        .map_err(|_| INVALID_PARAMETER)
}
fn write_word(cpu: &mut dyn Cpu, pointer: u32, value: u32) -> Result<(), u32> {
    writable(cpu, pointer, 4)?;
    cpu.write_mem(pointer, &value.to_le_bytes())
        .map_err(|_| INVALID_PARAMETER)
}
fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}
fn header(cpu: &mut dyn Cpu, pointer: u32) -> Result<(u32, Prepared), u32> {
    let bytes = readable(cpu, pointer, HEADER_SIZE)?;
    if word(&bytes, 0) < HEADER_SIZE {
        return Err(INVALID_PARAMETER);
    }
    writable(cpu, pointer, HEADER_SIZE)?;
    Ok((
        word(&bytes, 4),
        Prepared {
            source: word(&bytes, 12),
            source_length: word(&bytes, 16),
            destination: word(&bytes, 28),
            destination_length: word(&bytes, 32),
        },
    ))
}
fn validate_buffers(cpu: &mut dyn Cpu, h: Prepared) -> Result<(), u32> {
    if h.source_length > BUFFER_LIMIT || h.destination_length > BUFFER_LIMIT {
        return Err(INVALID_PARAMETER);
    }
    if h.source_length != 0 {
        if h.source == 0 || h.source.checked_add(h.source_length).is_none() {
            return Err(INVALID_PARAMETER);
        }
        cpu.check_guest_access(h.source, h.source_length, Prot::READ)
            .map_err(|_| INVALID_PARAMETER)?;
    }
    if h.destination_length != 0 {
        writable(cpu, h.destination, h.destination_length)?;
    }
    Ok(())
}
fn open(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let output = ctx.arg_u32(0)?;
    let driver = ctx.arg_u32(1)?;
    let source = ctx.arg_u32(2)?;
    let destination = ctx.arg_u32(3)?;
    let filter = ctx.arg_u32(4)?;
    let callback = ctx.arg_u32(5)?;
    let instance = ctx.arg_u32(6)?;
    let flags = ctx.arg_u32(7)?;
    outcome((|| {
        if flags & !5 != 0 {
            return Err(if flags & 2 != 0 {
                NOT_SUPPORTED
            } else {
                INVALID_FLAGS
            });
        }
        if driver != 0 {
            return Err(INVALID_HANDLE);
        }
        if filter != 0 || callback != 0 || instance != 0 {
            return Err(NOT_SUPPORTED);
        }
        if flags & 1 == 0 {
            write_word(ctx.cpu, output, 0)?;
        }
        let source = Format::parse(&readable(ctx.cpu, source, 18)?).ok_or(NOT_POSSIBLE)?;
        let destination =
            Format::parse(&readable(ctx.cpu, destination, 18)?).ok_or(NOT_POSSIBLE)?;
        let stream = Stream::new(source, destination).ok_or(NOT_POSSIBLE)?;
        if flags & 1 != 0 {
            return Ok(());
        }
        let handle = ctx
            .kernel
            .audio
            .conversions
            .insert(stream)
            .ok_or(NO_MEMORY)?;
        if let Err(error) = write_word(ctx.cpu, output, handle) {
            ctx.kernel.audio.conversions.entries.remove(&handle);
            return Err(error);
        }
        Ok(())
    })())
}
fn size(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let handle = ctx.arg_u32(0)?;
    let bytes = ctx.arg_u32(1)?;
    let output = ctx.arg_u32(2)?;
    let flags = ctx.arg_u32(3)?;
    outcome((|| {
        write_word(ctx.cpu, output, 0)?;
        let stream = ctx
            .kernel
            .audio
            .conversions
            .entries
            .get(&handle)
            .ok_or(INVALID_HANDLE)?;
        if flags > 1 {
            return Err(INVALID_FLAGS);
        }
        if bytes == 0 {
            return Err(INVALID_PARAMETER);
        }
        let length = stream.size(bytes, flags == 1).ok_or(NOT_POSSIBLE)?;
        write_word(ctx.cpu, output, length)
    })())
}
fn prepare(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let handle = ctx.arg_u32(0)?;
    let pointer = ctx.arg_u32(1)?;
    let flags = ctx.arg_u32(2)?;
    outcome((|| {
        if !ctx.kernel.audio.conversions.entries.contains_key(&handle) {
            return Err(INVALID_HANDLE);
        }
        if flags != 0 {
            return Err(INVALID_FLAGS);
        }
        let (status, h) = header(ctx.cpu, pointer)?;
        if status & INQUEUE != 0 {
            return Err(BUSY);
        }
        validate_buffers(ctx.cpu, h)?;
        for (&owner, stream) in &ctx.kernel.audio.conversions.entries {
            if let Some(old) = stream.headers.get(&pointer) {
                return if owner == handle && *old == h {
                    Ok(())
                } else {
                    Err(INVALID_PARAMETER)
                };
            }
        }
        write_word(
            ctx.cpu,
            pointer + 4,
            (status | PREPARED) & !(DONE | INQUEUE),
        )?;
        ctx.kernel
            .audio
            .conversions
            .entries
            .get_mut(&handle)
            .unwrap()
            .headers
            .insert(pointer, h);
        Ok(())
    })())
}
fn convert(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let handle = ctx.arg_u32(0)?;
    let pointer = ctx.arg_u32(1)?;
    let flags = ctx.arg_u32(2)?;
    outcome((|| {
        let stream = ctx
            .kernel
            .audio
            .conversions
            .entries
            .get(&handle)
            .ok_or(INVALID_HANDLE)?;
        if flags & !0x34 != 0 {
            return Err(INVALID_FLAGS);
        } // BLOCKALIGN, START, END
        let (status, h) = header(ctx.cpu, pointer)?;
        let old = stream.headers.get(&pointer).ok_or(UNPREPARED)?;
        if status & INQUEUE != 0 {
            return Err(BUSY);
        }
        if h.source != old.source
            || h.destination != old.destination
            || h.source_length > old.source_length
            || h.destination_length > old.destination_length
        {
            return Err(INVALID_PARAMETER);
        }
        validate_buffers(ctx.cpu, h)?;
        let frames = (h.source_length / stream.source.frame_bytes())
            .min(h.destination_length / stream.destination.frame_bytes());
        let input_length = frames * stream.source.frame_bytes();
        let input = if input_length == 0 {
            Vec::new()
        } else {
            readable(ctx.cpu, h.source, input_length)?
        };
        let (used, output) = stream.convert(&input, h.destination_length);
        if !output.is_empty() {
            ctx.cpu
                .write_mem(h.destination, &output)
                .map_err(|_| INVALID_PARAMETER)?;
        }
        write_word(ctx.cpu, pointer + 20, used)?;
        write_word(ctx.cpu, pointer + 36, output.len() as u32)?;
        write_word(ctx.cpu, pointer + 4, (status | PREPARED | DONE) & !INQUEUE)
    })())
}
fn unprepare(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let handle = ctx.arg_u32(0)?;
    let pointer = ctx.arg_u32(1)?;
    let flags = ctx.arg_u32(2)?;
    outcome((|| {
        let stream = ctx
            .kernel
            .audio
            .conversions
            .entries
            .get_mut(&handle)
            .ok_or(INVALID_HANDLE)?;
        if flags != 0 {
            return Err(INVALID_FLAGS);
        }
        let (status, h) = header(ctx.cpu, pointer)?;
        if status & INQUEUE != 0 {
            return Err(BUSY);
        }
        if let Some(old) = stream.headers.get(&pointer) {
            if *old != h {
                return Err(INVALID_PARAMETER);
            }
        } else {
            return Err(UNPREPARED);
        }
        write_word(ctx.cpu, pointer + 4, status & !PREPARED)?;
        stream.headers.remove(&pointer);
        Ok(())
    })())
}
fn close(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let handle = ctx.arg_u32(0)?;
    let flags = ctx.arg_u32(1)?;
    outcome((|| {
        let stream = ctx
            .kernel
            .audio
            .conversions
            .entries
            .get(&handle)
            .ok_or(INVALID_HANDLE)?;
        if flags != 0 {
            return Err(INVALID_FLAGS);
        }
        if !stream.headers.is_empty() {
            return Err(BUSY);
        }
        ctx.kernel.audio.conversions.entries.remove(&handle);
        Ok(())
    })())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth::tests::{call, setup};
    use pocket_cpu::stub::StubCpu;
    use pocket_kernel::KernelState;
    fn invoke(
        cpu: &mut StubCpu,
        kernel: &mut KernelState,
        dispatcher: &mut WinCeDispatcher,
        name: &str,
        args: &[u32],
    ) -> u32 {
        let DispatchOutcome::ReturnedR0(result) =
            call(cpu, kernel, dispatcher, "coredll.dll", name, args)
        else {
            panic!("ACM must return a status");
        };
        result
    }
    fn formats(cpu: &mut StubCpu) {
        for (pointer, bits) in [(0x2100, 16u16), (0x2200, 8u16)] {
            let mut b = Vec::new();
            b.extend(1u16.to_le_bytes());
            b.extend(1u16.to_le_bytes());
            b.extend(22050u32.to_le_bytes());
            b.extend((22050u32 * u32::from(bits / 8)).to_le_bytes());
            b.extend((bits / 8).to_le_bytes());
            b.extend(bits.to_le_bytes());
            b.extend(0u16.to_le_bytes());
            cpu.write_mem(pointer, &b).unwrap();
        }
    }
    fn make_header(cpu: &mut StubCpu) {
        let mut b = vec![0; HEADER_SIZE as usize];
        for (offset, value) in [
            (0, HEADER_SIZE),
            (12, 0x2400u32),
            (16, 6),
            (28, 0x2500),
            (32, 3),
        ] {
            b[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        cpu.write_mem(0x2300, &b).unwrap();
        cpu.write_mem(0x2400, &[0, 128, 0, 0, 255, 127]).unwrap();
        cpu.write_mem(0x2500, &[42; 4]).unwrap();
    }
    #[test]
    fn pcm_stream_lifecycle_status_counts_and_invalid_handles() {
        let (mut cpu, mut kernel, mut d) = setup();
        formats(&mut cpu);
        make_header(&mut cpu);
        assert_eq!(
            invoke(
                &mut cpu,
                &mut kernel,
                &mut d,
                "acmStreamOpen",
                &[0x2600, 0, 0x2100, 0x2200, 0, 0, 0, 4]
            ),
            0
        );
        let h = cpu.read_u32_le(0x2600).unwrap();
        assert_ne!(h, 0);
        assert_eq!(
            invoke(
                &mut cpu,
                &mut kernel,
                &mut d,
                "acmStreamSize",
                &[h, 6, 0x2604, 0]
            ),
            0
        );
        assert_eq!(cpu.read_u32_le(0x2604).unwrap(), 3);
        assert_eq!(
            invoke(
                &mut cpu,
                &mut kernel,
                &mut d,
                "acmStreamConvert",
                &[h, 0x2300, 0]
            ),
            UNPREPARED
        );
        assert_eq!(
            invoke(
                &mut cpu,
                &mut kernel,
                &mut d,
                "acmStreamPrepareHeader",
                &[h, 0x2300, 0]
            ),
            0
        );
        assert_eq!(
            invoke(&mut cpu, &mut kernel, &mut d, "acmStreamClose", &[h, 0]),
            BUSY
        );
        assert_eq!(
            invoke(
                &mut cpu,
                &mut kernel,
                &mut d,
                "acmStreamConvert",
                &[h, 0x2300, 0]
            ),
            0
        );
        assert_eq!(cpu.read_mem(0x2500, 4).unwrap(), [0, 128, 255, 42]);
        assert_eq!(cpu.read_u32_le(0x2314).unwrap(), 6);
        assert_eq!(cpu.read_u32_le(0x2324).unwrap(), 3);
        assert_eq!(cpu.read_u32_le(0x2304).unwrap(), PREPARED | DONE);
        assert_eq!(
            invoke(
                &mut cpu,
                &mut kernel,
                &mut d,
                "acmStreamUnprepareHeader",
                &[h, 0x2300, 0]
            ),
            0
        );
        assert_eq!(
            invoke(&mut cpu, &mut kernel, &mut d, "acmStreamClose", &[h, 0]),
            0
        );
        assert_eq!(
            invoke(&mut cpu, &mut kernel, &mut d, "acmStreamClose", &[h, 0]),
            INVALID_HANDLE
        );
        assert_eq!(
            invoke(
                &mut cpu,
                &mut kernel,
                &mut d,
                "acmStreamSize",
                &[h, 6, 0x2604, 0]
            ),
            INVALID_HANDLE
        );
        assert_eq!(cpu.read_u32_le(0x2604).unwrap(), 0);
    }
    #[test]
    fn query_and_unsupported_formats_do_not_allocate_streams() {
        let (mut cpu, mut kernel, mut d) = setup();
        formats(&mut cpu);
        assert_eq!(
            invoke(
                &mut cpu,
                &mut kernel,
                &mut d,
                "acmStreamOpen",
                &[0, 0, 0x2100, 0x2200, 0, 0, 0, 5]
            ),
            0
        );
        assert!(kernel.audio.conversions.entries.is_empty());
        cpu.write_mem(0x2100, &2u16.to_le_bytes()).unwrap();
        assert_eq!(
            invoke(
                &mut cpu,
                &mut kernel,
                &mut d,
                "acmStreamOpen",
                &[0x2600, 0, 0x2100, 0x2200, 0, 0, 0, 4]
            ),
            NOT_POSSIBLE
        );
        assert_eq!(cpu.read_u32_le(0x2600).unwrap(), 0);
        assert!(kernel.audio.conversions.entries.is_empty());
    }
    #[test]
    fn prepared_header_cannot_change_pointers_or_exceed_its_buffers() {
        let (mut cpu, mut kernel, mut d) = setup();
        formats(&mut cpu);
        make_header(&mut cpu);
        assert_eq!(
            invoke(
                &mut cpu,
                &mut kernel,
                &mut d,
                "acmStreamOpen",
                &[0x2600, 0, 0x2100, 0x2200, 0, 0, 0, 4]
            ),
            0
        );
        let h = cpu.read_u32_le(0x2600).unwrap();
        assert_eq!(
            invoke(
                &mut cpu,
                &mut kernel,
                &mut d,
                "acmStreamPrepareHeader",
                &[h, 0x2300, 0]
            ),
            0
        );
        cpu.write_mem(0x2320, &4u32.to_le_bytes()).unwrap();
        assert_eq!(
            invoke(
                &mut cpu,
                &mut kernel,
                &mut d,
                "acmStreamConvert",
                &[h, 0x2300, 0]
            ),
            INVALID_PARAMETER
        );
        assert_eq!(cpu.read_mem(0x2500, 4).unwrap(), [42; 4]);
        cpu.write_mem(0x2320, &3u32.to_le_bytes()).unwrap();
        cpu.write_mem(0x230c, &0x2401u32.to_le_bytes()).unwrap();
        assert_eq!(
            invoke(
                &mut cpu,
                &mut kernel,
                &mut d,
                "acmStreamConvert",
                &[h, 0x2300, 0]
            ),
            INVALID_PARAMETER
        );
        assert_eq!(
            invoke(
                &mut cpu,
                &mut kernel,
                &mut d,
                "acmStreamPrepareHeader",
                &[h, 0xfffffff0, 0]
            ),
            INVALID_PARAMETER
        );
    }
}
