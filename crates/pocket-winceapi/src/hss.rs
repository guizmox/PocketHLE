//! Hekkus Sound System (`hss.dll`).
//!
//! HSS is a freeware C++ audio mixer bundled with a great many Pocket PC
//! games. Unlike `waveOut`, where the guest hands us PCM it has already
//! decoded, HSS games hand us a filename, memory buffer or PE resource and
//! expect the library to do the decoding. This module handles WAV effects
//! and Protracker modules for music
//! (see [`pocket_kernel::tracker`]).
//!
//! The API is C++ methods, so every handler takes `this` in `r0`. The
//! clip objects are opaque; returned channel objects expose the guest ABI
//! method table, while playback state is kept host-side
//! in [`pocket_kernel::HssState`], keyed by that `this` pointer.
//!
//! `waveOut` remains the authoritative PCM transport for the games that
//! use it; the two paths are independent and mix together.

use std::sync::Arc;

use pocket_kernel::{
    tracker::Module, DispatchOutcome, GuestFormat, HssClip, KernelError, VoiceParams,
    HSS_MUSIC_GROUP, HSS_SOUND_GROUP,
};

use crate::{CallCtx, WinCeDispatcher};

pub fn register(d: &mut WinCeDispatcher) {
    let dll = "hss.dll";
    let identity_stubs = [
        "??0hssSound@@QAA@XZ",
        "??0hssMusic@@QAA@XZ",
        "??0hssSpeaker@@QAA@XZ",
        "??1hssSound@@UAA@XZ",
        "??1hssMusic@@UAA@XZ",
    ];
    for f in identity_stubs {
        d.register_handler(dll, f, this_returning);
    }

    // `load` takes the filename as `const wchar_t*` in the overload
    // JumpyBall imports (`PBG`) and as `void*, bool` in the one that
    // reads from memory (`PAX_N`). Both end up in the same handler; it
    // tells them apart by looking at what `r1` points to.
    d.register_handler(dll, "?load@hssSound@@QAAHPBG@Z", load_sound);
    d.register_handler(dll, "?load@hssSound@@QAAHPAX_N@Z", load_sound);
    d.register_handler(dll, "?load@hssMusic@@QAAHPBG@Z", load_music);
    d.register_handler(dll, "?load@hssMusic@@QAAHPAX_N@Z", load_music);
    d.register_handler(dll, "?load@hssSound@@QAAHPAUHINSTANCE__@@K@Z", load_sound);
    d.register_handler(dll, "?load@hssMusic@@QAAHPAUHINSTANCE__@@K@Z", load_music);
    d.register_handler(dll, "?suspend@hssSpeaker@@QAAXXZ", suspend_speaker);
    d.register_handler(dll, "?resume@hssSpeaker@@QAAXXZ", resume_speaker);
    d.register_handler(dll, "?close@hssSpeaker@@QAAHXZ", close_speaker);
    d.register_handler(dll, "??1hssSpeaker@@UAA@XZ", close_speaker);

    d.register_handler(dll, "?loop@hssSound@@QAAX_N@Z", set_loop);
    d.register_handler(dll, "?loop@hssMusic@@QAAX_N@Z", set_loop);
    d.register_handler(dll, "?volume@hssSound@@QAAXI@Z", set_clip_volume);
    d.register_handler(dll, "?volume@hssMusic@@QAAXI@Z", set_clip_volume);
    d.register_handler(dll, "?volume@hssSound@@QAAIXZ", get_clip_volume);
    d.register_handler(dll, "?volume@hssMusic@@QAAIXZ", get_clip_volume);

    d.register_handler(dll, "?open@hssSpeaker@@QAAHII_NII@Z", open_speaker);
    d.register_handler(
        dll,
        "?playSound@hssSpeaker@@QAAHPAVhssSound@@I@Z",
        play_sound,
    );
    d.register_handler(
        dll,
        "?playMusic@hssSpeaker@@QAAHPAVhssMusic@@I@Z",
        play_music,
    );
    d.register_handler(dll, "?stopSounds@hssSpeaker@@QAAXXZ", stop_sounds);
    d.register_handler(dll, "?stopMusics@hssSpeaker@@QAAXXZ", stop_musics);

    // The master volumes come in setter/getter pairs. JumpyBall imports
    // both halves of both pairs, and reads back what it wrote.
    d.register_handler(dll, "?volumeSounds@hssSpeaker@@QAAXI@Z", set_sound_volume);
    d.register_handler(dll, "?volumeMusics@hssSpeaker@@QAAXI@Z", set_music_volume);
    d.register_handler(dll, "?volumeSounds@hssSpeaker@@QAAIXZ", get_sound_volume);
    d.register_handler(dll, "?volumeMusics@hssSpeaker@@QAAIXZ", get_music_volume);

    // Pause/unpause map onto the engine's global pause; there is no
    // per-group pause in the mixer and no game we target needs one.
    d.register_handler(dll, "?pauseSounds@hssSpeaker@@QAAXXZ", pause);
    d.register_handler(dll, "?pauseMusics@hssSpeaker@@QAAXXZ", pause);
    d.register_handler(dll, "?unpauseSounds@hssSpeaker@@QAAXXZ", unpause);
    d.register_handler(dll, "?unpauseMusics@hssSpeaker@@QAAXXZ", unpause);

    // Remaining surface with no state behind it.
    let success_stubs = ["?bufferLength@hssSpeaker@@QAAHH@Z"];
    for f in success_stubs {
        d.register_handler(dll, f, ok);
    }
    d.register_handler(dll, "?channel@hssSpeaker@@QAAPAVhssChannel@@H@Z", channel);
    for name in CHANNEL_METHODS {
        d.register_handler(dll, name, channel_method);
    }
}

