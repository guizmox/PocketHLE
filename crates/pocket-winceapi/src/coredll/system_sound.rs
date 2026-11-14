//! MessageBeep and PlaySound use the mounted CE files and live device schemes.
//! Keep their mixer streams separate from waveOut/HSS, on Android as on Windows.
use super::*;
use pocket_kernel::system_sound::{
    self as settings, SoundCategory, SoundSelection, SoundWait, SystemEvent, EVENT_SOUND_STREAM,
    PLAY_SOUND_STREAM,
};

const SND_ASYNC: u32 = 1;
const SND_NODEFAULT: u32 = 2;
const SND_MEMORY: u32 = 4;
const SND_LOOP: u32 = 8;
const SND_NOSTOP: u32 = 0x10;
const SND_PURGE: u32 = 0x40;
const SND_ALIAS: u32 = 0x10000;
const SND_FILENAME: u32 = 0x20000;
const SND_RESOURCE: u32 = 0x40004;
const SND_ALIAS_ID: u32 = 0x110000;

fn submit(
    kernel: &mut KernelState,
    bytes: &[u8],
    stream: u32,
    category: SoundCategory,
    looped: bool,
) -> bool {
    kernel.sync_ce_settings();
    submit_at_volume(
        kernel,
        bytes,
        stream,
        category.volume(&kernel.registry),
        looped,
    )
}

fn submit_at_volume(
    kernel: &mut KernelState,
    bytes: &[u8],
    stream: u32,
    volume: u32,
    looped: bool,
) -> bool {
    let Some((format, samples)) = decode_pcm_wave(bytes) else {
        return false;
    };
    if samples.is_empty() || samples.len() % usize::from(format.channels) != 0 {
        return false;
    }
    kernel.audio.open_wave_stream(stream, format);
    kernel.audio.set_wave_volume(stream, volume);
    if looped {
        kernel.audio.queue_wave_loop(stream, samples, u32::MAX);
    } else {
        kernel.audio.push_wave_samples(stream, &samples);
    }
    kernel.audio.start();
    true
}

fn selected_bytes(kernel: &mut KernelState, sound: SoundSelection) -> Option<Vec<u8>> {
    match sound {
        SoundSelection::File(path) => {
            read_kernel_file(kernel, &path).filter(|bytes| !bytes.is_empty())
        }
        SoundSelection::Silent => Some(Vec::new()),
        SoundSelection::Missing => None,
    }
}

pub(crate) fn play_event(kernel: &mut KernelState, event: SystemEvent) -> bool {
    if SoundCategory::Event.volume(&kernel.registry) == 0 {
        return true;
    }
    let sound = settings::event_sound(&kernel.registry, event);
    let Some(bytes) = selected_bytes(kernel, sound) else {
        return false;
    };
    if bytes.is_empty() {
        return true;
    }
    let played = submit(
        kernel,
        &bytes,
        EVENT_SOUND_STREAM,
        SoundCategory::Event,
        false,
    );
    log::debug!("CE system sound {event:?}: played={played}");
    played
}

pub(crate) fn service_pending(kernel: &mut KernelState) {
    let main_finished = kernel.main_thread.exit_code.is_some();
    let threads = &kernel.threads;
    let deadlines = &mut kernel.wait_deadlines;
    kernel.ce.sounds.waits.retain(|wait| {
        // Explorer's worker IDs start at one; zero belongs to the main pump.
        let finished = if wait.thread == 0 {
            main_finished
        } else {
            threads
                .get(wait.thread - 1)
                .is_some_and(|thread| thread.finished)
        };
        if finished {
            deadlines.remove(&(wait.thread, wait.thunk, wait.frame.sp));
        }
        !finished
    });
    if std::mem::take(&mut kernel.ce.sounds.startup_pending) {
        play_event(kernel, SystemEvent::Startup);
    }
}

pub(super) fn preview(
    kernel: &mut KernelState,
    registry: &pocket_kernel::registry::Registry,
) -> bool {
    let selection = settings::event_sound(registry, SystemEvent::Default);
    let Some(bytes) = selected_bytes(kernel, selection) else {
        return false;
    };
    bytes.is_empty()
        || submit_at_volume(
            kernel,
            &bytes,
            EVENT_SOUND_STREAM,
            SoundCategory::Event.volume(registry),
            false,
        )
}

