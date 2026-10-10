//! Targeted missing-API reports, appended beside the main log without verbose tracing.
use pocket_core::winceapi::{UnimplementedApiCall, UnimplementedApiSink};
use std::{
    collections::HashMap,
    fs::{File, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{SystemTime, UNIX_EPOCH},
};
pub struct UnimplementedLog {
    path: PathBuf,
    game: String,
    process: String,
    enabled: Arc<AtomicBool>,
    file: Option<File>,
    failed: bool,
    seen: HashMap<(u32, u32), (String, String)>,
    calls: u64,
}
fn timestamp() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
impl UnimplementedLog {
    pub fn new(path: PathBuf, game: String, process: String, enabled: Arc<AtomicBool>) -> Self {
        let mut sink = Self {
            path,
            game,
            process,
            enabled,
            file: None,
            failed: false,
            seen: HashMap::new(),
            calls: 0,
        };
        if sink.enabled() {
            sink.open();
        }
        sink
    }
    fn open(&mut self) -> bool {
        if self.file.is_some() {
            return true;
        }
        let result = (|| {
            if let Some(parent) = self.path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)?;
            writeln!(
                file,
                "{}",
                serde_json::json!({"event":"launch","timestamp_unix_ms":timestamp(),
                "game":self.game,"process":self.process})
            )?;
            file.flush()?;
            Ok::<_, std::io::Error>(file)
        })();
        match result {
            Ok(file) => {
                self.file = Some(file);
                true
            }
            Err(e) => {
                self.fail(e);
                false
            }
        }
    }
    fn fail(&mut self, error: std::io::Error) {
        self.failed = true;
        self.file = None;
        log::warn!(
            "Missing-API report {} unavailable: {}",
            self.path.display(),
            error
        );
    }
    fn append(&mut self, record: serde_json::Value) {
        if !self.open() {
            return;
        }
        let line = format!("{record}\n");
        let result = self
            .file
            .as_mut()
            .unwrap()
            .write_all(line.as_bytes())
            .and_then(|_| self.file.as_mut().unwrap().flush());
        if let Err(e) = result {
            self.fail(e);
        }
    }
}
impl UnimplementedApiSink for UnimplementedLog {
    fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed) && !self.failed
    }
    fn record(&mut self, call: UnimplementedApiCall<'_>) {
        if !self.enabled() {
            return;
        }
        self.calls = self.calls.saturating_add(1);
        let key = (call.thunk_va, call.caller);
        if self
            .seen
            .get(&key)
            .map(|(dll, api)| dll == call.dll && api == call.api.as_ref())
            .unwrap_or(false)
        {
            return;
        }
        self.seen
            .insert(key, (call.dll.into(), call.api.to_string()));
        self.append(
            serde_json::json!({"event":"unimplemented_api","timestamp_unix_ms":timestamp(),
            "game":self.game,"process":self.process,"dll":call.dll,"api":call.api,
            "caller":format!("0x{:08x}",call.caller),"thunk":format!("0x{:08x}",call.thunk_va),
            "arguments":call.args.map(|arg|format!("0x{arg:08x}")),"process_id":call.process_id,
            "thread_id":call.thread_id,"action":if call.halts{"halt"}else{"return_zero"},
            "detail":"No API handler registered; first occurrence at this call site"}),
        );
    }
}
impl Drop for UnimplementedLog {
    fn drop(&mut self) {
        if self.enabled() && self.file.is_some() {
            self.append(serde_json::json!({"event":"end",
            "timestamp_unix_ms":timestamp(),"game":self.game,"process":self.process,
            "missing_calls":self.calls,"unique_call_sites":self.seen.len()}));
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn call() -> UnimplementedApiCall<'static> {
        UnimplementedApiCall {
            dll: "missing.dll",
            api: "UnknownAPI".into(),
            thunk_va: 0x7000,
            args: [1, 2, 3, 4],
            caller: 0x1234,
            process_id: 4,
            thread_id: 2,
            halts: false,
        }
    }
    #[test]
    fn disabled_logging_creates_no_file_and_live_enable_records_details_without_flooding() {
        let path = std::env::temp_dir().join(format!(
            "pockethle-api-log-{}-{}.log",
            std::process::id(),
            timestamp()
        ));
        let enabled = Arc::new(AtomicBool::new(false));
        let mut sink = UnimplementedLog::new(
            path.clone(),
            "Game \"Test\"".into(),
            "child.exe".into(),
            enabled.clone(),
        );
        sink.record(call());
        assert!(!path.exists());
        enabled.store(true, Ordering::Relaxed);
        sink.record(call());
        sink.record(call());
        enabled.store(false, Ordering::Relaxed);
        sink.record(call());
        let before = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<serde_json::Value> = before
            .lines()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[1]["game"], "Game \"Test\"");
        assert_eq!(lines[1]["process"], "child.exe");
        assert_eq!(lines[1]["arguments"][3], "0x00000004");
        assert_eq!(lines[1]["caller"], "0x00001234");
        drop(sink);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn reused_thunk_with_different_api_is_not_hidden_and_sessions_append() {
        let path = std::env::temp_dir().join(format!(
            "pockethle-api-slots-{}-{}.log",
            std::process::id(),
            timestamp()
        ));
        let enabled = Arc::new(AtomicBool::new(true));
        for _ in 0..2 {
            let mut sink = UnimplementedLog::new(
                path.clone(),
                "Game".into(),
                "parent.exe".into(),
                enabled.clone(),
            );
            sink.record(call());
            let mut next = call();
            next.api = "OtherAPI".into();
            sink.record(next);
        }
        let lines: Vec<serde_json::Value> = std::fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect();
        assert_eq!(lines.len(), 8);
        assert_eq!(lines[2]["api"], "OtherAPI");
        assert_eq!(lines[3]["missing_calls"], 2);
        std::fs::remove_file(path).unwrap();
    }
}