const CHANNEL_METHODS: [&str; 16] = [
    "?volume@hssChannel@@UAAXI@Z",
    "?pan@hssChannel@@UAAXI@Z",
    "?position@hssChannel@@UAAIXZ",
    "?position@hssChannel@@UAAXI@Z",
    "?length@hssChannel@@UAAIXZ",
    "?volume@hssChannel@@QAAIXZ",
    "?pan@hssChannel@@QAAIXZ",
    "?frequency@hssChannel@@QAAXI@Z",
    "?frequency@hssChannel@@QAAIXZ",
    "?playing@hssChannel@@QAA_NXZ",
    "?stop@hssChannel@@QAAXXZ",
    "?pause@hssChannel@@QAAXXZ",
    "?resume@hssChannel@@QAAXXZ",
    "?paused@hssChannel@@QAA_NXZ",
    "?loop@hssChannel@@QAAX_N@Z",
    "?loop@hssChannel@@QAA_NXZ",
];

fn channel(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let speaker = ctx.arg_u32(0)?;
    let index = ctx.arg_u32(1)?;
    let Some(state) = ctx.kernel.hss.channels.get_mut(&(speaker, index)) else {
        return Ok(DispatchOutcome::ReturnedR0(0));
    };
    if state.pointer != 0 {
        return Ok(DispatchOutcome::ReturnedR0(state.pointer));
    }
    let pointer = ctx
        .kernel
        .heap
        .alloc(24)
        .ok_or_else(|| KernelError::Loader("HSS channel allocation failed".into()))?;
    let table = pointer + 4;
    ctx.cpu.write_mem(pointer, &table.to_le_bytes())?;
    for (i, name) in CHANNEL_METHODS[..5].iter().enumerate() {
        let address = ctx
            .kernel
            .dynamic_exports
            .get(&pocket_kernel::HSS_MODULE_HANDLE)
            .and_then(|exports| exports.get(*name))
            .copied()
            .ok_or_else(|| KernelError::Loader(format!("HSS missing method {name}")))?;
        ctx.cpu
            .write_mem(table + i as u32 * 4, &address.to_le_bytes())?;
    }
    ctx.kernel
        .hss
        .channels
        .get_mut(&(speaker, index))
        .unwrap()
        .pointer = pointer;
    Ok(DispatchOutcome::ReturnedR0(pointer))
}

fn channel_method(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let pointer = ctx.arg_u32(0)?;
    let arg = ctx.arg_u32(1)?;
    let name = ctx
        .thunk
        .friendly_name
        .as_deref()
        .or_else(|| match &ctx.thunk.binding {
            pocket_pe::ImportBinding::Name(n) => Some(n.as_str()),
            _ => None,
        })
        .unwrap_or_default();
    let Some(state) = ctx
        .kernel
        .hss
        .channels
        .values_mut()
        .find(|c| c.pointer == pointer && pointer != 0)
    else {
        return Ok(DispatchOutcome::ReturnedR0(0));
    };
    let mut result = 0;
    if name.contains("?volume@") {
        if name.contains("I@Z") {
            state.volume = arg.min(64);
        } else {
            result = state.volume;
        }
    } else if name.contains("?pan@") {
        if name.contains("I@Z") {
            state.pan = arg.min(255);
        } else {
            result = state.pan;
        }
    } else if name.contains("?frequency@") {
        if name.contains("I@Z") {
            state.offset = state.position();
            state.started = std::time::Instant::now();
            state.rate = arg.max(1);
        } else {
            result = state.rate;
        }
    } else if name.contains("?position@") {
        if name.contains("I@Z") {
            state.offset = u64::from(arg).min(state.frames);
            state.started = std::time::Instant::now();
        } else {
            result = state.position() as u32;
        }
    } else if name.contains("?length@") {
        result = state.frames as u32;
    } else if name.contains("?playing@") {
        result = u32::from(state.active());
    } else if name.contains("?paused@") {
        result = u32::from(state.paused);
    } else if name.contains("?loop@") {
        if name.contains("_N@Z") {
            state.looped = arg != 0;
        } else {
            result = u32::from(state.looped);
        }
    } else if name.contains("?pause@") {
        state.offset = state.position();
        state.paused = true;
    } else if name.contains("?resume@") {
        state.started = std::time::Instant::now();
        state.paused = false;
    } else if name.contains("?stop@") {
        state.stopped = true;
        ctx.kernel.audio.stop_voice_group(state.group);
    }
    ctx.kernel.audio.configure_voice_group(
        state.group,
        state.volume as f32 / 64.0 * state.master,
        state.rate,
        state.pan as f32 / 255.0,
        if name.contains("?position@") && name.contains("I@Z") {
            Some(state.offset)
        } else {
            None
        },
        state.looped,
    );
    ctx.kernel
        .audio
        .pause_voice_group(state.group, state.paused || state.suspended);
    Ok(DispatchOutcome::ReturnedR0(result))
}

/// HSS volumes run 0..=64, like the tracker scale they came from.
const HSS_MAX_VOLUME: u32 = 64;

/// `int hssSound::load(const wchar_t* path)` and the `void*, bool`
/// overload. Native HSS returns zero on success and negative error codes;
/// playback returns a channel index (also possibly zero), not a boolean.
fn load_sound(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    load_clip(ctx, "hssSound")
}

/// `int hssMusic::load(const wchar_t* path)`.
fn load_music(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    load_clip(ctx, "hssMusic")
}

