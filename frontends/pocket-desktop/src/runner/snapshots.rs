//! Desktop adapters for the shared session engine. No checkpoint codec lives here.
use super::*;
use pocket_session::snapshot::{self, Child, Host, ProcessSnapshots};

impl Host for RunHook {
    fn snapshots(&mut self) -> &mut ProcessSnapshots {
        &mut self.snapshots
    }
    fn control(&self) -> pocket_session::Control {
        pocket_session::Control {
            stop: self.stop_all.clone(),
            error: self.session_error.clone(),
            foreground: self.foreground.clone(),
        }
    }
    fn pid(&self) -> u32 {
        self.pid
    }
    fn launch_context(&self) -> Option<pocket_session::LaunchContext> {
        self.launch_context
            .as_ref()
            .map(|context| pocket_session::LaunchContext {
                config: context.config.clone(),
                library: context.library_root.clone(),
                game: context.game.clone(),
                card: context.card_root.clone(),
            })
    }
    fn spawn(&mut self, child: Child) -> Result<(), String> {
        let runner = self
            .launch_context
            .as_ref()
            .ok_or("missing desktop launch context")?
            .runner
            .clone();
        let mut hook = self.child_hook();
        hook.snapshots.restore = child.snapshot;
        hook.snapshots.restore_group = child.restore_group;
        let startup = child.command_line.map(|command_line| ProcessStartup {
            ack: None,
            table: child.table.clone(),
            command_line,
            error: 193,
        });
        let job = snapshot::spawn_job(child.table, child.parent, self.control(), move || {
            runner.run_process(
                &child.context.library,
                &child.game,
                &child.context.card,
                Some(&child.guest),
                child.ram,
                child.handles,
                startup,
                &mut hook,
            )
        })
        .map_err(|error| error.to_string())?;
        self.children.push(job);
        Ok(())
    }
    fn reap_children(&mut self) {
        RunHook::reap_children(self);
    }
    fn reset_foreground(&mut self) {
        self.reset_process();
    }
    fn fail(&mut self, error: String) {
        self.runtime_error = Some(error.clone());
        self.control().fail(error);
    }
}
impl RunHook {
    pub(super) fn restore_checkpoint(
        &mut self,
        emulator: &mut pocket_core::Emulator,
        saved: pocket_core::SessionSnapshot,
    ) -> anyhow::Result<pocket_core::RestoredSession> {
        snapshot::restore(self, emulator, saved)
    }
    pub(super) fn capture_checkpoint(
        &mut self,
        cpu: &mut dyn pocket_core::cpu::Cpu,
        process: &mut pocket_core::kernel::Process,
        point: pocket_core::kernel::ResumePoint,
    ) {
        snapshot::capture(self, cpu, process, point, None);
    }
    pub(super) fn run_suspended_child(
        &mut self,
        cpu: &mut dyn pocket_core::cpu::Cpu,
        process: &mut pocket_core::kernel::Process,
        point: pocket_core::kernel::ResumePoint,
    ) {
        snapshot::wait_for_child(self, cpu, process, point);
    }
}
