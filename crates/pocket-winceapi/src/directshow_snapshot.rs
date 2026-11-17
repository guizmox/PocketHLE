//! Logical DirectShow state. Native decoder queues are reconstructed by seeking
//! the current mounted media, rather than serializing threads or FFmpeg pointers.
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(crate) struct GraphSnapshot {
    visible: bool,
    window: [i32; 4],
    source: Option<[i32; 4]>,
    destination: Option<[i32; 4]>,
    window_properties: Vec<u32>,
    objects: HashMap<String, (u32, u32)>,
    notify: u32,
    notify_msg: u32,
    notify_param: u32,
    notify_flags: u32,
    volume: i32,
    stream: Option<StreamSnapshot>,
}
#[derive(Serialize, Deserialize)]
struct StreamSnapshot {
    guest: String,
    length: u64,
    modified: Option<std::time::SystemTime>,
    info: Info,
    position: f64,
    stop: f64,
    playing: bool,
    paused: bool,
    ended: bool,
    completed: bool,
    completion_code: u32,
    audio_queued: bool,
    audio_end: u64,
    clock_started: bool,
    completion_delay: Duration,
    #[serde(with = "serde_bytes")]
    frame: Vec<u8>,
    frame_ready_sent: bool,
    frame_ready_pending: bool,
    waits: Vec<(usize, Duration)>,
}
fn invalid() -> String {
    "invalid saved DirectShow graph".into()
}
impl GraphSnapshot {
    fn validate(&self) -> Result<(), String> {
        if self.window_properties.len() != 46
            || self.objects.len() > 16
            || !(-10000..=0).contains(&self.volume)
            || self.objects.iter().any(|(kind, (object, table))| {
                !KINDS.iter().any(|(known, _, _)| *known == kind) || *object == 0 || *table == 0
            })
        {
            return Err(invalid());
        }
        if let Some(stream) = &self.stream {
            let size = u64::from(stream.info.width) * u64::from(stream.info.height) * 2;
            if stream.guest.is_empty()
                || stream.guest.len() > 32768
                || !stream.position.is_finite()
                || !stream.stop.is_finite()
                || !stream.info.duration.is_finite()
                || stream.info.duration < 0.
                || !stream.info.interval.is_finite()
                || stream.position < 0.
                || stream.position > stream.stop
                || stream.stop > stream.info.duration
                || size > 64 * 1024 * 1024
                || !stream.frame.is_empty() && stream.frame.len() as u64 != size
                || stream.waits.len() > 4096
                || stream
                    .waits
                    .iter()
                    .any(|(_, wait)| *wait > Duration::from_millis(u32::MAX as u64))
            {
                return Err(invalid());
            }
            let threads: std::collections::HashSet<_> =
                stream.waits.iter().map(|(thread, _)| thread).collect();
            if threads.len() != stream.waits.len() {
                return Err(invalid());
            }
        }
        Ok(())
    }
}
pub(crate) fn capture_graphs(at: Instant) -> Result<HashMap<u32, GraphSnapshot>, String> {
    MOVIES.with(|registry| {
        let graphs = registry
            .lock()
            .map_err(|_| "DirectShow registry lock poisoned")?;
        if graphs.len() > 256 {
            return Err("too many DirectShow graphs in checkpoint".into());
        }
        graphs
            .iter()
            .map(|(&root, graph)| {
                let stream = if let Some(path) = &graph.path {
                    let guest = graph
                        .guest_path
                        .clone()
                        .ok_or("missing DirectShow guest media identity")?;
                    let meta = std::fs::metadata(path)
                        .map_err(|error| format!("cannot identify DirectShow media: {error}"))?;
                    let elapsed = graph.paused
                        + graph
                            .start
                            .map(|start| at.saturating_duration_since(start))
                            .unwrap_or_default();
                    Some(StreamSnapshot {
                        guest,
                        length: meta.len(),
                        modified: meta.modified().ok(),
                        info: graph.info,
                        position: (graph.offset + elapsed.as_secs_f64()).min(graph.stop),
                        stop: graph.stop,
                        playing: graph.playing,
                        paused: graph.is_paused,
                        ended: graph.ended,
                        completed: graph.completed,
                        completion_code: graph.completion_code,
                        audio_queued: graph.audio_queued,
                        audio_end: graph.audio_end,
                        clock_started: graph.start.is_some() || graph.snapshot_resume_clock,
                        completion_delay: graph
                            .completion_delay
                            .unwrap_or_else(|| {
                                Duration::from_secs_f64(graph.frame_number as f64 / 30.)
                            })
                            .saturating_sub(elapsed),
                        frame: graph.frame.clone().unwrap_or_default(),
                        frame_ready_sent: graph.frame_ready_sent,
                        frame_ready_pending: graph.frame_ready_pending,
                        waits: graph.snapshot_waits.clone().unwrap_or_else(|| {
                            graph
                                .completion_waits
                                .iter()
                                .map(|(&thread, &start)| {
                                    (thread, at.saturating_duration_since(start))
                                })
                                .collect()
                        }),
                    })
                } else {
                    if graph.playing
                        || graph.worker.is_some()
                        || graph.receiver.is_some()
                        || !graph.completion_waits.is_empty()
                    {
                        return Err(invalid());
                    }
                    None
                };
                let saved = GraphSnapshot {
                    visible: graph.visible,
                    window: graph.window,
                    source: graph.source,
                    destination: graph.destination,
                    window_properties: graph.window_properties.to_vec(),
                    objects: graph.objects.clone(),
                    notify: graph.notify,
                    notify_msg: graph.notify_msg,
                    notify_param: graph.notify_param,
                    notify_flags: graph.notify_flags,
                    volume: graph.volume,
                    stream,
                };
                saved.validate()?;
                Ok((root, saved))
            })
            .collect()
    })
}
pub(crate) fn restore_graphs(
    saved: HashMap<u32, GraphSnapshot>,
    kernel: &mut pocket_kernel::KernelState,
) -> Result<(), String> {
    if saved.len() > 256 {
        return Err(invalid());
    }
    let mut graphs = HashMap::new();
    let mut restarted = Vec::new();
    for (root, saved) in saved {
        saved.validate()?;
        let mut graph = Movie::new();
        graph.visible = saved.visible;
        graph.window = saved.window;
        graph.source = saved.source;
        graph.destination = saved.destination;
        graph.window_properties = saved.window_properties.try_into().map_err(|_| invalid())?;
        graph.objects = saved.objects;
        graph.notify = saved.notify;
        graph.notify_msg = saved.notify_msg;
        graph.notify_param = saved.notify_param;
        graph.notify_flags = saved.notify_flags;
        graph.volume = saved.volume;
        if let Some(stream) = saved.stream {
            let path = kernel
                .vfs
                .resolve(&stream.guest)
                .filter(|path| path.is_file())
                .ok_or_else(|| format!("DirectShow media is missing: {}", stream.guest))?;
            let meta = std::fs::metadata(&path).map_err(|error| error.to_string())?;
            if meta.len() != stream.length || meta.modified().ok() != stream.modified {
                return Err(format!(
                    "DirectShow media changed since checkpoint: {}",
                    stream.guest
                ));
            }
            graph.path = Some(path.clone());
            graph.guest_path = Some(stream.guest);
            graph.info = stream.info;
            graph.offset = stream.position;
            graph.stop = stream.stop;
            graph.playing = stream.playing;
            graph.is_paused = stream.paused;
            graph.ended = stream.ended;
            graph.completed = stream.completed;
            graph.completion_code = stream.completion_code;
            graph.snapshot_resume_clock = stream.clock_started;
            if stream.ended {
                graph.audio_queued = stream.audio_queued;
                graph.audio_end = stream.audio_end;
                graph.completion_delay = Some(stream.completion_delay);
            }
            graph.frame = (!stream.frame.is_empty()).then_some(stream.frame);
            graph.frame_ready_sent = stream.frame_ready_sent;
            graph.frame_ready_pending = stream.frame_ready_pending;
            graph.snapshot_waits = Some(stream.waits);
            if !graph.ended && graph.offset < graph.stop {
                let (receiver, info, cancel, worker) = decode(path, graph.offset, graph.stop)
                    .map_err(|error| format!("cannot reopen DirectShow decoder: {error}"))?;
                graph.receiver = Some(receiver);
                graph.cancel = Some(cancel);
                graph.worker = Some(worker);
                if info.width != graph.info.width
                    || info.height != graph.info.height
                    || (info.duration - graph.info.duration).abs() > 0.001
                {
                    return Err("DirectShow decoder metadata changed".into());
                }
            }
            if !graph.ended {
                restarted.push(root);
            }
        }
        graphs.insert(root, graph);
    }
    MOVIES.with(|registry| {
        *registry
            .lock()
            .map_err(|_| "DirectShow registry lock poisoned")? = graphs;
        Ok::<(), String>(())
    })?;
    // Saved mixer queues belong to the old decoder range. A reopened decoder
    // supplies PCM starting at the saved playhead, with no duplicate voice.
    for root in restarted {
        kernel.audio.stop_mas_stream(root);
    }
    Ok(())
}