fn load_clip(ctx: &mut CallCtx<'_>, kind: &str) -> Result<DispatchOutcome, KernelError> {
    let this = ctx.arg_u32(0)?;
    let arg = ctx.arg_u32(1)?;
    if this == 0 {
        return Ok(DispatchOutcome::ReturnedR0((-10i32) as u32));
    }
    // Native loads release the previous clip even when replacement fails.
    ctx.kernel.hss.clips.remove(&this);
    let resource = matches!(&ctx.thunk.binding, pocket_pe::ImportBinding::Name(name) if name.contains("PAUHINSTANCE__"));
    if arg == 0 && !resource {
        return Ok(DispatchOutcome::ReturnedR0((-10i32) as u32));
    }
    let memory = matches!(&ctx.thunk.binding, pocket_pe::ImportBinding::Name(name) if name.contains("PAX_N"));
    let path = if resource {
        format!("resource:{arg:08x}:{}:{}", kind, ctx.arg_u32(2)? & 0xffff)
    } else if memory {
        format!("memory:{arg:08x}")
    } else {
        crate::coredll::read_guest_wstr_lossy(ctx, arg, 260)?
    };
    // Guest memory may be freed/reused after load; never cache by its address.
    let cached = if memory || resource {
        None
    } else {
        ctx.kernel.hss.decoded.get(&path).cloned()
    };
    let (format, samples) = match cached {
        Some(hit) => hit,
        None => {
            let bytes = if resource {
                read_resource_clip(ctx, arg, kind)?
            } else if memory {
                read_memory_clip(ctx, arg)?
            } else {
                crate::coredll::read_guest_file_for(ctx, &path)
            };
            let Some(bytes) = bytes else {
                log::debug!("{kind}::load({path:?}): no readable WAV/MOD container");
                return Ok(DispatchOutcome::ReturnedR0((-200i32) as u32));
            };
            let Some((format, samples)) = decode_clip(&bytes) else {
                log::debug!(
                    "{kind}::load({path:?}): unsupported/corrupt audio; header={:02x?}",
                    &bytes[..bytes.len().min(48)]
                );
                return Ok(DispatchOutcome::ReturnedR0((-100i32) as u32));
            };
            let entry = (format, Arc::new(samples));
            if !memory && !resource {
                ctx.kernel.hss.decoded.insert(path.clone(), entry.clone());
            }
            entry
        }
    };

    log::debug!(
        "{kind}::load({path:?}) -> {} samples @ {} Hz x{}",
        samples.len(),
        format.sample_rate,
        format.channels
    );
    ctx.kernel.hss.clips.insert(
        this,
        HssClip {
            samples,
            format,
            looped: false,
            volume: HSS_MAX_VOLUME,
            path,
        },
    );
    Ok(DispatchOutcome::ReturnedR0(0))
}

/// Match the native resource overload: WAVE effects, PTMOD music and a
/// low-word resource ID, within the supplied module (never another DLL).
fn read_resource_clip(
    ctx: &mut CallCtx<'_>,
    module: u32,
    kind: &str,
) -> Result<Option<Vec<u8>>, KernelError> {
    use pocket_pe::resources::ResourceKey;
    if module != 0
        && module != pocket_kernel::PROCESS_INSTANCE_HANDLE
        && module != ctx.kernel.image_base
        && ctx.kernel.module_by_handle(module).is_none()
    {
        return Ok(None);
    }
    let id = ctx.arg_u32(2)? & 0xffff;
    let ty = if kind == "hssMusic" { "PTMOD" } else { "WAVE" };
    let (entries, base) = ctx.kernel.resource_scope(module);
    let Some(entry) = entries.iter().find(|e| {
        e.name == ResourceKey::Id(id)
            && matches!(&e.ty, ResourceKey::Name(name) if name.eq_ignore_ascii_case(ty))
    }) else {
        return Ok(None);
    };
    if entry.size == 0 || entry.size > 32 * 1024 * 1024 {
        return Ok(None);
    }
    let Some(address) = base.checked_add(entry.data_rva) else {
        return Ok(None);
    };
    ctx.cpu
        .check_guest_access(address, entry.size, pocket_cpu::Prot::READ)?;
    Ok(Some(ctx.cpu.read_mem(address, entry.size)?))
}

/// Memory overloads contain the same self-describing WAV/MOD bytes as files.
fn read_memory_clip(ctx: &mut CallCtx<'_>, address: u32) -> Result<Option<Vec<u8>>, KernelError> {
    let Ok(prefix) = ctx.cpu.read_mem(address, 12) else {
        return Ok(None);
    };
    let size = if &prefix[..4] == b"RIFF" && &prefix[8..12] == b"WAVE" {
        u32::from_le_bytes(prefix[4..8].try_into().unwrap()).checked_add(8)
    } else {
        let Ok(header) = ctx.cpu.read_mem(address, 1084) else {
            return Ok(None);
        };
        let channels = match &header[1080..1084] {
            b"M.K." | b"M!K!" | b"4CHN" | b"FLT4" => 4,
            b"6CHN" => 6,
            b"8CHN" | b"FLT8" => 8,
            _ => return Ok(None),
        };
        let song = header[950] as usize;
        if !(1..=128).contains(&song) {
            return Ok(None);
        }
        let patterns = header[952..952 + song].iter().copied().max().unwrap_or(0) as u32 + 1;
        let samples: u32 = (0..31)
            .map(|i| u16::from_be_bytes([header[42 + i * 30], header[43 + i * 30]]) as u32 * 2)
            .sum();
        Some(1084 + patterns * 64 * channels * 4 + samples)
    };
    let Some(size) = size.filter(|n| *n <= 32 * 1024 * 1024) else {
        return Ok(None);
    };
    if ctx
        .cpu
        .check_guest_access(address, size, pocket_cpu::Prot::READ)
        .is_err()
    {
        return Ok(None);
    }
    Ok(ctx.cpu.read_mem(address, size).ok())
}

/// Decode whatever HSS was handed. A `.tkm` is a renamed Protracker
/// module and a `.wav` is a RIFF file; the extension is not load-bearing
/// on a device and it is not load-bearing here either — both decoders
/// are tried against the content.
fn decode_clip(bytes: &[u8]) -> Option<(GuestFormat, Vec<i16>)> {
    if let Some(decoded) = crate::coredll::decode_pcm_wave(bytes) {
        return Some(decoded);
    }
    let module = Module::parse(bytes)?;
    // Modules are rendered at the mixer rate the games ask for. 22050
    // is what JumpyBall's `hssSpeaker::open` requests, and rendering
    // once at load time keeps the cost off the audio callback.
    const MODULE_RATE: u32 = 22_050;
    // Long enough that every module we have seen reaches its own end
    // and the renderer stops on the order table rather than the clock.
    // Cutting a song short would put the loop seam mid-phrase, which is
    // far more audible than the memory costs: JumpyBall's longest track
    // is 243 s, or about 21 MB of `i16`, rendered once at load.
    const MODULE_SECONDS: u32 = 360;
    let samples = module.render(MODULE_RATE, MODULE_SECONDS);
    if samples.is_empty() {
        return None;
    }
    log::debug!(
        "hss: rendered module {:?} -> {:.1}s",
        module.title,
        samples.len() as f32 / 2.0 / MODULE_RATE as f32
    );
    Some((
        GuestFormat {
            sample_rate: MODULE_RATE,
            channels: 2,
            bits_per_sample: 16,
        },
        samples,
    ))
}

