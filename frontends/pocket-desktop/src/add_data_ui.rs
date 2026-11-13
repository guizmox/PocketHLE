//! Shared data workflow, using the launcher file picker and controller widgets.
use super::add_data_prompt::DataChoice;
use super::*;
use pocket_library::add_data::{self, PreparedData};
use std::path::PathBuf;

impl PocketLauncher {
    pub(super) fn prepare_data(&mut self, id: String, path: PathBuf, extract: bool) {
        self.data_game = Some(id.clone());
        self.library_busy = true;
        self.status = "Checking game data…".into();
        let root = self.library.root().to_path_buf();
        let profile = self.library.profile();
        let tx = self.events_tx.clone();
        std::thread::spawn(move || {
            let result = (|| -> anyhow::Result<PreparedData> {
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .ok_or_else(|| anyhow::anyhow!("Invalid filename"))?;
                add_data::prepare(
                    &Library::open_with_profile(root, profile)?,
                    &id,
                    &path,
                    name,
                    extract,
                )
            })()
            .map_err(|e| format!("{e:#}"));
            let _ = tx.send(UiEvent::DataPrepared(result));
        });
    }

    pub(super) fn apply_data(&mut self, pending: PreparedData, overwrite: bool) {
        if self.running_game.is_some() || self.pending_run.is_some() {
            self.add_data = Some(DataPrompt::Notice {
                title: "Add/Replace Data failed",
                message: "Stop the running game before adding data.".into(),
            });
            return;
        }
        self.library_busy = true;
        self.status = "Adding game data…".into();
        let root = self.library.root().to_path_buf();
        let profile = self.library.profile();
        let tx = self.events_tx.clone();
        std::thread::spawn(move || {
            let result = (|| -> anyhow::Result<(String, Option<String>)> {
                let destination = pending.destination().display().to_string();
                let selected_game = pending.selected_game_id().map(str::to_owned);
                pending.apply(&mut Library::open_with_profile(root, profile)?, overwrite)?;
                Ok((format!(
                    "Data added successfully.\n\nFiles were placed in:\n{destination}"
                ), selected_game))
            })()
            .map_err(|e| format!("{e:#}"));
            let _ = tx.send(UiEvent::DataFinished(result));
        });
    }

    pub(super) fn confirm_data(&mut self, pending: PreparedData) {
        if pending.conflicts().is_empty() {
            self.apply_data(pending, false);
        } else {
            self.add_data = Some(DataPrompt::Overwrite(pending));
        }
    }

    pub(super) fn ui_add_data(&mut self, ctx: &egui::Context) {
        let Some(prompt) = self.add_data.take() else {
            return;
        };
        let Some(choice) = prompt.show(ctx) else {
            self.add_data = Some(prompt);
            return;
        };
        // Only clear input on a transition, after the current frame has consumed
        // it. Clearing every draw would discard queued controller presses.
        self.controller_ui.stop_repeat();
        match (prompt, choice) {
            (_, DataChoice::Cancel) => self.status = "Add/Replace Data closed.".into(),
            (DataPrompt::Zip { id, path }, DataChoice::Zip(extract)) => {
                self.prepare_data(id, path, extract)
            }
            (DataPrompt::Executable(mut pending), DataChoice::Executable(selected)) => {
                match pending.choose_executable(selected.as_deref()) {
                    Ok(()) => self.confirm_data(pending),
                    Err(error) => {
                        self.add_data = Some(DataPrompt::Notice {
                            title: "Add/Replace Data failed",
                            message: error.to_string(),
                        })
                    }
                }
            }
            (DataPrompt::Overwrite(pending), DataChoice::Overwrite) => {
                self.apply_data(pending, true)
            }
            (prompt, _) => self.add_data = Some(prompt),
        }
    }
}
