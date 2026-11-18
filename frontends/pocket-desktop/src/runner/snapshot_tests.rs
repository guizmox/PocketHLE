//! Opt-in end-to-end snapshot probe using an installed game.
use super::*;

#[test]
#[ignore = "requires an installed ARM game and native OpenGL"]
fn external_game_device_snapshot_cold_restore() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .is_test(true)
        .try_init();
    let root =
        PathBuf::from(std::env::var_os("POCKETHLE_SNAPSHOT_TEST_LIBRARY").expect("test library"));
    let id = std::env::var("POCKETHLE_SNAPSHOT_TEST_GAME").expect("game id");
    let mut game: GameEntry = serde_json::from_slice(
        &std::fs::read(root.join("games").join(&id).join("game.json")).unwrap(),
    )
    .unwrap();
    game.executable = PathBuf::from(game.executable.to_string_lossy().replace('\\', "/"));
    game.settings.max_slices = 0;
    let ui = pocket_gles::gpu::Renderer::new(800, 800).unwrap();
    let runner = Runner::default();
    runner
        .shared_export_policy()
        .store(true, std::sync::atomic::Ordering::Release);
    let owned_context = std::env::var_os("POCKETHLE_SNAPSHOT_TEST_OWNED_CONTEXT").is_some();
    runner.set_shared_context_source((!owned_context).then(|| ui.shared_context_source()));
    eprintln!(
        "probe uses {} context",
        if owned_context { "owned" } else { "shared" }
    );
    let capture_after = std::env::var("POCKETHLE_SNAPSHOT_TEST_CAPTURE_AFTER")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(6);
    let taps: Vec<(u64, u16, u16)> = std::env::var("POCKETHLE_SNAPSHOT_TEST_TAPS")
        .unwrap_or_default()
        .split(';')
        .filter(|tap| !tap.is_empty())
        .map(|tap| {
            let words: Vec<_> = tap.split(':').collect();
            (
                words[0].parse().unwrap(),
                words[1].parse().unwrap(),
                words[2].parse().unwrap(),
            )
        })
        .collect();
    let keys: Vec<(u64, u16)> = std::env::var("POCKETHLE_SNAPSHOT_TEST_KEYS")
        .unwrap_or_default()
        .split(';')
        .filter(|key| !key.is_empty())
        .map(|key| {
            let (at, vk) = key.split_once(':').unwrap();
            (at.parse().unwrap(), vk.parse().unwrap())
        })
        .collect();
    for restore in [false, true] {
        let (input, receiver) = std::sync::mpsc::channel();
        let (frames, images) = crate::latest_frame::channel();
        let worker_runner = runner.clone();
        let library = root.clone();
        let worker_game = game.clone();
        let worker = std::thread::spawn(move || {
            worker_runner.run_game(
                library,
                worker_game,
                Some(frames),
                Some(receiver),
                None,
                None,
                Default::default(),
                restore,
            )
        });
        let start = Instant::now();
        let mut requested = restore;
        let mut reply = None;
        let mut displayed = 0;
        let mut sent_taps = 0;
        let mut held_tap = None;
        let mut capture_error = None;
        let mut first_frame = false;
        let mut request_at = None;
        let mut sent_keys = 0;
        let mut held_key = None;
        while start.elapsed() < Duration::from_secs(if restore { 8 } else { capture_after + 9 })
            && !worker.is_finished()
        {
            if let Ok(frame) = images.try_recv() {
                displayed += 1;
                if !first_frame {
                    eprintln!("first frame (restore={restore}): {:?}", start.elapsed());
                    first_frame = true;
                }
                if displayed % 60 == 0 {
                    if let Ok(rgba) = frame.cpu_rgba() {
                        let mut ppm = format!(
                            "P6\n{} {}\n255\n",
                            frame.texture_width, frame.texture_height
                        )
                        .into_bytes();
                        for pixel in rgba.chunks_exact(4) {
                            ppm.extend_from_slice(&pixel[..3]);
                        }
                        std::fs::write(root.join(format!("probe-{restore}-{displayed}.ppm")), ppm)
                            .unwrap();
                    }
                }
            }
            if !restore {
                if let Some((at, vk)) = held_key {
                    if start.elapsed().as_millis() >= u128::from(at + 150) {
                        input
                            .send(InputCommand::Input(InputEvent::KeyUp { vk }))
                            .unwrap();
                        held_key = None;
                    }
                } else if let Some(&(at, vk)) = keys.get(sent_keys) {
                    if start.elapsed().as_millis() >= u128::from(at) {
                        input
                            .send(InputCommand::Input(InputEvent::KeyDown { vk }))
                            .unwrap();
                        held_key = Some((at, vk));
                        sent_keys += 1;
                    }
                }
                if let Some((at, x, y)) = held_tap {
                    if start.elapsed().as_millis() >= u128::from(at + 150) {
                        input
                            .send(InputCommand::Input(InputEvent::PointerUp { x, y }))
                            .unwrap();
                        held_tap = None;
                    }
                } else if let Some(&(at, x, y)) = taps.get(sent_taps) {
                    if start.elapsed().as_millis() >= u128::from(at) {
                        input
                            .send(InputCommand::Input(InputEvent::PointerDown { x, y }))
                            .unwrap();
                        held_tap = Some((at, x, y));
                        sent_taps += 1;
                    }
                }
            }
            if !requested && start.elapsed() > Duration::from_secs(capture_after) {
                let (tx, rx) = std::sync::mpsc::channel();
                request_at = Some(Instant::now());
                input.send(InputCommand::SaveSnapshot(tx)).unwrap();
                reply = Some(rx);
                requested = true;
            }
            if let Some(rx) = reply.as_ref() {
                if let Ok(result) = rx.try_recv() {
                    eprintln!(
                        "snapshot capture/write response after {:?}: {result:?}",
                        request_at.unwrap().elapsed()
                    );
                    capture_error = result.err();
                    reply = None;
                    if capture_error.is_some() {
                        break;
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = input.send(InputCommand::Stop);
        let (outcome, diagnostic) = worker.join().unwrap();
        eprintln!("{restore}: {}", outcome.summary);
        assert!(diagnostic.is_none(), "{diagnostic:?}");
        assert!(displayed > 0);
        assert!(capture_error.is_none(), "{capture_error:?}");
        assert!(reply.is_none(), "snapshot request did not finish");
        if !restore {
            let path = pocket_library::snapshot_slot::path_for(&root, &game);
            let (_, payload) = pocket_library::snapshot_slot::open(&path).unwrap().unwrap();
            let mut saved = pocket_core::SessionSnapshot::read(payload).unwrap();
            eprintln!(
                "saved root={} module={} device={}",
                saved.process_id(),
                saved.program_path(),
                saved.has_device()
            );
            if let Some(archive) = saved.take_device() {
                let (shared, foreground, children) = archive.into_parts();
                eprintln!(
                    "saved processes={:?} foreground={foreground}",
                    shared.process_ids()
                );
                for child in children {
                    eprintln!(
                        "saved child={} module={}",
                        child.process_id(),
                        child.program_path()
                    );
                }
            }
        }
    }
}