#[cfg(all(test, feature = "video-static"))]
mod tests {
    use super::*;
    use pocket_cpu::stub::StubCpu;
    fn media_kernel() -> pocket_kernel::KernelState {
        let mut kernel = crate::gx::tests::fresh_kernel();
        kernel.vfs.mount(
            r"\Media\",
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures"),
        );
        kernel
    }
    fn thunk() -> pocket_kernel::Thunk {
        pocket_kernel::Thunk {
            thunk_va: 0,
            iat_va: 0,
            dll: "coredll.dll".into(),
            binding: pocket_pe::ImportBinding::Name("ShellExecuteEx".into()),
            friendly_name: None,
        }
    }
    #[test]
    fn loaded_video_cold_reopens_decoder_at_saved_playhead_and_retains_frame() {
        reset();
        let mut cpu = StubCpu::new();
        let mut kernel = media_kernel();
        let thunk = thunk();
        let mut ctx = CallCtx {
            cpu: &mut cpu,
            kernel: &mut kernel,
            thunk: &thunk,
        };
        play_system_media(&mut ctx, r"\Media\seeking.wmv").unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            service(&mut ctx).unwrap();
            if MOVIES.with(|slot| {
                slot.lock()
                    .unwrap()
                    .get(&SYSTEM_PLAYER)
                    .unwrap()
                    .frame_number
            }) >= 4
            {
                break;
            }
            assert!(Instant::now() < deadline, "native video did not advance");
            std::thread::sleep(Duration::from_millis(5));
        }
        MOVIES.with(|slot| {
            slot.lock()
                .unwrap()
                .get_mut(&SYSTEM_PLAYER)
                .unwrap()
                .completion_waits
                .insert(0, Instant::now() - Duration::from_millis(50))
        });
        let saved = capture_graphs(Instant::now()).unwrap();
        let position = saved[&SYSTEM_PLAYER].stream.as_ref().unwrap().position;
        let frame = saved[&SYSTEM_PLAYER].stream.as_ref().unwrap().frame.clone();
        assert!(position > 0.05);
        let bytes = serde_json::to_vec(&saved).unwrap();
        reset();
        drop(kernel);
        let mut destination = media_kernel();
        restore_graphs(serde_json::from_slice(&bytes).unwrap(), &mut destination).unwrap();
        MOVIES.with(|slot| {
            let graphs = slot.lock().unwrap();
            let movie = &graphs[&SYSTEM_PLAYER];
            assert_eq!(movie.frame.as_ref().unwrap(), &frame);
            assert_eq!(movie.position(), position);
            assert!(movie.worker.is_some());
            assert_eq!(movie.snapshot_waits.as_ref().unwrap().len(), 1);
        });
        let mut ctx = CallCtx {
            cpu: &mut cpu,
            kernel: &mut destination,
            thunk: &thunk,
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            service(&mut ctx).unwrap();
            if MOVIES.with(|slot| slot.lock().unwrap()[&SYSTEM_PLAYER].frame_number) > 2 {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "restored native decoder did not advance"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        MOVIES.with(|slot| assert!(slot.lock().unwrap()[&SYSTEM_PLAYER].position() > position));
        reset();
    }
    #[test]
    fn changed_media_is_rejected_without_installing_partial_graphs() {
        reset();
        let mut cpu = StubCpu::new();
        let mut kernel = media_kernel();
        let thunk = thunk();
        play_system_media(
            &mut CallCtx {
                cpu: &mut cpu,
                kernel: &mut kernel,
                thunk: &thunk,
            },
            r"\Media\seeking.wmv",
        )
        .unwrap();
        let mut saved = capture_graphs(Instant::now()).unwrap();
        saved
            .get_mut(&SYSTEM_PLAYER)
            .unwrap()
            .stream
            .as_mut()
            .unwrap()
            .length += 1;
        reset();
        assert!(restore_graphs(saved, &mut media_kernel())
            .unwrap_err()
            .contains("changed since checkpoint"));
        MOVIES.with(|slot| assert!(slot.lock().unwrap().is_empty()));
    }
}