/// `void hssSound::loop(bool)` / `void hssMusic::loop(bool)`.
fn set_loop(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let this = ctx.arg_u32(0)?;
    let looped = ctx.arg_u32(1)? != 0;
    if let Some(clip) = ctx.kernel.hss.clips.get_mut(&this) {
        clip.looped = looped;
    }
    Ok(DispatchOutcome::ReturnedR0(1))
}

/// `void hssSound::volume(UINT)` / `void hssMusic::volume(UINT)`.
fn set_clip_volume(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let this = ctx.arg_u32(0)?;
    let volume = ctx.arg_u32(1)?.min(HSS_MAX_VOLUME);
    if let Some(clip) = ctx.kernel.hss.clips.get_mut(&this) {
        clip.volume = volume;
    }
    Ok(DispatchOutcome::ReturnedR0(1))
}

/// `UINT hssSound::volume()` / `UINT hssMusic::volume()`.
fn get_clip_volume(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let this = ctx.arg_u32(0)?;
    let volume = ctx
        .kernel
        .hss
        .clips
        .get(&this)
        .map_or(HSS_MAX_VOLUME, |c| c.volume);
    Ok(DispatchOutcome::ReturnedR0(volume))
}

/// `int hssSpeaker::open(UINT rate, UINT bits, bool stereo, UINT musics, UINT sounds)`.
///
/// The guest picks the mixer format here; we adopt it so the engine
/// resamples everything to the rate the game expects to hear.
fn open_speaker(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let speaker = ctx.arg_u32(0)?;
    // Native HSS rejects a second open without replacing the existing mixer.
    if ctx.kernel.hss.speakers.contains_key(&speaker) {
        return Ok(DispatchOutcome::ReturnedR0((-2i32) as u32));
    }
    let rate = ctx.arg_u32(1)?;
    let bits = ctx.arg_u32(2)?;
    let stereo = ctx.arg_u32(3)? != 0;
    let format = GuestFormat {
        sample_rate: if rate == 0 { 22_050 } else { rate },
        channels: if stereo { 2 } else { 1 },
        bits_per_sample: if bits == 8 { 8 } else { 16 },
    };
    let speaker = ctx.arg_u32(0)?;
    // HSS orders its music channels first, then its sample/SFX channels.
    let music = ctx.arg_u32(4)?.clamp(1, 3);
    let sounds = ctx.arg_u32(5)?.clamp(8, 64);
    ctx.kernel.hss.speakers.insert(speaker, (sounds, music));
    for index in 0..sounds + music {
        let group = 0x4853_0000u32.wrapping_add(ctx.kernel.hss.channels.len() as u32);
        let channel = ctx
            .kernel
            .hss
            .channels
            .entry((speaker, index))
            .or_insert_with(|| pocket_kernel::HssChannel::new(group));
        channel.stopped = true;
        channel.music = index < music;
        ctx.kernel.audio.stop_voice_group(channel.group);
    }
    ctx.kernel.hss.format = format;
    ctx.kernel.hss.opened = true;
    ctx.kernel.audio.set_guest_format(format);
    ctx.kernel.audio.start();
    log::debug!(
        "hssSpeaker::open({} Hz, {} bit, {})",
        format.sample_rate,
        format.bits_per_sample,
        if stereo { "stereo" } else { "mono" }
    );
    Ok(DispatchOutcome::ReturnedR0(0))
}

/// `int hssSpeaker::playSound(hssSound*, UINT)`.
fn play_sound(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    play(ctx, HSS_SOUND_GROUP)
}

/// `int hssSpeaker::playMusic(hssMusic*, UINT)` uses the music channel pool.
fn play_music(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    play(ctx, HSS_MUSIC_GROUP)
}

fn play(ctx: &mut CallCtx<'_>, group: u32) -> Result<DispatchOutcome, KernelError> {
    let clip_this = ctx.arg_u32(1)?;
    let speaker = ctx.arg_u32(0)?;
    let requested = ctx.arg_u32(2)?;
    let Some((sounds, musics)) = ctx.kernel.hss.speakers.get(&speaker).copied() else {
        return Ok(DispatchOutcome::ReturnedR0((-1000i32) as u32));
    };
    let (start, count) = if group == HSS_MUSIC_GROUP {
        (0, musics)
    } else {
        (musics, sounds)
    };
    let selected = if requested & 0x1000_0000 != 0 {
        (start..start + count).find(|index| {
            ctx.kernel
                .hss
                .channels
                .get(&(speaker, *index))
                .is_none_or(|channel| !channel.active())
        })
    } else {
        let index = requested & 0xffff;
        (index < count).then_some(start + index)
    };
    let Some(index) = selected else {
        return Ok(DispatchOutcome::ReturnedR0((-1000i32) as u32));
    };
    let Some(clip) = ctx.kernel.hss.clips.get(&clip_this) else {
        // A `play` for something that never loaded. Report failure so a
        // guest that checks gets an honest answer.
        log::debug!("hss: play of unloaded clip 0x{clip_this:08x}");
        return Ok(DispatchOutcome::ReturnedR0((-1000i32) as u32));
    };

    // Two gains multiply on a device: the per-clip volume and the
    // speaker's master volume for that group.
    let master = if group == HSS_MUSIC_GROUP {
        ctx.kernel.hss.music_volume
    } else {
        ctx.kernel.hss.sound_volume
    };
    let volume =
        (clip.volume as f32 / HSS_MAX_VOLUME as f32) * (master as f32 / HSS_MAX_VOLUME as f32);

    let samples = Arc::clone(&clip.samples);
    let format = clip.format;
    let looped = clip.looped;
    let path = clip.path.clone();

    let voice_group = 0x4853_0000u32.wrapping_add(ctx.kernel.hss.channels.len() as u32);
    let channel = ctx
        .kernel
        .hss
        .channels
        .entry((speaker, index))
        .or_insert_with(|| pocket_kernel::HssChannel::new(voice_group));
    ctx.kernel.audio.stop_voice_group(channel.group);
    channel.format = format;
    channel.rate = format.sample_rate;
    channel.frames = samples.len() as u64 / u64::from(format.channels.max(1));
    channel.started = std::time::Instant::now();
    channel.offset = 0;
    channel.stopped = false;
    channel.paused = requested & 0x2000_0000 != 0;
    channel.suspended = ctx.kernel.hss.suspended_speakers.contains(&speaker);
    channel.looped = looped;
    channel.volume = clip.volume;
    channel.master = master as f32 / 64.0;
    channel.music = group == HSS_MUSIC_GROUP;
    let channel_group = channel.group;
    let paused = channel.paused || channel.suspended;
    ctx.kernel.audio.play_voice_with(
        &samples,
        format,
        VoiceParams {
            looped,
            group: channel_group,
            volume,
        },
    );
    ctx.kernel.audio.pause_voice_group(channel_group, paused);
    ctx.kernel.audio.start();
    log::debug!(
        "hss: play {path:?} ({} samples, loop={looped}, gain={volume:.2})",
        samples.len()
    );
    Ok(DispatchOutcome::ReturnedR0(index))
}

