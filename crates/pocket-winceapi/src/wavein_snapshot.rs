//! Reopen real input devices; retain only guest-owned queues and callback state.
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(crate) struct InputSnapshot {
    next: u32,
    devices: HashMap<u32, SavedCapture>,
    closed: HashMap<u32, WaveOutDevice>,
}
#[derive(Serialize, Deserialize)]
struct SavedCapture {
    meta: WaveOutDevice,
    active: bool,
    buffers: VecDeque<Buffer>,
    #[serde(with = "serde_bytes")]
    pcm: Vec<u8>,
    bytes: u64,
}
impl InputSnapshot {
    pub(crate) fn capture() -> Result<Self, String> {
        INPUTS.with(|registry| {
            let inputs = registry
                .lock()
                .map_err(|_| "microphone registry poisoned")?;
            let saved = Self {
                next: inputs.next,
                closed: inputs.closed.clone(),
                devices: inputs
                    .devices
                    .iter()
                    .map(|(&handle, device)| {
                        (
                            handle,
                            SavedCapture {
                                meta: device.meta,
                                active: device
                                    .resume_active
                                    .unwrap_or_else(|| device.active.load(Ordering::Acquire)),
                                buffers: device.buffers.clone(),
                                pcm: device.pcm.iter().copied().collect(),
                                bytes: device.bytes,
                            },
                        )
                    })
                    .collect(),
            };
            saved.validate()?;
            Ok(saved)
        })
    }
    fn validate(&self) -> Result<(), String> {
        let valid_format = |meta: &WaveOutDevice| {
            let format = meta.format;
            (1..=96000).contains(&format.sample_rate)
                && matches!(format.channels, 1 | 2)
                && matches!(format.bits_per_sample, 8 | 16)
        };
        if self.devices.len() > 256
            || self.closed.len() > 256
            || self.next < 0xdead5100
            || self.closed.iter().any(|(&handle, meta)| {
                handle < 0xdead5100 || handle >= self.next || !valid_format(meta)
            })
            || self.devices.iter().any(|(&handle, device)| {
                handle < 0xdead5100
                    || handle >= self.next
                    || self.closed.contains_key(&handle)
                    || !valid_format(&device.meta)
                    || device.buffers.len() > 4096
                    || device.pcm.len() > 16 * 1024 * 1024
                    || device.buffers.iter().any(|buffer| {
                        buffer.hdr == 0
                            || buffer.hdr.checked_add(32).is_none()
                            || buffer.data == 0
                            || buffer.data.checked_add(buffer.len).is_none()
                            || buffer.written > buffer.len
                    })
            })
        {
            return Err("invalid saved microphone state".into());
        }
        Ok(())
    }
    pub(crate) fn restore(self) -> Result<(), String> {
        self.validate()?;
        let mut inputs = Inputs {
            next: self.next,
            closed: self.closed,
            devices: HashMap::new(),
        };
        for (handle, saved) in self.devices {
            let (rx, quit, active, worker) = backend(saved.meta.format)
                .map_err(|error| format!("cannot reopen microphone provider: {error}"))?;
            inputs.devices.insert(
                handle,
                Capture {
                    meta: saved.meta,
                    active,
                    resume_active: Some(saved.active),
                    rx,
                    quit,
                    worker: Some(worker),
                    buffers: saved.buffers,
                    pcm: saved.pcm.into(),
                    bytes: saved.bytes,
                },
            );
        }
        // Replacement drops/joins any previous workers. New workers stay gated
        // until this process services its first resumed WinCE API.
        INPUTS.with(|registry| {
            *registry
                .lock()
                .map_err(|_| "microphone registry poisoned")? = inputs;
            Ok(())
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn meta() -> WaveOutDevice {
        WaveOutDevice {
            callback_kind: WaveCallbackKind::Event,
            callback_target: 0xdead6100,
            instance: 7,
            owner_thread: 0,
            format: GuestFormat {
                sample_rate: 22050,
                channels: 1,
                bits_per_sample: 16,
            },
            paused: false,
        }
    }
    #[test]
    fn closed_capture_retains_pending_close_callback_and_allocator() {
        reset();
        INPUTS.with(|inputs| {
            let mut inputs = inputs.lock().unwrap();
            inputs.next = 0xdead5102;
            inputs.closed.insert(0xdead5101, meta());
        });
        let saved = InputSnapshot::capture().unwrap();
        reset();
        saved.restore().unwrap();
        assert_eq!(callback_device(0xdead5101, 0x3bf).unwrap().instance, 7);
        assert!(callback_device(0xdead5101, 0x3bf).is_none());
        INPUTS.with(|inputs| assert_eq!(inputs.lock().unwrap().next, 0xdead5102));
        reset();
    }
    #[cfg(not(feature = "audio-cpal"))]
    #[test]
    fn active_capture_requires_real_backend_instead_of_silent_input() {
        let saved = InputSnapshot {
            next: 0xdead5101,
            closed: HashMap::new(),
            devices: HashMap::from([(
                0xdead5100,
                SavedCapture {
                    meta: meta(),
                    active: true,
                    buffers: VecDeque::new(),
                    pcm: vec![1, 2],
                    bytes: 12,
                },
            )]),
        };
        assert!(saved
            .restore()
            .unwrap_err()
            .contains("microphone backend is disabled"));
    }
}
