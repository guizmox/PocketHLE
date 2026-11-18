//! External peers cannot be rewound. Retain received data and guest identities,
//! then report the real loss of the captured transport after restoring it.
use super::*;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
const BUFFER_LIMIT: usize = 65536;
const CONNECTION_RESET: u32 = 10054;

#[derive(Clone, Serialize, Deserialize)]
pub struct TransportSnapshot {
    #[serde(with = "serde_bytes")]
    bytes: Vec<u8>,
    end: BtResult<()>,
}
impl TransportSnapshot {
    pub fn new(bytes: Vec<u8>, end: BtResult<()>) -> Result<Self, String> {
        if bytes.len() > BUFFER_LIMIT || end == Err(0) {
            return Err("invalid Bluetooth receive checkpoint".into());
        }
        Ok(Self { bytes, end })
    }
    pub fn interrupted(error: u32) -> Self {
        Self {
            bytes: Vec::new(),
            end: Err(error),
        }
    }
    fn read(&mut self, out: &mut [u8], peek: bool) -> BtResult<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        if self.bytes.is_empty() {
            return self.end.map(|_| 0);
        }
        let n = out.len().min(self.bytes.len());
        out[..n].copy_from_slice(&self.bytes[..n]);
        if !peek {
            self.bytes.drain(..n);
        }
        Ok(n)
    }
}
struct InterruptedTransport(TransportSnapshot);
impl InterruptedTransport {
    fn error(&self) -> u32 {
        self.0.end.err().unwrap_or(CONNECTION_RESET)
    }
}
impl Stream for InterruptedTransport {
    fn read(&mut self, bytes: &mut [u8]) -> BtResult<usize> {
        self.0.read(bytes, false)
    }
    fn write(&mut self, _: &[u8]) -> BtResult<usize> {
        Err(self.error())
    }
    fn capture_received(&self) -> Result<TransportSnapshot, String> {
        Ok(self.0.clone())
    }
}
impl RfcommSocket for InterruptedTransport {
    fn bind(&mut self, _: &SocketAddress) -> BtResult<()> {
        Err(self.error())
    }
    fn listen(&mut self, _: i32) -> BtResult<()> {
        Err(self.error())
    }
    fn connect(&mut self, _: &SocketAddress) -> BtResult<()> {
        Err(self.error())
    }
    fn accept(&mut self) -> BtResult<(Box<dyn RfcommSocket>, SocketAddress)> {
        Err(self.error())
    }
    fn read(&mut self, out: &mut [u8], flags: i32) -> BtResult<usize> {
        self.0.read(out, flags & 2 != 0)
    }
    fn write(&mut self, _: &[u8], _: i32) -> BtResult<usize> {
        Err(self.error())
    }
    fn readiness(&mut self) -> BtResult<(bool, bool, bool)> {
        if self.0.bytes.is_empty() {
            self.0.end.map(|_| (true, false, false))
        } else {
            Ok((true, false, self.0.end.is_err()))
        }
    }
    fn available(&mut self) -> BtResult<u32> {
        if self.0.bytes.is_empty() {
            self.0.end.map(|_| 0)
        } else {
            Ok(self.0.bytes.len() as u32)
        }
    }
    fn set_option(&mut self, _: i32, _: i32, _: &[u8]) -> BtResult<()> {
        Err(self.error())
    }
    fn get_option(&mut self, level: i32, name: i32, size: usize) -> BtResult<Vec<u8>> {
        // SO_ERROR reports the logical transport loss; no new host socket is
        // represented as connected, writable or successfully reconfigured.
        if level == 0xffff && name == 0x1007 && size >= 4 {
            Ok(self.error().to_le_bytes().to_vec())
        } else {
            Err(self.error())
        }
    }
    fn address(&mut self, _: bool) -> BtResult<SocketAddress> {
        Err(self.error())
    }
    fn shutdown(&mut self, _: i32) -> BtResult<()> {
        Err(self.error())
    }
    fn capture_received(&self) -> Result<TransportSnapshot, String> {
        Ok(self.0.clone())
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct PortSnapshot {
    id: u64,
    mask: u32,
    received: TransportSnapshot,
}
impl Port {
    pub(crate) fn capture_snapshot(&self, at: Instant) -> Result<Arc<PortSnapshot>, String> {
        // All aliases/processes use the first immutable receive boundary for
        // this checkpoint epoch. A weak cache retains no history during play.
        let mut checkpoint = self
            .checkpoint
            .lock()
            .map_err(|_| "Bluetooth checkpoint cache poisoned")?;
        if let Some((epoch, saved)) = &*checkpoint {
            if *epoch == at {
                if let Some(saved) = saved.upgrade() {
                    return Ok(saved);
                }
            }
        }
        // Match the I/O lock order. Capture never consumes native bytes.
        let stream = self.stream.lock().map_err(|_| "Bluetooth port poisoned")?;
        let rx = self
            .rx
            .lock()
            .map_err(|_| "Bluetooth receive queue poisoned")?;
        let received = match stream.as_ref() {
            Some(stream) => {
                let tail = stream.capture_received()?;
                let mut bytes: Vec<u8> = rx.iter().copied().collect();
                bytes.extend(&tail.bytes);
                TransportSnapshot::new(bytes, tail.end)?
            }
            None => TransportSnapshot::interrupted(CANCELLED),
        };
        let saved = Arc::new(PortSnapshot {
            id: self as *const Self as u64,
            mask: self.mask.load(std::sync::atomic::Ordering::Acquire),
            received,
        });
        *checkpoint = Some((at, Arc::downgrade(&saved)));
        Ok(saved)
    }
}
impl Service {
    pub(crate) fn restore_port(&self, saved: Arc<PortSnapshot>) -> Result<Arc<Port>, String> {
        TransportSnapshot::new(saved.received.bytes.clone(), saved.received.end)?;
        if saved.id == 0 {
            return Err("invalid Bluetooth port identity".into());
        }
        let mut service = self.0.lock().map_err(|_| "Bluetooth service poisoned")?;
        if let Some(port) = service
            .restored_ports
            .get(&saved.id)
            .and_then(|weak| weak.upgrade())
        {
            return Ok(port);
        }
        let port = Arc::new(Port::new(Box::new(InterruptedTransport(
            saved.received.clone(),
        ))));
        port.mask
            .store(saved.mask, std::sync::atomic::Ordering::Release);
        service
            .restored_ports
            .retain(|_, weak| weak.strong_count() != 0);
        service
            .restored_ports
            .insert(saved.id, Arc::downgrade(&port));
        Ok(port)
    }
}
#[derive(Serialize, Deserialize)]
pub(crate) struct StateSnapshot {
    enabled: bool,
    startups: u32,
    discovery_aliases: HashMap<u64, u64>,
    next_lookup: u32,
    next_socket: u32,
    next_port: u32,
    ports: Vec<(u32, u32, Arc<PortSnapshot>)>,
    sockets: Vec<(u32, TransportSnapshot, bool, bool, u32, u32)>,
    lookups: Vec<(u32, BtResult<Vec<Device>>, usize)>,
    writes: Vec<((usize, u32, u32), u32, u32, Vec<u8>, usize)>,
    deadlines: Vec<((usize, u32, u32), Duration)>,
}
impl State {
    pub(crate) fn capture_snapshot(&self, at: Instant) -> Result<StateSnapshot, String> {
        let service = self
            .service
            .0
            .lock()
            .map_err(|_| "Bluetooth service poisoned")?;
        let ports = service
            .ports
            .iter()
            .map(|(&i, (h, p))| Ok((i, *h, p.capture_snapshot(at)?)))
            .collect::<Result<_, String>>()?;
        let sockets = self
            .sockets
            .iter()
            .map(|(&id, s)| {
                Ok((
                    id,
                    s.native.capture_received()?,
                    s.nonblocking,
                    s.connecting,
                    s.recv_timeout_ms,
                    s.send_timeout_ms,
                ))
            })
            .collect::<Result<_, String>>()?;
        let lookups = self
            .lookups
            .iter()
            .map(|(&id, l)| {
                Ok((
                    id,
                    l.result
                        .lock()
                        .map_err(|_| "Bluetooth lookup poisoned")?
                        .clone()
                        .unwrap_or(Err(CANCELLED)),
                    l.index,
                ))
            })
            .collect::<Result<_, String>>()?;
        let writes = self
            .writes
            .iter()
            .map(|(&key, w)| (key, w.handle, w.pointer, w.bytes.clone(), w.offset))
            .collect();
        let now = at;
        let saved = StateSnapshot {
            enabled: service.enabled,
            startups: self.startups,
            discovery_aliases: self.discovery_aliases.clone(),
            next_lookup: self.next_lookup,
            next_socket: self.next_socket,
            next_port: service.next,
            ports,
            sockets,
            lookups,
            writes,
            deadlines: self
                .socket_deadlines
                .iter()
                .map(|(&key, t)| (key, t.saturating_duration_since(now)))
                .collect(),
        };
        saved.validate()?;
        Ok(saved)
    }
    pub(crate) fn restore_snapshot(&mut self, saved: StateSnapshot) -> Result<(), String> {
        saved.validate()?;
        if saved.enabled {
            self.service
                .start_driver()
                .map_err(|code| format!("cannot restore Bluetooth driver: WinCE error {code}"))?;
        }
        let mut ports = HashMap::new();
        for (index, handle, port) in saved.ports {
            if !(1..=9).contains(&index)
                || handle < 0xb7000000
                || handle >= saved.next_port
                || ports.contains_key(&index)
            {
                return Err("invalid Bluetooth registration".into());
            }
            ports.insert(index, (handle, self.service.restore_port(port)?));
        }
        {
            let mut service = self
                .service
                .0
                .lock()
                .map_err(|_| "Bluetooth service poisoned")?;
            for (&index, (handle, port)) in &ports {
                if let Some((old_handle, old_port)) = service.ports.get(&index) {
                    if old_handle != handle || !Arc::ptr_eq(old_port, port) {
                        return Err("conflicting shared Bluetooth port registration".into());
                    }
                }
            }
            service.next = saved.next_port;
            service.ports.extend(ports);
        }
        let mut sockets = HashMap::new();
        for (id, tail, nonblocking, connecting, recv_timeout_ms, send_timeout_ms) in saved.sockets {
            TransportSnapshot::new(tail.bytes.clone(), tail.end)?;
            if id < 0xb7200000 || id >= saved.next_socket || sockets.contains_key(&id) {
                return Err("invalid Bluetooth socket handle".into());
            }
            let interrupted: Box<dyn RfcommSocket> = Box::new(InterruptedTransport(tail));
            let native = if saved.enabled {
                self.service.manage_socket(interrupted).map_err(|code| {
                    format!("cannot restore Bluetooth socket: WinCE error {code}")
                })?
            } else {
                interrupted
            };
            sockets.insert(
                id,
                GuestSocket {
                    native,
                    nonblocking,
                    connecting,
                    recv_timeout_ms,
                    send_timeout_ms,
                },
            );
        }
        let mut lookups = HashMap::new();
        for (id, result, index) in saved.lookups {
            if id < 0xb7100000
                || id >= saved.next_lookup
                || lookups.contains_key(&id)
                || result
                    .as_ref()
                    .is_ok_and(|v| v.len() > 1024 || index > v.len())
            {
                return Err("invalid Bluetooth inquiry".into());
            }
            lookups.insert(
                id,
                Lookup {
                    result: Arc::new(Mutex::new(Some(result))),
                    index,
                },
            );
        }
        let mut writes = HashMap::new();
        for (key, handle, pointer, bytes, offset) in saved.writes {
            if bytes.len() > 16 * 1024 * 1024 || offset > bytes.len() || writes.contains_key(&key) {
                return Err("invalid pending Bluetooth write".into());
            }
            writes.insert(
                key,
                PendingWrite {
                    handle,
                    pointer,
                    bytes,
                    offset,
                },
            );
        }
        let mut deadlines = HashMap::new();
        for (key, delay) in saved.deadlines {
            if delay > Duration::from_millis(u32::MAX as u64)
                || deadlines.insert(key, Instant::now() + delay).is_some()
            {
                return Err("invalid Bluetooth deadline".into());
            }
        }
        self.startups = saved.startups;
        self.discovery_aliases = saved.discovery_aliases;
        self.next_lookup = saved.next_lookup;
        self.next_socket = saved.next_socket;
        self.sockets = sockets;
        self.lookups = lookups;
        self.writes = writes;
        self.socket_deadlines = deadlines;
        Ok(())
    }
}

impl StateSnapshot {
    fn validate(&self) -> Result<(), String> {
        if self.next_port < 0xb7000000
            || (self.next_lookup != 0 && self.next_lookup < 0xb7100000)
            || (self.next_socket != 0 && self.next_socket < 0xb7200000)
        {
            return Err("invalid Bluetooth handle allocator".into());
        }
        if self.sockets.len() > 256
            || self.ports.len() > 9
            || self.lookups.len() > 256
            || self.writes.len() > 256
            || self.deadlines.len() > 1024
            || self.discovery_aliases.len() > 1024
        {
            return Err("Bluetooth checkpoint exceeds its resource limits".into());
        }
        let mut bytes = 0usize;
        for (_, _, port) in &self.ports {
            if port.id == 0
                || port.received.bytes.len() > BUFFER_LIMIT
                || port.received.end == Err(0)
            {
                return Err("invalid Bluetooth port checkpoint".into());
            }
            bytes = bytes
                .checked_add(port.received.bytes.len())
                .ok_or("Bluetooth checkpoint size overflow")?;
        }
        for (_, tail, ..) in &self.sockets {
            if tail.bytes.len() > BUFFER_LIMIT || tail.end == Err(0) {
                return Err("invalid Bluetooth socket checkpoint".into());
            }
            bytes = bytes
                .checked_add(tail.bytes.len())
                .ok_or("Bluetooth checkpoint size overflow")?;
        }
        for (_, _, _, data, offset) in &self.writes {
            if *offset > data.len() {
                return Err("invalid pending Bluetooth write".into());
            }
            bytes = bytes
                .checked_add(data.len())
                .ok_or("Bluetooth checkpoint size overflow")?;
        }
        if bytes > 64 * 1024 * 1024 {
            return Err("Bluetooth checkpoint exceeds 64 MiB".into());
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "bluetooth_snapshot_tests.rs"]
mod tests;
