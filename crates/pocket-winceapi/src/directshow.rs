//! DirectShow playback through statically linked FFmpeg. Guest memory is
//! accessed only by the emulator thread; decoder workers own host buffers.
use crate::{CallCtx, WinCeDispatcher};
use once_cell::sync::Lazy;
use pocket_kernel::{audio::GuestFormat, DispatchOutcome, KernelError};
use std::{
    collections::HashMap,
    sync::{mpsc, Mutex},
    time::{Duration, Instant},
};
pub(crate) const CLASS: [u8; 16] = [
    0xb3, 0xeb, 0x36, 0xe4, 0x4f, 0x52, 0xce, 0x11, 0x9f, 0x53, 0, 0x20, 0xaf, 0x0b, 0xa7, 0x70,
];
const UNKNOWN: [u8; 16] = [0, 0, 0, 0, 0, 0, 0, 0, 0xc0, 0, 0, 0, 0, 0, 0, 0x46];
const TAIL: [u8; 12] = [
    0xd4, 0x0a, 0xce, 0x11, 0xb0, 0x3a, 0, 0x20, 0xaf, 0x0b, 0xa7, 0x70,
];
const KINDS: [(&str, u32, usize); 8] = [
    ("graph", 0x56a868a9, 18),
    ("control", 0x56a868b1, 16),
    ("event", 0x56a868b6, 13),
    ("eventex", 0x56a868c0, 16),
    ("filter", 0x56a86899, 15),
    ("window", 0x56a868b4, 46),
    ("audio", 0x56a868b3, 11),
    ("filtergraph", 0x56a8689f, 11),
];
fn kind(iid: &[u8]) -> Option<&'static str> {
    if iid == UNKNOWN {
        return Some("graph");
    }
    if iid.len() != 16 || iid[4..] != TAIL {
        return None;
    }
    let id = u32::from_le_bytes(iid[..4].try_into().ok()?);
    KINDS.iter().find(|(_, n, _)| *n == id).map(|(k, _, _)| *k)
}
// Only the static FFmpeg worker constructs packets in production builds.
#[cfg_attr(not(feature = "video-static"), allow(dead_code))]
enum Packet {
    Audio(Vec<i16>),
    Frame(Vec<u8>),
    End,
    Error(String),
}
struct Movie {
    objects: HashMap<String, (u32, u32)>,
    receiver: Option<mpsc::Receiver<Packet>>,
    playing: bool,
    completion_waits: HashMap<usize, Instant>,
    is_paused: bool,
    paused: Duration,
    start: Option<Instant>,
    frame_number: u64,
    frame: Option<Vec<u8>>,
    pending: Option<Vec<u8>>,
    ended: bool,
    completed: bool,
    completion_code: u32,
    audio: Option<Vec<i16>>,
    audio_queued: bool,
    audio_end: u64,
    notify: u32,
    notify_msg: u32,
    notify_param: u32,
    notify_flags: u32,
    volume: i32,
}
impl Movie {
    fn new() -> Self {
        Self {
            objects: HashMap::new(),
            receiver: None,
            playing: false,
            completion_waits: HashMap::new(),
            is_paused: false,
            paused: Duration::ZERO,
            start: None,
            frame_number: 0,
            frame: None,
            pending: None,
            ended: false,
            completed: false,
            completion_code: 1,
            audio: None,
            audio_queued: false,
            audio_end: 0,
            notify: 0,
            notify_msg: 0,
            notify_param: 0,
            notify_flags: 0,
            volume: 0,
        }
    }
}
static MOVIES: Lazy<Mutex<HashMap<u32, Movie>>> = Lazy::new(|| Mutex::new(HashMap::new()));
fn exports(ctx: &CallCtx<'_>, name: &str) -> u32 {
    ctx.kernel
        .dynamic_exports
        .get(&pocket_kernel::OLE32_MODULE_HANDLE)
        .and_then(|v| v.get(name))
        .copied()
        .unwrap_or(0)
}
pub fn register(d: &mut WinCeDispatcher) {
    for (kind, _, slots) in KINDS {
        for slot in 0..slots {
            d.register_handler("ole32.dll", &format!("ds_{kind}_{slot}"), method);
        }
    }
}
fn allocate(ctx: &mut CallCtx<'_>, root: u32, k: &str) -> Result<Option<(u32, u32)>, KernelError> {
    let slots = KINDS.iter().find(|(n, _, _)| *n == k).unwrap().2;
    let addresses: Vec<_> = (0..slots)
        .map(|i| exports(ctx, &format!("ds_{k}_{i}")))
        .collect();
    if addresses.contains(&0) {
        return Ok(None);
    }
    let table = ctx.kernel.heap.alloc(slots as u32 * 4).unwrap_or(0);
    let object = ctx.kernel.heap.alloc(12).unwrap_or(0);
    if table == 0 || object == 0 {
        if table != 0 {
            ctx.kernel.heap.free(table);
        }
        if object != 0 {
            ctx.kernel.heap.free(object);
        }
        return Ok(None);
    }
    for (i, v) in addresses.iter().enumerate() {
        ctx.cpu.write_mem(table + i as u32 * 4, &v.to_le_bytes())?;
    }
    ctx.cpu.write_mem(object, &table.to_le_bytes())?;
    ctx.cpu.write_mem(object + 4, &1u32.to_le_bytes())?;
    ctx.cpu.write_mem(
        object + 8,
        &(if root == 0 { object } else { root }).to_le_bytes(),
    )?;
    Ok(Some((object, table)))
}
pub(crate) fn create(
    ctx: &mut CallCtx<'_>,
    iid: &[u8],
    out: u32,
) -> Result<DispatchOutcome, KernelError> {
    ctx.cpu.write_mem(out, &0u32.to_le_bytes())?;
    let Some(k) = kind(iid) else {
        return Ok(DispatchOutcome::ReturnedR0(0x80004002));
    };
    let Some((root, table)) = allocate(ctx, 0, "graph")? else {
        return Ok(DispatchOutcome::ReturnedR0(0x8007000e));
    };
    let mut movie = Movie::new();
    movie.objects.insert("graph".into(), (root, table));
    let object = if k == "graph" {
        root
    } else {
        let Some(pair) = allocate(ctx, root, k)? else {
            ctx.kernel.heap.free(root);
            ctx.kernel.heap.free(table);
            return Ok(DispatchOutcome::ReturnedR0(0x8007000e));
        };
        movie.objects.insert(k.into(), pair);
        pair.0
    };
    MOVIES.lock().unwrap().insert(root, movie);
    ctx.cpu.write_mem(out, &object.to_le_bytes())?;
    log::info!("DirectShow FilterGraph created at 0x{root:08x}");
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn read_path(ctx: &mut CallCtx<'_>, pointer: u32) -> Result<String, KernelError> {
    let mut text = Vec::new();
    for i in 0..32768u32 {
        let v = ctx.cpu.read_mem(pointer + i * 2, 2)?;
        let v = u16::from_le_bytes([v[0], v[1]]);
        if v == 0 {
            break;
        }
        text.push(v);
    }
    Ok(String::from_utf16_lossy(&text))
}
#[cfg(not(feature = "video-static"))]
fn decode(
    _path: std::path::PathBuf,
    _width: u32,
    _height: u32,
) -> Result<mpsc::Receiver<Packet>, String> {
    Err("PocketHLE was compiled without the video-static feature".into())
}

#[cfg(feature = "video-static")]
fn decode(
    path: std::path::PathBuf,
    width: u32,
    height: u32,
) -> Result<mpsc::Receiver<Packet>, String> {
    use crate::media_static::Decoder;
    let mut video = Decoder::open(&path, true, width, height)?.ok_or("Video stream missing")?;
    let (tx, rx) = mpsc::sync_channel(3);
    std::thread::spawn(move || {
        let mut run = || -> Result<(), String> {
            // Retain the validated finite PCM queue and independent audio
            // stream. Only the producer changes; no host processes or DLLs.
            if let Some(mut audio) = Decoder::open(&path, false, 0, 0)? {
                let mut samples = Vec::new();
                while let Some((bytes, _)) = audio.next()? {
                    samples.extend(
                        bytes
                            .chunks_exact(2)
                            .map(|v| i16::from_ne_bytes([v[0], v[1]])),
                    );
                }
                if tx.send(Packet::Audio(samples)).is_err() {
                    return Ok(());
                }
            }
            // Convert native timestamps to the existing 30-Hz presentation
            // timeline, repeating/dropping frames for other input rates.
            let mut previous: Option<Vec<u8>> = None;
            let mut emitted = 0u64;
            while let Some((frame, seconds)) = video.next()? {
                if !seconds.is_finite() || seconds < 0.0 {
                    return Err("Invalid video timestamp".into());
                }
                let due = (seconds * 30.0).round() as u64;
                if let Some(held) = previous.as_ref() {
                    while emitted < due {
                        if tx.send(Packet::Frame(held.clone())).is_err() {
                            return Ok(());
                        }
                        emitted += 1;
                    }
                }
                previous = Some(frame);
            }
            if let Some(frame) = previous {
                if tx.send(Packet::Frame(frame)).is_err() {
                    return Ok(());
                }
            }
            let _ = tx.send(Packet::End);
            Ok(())
        };
        if let Err(error) = run() {
            let _ = tx.send(Packet::Error(error));
        }
        // Decoder Drop frees native contexts on every path, including when
        // Stop/Release closes the receiver during a blocked send.
    });
    Ok(rx)
}
fn root(ctx: &mut CallCtx<'_>, this: u32) -> Result<u32, KernelError> {
    Ok(ctx.cpu.read_u32_le(this + 8)?)
}
fn elapsed(m: &Movie) -> Duration {
    m.paused + m.start.map(|v| v.elapsed()).unwrap_or_default()
}
fn update(ctx: &mut CallCtx<'_>, id: u32, m: &mut Movie) -> Result<(), KernelError> {
    if !m.playing {
        return Ok(());
    }
    let mut budget = 64;
    loop {
        if budget == 0 {
            break;
        }
        budget -= 1;
        if let Some(frame) = m.pending.take() {
            if m.start.is_none() {
                m.start = Some(Instant::now());
            }
            let due = Duration::from_secs_f64(m.frame_number as f64 / 30.0);
            if elapsed(m) < due {
                m.pending = Some(frame);
                break;
            }
            m.frame = Some(frame);
            m.frame_number += 1;
        }
        let packet = m.receiver.as_ref().and_then(|r| r.try_recv().ok());
        match packet {
            Some(Packet::Audio(audio)) => m.audio = Some(audio),
            Some(Packet::Frame(frame)) => m.pending = Some(frame),
            Some(Packet::End) => {
                m.ended = true;
                break;
            }
            Some(Packet::Error(e)) => {
                log::error!("DirectShow: {e}");
                m.ended = true;
                m.completed = true;
                m.completion_code = 3;
                m.playing = false;
                if m.notify != 0 && m.notify_msg != 0 && m.notify_flags == 0 {
                    ctx.kernel.posted_messages.push_back((
                        m.notify,
                        m.notify_msg,
                        0,
                        m.notify_param,
                    ));
                }
                break;
            }
            None => break,
        }
    }
    if m.start.is_some() && !m.audio_queued {
        if let Some(samples) = m.audio.take() {
            ctx.kernel.audio.start();
            let gain = 10f64.powf(m.volume as f64 / 2000.0);
            let samples: Vec<i16> = samples
                .into_iter()
                .map(|v| (v as f64 * gain).round() as i16)
                .collect();
            m.audio_end = ctx.kernel.audio.queue_mas_samples(
                id,
                GuestFormat {
                    sample_rate: 44100,
                    channels: 2,
                    bits_per_sample: 16,
                },
                &samples,
            );
        }
        m.audio_queued = true;
    }
    if let Some(frame) = &m.frame {
        if frame.len() == ctx.kernel.framebuffer.pixels.len() {
            ctx.kernel.framebuffer.pixels.copy_from_slice(frame);
            ctx.kernel.framebuffer.mark_dirty();
            if ctx.kernel.fb_mapped {
                ctx.cpu
                    .write_mem(pocket_kernel::SYNTHETIC_FRAMEBUFFER_BASE, frame)?;
                ctx.kernel.gx_last_pushed_counter = ctx.kernel.framebuffer.frame_counter;
            }
        }
    }
    if m.ended
        && m.pending.is_none()
        && elapsed(m) >= Duration::from_secs_f64(m.frame_number as f64 / 30.0)
        && ctx.kernel.audio.mas_playback_cursor(id) >= m.audio_end
        && !m.completed
    {
        m.completed = true;
        m.playing = false;
        m.start = None;
        log::info!("DirectShow playback completed ({})", m.frame_number);
        if m.notify != 0 && m.notify_msg != 0 && m.notify_flags == 0 {
            ctx.kernel
                .posted_messages
                .push_back((m.notify, m.notify_msg, 0, m.notify_param));
        }
    }
    Ok(())
}
pub(crate) fn service(ctx: &mut CallCtx<'_>) -> Result<(), KernelError> {
    let mut movies = MOVIES.lock().unwrap();
    for (&id, m) in movies.iter_mut() {
        update(ctx, id, m)?;
    }
    Ok(())
}
fn method(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let name = ctx.thunk.friendly_name.clone().unwrap_or_default();
    let mut parts = name.split('_');
    let _ = parts.next();
    let k = parts.next().unwrap_or("");
    let slot: usize = parts
        .next()
        .and_then(|v| v.parse().ok())
        .unwrap_or(usize::MAX);
    let this = ctx.arg_u32(0)?;
    let id = root(ctx, this)?;
    let mut movies = MOVIES.lock().unwrap();
    let Some(m) = movies.get_mut(&id) else {
        return Ok(DispatchOutcome::ReturnedR0(0x80004003));
    };
    if slot == 0 {
        let iid = ctx.arg_u32(1)?;
        let out = ctx.arg_u32(2)?;
        if out == 0 {
            return Ok(DispatchOutcome::ReturnedR0(0x80004003));
        }
        ctx.cpu.write_mem(out, &0u32.to_le_bytes())?;
        let bytes = ctx.cpu.read_mem(iid, 16)?;
        let Some(k) = kind(&bytes) else {
            return Ok(DispatchOutcome::ReturnedR0(0x80004002));
        };
        let pair = if let Some(p) = m.objects.get(k) {
            *p
        } else {
            let Some(pair) = allocate(ctx, id, k)? else {
                return Ok(DispatchOutcome::ReturnedR0(0x8007000e));
            };
            m.objects.insert(k.into(), pair);
            pair
        };
        let refs = ctx.cpu.read_u32_le(id + 4)?.saturating_add(1);
        ctx.cpu.write_mem(id + 4, &refs.to_le_bytes())?;
        ctx.cpu.write_mem(out, &pair.0.to_le_bytes())?;
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    if slot == 1 || slot == 2 {
        let refs = ctx.cpu.read_u32_le(id + 4)?;
        let refs = if slot == 1 {
            refs.saturating_add(1)
        } else {
            refs.saturating_sub(1)
        };
        ctx.cpu.write_mem(id + 4, &refs.to_le_bytes())?;
        if refs == 0 {
            ctx.kernel.audio.stop_mas_stream(id);
            let m = movies.remove(&id).unwrap();
            for (_, (object, table)) in m.objects {
                ctx.kernel.heap.free(object);
                ctx.kernel.heap.free(table);
            }
        }
        return Ok(DispatchOutcome::ReturnedR0(refs));
    }
    // IMediaEventEx inherits all IMediaEvent slots. Keep its own vtable and
    // IID for QueryInterface, but share the graph's event queue and state.
    let k = if k == "eventex" { "event" } else { k };
    let mut result = 0;
    match (k, slot) {
        ("graph", 13) => {
            let p = ctx.arg_u32(1)?;
            if p == 0 {
                return Ok(DispatchOutcome::ReturnedR0(0x80004003));
            }
            if ctx.arg_u32(2)? != 0 {
                return Ok(DispatchOutcome::ReturnedR0(0x80070057));
            }
            let path = read_path(ctx, p)?;
            let Some(host) = ctx.kernel.vfs.resolve(&path) else {
                return Ok(DispatchOutcome::ReturnedR0(0x80070002));
            };
            if !host.is_file() {
                return Ok(DispatchOutcome::ReturnedR0(0x80070002));
            }
            let w = ctx.kernel.framebuffer.width;
            let h = ctx.kernel.framebuffer.height;
            match decode(host, w, h) {
                Ok(rx) => {
                    ctx.kernel.audio.stop_mas_stream(id);
                    m.completion_waits.clear();
                    m.playing = false;
                    m.is_paused = false;
                    m.receiver = Some(rx);
                    m.frame = None;
                    m.pending = None;
                    m.frame_number = 0;
                    m.ended = false;
                    m.completed = false;
                    m.completion_code = 1;
                    m.start = None;
                    m.paused = Duration::ZERO;
                    m.audio = None;
                    m.audio_queued = false;
                    m.audio_end = 0;
                    log::info!(
                        "DirectShow RenderFile({path:?}): statically linked FFmpeg decoder started"
                    );
                }
                Err(e) => {
                    log::warn!("DirectShow native decoder unavailable: {e}");
                    result = 0x80004001;
                }
            }
        }
        ("control", 7) | ("filter", 6) => {
            if m.receiver.is_none() {
                return Ok(DispatchOutcome::ReturnedR0(0x80040209));
            }
            m.playing = true;
            m.is_paused = false;
            ctx.kernel.audio.pause_mas_stream(id, false);
            if m.frame.is_some() {
                m.start = Some(Instant::now());
            }
        }
        ("control", 8) | ("filter", 5) => {
            m.is_paused = true;
            m.paused = elapsed(m);
            m.start = None;
            m.playing = false;
            ctx.kernel.audio.pause_mas_stream(id, true);
        }
        ("control", 9) | ("filter", 4) => {
            m.completion_waits.clear();
            m.playing = false;
            m.is_paused = false;
            m.completed = false;
            m.receiver = None;
            m.frame = None;
            m.pending = None;
            ctx.kernel.audio.stop_mas_stream(id);
        }
        ("control", 10) | ("filter", 7) => {
            let p = ctx.arg_u32(2)?;
            if p != 0 {
                ctx.cpu.write_mem(
                    p,
                    &(if m.playing {
                        2u32
                    } else if m.is_paused {
                        1
                    } else {
                        0
                    })
                    .to_le_bytes(),
                )?;
            }
        }
        ("event", 8) => {
            update(ctx, id, m)?;
            if m.completed {
                for (p, v) in [
                    (ctx.arg_u32(1)?, m.completion_code),
                    (
                        ctx.arg_u32(2)?,
                        if m.completion_code == 3 {
                            0x80004005
                        } else {
                            0
                        },
                    ),
                    (ctx.arg_u32(3)?, 0),
                ] {
                    if p != 0 {
                        ctx.cpu.write_mem(p, &v.to_le_bytes())?;
                    }
                }
                m.completed = false;
            } else {
                result = 0x80004004;
            }
        }
        ("event", 9) => {
            let timeout = ctx.arg_u32(1)?;
            let p = ctx.arg_u32(2)?;
            if p == 0 {
                return Ok(DispatchOutcome::ReturnedR0(0x80004003));
            }
            // WinCE specifies zero when the wait times out. Colors inspects
            // this output even on E_ABORT; stale stack data skips its intro.
            ctx.cpu.write_mem(p, &0u32.to_le_bytes())?;
            update(ctx, id, m)?;
            let thread = ctx.kernel.current_thread;
            if m.completed {
                m.completion_waits.remove(&thread);
                ctx.cpu.write_mem(p, &m.completion_code.to_le_bytes())?;
            } else if !m.playing {
                m.completion_waits.remove(&thread);
                result = 0x80040227; // VFW_E_WRONG_STATE
            } else {
                let started = *m
                    .completion_waits
                    .entry(thread)
                    .or_insert_with(Instant::now);
                if timeout != 0
                    && (timeout == u32::MAX
                        || started.elapsed() < Duration::from_millis(u64::from(timeout)))
                {
                    // Re-enter with unchanged guest registers. Let the run loop
                    // present frames between slices instead of blocking the host.
                    return Ok(DispatchOutcome::JumpTo(ctx.thunk.thunk_va));
                }
                m.completion_waits.remove(&thread);
                result = 0x80004004;
            }
        }
        ("event", 12) => {}
        ("event", 13) => {
            m.notify = ctx.arg_u32(1)?;
            m.notify_msg = ctx.arg_u32(2)?;
            m.notify_param = ctx.arg_u32(3)?;
        }
        ("event", 14) => {
            let flags = ctx.arg_u32(1)?;
            if flags > 1 {
                result = 0x80070057;
            }
            // Only AM_MEDIAEVENT_NONOTIFY.
            else {
                m.notify_flags = flags;
            }
        }
        ("event", 15) => {
            let p = ctx.arg_u32(1)?;
            if p == 0 {
                result = 0x80004003;
            } else {
                ctx.cpu.write_mem(p, &m.notify_flags.to_le_bytes())?;
            }
        }
        ("audio", 7) => m.volume = (ctx.arg_u32(1)? as i32).clamp(-10000, 0),
        ("audio", 8) => {
            let p = ctx.arg_u32(1)?;
            ctx.cpu.write_mem(p, &m.volume.to_le_bytes())?;
        }
        ("window", 8)
        | ("window", 9)
        | ("window", 19)
        | ("window", 25)
        | ("window", 29)
        | ("window", 31)
        | ("window", 36)
        | ("window", 39) => {}
        ("filter", 9) => {}
        _ => {
            log::debug!("DirectShow unsupported {k} slot {slot}");
            result = 0x80004001;
        }
    }
    Ok(DispatchOutcome::ReturnedR0(result))
}
pub(crate) struct SuspendedMovies {
    movies: HashMap<u32, Movie>,
    at: Instant,
}
pub(crate) fn suspend() -> SuspendedMovies {
    SuspendedMovies {
        movies: std::mem::take(&mut *MOVIES.lock().unwrap()),
        at: Instant::now(),
    }
}
pub(crate) fn resume(mut saved: SuspendedMovies) {
    let duration = saved.at.elapsed();
    for movie in saved.movies.values_mut() {
        if let Some(start) = movie.start.as_mut() {
            *start += duration;
        }
        for start in movie.completion_waits.values_mut() {
            *start += duration;
        }
    }
    *MOVIES.lock().unwrap() = saved.movies;
}

pub(crate) fn reset() {
    MOVIES.lock().unwrap().clear();
}
#[cfg(test)]
mod tests {
    #[test]
    fn directshow_suspension_restores_graph_and_freezes_movie_clock() {
        reset();
        let mut movie = Movie::new();
        movie.playing = true;
        movie.start = Some(Instant::now() - Duration::from_secs(2));
        MOVIES.lock().unwrap().insert(0x50001100, movie);
        let mut saved = suspend();
        assert!(MOVIES.lock().unwrap().is_empty());
        // Simulate a long child lifetime without slowing the test.
        saved.at -= Duration::from_secs(30);
        for movie in saved.movies.values_mut() {
            movie.start = movie.start.map(|start| start - Duration::from_secs(30));
        }
        MOVIES.lock().unwrap().insert(0x50001100, Movie::new());
        reset(); // Child Emulator Drop must not erase the detached parent.
        resume(saved);
        let movies = MOVIES.lock().unwrap();
        let parent = movies.get(&0x50001100).unwrap();
        assert!(parent.playing);
        assert!(elapsed(parent) >= Duration::from_secs(2));
        assert!(elapsed(parent) < Duration::from_secs(3));
        drop(movies);
        reset();
    }
    use super::*;
    use pocket_cpu::{regs::ArmReg, stub::StubCpu, Cpu, Prot};
    use pocket_kernel::{KernelState, Thunk};
    use pocket_pe::ImportBinding;
    fn thunk(name: &str) -> Thunk {
        Thunk {
            thunk_va: 0x70000000,
            iat_va: 0x20000,
            dll: "ole32.dll".into(),
            binding: ImportBinding::Name(name.into()),
            friendly_name: Some(name.into()),
        }
    }
    fn call(
        cpu: &mut StubCpu,
        kernel: &mut KernelState,
        name: &str,
        args: [u32; 3],
    ) -> DispatchOutcome {
        for (reg, value) in [ArmReg::R0, ArmReg::R1, ArmReg::R2].into_iter().zip(args) {
            cpu.write_reg(reg, value).unwrap();
        }
        let t = thunk(name);
        method(&mut CallCtx {
            cpu,
            kernel,
            thunk: &t,
        })
        .unwrap()
    }
    #[test]
    fn directshow_graph_identity_interfaces_and_reference_lifetime() {
        let mut cpu = StubCpu::new();
        let mut kernel = crate::gx::tests::fresh_kernel();
        cpu.map_region(0x1000, 0x1000, Prot::READ | Prot::WRITE)
            .unwrap();
        cpu.map_region(0x50000000, 0x10000, Prot::READ | Prot::WRITE)
            .unwrap();
        let e = kernel
            .dynamic_exports
            .entry(pocket_kernel::OLE32_MODULE_HANDLE)
            .or_default();
        let mut address = 0x70001000;
        for (k, _, slots) in KINDS {
            for slot in 0..slots {
                e.insert(format!("ds_{k}_{slot}"), address);
                address += 16;
            }
        }
        let mut iid = [0u8; 16];
        iid[..4].copy_from_slice(&0x56a868a9u32.to_le_bytes());
        iid[4..].copy_from_slice(&TAIL);
        let t = thunk("CoCreateInstance");
        assert_eq!(
            create(
                &mut CallCtx {
                    cpu: &mut cpu,
                    kernel: &mut kernel,
                    thunk: &t
                },
                &iid,
                0x1000
            )
            .unwrap(),
            DispatchOutcome::ReturnedR0(0)
        );
        let object = cpu.read_u32_le(0x1000).unwrap();
        let table = cpu.read_u32_le(object).unwrap();
        assert_eq!(cpu.read_u32_le(table + 0x34).unwrap(), 0x70001000 + 13 * 16);
        let mut control = iid;
        control[..4].copy_from_slice(&0x56a868b1u32.to_le_bytes());
        cpu.write_mem(0x1100, &control).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                "ds_graph_0",
                [object, 0x1100, 0x1200]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        let child = cpu.read_u32_le(0x1200).unwrap();
        assert_ne!(child, object);
        assert_eq!(cpu.read_u32_le(child + 8).unwrap(), object);
        cpu.write_mem(0x1100, &UNKNOWN).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                "ds_control_0",
                [child, 0x1100, 0x1200]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(cpu.read_u32_le(0x1200).unwrap(), object);
        // Jump requests IMediaEventEx during startup and checks the HRESULT.
        // Both event IIDs share IUnknown identity and inherited event state.
        let mut eventex = iid;
        eventex[..4].copy_from_slice(&0x56a868c0u32.to_le_bytes());
        cpu.write_mem(0x1100, &eventex).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                "ds_graph_0",
                [object, 0x1100, 0x1200]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        let extended = cpu.read_u32_le(0x1200).unwrap();
        assert_ne!(extended, object);
        let extended_table = cpu.read_u32_le(extended).unwrap();
        assert_eq!(
            cpu.read_u32_le(extended_table + 13 * 4).unwrap(),
            kernel.dynamic_exports[&pocket_kernel::OLE32_MODULE_HANDLE]["ds_eventex_13"]
        );
        cpu.write_mem(0x1100, &UNKNOWN).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                "ds_eventex_0",
                [extended, 0x1100, 0x1200]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(cpu.read_u32_le(0x1200).unwrap(), object);
        assert_eq!(
            call(&mut cpu, &mut kernel, "ds_graph_2", [object, 0, 0]),
            DispatchOutcome::ReturnedR0(4)
        );
        let mut event = iid;
        event[..4].copy_from_slice(&0x56a868b6u32.to_le_bytes());
        cpu.write_mem(0x1100, &event).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                "ds_eventex_0",
                [extended, 0x1100, 0x1200]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        let base_event = cpu.read_u32_le(0x1200).unwrap();
        assert_ne!(base_event, extended);
        cpu.write_reg(ArmReg::R3, 0xab).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                "ds_eventex_13",
                [extended, 0xdead0001, 0x400]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(
            call(&mut cpu, &mut kernel, "ds_eventex_14", [extended, 1, 0]),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                "ds_eventex_15",
                [extended, 0x1200, 0]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(cpu.read_u32_le(0x1200).unwrap(), 1);
        assert_eq!(
            call(&mut cpu, &mut kernel, "ds_eventex_14", [extended, 2, 0]),
            DispatchOutcome::ReturnedR0(0x80070057)
        );
        assert_eq!(
            call(&mut cpu, &mut kernel, "ds_eventex_15", [extended, 0, 0]),
            DispatchOutcome::ReturnedR0(0x80004003)
        );
        assert_eq!(
            call(&mut cpu, &mut kernel, "ds_event_2", [base_event, 0, 0]),
            DispatchOutcome::ReturnedR0(4)
        );
        assert_eq!(
            call(&mut cpu, &mut kernel, "ds_eventex_2", [extended, 0, 0]),
            DispatchOutcome::ReturnedR0(3)
        );
        // A real decoder EOF and expired frame presentation time produce
        // exactly one completion event, independently of decoder speed.
        let (tx, rx) = mpsc::sync_channel(3);
        {
            let mut movies = MOVIES.lock().unwrap();
            let movie = movies.get_mut(&object).unwrap();
            movie.receiver = Some(rx);
            movie.playing = true;
        }
        // A pending wait must overwrite a nonzero caller value, including
        // a finite timeout (which re-enters without clobbering arguments).
        cpu.write_mem(0x1200, &0xdeadbeefu32.to_le_bytes()).unwrap();
        assert_eq!(
            call(&mut cpu, &mut kernel, "ds_event_9", [object, 50, 0x1200]),
            DispatchOutcome::JumpTo(0x70000000)
        );
        assert_eq!(cpu.read_u32_le(0x1200).unwrap(), 0);
        {
            MOVIES
                .lock()
                .unwrap()
                .get_mut(&object)
                .unwrap()
                .completion_waits
                .insert(0, Instant::now() - Duration::from_secs(1));
        }
        assert_eq!(
            call(&mut cpu, &mut kernel, "ds_event_9", [object, 50, 0x1200]),
            DispatchOutcome::ReturnedR0(0x80004004)
        );
        assert_eq!(cpu.read_u32_le(0x1200).unwrap(), 0);
        assert_eq!(
            call(&mut cpu, &mut kernel, "ds_event_9", [object, 0, 0]),
            DispatchOutcome::ReturnedR0(0x80004003)
        );
        assert_eq!(
            call(&mut cpu, &mut kernel, "ds_filter_7", [object, 50, 0x1200]),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(cpu.read_u32_le(0x1200).unwrap(), 2);
        tx.send(Packet::Frame(vec![0; kernel.framebuffer.pixels.len()]))
            .unwrap();
        tx.send(Packet::End).unwrap();
        {
            let mut movies = MOVIES.lock().unwrap();
            let movie = movies.get_mut(&object).unwrap();
            movie.start = Some(Instant::now() - Duration::from_secs(1));
        }
        assert_eq!(
            call(&mut cpu, &mut kernel, "ds_event_9", [object, 0, 0x1200]),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(cpu.read_u32_le(0x1200).unwrap(), 1);
        assert!(kernel.posted_messages.is_empty()); // NONOTIFY does not suppress polling.
        cpu.write_reg(ArmReg::R3, 0x1300).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                "ds_event_8",
                [object, 0x1200, 0x1240]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(cpu.read_u32_le(0x1200).unwrap(), 1);
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                "ds_event_8",
                [object, 0x1200, 0x1240]
            ),
            DispatchOutcome::ReturnedR0(0x80004004)
        );
        assert_eq!(
            call(&mut cpu, &mut kernel, "ds_graph_2", [object, 0, 0]),
            DispatchOutcome::ReturnedR0(2)
        );
        assert_eq!(
            call(&mut cpu, &mut kernel, "ds_control_2", [child, 0, 0]),
            DispatchOutcome::ReturnedR0(1)
        );
        assert_eq!(
            call(&mut cpu, &mut kernel, "ds_graph_2", [object, 0, 0]),
            DispatchOutcome::ReturnedR0(0)
        );
        assert!(!MOVIES.lock().unwrap().contains_key(&object));
    }
}
