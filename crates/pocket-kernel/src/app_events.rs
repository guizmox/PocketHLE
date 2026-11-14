//! Session-wide CE application notifications. Names and command lines are
//! owned strings, independent of the registering process's guest memory.
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Default)]
pub(crate) struct AppEvents {
    registrations: BTreeMap<String, (String, BTreeSet<u32>)>,
    pending: VecDeque<(String, String)>,
}
impl AppEvents {
    pub fn register(&mut self, name: String, event: u32) -> Result<(), u32> {
        if event > 15 || name.is_empty() {
            return Err(87);
        }
        let key = name.replace('/', "\\").to_lowercase();
        if event == 0 {
            self.registrations.remove(&key);
            self.pending
                .retain(|(app, _)| app.replace('/', "\\").to_lowercase() != key);
        } else {
            if !self.registrations.contains_key(&key) && self.registrations.len() >= 1024 {
                return Err(8);
            }
            self.registrations
                .entry(key)
                .or_insert_with(|| (name, BTreeSet::new()))
                .1
                .insert(event);
        }
        Ok(())
    }
    pub fn emit(&mut self, event: u32) {
        let command = match event {
            1 => "AppRunAfterTimeChange",
            2 => "AppRunAfterSync",
            3 => "AppRunAtAcPowerOn",
            4 => "AppRunAtAcPowerOff",
            5 => "AppRunAtNetConnect",
            6 => "AppRunAtNetDisconnect",
            7 => "AppRunDeviceChange",
            8 => "AppRunAtIrDiscovery",
            9 => "AppRunAtRs232Detect",
            10 => "AppRunAfterRestore",
            11 => "AppRunAfterWakeup",
            12 => "AppRunAfterTzChange",
            13 => "AppRunAfterExtendedEvent",
            14 => "AppRunAfterRndisFnDetected",
            15 => "AppRunAfterInternetProxyChange",
            _ => return,
        };
        for (app, events) in self.registrations.values() {
            if events.contains(&event)
                && !self.pending.iter().any(|(a, c)| a == app && c == command)
            {
                self.pending.push_back((app.clone(), command.into()));
            }
        }
    }
    pub fn take(&mut self) -> Option<(String, String)> {
        self.pending.pop_front()
    }
}
