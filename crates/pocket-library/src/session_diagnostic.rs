use std::cell::RefCell;
use std::collections::VecDeque;

use serde::Serialize;

thread_local! {
    static SESSION_LOG: RefCell<Option<VecDeque<String>>> = const { RefCell::new(None) };
}

pub fn capture<T>(run: impl FnOnce() -> T) -> (T, Vec<String>) {
    SESSION_LOG.with(|slot| *slot.borrow_mut() = Some(VecDeque::new()));
    let result = run();
    let log = SESSION_LOG.with(|slot| slot.borrow_mut().take().unwrap_or_default());
    (result, log.into())
}

pub fn record(record: &log::Record<'_>) {
    if record.level() > log::Level::Info {
        return;
    }
    SESSION_LOG.with(|slot| {
        if let Some(lines) = slot.borrow_mut().as_mut() {
            if lines.len() == 64 {
                lines.pop_front();
            }
            let line = format!("[{} {}] {}", record.level(), record.target(), record.args());
            lines.push_back(line.chars().take(2048).collect());
        }
    });
}

#[derive(Debug, Serialize)]
pub struct Diagnostic {
    pub title: &'static str,
    pub reason: String,
    pub details: String,
}

pub fn startup_failure(reason: String, log: Vec<String>) -> Diagnostic {
    Diagnostic {
        title: "Could not start the game",
        details: [reason.as_str(), &log.join("\n")].join("\n\n"),
        reason,
    }
}

/// Limit exit-code and stderr heuristics to startup, excluding frontend teardown.
pub const STARTUP_DIAGNOSTIC_WINDOW: std::time::Duration = std::time::Duration::from_secs(2);

pub fn classify(
    stopped: bool,
    started: bool,
    exit_code: u32,
    startup_exit: bool,
    runtime_error: Option<&str>,
    summary: &str,
    log: Vec<String>,
) -> Option<Diagnostic> {
    if stopped {
        return None;
    }
    let code = format!("Exit code 0x{exit_code:08X}.");
    let (title, reason) = if !started {
        (
            "Could not start the game",
            summary.lines().last().unwrap_or(&code).to_owned(),
        )
    } else if let Some(error) = runtime_error {
        ("Emulation stopped", error.to_owned())
    } else if startup_exit && exit_code != 0 {
        ("The game exited during startup", code.clone())
    } else {
        let reason = startup_exit
            .then(|| {
                log.iter()
                    .rev()
                    .find_map(|line| line.strip_prefix("[INFO pockethle_guest_stderr] "))
            })
            .flatten()?;
        ("The game exited during startup", reason.to_owned())
    };
    Some(Diagnostic {
        title,
        reason,
        details: [code.as_str(), summary, &log.join("\n")].join("\n\n"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_stderr_explains_a_clean_exit_without_reporting_normal_quits() {
        let stderr = || vec!["[INFO pockethle_guest_stderr] Initialization failed".into()];
        let diagnostic = classify(false, true, 0, true, None, "", stderr()).unwrap();
        assert_eq!(diagnostic.reason, "Initialization failed");
        assert!(diagnostic.details.contains("Exit code 0x00000000."));
        assert!(classify(false, true, 0, false, None, "", stderr()).is_none());
        assert!(classify(false, true, 0, true, None, "", vec![]).is_none());
        assert!(classify(true, true, 0, true, None, "", stderr()).is_none());
    }

    #[test]
    fn guest_exit_codes_trigger_diagnostics_during_startup_only() {
        assert!(classify(false, true, 13, false, None, "", vec![]).is_none());

        let startup = classify(false, true, 13, true, None, "", vec![]).unwrap();
        assert_eq!(startup.reason, "Exit code 0x0000000D.");

        let crash =
            classify(false, true, 13, false, Some("CPU exception"), "", vec![]).unwrap();
        assert_eq!(crash.reason, "CPU exception");
    }
}