pub(super) fn message_beep(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let kind = ctx.arg_u32(0)?;
    let Some(event) = SystemEvent::from_message_beep(kind) else {
        set_thread_error(ctx, 87);
        return Ok(DispatchOutcome::ReturnedR0(0));
    };
    let mut played = play_event(ctx.kernel, event);
    if !played && event != SystemEvent::Default {
        played = play_event(ctx.kernel, SystemEvent::Default);
    }
    // The documented simple beep also works on a profile without a default WAV.
    // Never replace an intentionally silent scheme or disabled event with a tone.
    if !played {
        let format = pocket_kernel::audio::GuestFormat {
            sample_rate: 22050,
            channels: 1,
            bits_per_sample: 16,
        };
        let frames = 2646usize;
        let samples: Vec<i16> = (0..frames)
            .map(|i| {
                let envelope = (i.min(frames - 1 - i) as f32 / 110.0).min(1.0);
                let phase = i as f32 * 880.0 * std::f32::consts::TAU / format.sample_rate as f32;
                (phase.sin() * envelope * 3500.0) as i16
            })
            .collect();
        ctx.kernel
            .audio
            .open_wave_stream(EVENT_SOUND_STREAM, format);
        ctx.kernel.audio.set_wave_volume(
            EVENT_SOUND_STREAM,
            SoundCategory::Event.volume(&ctx.kernel.registry),
        );
        ctx.kernel
            .audio
            .push_wave_samples(EVENT_SOUND_STREAM, &samples);
        ctx.kernel.audio.start();
        played = true;
    }
    Ok(DispatchOutcome::ReturnedR0(u32::from(played)))
}

fn supersede_sound(kernel: &mut KernelState) {
    kernel.ce.sounds.generation = kernel.ce.sounds.generation.wrapping_add(1);
    for wait in &kernel.ce.sounds.waits {
        kernel
            .wait_deadlines
            .remove(&(wait.thread, wait.thunk, wait.frame.sp));
    }
}

fn stop(kernel: &mut KernelState) {
    supersede_sound(kernel);
    kernel.audio.close_wave_stream(PLAY_SOUND_STREAM);
}

fn wait_for_sound(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    if let Some(outcome) = wait_display_until(
        ctx,
        std::time::Instant::now() + std::time::Duration::from_millis(4),
    )? {
        return Ok(outcome);
    }
    Ok(DispatchOutcome::JumpTo(ctx.thunk.thunk_va))
}