/// `void hssSpeaker::stopSounds()`.
fn stop_sounds(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    for channel in ctx.kernel.hss.channels.values_mut().filter(|c| !c.music) {
        channel.stopped = true;
        ctx.kernel.audio.stop_voice_group(channel.group);
    }
    ctx.kernel.audio.stop_voice_group(HSS_SOUND_GROUP);
    Ok(DispatchOutcome::ReturnedR0(1))
}

/// `void hssSpeaker::stopMusics()`. Must not touch the sound effects —
/// JumpyBall calls this on every level change while effects are still
/// playing.
fn stop_musics(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    for channel in ctx.kernel.hss.channels.values_mut().filter(|c| c.music) {
        channel.stopped = true;
        ctx.kernel.audio.stop_voice_group(channel.group);
    }
    ctx.kernel.audio.stop_voice_group(HSS_MUSIC_GROUP);
    Ok(DispatchOutcome::ReturnedR0(1))
}

/// `void hssSpeaker::volumeSounds(UINT)`.
fn set_sound_volume(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    ctx.kernel.hss.sound_volume = ctx.arg_u32(1)?.min(HSS_MAX_VOLUME);
    Ok(DispatchOutcome::ReturnedR0(1))
}

/// `void hssSpeaker::volumeMusics(UINT)`.
fn set_music_volume(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    ctx.kernel.hss.music_volume = ctx.arg_u32(1)?.min(HSS_MAX_VOLUME);
    Ok(DispatchOutcome::ReturnedR0(1))
}

/// `UINT hssSpeaker::volumeSounds()`.
fn get_sound_volume(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(ctx.kernel.hss.sound_volume))
}

/// `UINT hssSpeaker::volumeMusics()`.
fn get_music_volume(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(ctx.kernel.hss.music_volume))
}

