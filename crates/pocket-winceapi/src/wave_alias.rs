//! Repeated ordinary headers can share one mutable PCM backing (LeoFS mixer).
//! Reuse the existing live-loop transport/codec; header retirement stays WinMM's.
use crate::CallCtx;
use pocket_kernel::{KernelError, LiveWaveLoop};
pub(crate) fn queue(
    ctx: &mut CallCtx<'_>,
    handle: u32,
    hdr: u32,
    data: u32,
    bytes: &[u8],
    samples: &[i16],
) -> Result<bool, KernelError> {
    let Some(previous) = ctx
        .kernel
        .wave_out
        .pending
        .iter()
        .rev()
        .find(|v| v.handle == handle)
        .copied()
    else {
        return Ok(false);
    };
    let raw = ctx.cpu.read_mem(previous.hdr, 20)?;
    let word = |i: usize| u32::from_le_bytes(raw[i..i + 4].try_into().unwrap());
    // Exact aliases, consecutive on this device; never collapse distinct PCM
    // or turn an explicit BEGINLOOP/ENDLOOP header into ordinary submissions.
    if word(0) != data || word(4) as usize != bytes.len() || word(16) & 0xc != 0 {
        return Ok(false);
    }
    if ctx
        .kernel
        .wave_out
        .live_loops
        .get(&handle)
        .is_some_and(|source| {
            source.hdr != previous.hdr
                || source.data != data
                || source.bytes as usize != bytes.len()
        })
    {
        return Ok(false);
    }
    if !ctx
        .kernel
        .audio
        .queue_wave_alias(handle, previous.end_cursor, samples)
    {
        return Ok(false);
    }
    // Last header owns refresh retirement. Earlier alias headers still receive
    // their individual completion at their unchanged end_cursor boundaries.
    ctx.kernel.wave_out.live_loops.insert(
        handle,
        LiveWaveLoop {
            hdr,
            data,
            bytes: bytes.len() as u32,
        },
    );
    ctx.kernel
        .wave_out
        .loop_pcm_bytes
        .insert(handle, bytes.to_vec());
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth::tests::{call, setup};
    use pocket_cpu::{Cpu, Prot};
    #[test]
    fn aliased_headers_follow_live_pcm_and_keep_separate_completion_and_break_policy() {
        let (mut cpu, mut k, mut d) = setup();
        cpu.map_region(0x1000, 0x3000, Prot::READ | Prot::WRITE)
            .unwrap();
        let fmt = pocket_kernel::audio::GuestFormat {
            sample_rate: 48000,
            channels: 1,
            bits_per_sample: 8,
        };
        k.audio.open_wave_stream(1, fmt);
        k.wave_out.devices.insert(
            1,
            pocket_kernel::WaveOutDevice {
                format: fmt,
                callback_kind: pocket_kernel::WaveCallbackKind::None,
                callback_target: 0,
                instance: 0,
                owner_thread: 0,
                paused: false,
            },
        );
        let tap = k.audio.tap();
        tap.drain_into(&mut []);
        cpu.write_mem(0x2800, &[128; 4]).unwrap();
        for hdr in [0x2000u32, 0x2020] {
            for (offset, value) in [(0, 0x2800u32), (4, 4), (16, 2)] {
                cpu.write_mem(hdr + offset, &value.to_le_bytes()).unwrap();
            }
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "waveOutWrite",
                &[1, hdr, 32],
            );
        }
        assert_eq!(k.audio.wave_written_samples(1), 8);
        assert_eq!(k.wave_out.live_loops[&1].hdr, 0x2020);
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "waveOutBreakLoop",
            &[1],
        );
        assert_eq!(k.audio.wave_written_samples(1), 8);
        assert_eq!(
            k.wave_out
                .pending
                .iter()
                .map(|v| v.end_cursor)
                .collect::<Vec<_>>(),
            vec![4, 8]
        );
        cpu.write_mem(0x2800, &[129; 4]).unwrap();
        let thunk = pocket_kernel::Thunk {
            thunk_va: 0x70000000,
            iat_va: 0,
            dll: "coredll.dll".into(),
            binding: pocket_pe::ImportBinding::Name("waveOutGetPosition".into()),
            friendly_name: None,
        };
        crate::coredll::refresh_wave_loops(&mut CallCtx {
            cpu: &mut cpu,
            kernel: &mut k,
            thunk: &thunk,
        })
        .unwrap();
        let mut out = [0; 4];
        tap.drain_into(&mut out);
        assert_eq!(out, [256; 4]);
        cpu.write_mem(0x2100, &2u32.to_le_bytes()).unwrap();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "waveOutGetPosition",
            &[1, 0x2100, 8],
        );
        assert_eq!(cpu.read_u32_le(0x2010).unwrap() & 0x11, 1);
        assert_eq!(cpu.read_u32_le(0x2030).unwrap() & 0x11, 0x10);
        assert!(k.wave_out.live_loops.contains_key(&1));
        cpu.write_mem(0x2800, &[130; 4]).unwrap();
        crate::coredll::refresh_wave_loops(&mut CallCtx {
            cpu: &mut cpu,
            kernel: &mut k,
            thunk: &thunk,
        })
        .unwrap();
        tap.drain_into(&mut out);
        assert_eq!(out, [512; 4]);
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "waveOutGetPosition",
            &[1, 0x2100, 8],
        );
        assert_eq!(cpu.read_u32_le(0x2030).unwrap() & 0x11, 1);
        assert!(!k.wave_out.live_loops.contains_key(&1));
        assert!(k.wave_out.pending.is_empty());
    }
}
