//! Android process adapters for the same checkpoint engine used by Desktop.
use super::*;
use pocket_session::snapshot::{self, Child, Host, ProcessSnapshots};
impl Host for SessionHook {
    fn snapshots(&mut self) -> &mut ProcessSnapshots {
        &mut self.snapshots
    }
    fn control(&self) -> pocket_session::Control {
        pocket_session::Control {
            stop: self.stop_all.clone(),
            foreground: self.foreground.clone(),
            error: self.state.session_error.clone(),
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
                game: context.entry.clone(),
                card: context.card_root.clone(),
            })
    }
    fn spawn(&mut self, child: Child) -> Result<(), String> {
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
            run_process(
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
        SessionHook::reap_children(self);
    }
    fn reset_foreground(&mut self) {
        self.last_frame = 0;
        self.last_emit_at = None;
        self.last_shared = None;
        self.saw_non_black = false;
    }
    fn fail(&mut self, error: String) {
        self.runtime_error = Some(error.clone());
        self.control().fail(error);
    }
}