fn suspend_speaker(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    set_speaker_suspended(ctx, true)
}
fn resume_speaker(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    set_speaker_suspended(ctx, false)
}
fn set_speaker_suspended(
    ctx: &mut CallCtx<'_>,
    suspended: bool,
) -> Result<DispatchOutcome, KernelError> {
    let speaker = ctx.arg_u32(0)?;
    if !ctx.kernel.hss.speakers.contains_key(&speaker) {
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    if suspended {
        ctx.kernel.hss.suspended_speakers.insert(speaker);
    } else {
        ctx.kernel.hss.suspended_speakers.remove(&speaker);
    }
    for ((owner, _), channel) in &mut ctx.kernel.hss.channels {
        if *owner != speaker || channel.suspended == suspended {
            continue;
        }
        channel.offset = channel.position();
        channel.started = std::time::Instant::now();
        channel.suspended = suspended;
        ctx.kernel
            .audio
            .pause_voice_group(channel.group, channel.paused || suspended);
    }
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn close_speaker(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let speaker = ctx.arg_u32(0)?;
    for ((owner, _), channel) in &mut ctx.kernel.hss.channels {
        if *owner == speaker {
            channel.stopped = true;
            channel.suspended = false;
            ctx.kernel.audio.stop_voice_group(channel.group);
        }
    }
    // Keep channel handles/group IDs reserved so another speaker cannot alias
    // a live voice group when this speaker closes and later reopens.
    ctx.kernel.hss.speakers.remove(&speaker);
    ctx.kernel.hss.suspended_speakers.remove(&speaker);
    ctx.kernel.hss.opened = !ctx.kernel.hss.speakers.is_empty();
    Ok(DispatchOutcome::ReturnedR0(0))
}

fn pause(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    ctx.kernel.audio.set_paused(true);
    Ok(DispatchOutcome::ReturnedR0(1))
}

fn unpause(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    ctx.kernel.audio.set_paused(false);
    Ok(DispatchOutcome::ReturnedR0(1))
}

fn ok(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let _ = ctx.arg_u32(0)?;
    let name = ctx
        .thunk
        .friendly_name
        .as_deref()
        .or(match &ctx.thunk.binding {
            pocket_pe::ImportBinding::Name(name) => Some(name.as_str()),
            pocket_pe::ImportBinding::Ordinal(_) => None,
        })
        .unwrap_or_default();
    let result = if name.contains("bufferLength@hssSpeaker") {
        4096
    } else {
        1
    };
    Ok(DispatchOutcome::ReturnedR0(result))
}

/// Constructor stub: returns `this` unchanged, which is the ARM C++ ABI
/// contract for a constructor.
fn this_returning(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(ctx.arg_u32(0)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_overloads_use_wave_or_ptmod_and_return_native_status_codes() {
        use crate::bluetooth::tests::{call, setup};
        use pocket_cpu::Cpu;
        use pocket_pe::resources::{ResourceEntry, ResourceKey};
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        let wave = tiny_wav();
        let module = tiny_module();
        kernel.image_base = 0x1000;
        cpu.write_mem(0x1800, &wave).unwrap();
        cpu.write_mem(0x2000, &module).unwrap();
        kernel.resources = vec![
            ResourceEntry {
                ty: ResourceKey::Name("WAVE".into()),
                name: ResourceKey::Id(7),
                data_rva: 0x800,
                size: wave.len() as u32,
                codepage: 0,
            },
            ResourceEntry {
                ty: ResourceKey::Name("PTMOD".into()),
                name: ResourceKey::Id(7),
                data_rva: 0x1000,
                size: module.len() as u32,
                codepage: 0,
            },
        ];
        let sound = "?load@hssSound@@QAAHPAUHINSTANCE__@@K@Z";
        let music = "?load@hssMusic@@QAAHPAUHINSTANCE__@@K@Z";
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "hss.dll",
                sound,
                &[0x9000, 0, 0x10007]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(*kernel.hss.clips[&0x9000].samples, vec![1000, -1000]);
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "hss.dll",
                music,
                &[0x9100, pocket_kernel::PROCESS_INSTANCE_HANDLE, 7]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert!(kernel.hss.clips[&0x9100].samples.iter().any(|s| *s != 0));
        assert!(
            kernel.hss.decoded.is_empty(),
            "resource addresses must not be cached like filenames"
        );
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "hss.dll",
                sound,
                &[0x9000, 0, 8]
            ),
            DispatchOutcome::ReturnedR0((-200i32) as u32)
        );
        assert!(
            !kernel.hss.clips.contains_key(&0x9000),
            "failed reload releases the old clip"
        );
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "hss.dll",
                sound,
                &[0x9000, 0xdead, 7]
            ),
            DispatchOutcome::ReturnedR0((-200i32) as u32)
        );
        // The same numeric ID in a satellite must not resolve to the EXE.
        let mut satellite_wave = wave.clone();
        satellite_wave[44..46].copy_from_slice(&2000i16.to_le_bytes());
        cpu.write_mem(0x3100, &satellite_wave).unwrap();
        kernel.modules.push(pocket_kernel::LoadedModule {
            dependencies: vec![],
            attached: false,
            satellites: vec![],
            resident_regions: vec![],
            handle: 0x3000,
            name: "audio-fixture.dll".into(),
            base: 0x3000,
            image_size: 0x1000,
            image_entry: 0,
            refcount: 1,
            resources: vec![ResourceEntry {
                ty: ResourceKey::Name("WAVE".into()),
                name: ResourceKey::Id(7),
                data_rva: 0x100,
                size: wave.len() as u32,
                codepage: 0,
            }],
        });
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "hss.dll",
                sound,
                &[0x9300, 0x3000, 7]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(kernel.hss.clips[&0x9300].samples[0], 2000);
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "hss.dll",
                music,
                &[0x9400, 0x3000, 7]
            ),
            DispatchOutcome::ReturnedR0((-200i32) as u32)
        );
        satellite_wave[44..46].copy_from_slice(&3000i16.to_le_bytes());
        cpu.write_mem(0x3100, &satellite_wave).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "hss.dll",
                sound,
                &[0x9300, 0x3000, 7]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(
            kernel.hss.clips[&0x9300].samples[0], 3000,
            "resource reload must read current bytes"
        );
        kernel.resources[0].size = 0x4000000;
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "hss.dll",
                sound,
                &[0x9000, 0, 7]
            ),
            DispatchOutcome::ReturnedR0((-200i32) as u32)
        );
    }

    #[test]
    fn speaker_reopen_suspend_resume_and_close_preserve_other_speakers_and_pauses() {
        use crate::bluetooth::tests::{call, setup};
        use pocket_cpu::Cpu;
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        let open = "?open@hssSpeaker@@QAAHII_NII@Z";
        for speaker in [0x2200, 0x2400] {
            assert_eq!(
                call(
                    &mut cpu,
                    &mut kernel,
                    &mut dispatcher,
                    "hss.dll",
                    open,
                    &[speaker, 22050, 16, 0, 1, 8]
                ),
                DispatchOutcome::ReturnedR0(0)
            );
        }
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "hss.dll",
                open,
                &[0x2200, 11025, 8, 1, 3, 64]
            ),
            DispatchOutcome::ReturnedR0((-2i32) as u32)
        );
        assert_eq!(kernel.hss.format.sample_rate, 22050);
        assert_eq!(kernel.hss.speakers[&0x2200], (8, 1));
        cpu.write_mem(0x1200, &tiny_wav()).unwrap();
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "hss.dll",
            "?load@hssSound@@QAAHPAX_N@Z",
            &[0x2300, 0x1200, 1],
        );
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "hss.dll",
            "?loop@hssSound@@QAAX_N@Z",
            &[0x2300, 1],
        );
        let play = "?playSound@hssSpeaker@@QAAHPAVhssSound@@I@Z";
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "hss.dll",
            play,
            &[0x2200, 0x2300, 0x20000000],
        );
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "hss.dll",
            play,
            &[0x2400, 0x2300, 0],
        );
        let tap = kernel.audio.tap();
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "hss.dll",
            "?suspend@hssSpeaker@@QAAXXZ",
            &[0x2200],
        );
        assert!(kernel.hss.channels[&(0x2200, 1)].suspended);
        assert!(!kernel.hss.channels[&(0x2400, 1)].suspended);
        let mut output = [0i16; 64];
        tap.drain_into(&mut output);
        assert!(
            output.iter().any(|s| *s != 0),
            "another speaker must remain audible"
        );
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "hss.dll",
            play,
            &[0x2200, 0x2300, 1],
        );
        assert!(kernel.hss.channels[&(0x2200, 2)].suspended);
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "hss.dll",
            "?resume@hssSpeaker@@QAAXXZ",
            &[0x2200],
        );
        assert!(
            kernel.hss.channels[&(0x2200, 1)].paused,
            "individual pause survives speaker resume"
        );
        assert!(!kernel.hss.channels[&(0x2200, 2)].suspended);
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "hss.dll",
            "?close@hssSpeaker@@QAAHXZ",
            &[0x2200],
        );
        assert!(kernel.hss.channels[&(0x2200, 2)].stopped);
        assert!(kernel.hss.channels[&(0x2400, 1)].active());
        assert!(!kernel.hss.speakers.contains_key(&0x2200));
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "hss.dll",
                play,
                &[0x2200, 0x2300, 0]
            ),
            DispatchOutcome::ReturnedR0((-1000i32) as u32)
        );
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "hss.dll",
                open,
                &[0x2200, 11025, 8, 0, 1, 8]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
    }

    /// A one-sample 22050 Hz mono PCM WAV.
    fn tiny_wav() -> Vec<u8> {
        let mut w = Vec::new();
        w.extend_from_slice(b"RIFF");
        w.extend_from_slice(&(36u32 + 4).to_le_bytes());
        w.extend_from_slice(b"WAVEfmt ");
        w.extend_from_slice(&16u32.to_le_bytes());
        w.extend_from_slice(&1u16.to_le_bytes()); // PCM
        w.extend_from_slice(&1u16.to_le_bytes()); // mono
        w.extend_from_slice(&22050u32.to_le_bytes());
        w.extend_from_slice(&44100u32.to_le_bytes());
        w.extend_from_slice(&2u16.to_le_bytes());
        w.extend_from_slice(&16u16.to_le_bytes());
        w.extend_from_slice(b"data");
        w.extend_from_slice(&4u32.to_le_bytes());
        w.extend_from_slice(&1000i16.to_le_bytes());
        w.extend_from_slice(&(-1000i16).to_le_bytes());
        w
    }

    /// A structurally valid one-note Protracker module.
    fn tiny_module() -> Vec<u8> {
        let order_off = 20 + 31 * 30;
        let magic_off = order_off + 2 + 128;
        let mut m = vec![0u8; magic_off + 4];
        m[0..4].copy_from_slice(b"tiny");
        m[20 + 22..20 + 24].copy_from_slice(&16u16.to_be_bytes());
        m[20 + 25] = 64;
        m[20 + 29] = 1;
        m[order_off] = 1;
        m[magic_off..magic_off + 4].copy_from_slice(b"M.K.");
        let mut pattern = vec![0u8; 64 * 4 * 4];
        pattern[0] = (428u16 >> 8) as u8;
        pattern[1] = (428u16 & 0xFF) as u8;
        pattern[2] = 0x10;
        m.extend_from_slice(&pattern);
        m.extend((0..32).map(|i| if i < 16 { 100u8 } else { 156u8 }));
        m
    }

    /// `load` is handed a filename with no reliable extension, so the
    /// decoder has to recognise both formats from their content.
    #[test]
    fn a_wav_and_a_module_both_decode() {
        let (fmt, samples) = decode_clip(&tiny_wav()).expect("WAV should decode");
        assert_eq!(fmt.sample_rate, 22050);
        assert_eq!(fmt.channels, 1);
        assert_eq!(samples, vec![1000, -1000]);

        let (fmt, samples) = decode_clip(&tiny_module()).expect("module should decode");
        assert_eq!(fmt.channels, 2, "modules render to stereo");
        assert!(!samples.is_empty());
        assert!(samples.iter().any(|&s| s != 0), "module rendered silent");
    }

    /// A game re-loads the same track into a fresh object whenever it
    /// changes level — JumpyBall loads `mainmenu.tkm` three times before
    /// the first level starts. Rendering a module is the expensive half
    /// of `load`, so the second load must reuse the first decode rather
    /// than repeat it, and both clips must alias one buffer instead of
    /// holding a copy each.
    #[test]
    fn loading_the_same_file_twice_decodes_it_once() {
        use pocket_cpu::{regs::ArmReg, stub::StubCpu, Cpu};

        let dir = std::env::temp_dir().join(format!("pockethle-hss-cache-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        std::fs::write(dir.join("song.tkm"), tiny_module()).expect("write module");

        let mut cpu = StubCpu::new();
        let mut kernel = crate::gx::tests::fresh_kernel();
        kernel.vfs.mount("\\Game\\", &dir);
        let thunk = pocket_kernel::Thunk {
            thunk_va: 0x7000_0400,
            iat_va: 0x4000_0400,
            dll: "hss.dll".to_string(),
            binding: pocket_pe::ImportBinding::Name("?load@hssMusic@@QAAHPBG@Z".to_string()),
            friendly_name: None,
        };

        // The guest hands `load` a wide-string path; put one where the
        // handler will look for it.
        const PATH_VA: u32 = 0x5000_0800;
        let mut wide: Vec<u8> = "\\Game\\song.tkm"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        wide.extend_from_slice(&[0, 0]);
        cpu.map_region(
            PATH_VA & !0xfff,
            0x2000,
            pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE,
        )
        .expect("map path page");
        cpu.write_mem(PATH_VA, &wide).expect("write path");
        cpu.write_reg(ArmReg::Sp, 0x4000).expect("sp");

        let load = |cpu: &mut StubCpu, kernel: &mut pocket_kernel::KernelState, this: u32| {
            cpu.write_reg(ArmReg::R0, this).expect("r0");
            cpu.write_reg(ArmReg::R1, PATH_VA).expect("r1");
            let mut ctx = CallCtx {
                cpu,
                thunk: &thunk,
                kernel,
            };
            load_music(&mut ctx).expect("load should not fault")
        };

        assert_eq!(
            load(&mut cpu, &mut kernel, 0x1111),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(kernel.hss.decoded.len(), 1, "first load should cache");

        // Delete the file: a second load that still succeeds can only be
        // reading the cache, which is exactly the claim under test.
        std::fs::remove_file(dir.join("song.tkm")).expect("remove module");
        assert_eq!(
            load(&mut cpu, &mut kernel, 0x2222),
            DispatchOutcome::ReturnedR0(0),
            "second load should be served from the decode cache"
        );

        let first = &kernel.hss.clips[&0x1111].samples;
        let second = &kernel.hss.clips[&0x2222].samples;
        assert!(
            Arc::ptr_eq(first, second),
            "both clips should share one decoded buffer"
        );
        assert!(!first.is_empty(), "cached decode should not be empty");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn music_pool_precedes_sfx_pool_and_all_requested_effect_channels_are_available() {
        use crate::bluetooth::tests::{call, setup};
        use pocket_cpu::Cpu;
        let (mut cpu, mut k, mut d) = setup();
        cpu.write_mem(0x1200, &tiny_wav()).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "hss.dll",
                "?open@hssSpeaker@@QAAHII_NII@Z",
                &[0x2200, 22050, 16, 1, 1, 16]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "hss.dll",
            "?load@hssSound@@QAAHPAX_N@Z",
            &[0x2300, 0x1200, 1],
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "hss.dll",
            "?loop@hssSound@@QAAX_N@Z",
            &[0x2300, 1],
        );
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "hss.dll",
                "?playMusic@hssSpeaker@@QAAHPAVhssMusic@@I@Z",
                &[0x2200, 0x2300, 0x10000000]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        for channel in 1..=16 {
            assert_eq!(
                call(
                    &mut cpu,
                    &mut k,
                    &mut d,
                    "hss.dll",
                    "?playSound@hssSpeaker@@QAAHPAVhssSound@@I@Z",
                    &[0x2200, 0x2300, 0x10000000]
                ),
                DispatchOutcome::ReturnedR0(channel)
            );
        }
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "hss.dll",
                "?playSound@hssSpeaker@@QAAHPAVhssSound@@I@Z",
                &[0x2200, 0x2300, 0x10000000]
            ),
            DispatchOutcome::ReturnedR0((-1000i32) as u32)
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "hss.dll",
            "?stopMusics@hssSpeaker@@QAAXXZ",
            &[0x2200],
        );
        assert!(!k.hss.channels[&(0x2200, 0)].active());
        assert!((1..=16).all(|n| k.hss.channels[&(0x2200, n)].active()));
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "hss.dll",
            "?stopSounds@hssSpeaker@@QAAXXZ",
            &[0x2200],
        );
        assert!(k.hss.channels.values().all(|c| !c.active()));
    }

    /// Neither decoder should claim a file it cannot actually play —
    /// `load` reports failure to the guest on this path.
    #[test]
    fn memory_wav_and_virtual_channel_have_real_state() {
        use crate::bluetooth::tests::{call, setup};
        use pocket_cpu::{Cpu, Prot};
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        cpu.map_region(0x5000_0000, 0x10000, Prot::READ | Prot::WRITE)
            .unwrap();
        let exports = CHANNEL_METHODS
            .iter()
            .enumerate()
            .map(|(i, n)| (n.to_string(), 0x7000_1000 + i as u32 * 32))
            .collect();
        kernel
            .dynamic_exports
            .insert(pocket_kernel::HSS_MODULE_HANDLE, exports);
        cpu.write_mem(0x1200, &tiny_wav()).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "hss.dll",
                "?open@hssSpeaker@@QAAHII_NII@Z",
                &[0x2200, 22050, 16, 0, 1, 2]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "hss.dll",
                "?load@hssSound@@QAAHPAX_N@Z",
                &[0x2300, 0x1200, 1]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert!(!kernel.hss.clips[&0x2300].samples.is_empty());
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "hss.dll",
            "?loop@hssSound@@QAAX_N@Z",
            &[0x2300, 1],
        );
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "hss.dll",
                "?playSound@hssSpeaker@@QAAHPAVhssSound@@I@Z",
                &[0x2200, 0x2300, 0x10000000]
            ),
            DispatchOutcome::ReturnedR0(1)
        );
        let DispatchOutcome::ReturnedR0(ptr) = call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "hss.dll",
            "?channel@hssSpeaker@@QAAPAVhssChannel@@H@Z",
            &[0x2200, 1],
        ) else {
            panic!("missing channel")
        };
        assert_ne!(ptr, 0);
        let table = u32::from_le_bytes(cpu.read_mem(ptr, 4).unwrap().try_into().unwrap());
        assert_eq!(
            u32::from_le_bytes(cpu.read_mem(table, 4).unwrap().try_into().unwrap()),
            0x70001000
        );
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "hss.dll",
                "?channel@hssSpeaker@@QAAPAVhssChannel@@H@Z",
                &[0x2200, 1]
            ),
            DispatchOutcome::ReturnedR0(ptr)
        );
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "hss.dll",
            CHANNEL_METHODS[0],
            &[ptr, 37],
        );
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "hss.dll",
                "?volume@hssChannel@@QAAIXZ",
                &[ptr]
            ),
            DispatchOutcome::ReturnedR0(37)
        );
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "hss.dll",
                "?playing@hssChannel@@QAA_NXZ",
                &[ptr]
            ),
            DispatchOutcome::ReturnedR0(1)
        );
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "hss.dll",
            "?stop@hssChannel@@QAAXXZ",
            &[ptr],
        );
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "hss.dll",
                "?playing@hssChannel@@QAA_NXZ",
                &[ptr]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
    }

    #[test]
    fn an_unrecognised_file_decodes_to_nothing() {
        assert!(decode_clip(&[0u8; 64]).is_none());
        assert!(decode_clip(b"not audio at all").is_none());
        // A RIFF header with a compressed codec tag is not PCM.
        let mut adpcm = tiny_wav();
        adpcm[20] = 2;
        assert!(decode_clip(&adpcm).is_none());
    }
}
