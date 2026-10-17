//! Headless reproduction of desktop card mounts and module-path selection.
use pocket_core::{
    kernel::{FrameAction, InputEvent, KernelState},
    Emulator,
};
use pocket_library::{GameEntry, GameSettings};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
fn main() -> anyhow::Result<()> {
    env_logger::init();
    let args: Vec<_> = std::env::args().collect();
    let root = PathBuf::from(args.get(1).expect("library root"));
    let legacy = args.get(2).is_some_and(|s| s == "legacy");
    let out = args.get(3).expect("snapshot path");
    let game = GameEntry {
        id: "GZAP250006".into(),
        display_name: "Navigator probe".into(),
        provider: None,
        executable: "extracted/GZAP250006/CoPilot.exe".into(),
        source_cab: "Navigator.zip".into(),
        install_dir: None,
        install_dirs: vec!["\\SD Card\\GZAP250006\\".into()],
        save_prefix: None,
        registry: vec![],
        imported_at: 0,
        settings: GameSettings::default(),
        icon: None,
        companions: vec![],
    };
    let mut emu = Emulator::with_unicorn_cpu()?;
    emu.load_pe(game.launch_path(&root))?;
    emu.set_memory_division(Some(
        pocket_core::kernel::memory_division::MemoryDivision::gizmondo_sdk_default(),
    ));
    emu.set_screen_size(320, 240);
    emu.mount_read_only_dir("\\SD Card\\", game.gizmondo_card_root(&root));
    emu.mount_read_only_dir("\\Storage Card\\", game.gizmondo_card_root(&root));
    for (prefix, directory) in game.install_mounts(&root) {
        emu.mount_read_only_dir(&prefix, directory);
    }
    emu.mount_save_dir("\\Flash Disk\\", root.join("flash-probe"));
    let module = if legacy {
        "\\SD Card\\CoPilot.exe".into()
    } else {
        game.gizmondo_guest_exe_path(&root).unwrap()
    };
    emu.set_module_path(&module);
    emu.set_default_dir(&format!("{}\\", module.rsplit_once('\\').unwrap().0));
    emu.set_synthetic_message_budget(0);
    emu.max_slices = 0;
    let gps = emu.process().unwrap().state.vfs.gps_service();
    gps.set_fixed_position(Some((0., 0.))).unwrap();
    gps.set_allowed(true);
    println!("legacy={legacy} module={module:?} GPS=0,0 SD=read-only");
    let start = Instant::now();
    let mut stopped = false;
    let mut stage = 0;
    emu.run_with_hook(&mut |state: &mut KernelState| {
        let elapsed = start.elapsed();
        if stage == 0 && elapsed > Duration::from_secs(8) {
            state
                .pending_input
                .push_back(InputEvent::KeyDown { vk: 13 });
            state.pending_input.push_back(InputEvent::KeyUp { vk: 13 });
            stage = 1;
        }
        if elapsed > Duration::from_secs(15) {
            stopped = true;
            FrameAction::Stop
        } else {
            FrameAction::Continue
        }
    })?;
    let state = &emu.process().unwrap().state;
    println!(
        "requested_stop={stopped} modules={} framebuffer={} active=0x{:08x}",
        state.modules.len(),
        state.framebuffer.frame_counter,
        state.active_window
    );
    std::fs::write(out, state.framebuffer.snapshot_ppm())?;
    if legacy {
        anyhow::ensure!(!stopped, "legacy failure did not reproduce");
    } else {
        anyhow::ensure!(
            stopped && state.modules.len() > 5,
            "startup still terminates before requested stop"
        );
    }
    Ok(())
}