pub(super) fn play(
    ctx: &mut CallCtx<'_>,
    sound: u32,
    module: u32,
    flags: u32,
    wide: bool,
) -> Result<DispatchOutcome, KernelError> {
    let sp = ctx.cpu.read_reg(ArmReg::Sp)?;
    if let Some(index) = ctx.kernel.ce.sounds.waits.iter().position(|wait| {
        wait.thread == ctx.kernel.current_thread
            && wait.thunk == ctx.thunk.thunk_va
            && wait.frame.sp == sp
    }) {
        let wait = &ctx.kernel.ce.sounds.waits[index];
        if wait.generation == ctx.kernel.ce.sounds.generation
            && ctx.kernel.audio.wave_written_samples(PLAY_SOUND_STREAM) != 0
            && ctx.kernel.audio.wave_playback_cursor(PLAY_SOUND_STREAM) < wait.samples
        {
            return wait_for_sound(ctx);
        }
        let wait = ctx.kernel.ce.sounds.waits.remove(index);
        ctx.kernel
            .wait_deadlines
            .remove(&(wait.thread, wait.thunk, wait.frame.sp));
        for (i, value) in wait.frame.args.into_iter().enumerate() {
            ctx.cpu.write_arg(i as u8, value)?;
        }
        ctx.cpu.write_reg(ArmReg::Sp, wait.frame.sp)?;
        ctx.cpu.write_reg(ArmReg::Lr, wait.frame.lr)?;
        return Ok(DispatchOutcome::ReturnedR0(1));
    }
    if sound == 0 || flags & SND_PURGE != 0 {
        stop(ctx.kernel);
        return Ok(DispatchOutcome::ReturnedR0(1));
    }
    if flags & SND_LOOP != 0 && flags & SND_ASYNC == 0 {
        // A synchronous infinite loop cannot complete; CE requires SND_ASYNC.
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    if flags & SND_NOSTOP != 0
        && ctx.kernel.audio.wave_playback_cursor(PLAY_SOUND_STREAM)
            < ctx.kernel.audio.wave_written_samples(PLAY_SOUND_STREAM)
    {
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    let mut category = SoundCategory::Application;
    let bytes = if flags & SND_RESOURCE == SND_RESOURCE {
        let name = if sound <= 0xffff {
            ResourceKey::Id(sound)
        } else if wide {
            read_wide_resource_key(ctx, sound)?
        } else {
            ResourceKey::Name(crate::crt_ansi::decode(&read_cstr(ctx, sound, 260)?))
        };
        let Some((resource, base)) =
            lookup_resource(ctx.kernel, module, &ResourceKey::Name("WAVE".into()), &name)
                .or_else(|| lookup_resource(ctx.kernel, module, &ResourceKey::Id(10), &name))
        else {
            return Ok(DispatchOutcome::ReturnedR0(0));
        };
        if resource.size == 0 || resource.size > 64 * 1024 * 1024 {
            return Ok(DispatchOutcome::ReturnedR0(0));
        }
        ctx.cpu
            .read_mem(base.wrapping_add(resource.data_rva), resource.size)?
    } else if flags & SND_MEMORY != 0 {
        let header = ctx.cpu.read_mem(sound, 12)?;
        if &header[..4] != b"RIFF" || &header[8..12] != b"WAVE" {
            return Ok(DispatchOutcome::ReturnedR0(0));
        }
        let Some(size) = u32::from_le_bytes(header[4..8].try_into().unwrap())
            .checked_add(8)
            .filter(|n| (12..=64 * 1024 * 1024).contains(n))
        else {
            return Ok(DispatchOutcome::ReturnedR0(0));
        };
        ctx.cpu.read_mem(sound, size)?
    } else {
        let selected = if flags & SND_ALIAS_ID == SND_ALIAS_ID {
            category = SoundCategory::Event;
            let selection = SystemEvent::from_alias_id(sound)
                .map(|event| settings::event_sound(&ctx.kernel.registry, event))
                .unwrap_or(SoundSelection::Missing);
            selected_bytes(ctx.kernel, selection)
        } else {
            let name = if wide {
                String::from_utf16_lossy(&read_wstr(ctx, sound, 260)?)
            } else {
                crate::crt_ansi::decode(&read_cstr(ctx, sound, 260)?)
            };
            if flags & SND_ALIAS != 0 {
                category = SoundCategory::Event;
                let selection = settings::alias_sound(&ctx.kernel.registry, &name);
                selected_bytes(ctx.kernel, selection)
            } else if let Some(bytes) =
                read_kernel_file(ctx.kernel, &name).filter(|bytes| !bytes.is_empty())
            {
                Some(bytes)
            } else if flags & SND_FILENAME == 0 {
                category = SoundCategory::Event;
                let selection = settings::alias_sound(&ctx.kernel.registry, &name);
                selected_bytes(ctx.kernel, selection)
            } else {
                None
            }
        };
        match selected {
            Some(bytes) => bytes,
            None if flags & SND_NODEFAULT == 0 => {
                category = SoundCategory::Event;
                let selection = settings::event_sound(&ctx.kernel.registry, SystemEvent::Default);
                let Some(bytes) = selected_bytes(ctx.kernel, selection) else {
                    return Ok(DispatchOutcome::ReturnedR0(0));
                };
                bytes
            }
            None => return Ok(DispatchOutcome::ReturnedR0(0)),
        }
    };
    if flags & 0x04000000 != 0 {
        category = SoundCategory::Notification;
    } else if flags & 0x02000000 != 0 {
        category = SoundCategory::Event;
    }
    if bytes.is_empty() {
        stop(ctx.kernel);
        return Ok(DispatchOutcome::ReturnedR0(1));
    }
    if !submit(
        ctx.kernel,
        &bytes,
        PLAY_SOUND_STREAM,
        category,
        flags & SND_LOOP != 0,
    ) {
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    ctx.kernel.ce.sounds.category = category;
    supersede_sound(ctx.kernel);
    log::debug!(
        "PlaySound: {} bytes, category={category:?}, flags=0x{flags:08x}",
        bytes.len()
    );
    if flags & SND_ASYNC != 0 {
        return Ok(DispatchOutcome::ReturnedR0(1));
    }
    let frame = GuestCallFrame {
        args: [
            ctx.arg_u32(0)?,
            ctx.arg_u32(1)?,
            ctx.arg_u32(2)?,
            ctx.arg_u32(3)?,
        ],
        sp,
        lr: ctx.cpu.read_reg(ArmReg::Lr)?,
    };
    ctx.kernel.ce.sounds.waits.push(SoundWait {
        thread: ctx.kernel.current_thread,
        thunk: ctx.thunk.thunk_va,
        frame,
        generation: ctx.kernel.ce.sounds.generation,
        samples: ctx.kernel.audio.wave_written_samples(PLAY_SOUND_STREAM),
    });
    wait_for_sound(ctx)
}

pub(super) fn window_event(kernel: &mut KernelState, hwnd: u32, event: SystemEvent) {
    if kernel.ce.version.is_some()
        && kernel
            .window_styles
            .get(&hwnd)
            .is_some_and(|s| s.0 & 0x40c00000 == 0xc00000)
    {
        play_event(kernel, event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth::tests::{call, setup};
    use pocket_cpu::Cpu;
    use pocket_kernel::registry::RegistryValue;

    fn wav(value: i16, frames: u32) -> Vec<u8> {
        let data: Vec<_> = (0..frames).flat_map(|_| value.to_le_bytes()).collect();
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&8000u32.to_le_bytes());
        bytes.extend_from_slice(&16000u32.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
        bytes.extend(data);
        bytes
    }
    fn mount_sounds(kernel: &mut KernelState, root: &std::path::Path) -> std::path::PathBuf {
        std::fs::create_dir(root.join("Windows")).unwrap();
        for (name, value) in [
            ("asterisk", 1000),
            ("critical", 2000),
            ("default", 3000),
            ("exclam", 4000),
            ("question", 5000),
            ("startup", 6000),
            ("menupop", 7000),
            ("menusel", 8000),
            ("openprog", 9000),
            ("close", 10000),
            ("windmin", 11000),
            ("windmax", 12000),
        ] {
            std::fs::write(
                root.join("Windows").join(format!("{name}.wav")),
                wav(value, 64),
            )
            .unwrap();
        }
        kernel.vfs.mount_read_only_filesystem("\\", root);
        kernel.ce.version = Some((6, 0, 0));
        kernel
            .registry
            .set_value(settings::VOLUME_KEY, "Mute", RegistryValue::Dword(7));
        let capture = root.join("submitted.wav");
        kernel.audio.capture_to(&capture).unwrap();
        capture
    }
    fn wide(cpu: &mut dyn Cpu, pointer: u32, text: &str) {
        cpu.write_mem(
            pointer,
            &text
                .encode_utf16()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    }
    fn pcm(path: &std::path::Path) -> Vec<u8> {
        let bytes = std::fs::read(path).unwrap();
        if bytes.is_empty() {
            Vec::new()
        } else {
            assert_eq!(&bytes[..4], b"RIFF");
            bytes[44..].to_vec()
        }
    }

    #[test]
    fn message_beep_and_named_numeric_ansi_and_legacy_aliases_submit_real_pcm() {
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        let root = tempfile::tempdir().unwrap();
        let capture = mount_sounds(&mut kernel, root.path());
        kernel.thread_last_errors.insert(0, 0x1234);
        let mut expected = Vec::new();
        for (kind, value) in [
            (0, 3000),
            (0x10, 2000),
            (0x20, 5000),
            (0x30, 4000),
            (0x40, 1000),
        ] {
            assert_eq!(
                call(
                    &mut cpu,
                    &mut kernel,
                    &mut dispatcher,
                    "coredll.dll",
                    "MessageBeep",
                    &[kind]
                ),
                DispatchOutcome::ReturnedR0(1)
            );
            expected.extend_from_slice(&wav(value, 64)[44..]);
        }
        assert_eq!(kernel.thread_last_errors[&0], 0x1234);
        wide(&mut cpu, 0x1000, "SystemAsterisk");
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "PlaySoundW",
                &[0x1000, 0, SND_ALIAS | SND_ASYNC]
            ),
            DispatchOutcome::ReturnedR0(1)
        );
        expected.extend_from_slice(&wav(1000, 64)[44..]);
        // An integer alias is not a guest pointer, including through the ANSI API.
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "PlaySoundA",
                &[0x3f53, 0, SND_ALIAS_ID | SND_ASYNC]
            ),
            DispatchOutcome::ReturnedR0(1)
        );
        expected.extend_from_slice(&wav(5000, 64)[44..]);
        wide(&mut cpu, 0x1000, "SystemExclamation");
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "sndPlaySoundW",
                &[0x1000, SND_ALIAS | SND_ASYNC, SND_RESOURCE]
            ),
            DispatchOutcome::ReturnedR0(1)
        );
        expected.extend_from_slice(&wav(4000, 64)[44..]);
        kernel.registry.set_value(
            r"HKCU\AppEvents\Schemes\Apps\.Default\Alarm\.Current",
            "",
            RegistryValue::Sz(r"\Windows\question.wav".into()),
        );
        cpu.write_mem(0x1000, b"Alarm\0").unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "sndPlaySoundA",
                &[0x1000, SND_ALIAS | SND_ASYNC]
            ),
            DispatchOutcome::ReturnedR0(1)
        );
        expected.extend_from_slice(&wav(5000, 64)[44..]);
        assert_eq!(pcm(&capture), expected);
    }

    #[test]
    fn nodefault_empty_scheme_slots_and_muted_events_remain_silent() {
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        let root = tempfile::tempdir().unwrap();
        let capture = mount_sounds(&mut kernel, root.path());
        wide(&mut cpu, 0x1000, "UnknownEvent");
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "PlaySoundW",
                &[0x1000, 0, SND_ALIAS | SND_ASYNC | SND_NODEFAULT]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert!(pcm(&capture).is_empty());
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "PlaySoundW",
                &[0x1000, 0, SND_ALIAS | SND_ASYNC]
            ),
            DispatchOutcome::ReturnedR0(1)
        );
        let mut expected = wav(3000, 64)[44..].to_vec();
        wide(&mut cpu, 0x1000, "MenuPopup");
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "PlaySoundW",
                &[0x1000, 0, SND_ALIAS | SND_ASYNC]
            ),
            DispatchOutcome::ReturnedR0(1)
        );
        assert_eq!(
            pcm(&capture),
            expected,
            "the default scheme's empty slot is intentional"
        );
        kernel.registry.set_value(
            settings::EVENT_KEY,
            ".Scheme",
            RegistryValue::Sz(".AllSounds".into()),
        );
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "PlaySoundW",
                &[0x1000, 0, SND_ALIAS | SND_ASYNC]
            ),
            DispatchOutcome::ReturnedR0(1)
        );
        expected.extend_from_slice(&wav(7000, 64)[44..]);
        kernel.registry.set_value(
            settings::EVENT_KEY,
            ".Scheme",
            RegistryValue::Sz(".NoSounds".into()),
        );
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "MessageBeep",
                &[0x20]
            ),
            DispatchOutcome::ReturnedR0(1)
        );
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "PlaySoundW",
                &[0x1000, 0, SND_ALIAS | SND_ASYNC]
            ),
            DispatchOutcome::ReturnedR0(1)
        );
        kernel.registry.set_value(
            settings::EVENT_KEY,
            ".Scheme",
            RegistryValue::Sz(".DefaultSounds".into()),
        );
        kernel
            .registry
            .set_value(settings::VOLUME_KEY, "Mute", RegistryValue::Dword(3));
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "MessageBeep",
                &[0x30]
            ),
            DispatchOutcome::ReturnedR0(1)
        );
        assert_eq!(pcm(&capture), expected);
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "MessageBeep",
                &[0x50]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(kernel.thread_last_errors[&0], 87);
    }

    #[test]
    fn an_empty_wave_file_is_invalid_and_can_use_the_default_sound() {
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        let root = tempfile::tempdir().unwrap();
        let capture = mount_sounds(&mut kernel, root.path());
        std::fs::write(root.path().join("Windows").join("empty.wav"), []).unwrap();
        wide(&mut cpu, 0x1000, r"\Windows\empty.wav");
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "PlaySoundW",
                &[0x1000, 0, SND_FILENAME | SND_ASYNC | SND_NODEFAULT]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert!(pcm(&capture).is_empty());
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "PlaySoundW",
                &[0x1000, 0, SND_FILENAME | SND_ASYNC]
            ),
            DispatchOutcome::ReturnedR0(1)
        );
        assert_eq!(pcm(&capture), wav(3000, 64)[44..]);
    }

    #[test]
    fn play_sound_loop_stop_and_wave_device_close_preserve_other_transports() {
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        cpu.write_mem(0x1000, &wav(1234, 64)).unwrap();
        let format = pocket_kernel::audio::GuestFormat {
            sample_rate: 8000,
            channels: 1,
            bits_per_sample: 16,
        };
        kernel.audio.open_wave_stream(99, format);
        kernel.audio.push_wave_samples(99, &[42; 8000]);
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "PlaySoundW",
                &[0x1000, 0, SND_MEMORY | SND_LOOP | SND_ASYNC]
            ),
            DispatchOutcome::ReturnedR0(1)
        );
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "PlaySoundW",
                &[0x1000, 0, SND_MEMORY | SND_NOSTOP | SND_ASYNC]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(kernel.audio.wave_written_samples(99), 8000);
        let loop_length = kernel.audio.wave_written_samples(PLAY_SOUND_STREAM);
        assert!(loop_length > 64);
        kernel.audio.flush_wave_out();
        assert_eq!(kernel.audio.wave_written_samples(99), 0);
        assert_eq!(
            kernel.audio.wave_written_samples(PLAY_SOUND_STREAM),
            loop_length
        );
        kernel.audio.open_wave_stream(99, format);
        kernel.audio.push_wave_samples(99, &[42; 8000]);
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "sndPlaySoundW",
                &[0, SND_NODEFAULT]
            ),
            DispatchOutcome::ReturnedR0(1)
        );
        assert_eq!(kernel.audio.wave_written_samples(PLAY_SOUND_STREAM), 0);
        assert_eq!(kernel.audio.wave_written_samples(99), 8000);
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "PlaySoundW",
                &[0x1000, 0, SND_MEMORY | SND_LOOP]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
    }

    #[test]
    fn synchronous_sound_yields_without_replaying_and_restores_its_cancelled_caller() {
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        let root = tempfile::tempdir().unwrap();
        let capture = root.path().join("submitted.wav");
        kernel.audio.capture_to(&capture).unwrap();
        cpu.write_mem(0x1000, &wav(1234, 8000)).unwrap();
        cpu.write_reg(ArmReg::Lr, 0x12345678).unwrap();
        let args = [0x1000, 0, SND_MEMORY, 0xabcdef];
        assert!(matches!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "PlaySoundW",
                &args
            ),
            DispatchOutcome::JumpTo(_)
        ));
        assert!(matches!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "PlaySoundW",
                &args
            ),
            DispatchOutcome::JumpTo(_)
        ));
        assert_eq!(
            pcm(&capture).len(),
            16000,
            "thunk retries cannot submit the sound again"
        );
        let wait = &kernel.ce.sounds.waits[0];
        let key = (wait.thread, wait.thunk, wait.frame.sp);
        kernel.wait_deadlines.insert(key, u64::MAX);
        kernel.current_thread = 1;
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "coredll.dll",
            "PlaySoundW",
            &[0, 0, 0],
        );
        assert!(!kernel.wait_deadlines.contains_key(&key));
        kernel.current_thread = 0;
        // Completion also retires a scheduler deadline left by an earlier park.
        kernel.wait_deadlines.insert(key, u64::MAX);
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "PlaySoundW",
                &args
            ),
            DispatchOutcome::ReturnedR0(1)
        );
        assert!(kernel.ce.sounds.waits.is_empty());
        assert!(!kernel.wait_deadlines.contains_key(&key));
        assert_eq!(cpu.read_reg(ArmReg::Lr).unwrap(), 0x12345678);
        assert_eq!(cpu.read_reg(ArmReg::Sp).unwrap(), 0x4000);
        assert_eq!(cpu.read_reg(ArmReg::R3).unwrap(), 0xabcdef);
    }

    #[test]
    fn finished_workers_do_not_retire_the_main_threads_sound_wait() {
        let (_, mut kernel, _) = setup();
        let mut worker = pocket_kernel::GuestThread::new(
            0x11000, 0, 0x50000, 0x1000, 0xf000fe00, 0x11000, 0xdead7c00, [0; 17],
        );
        worker.finished = true;
        worker.exit_code = Some(0);
        kernel.threads.push(worker);
        for (thread, sp) in [(0, 0x4000), (1, 0x5000)] {
            let wait = SoundWait {
                thread,
                thunk: 0xf0001000,
                frame: GuestCallFrame {
                    args: [0; 4],
                    sp,
                    lr: 0x12345678,
                },
                generation: 1,
                samples: 8000,
            };
            kernel
                .wait_deadlines
                .insert((thread, wait.thunk, sp), u64::MAX);
            kernel.ce.sounds.waits.push(wait);
        }
        service_pending(&mut kernel);
        assert_eq!(kernel.ce.sounds.waits.len(), 1);
        assert_eq!(kernel.ce.sounds.waits[0].thread, 0);
        assert!(kernel.wait_deadlines.contains_key(&(0, 0xf0001000, 0x4000)));
        assert!(!kernel.wait_deadlines.contains_key(&(1, 0xf0001000, 0x5000)));
        kernel.main_thread.exit_code = Some(0);
        service_pending(&mut kernel);
        assert!(kernel.ce.sounds.waits.is_empty());
        assert!(kernel.wait_deadlines.is_empty());
    }

    #[test]
    fn application_mute_is_independent_of_system_events_and_live_volume_changes() {
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        let root = tempfile::tempdir().unwrap();
        mount_sounds(&mut kernel, root.path());
        kernel
            .registry
            .set_value(settings::VOLUME_KEY, "Mute", RegistryValue::Dword(5));
        wide(&mut cpu, 0x1000, "SystemAsterisk");
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "coredll.dll",
            "PlaySoundW",
            &[0x1000, 0, SND_ALIAS | SND_ASYNC],
        );
        assert_eq!(kernel.audio.wave_volume(PLAY_SOUND_STREAM), Some(u32::MAX));
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "coredll.dll",
            "MessageBeep",
            &[0x30],
        );
        assert_eq!(kernel.audio.wave_volume(EVENT_SOUND_STREAM), Some(u32::MAX));
        wide(&mut cpu, 0x1000, r"\Windows\default.wav");
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "coredll.dll",
            "PlaySoundW",
            &[0x1000, 0, SND_FILENAME | SND_ASYNC],
        );
        assert_eq!(kernel.audio.wave_volume(PLAY_SOUND_STREAM), Some(0));
        kernel
            .registry
            .set_value(settings::VOLUME_KEY, "Mute", RegistryValue::Dword(2));
        kernel.registry.set_value(
            settings::VOLUME_KEY,
            "Volume",
            RegistryValue::Dword(0x80008000),
        );
        kernel.sync_ce_settings();
        assert_eq!(
            kernel.audio.wave_volume(PLAY_SOUND_STREAM),
            Some(0x80008000)
        );
        assert_eq!(kernel.audio.wave_volume(EVENT_SOUND_STREAM), Some(0));
        kernel.audio.open_wave_stream(
            99,
            pocket_kernel::audio::GuestFormat {
                sample_rate: 8000,
                channels: 1,
                bits_per_sample: 16,
            },
        );
        kernel.audio.set_wave_volume(99, 0x12345678);
        for mute in [6, 7, 3] {
            kernel
                .registry
                .set_value(settings::VOLUME_KEY, "Mute", RegistryValue::Dword(mute));
            kernel.sync_ce_settings();
            assert_eq!(kernel.audio.wave_volume(99), Some(0x12345678));
        }
    }

    #[test]
    fn control_panel_preview_and_dropdown_keys_leave_live_settings_unchanged() {
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        let root = tempfile::tempdir().unwrap();
        let capture = mount_sounds(&mut kernel, root.path());
        kernel.registry.set_value(
            settings::VOLUME_KEY,
            "Volume",
            RegistryValue::Dword(u32::MAX),
        );
        kernel
            .registry
            .set_value(settings::VOLUME_KEY, "Mute", RegistryValue::Dword(2));
        kernel.registry.set_value(
            settings::EVENT_KEY,
            ".Scheme",
            RegistryValue::Sz(".NoSounds".into()),
        );
        let (_, mut observer, _) = setup();
        let (child, _) = kernel.object_handles.new_child().unwrap();
        observer.attach_handle_context(kernel.child_handle_context(child).unwrap());
        let args = [0, 5, 5, 0];
        assert!(matches!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "cplmain.cpl",
                "CPlApplet",
                &args
            ),
            DispatchOutcome::JumpTo(_)
        ));
        // Open the restored Sounds page before interacting with its controls.
        kernel.pending_input.extend([
            InputEvent::KeyDown { vk: 0x11 },
            InputEvent::KeyDown { vk: 0x09 },
            InputEvent::KeyUp { vk: 0x09 },
            InputEvent::KeyUp { vk: 0x11 },
        ]);
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "cplmain.cpl",
            "CPlApplet",
            &args,
        );
        assert_eq!(kernel.ce.control_panel.as_ref().unwrap().selected_page, 1);
        let state = kernel.ce.control_panel.as_ref().unwrap();
        let hwnd = state.hwnd;
        let fields = state.fields.clone();
        let all_sounds = state
            .sound_schemes
            .iter()
            .position(|name| name == ".AllSounds")
            .unwrap();
        kernel.controls.get_mut(fields[0]).unwrap().text = "25".into();
        kernel.controls.get_mut(fields[1]).unwrap().checked = false;
        kernel.controls.get_mut(fields[2]).unwrap().checked = true;
        kernel.controls.get_mut(fields[3]).unwrap().list_selected = Some(all_sounds);
        for vk in [0x1b, 0x0d] {
            kernel.controls.focus = fields[3];
            kernel.controls.get_mut(fields[3]).unwrap().choice_open = true;
            kernel.pending_input.push_back(InputEvent::KeyDown { vk });
            assert!(matches!(
                call(
                    &mut cpu,
                    &mut kernel,
                    &mut dispatcher,
                    "cplmain.cpl",
                    "CPlApplet",
                    &args
                ),
                DispatchOutcome::JumpTo(_)
            ));
            assert!(kernel.ce.control_panel.is_some());
            assert!(!kernel.controls.get(fields[3]).unwrap().choice_open);
        }
        kernel.controls.focus = kernel
            .controls
            .children_of(hwnd)
            .into_iter()
            .find(|h| kernel.controls.get(*h).is_some_and(|c| c.id == 4))
            .unwrap();
        kernel
            .pending_input
            .push_back(InputEvent::KeyDown { vk: 0x20 });
        assert!(matches!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "cplmain.cpl",
                "CPlApplet",
                &args
            ),
            DispatchOutcome::JumpTo(_)
        ));
        assert_eq!(pcm(&capture), wav(3000, 64)[44..]);
        assert_eq!(
            kernel.audio.wave_volume(EVENT_SOUND_STREAM),
            Some(0x3fff3fff)
        );
        kernel
            .pending_input
            .push_back(InputEvent::KeyDown { vk: 0x1b });
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "cplmain.cpl",
                "CPlApplet",
                &args
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert!(kernel.ce.control_panel.is_none());
        for registry in [&kernel.registry, &observer.registry] {
            assert_eq!(
                registry.value(settings::VOLUME_KEY, "Volume"),
                Some(RegistryValue::Dword(u32::MAX))
            );
            assert_eq!(
                registry.value(settings::VOLUME_KEY, "Mute"),
                Some(RegistryValue::Dword(2))
            );
            assert_eq!(
                registry.value(settings::EVENT_KEY, ".Scheme"),
                Some(RegistryValue::Sz(".NoSounds".into()))
            );
        }
    }

    #[test]
    fn startup_window_transitions_and_modal_retries_emit_each_event_once() {
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        let root = tempfile::tempdir().unwrap();
        let capture = mount_sounds(&mut kernel, root.path());
        kernel.registry.set_value(
            settings::EVENT_KEY,
            ".Scheme",
            RegistryValue::Sz(".AllSounds".into()),
        );
        kernel.ce.sounds.startup_pending = true;
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "coredll.dll",
            "GetTickCount",
            &[],
        );
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "coredll.dll",
            "GetTickCount",
            &[],
        );
        let mut expected = wav(6000, 64)[44..].to_vec();
        kernel.ce.windows.enabled = true;
        let hwnd = 0xdead1080;
        kernel.window_classes.insert(hwnd, "TestWindow".into());
        kernel.window_styles.insert(hwnd, (0x80c00000, 0));
        kernel.window_rects.insert(hwnd, (10, 10, 180, 120));
        kernel.window_visibility.insert(hwnd, false);
        for _ in 0..2 {
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "ShowWindow",
                &[hwnd, 1],
            );
        }
        expected.extend_from_slice(&wav(9000, 64)[44..]);
        classic_window::show_state(&mut kernel, hwnd, 0xf020);
        classic_window::show_state(&mut kernel, hwnd, 0xf020);
        expected.extend_from_slice(&wav(11000, 64)[44..]);
        classic_window::show_state(&mut kernel, hwnd, 0xf120);
        expected.extend_from_slice(&wav(12000, 64)[44..]);
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "coredll.dll",
            "DestroyWindow",
            &[hwnd],
        );
        expected.extend_from_slice(&wav(10000, 64)[44..]);
        wide(&mut cpu, 0x1100, "Warning");
        for _ in 0..2 {
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "MessageBoxW",
                &[0, 0x1100, 0, 0x30],
            );
        }
        expected.extend_from_slice(&wav(4000, 64)[44..]);
        assert_eq!(pcm(&capture), expected);
    }
}
