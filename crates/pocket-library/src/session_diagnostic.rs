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

pub fn classify(
    stopped: bool,
    started: bool,
    produced_frame: bool,
    exit_code: u32,
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
    } else if exit_code != 0 {
        ("The game exited with an error", code.clone())
    } else if !produced_frame {
        (
            "The game exited during startup",
            format!("{code} The reason is unknown."),
        )
    } else {
        return None;
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
    fn reports_startup_exit_and_runtime_errors_but_allows_normal_return() {
        assert!(classify(false, true, false, 0, None, "", vec![]).is_some());
        assert!(classify(false, true, true, 0, None, "", vec![]).is_none());
        assert!(classify(true, true, false, 0, None, "", vec![]).is_none());
        assert!(classify(false, true, true, 0, Some("ARM fault"), "", vec![]).is_some());
    }
}
