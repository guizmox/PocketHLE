//! Real loopback I/O validates the transport boundary, not a physical BT radio.
use super::*;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
struct TcpTransport(TcpStream);
fn io_error(error: std::io::Error) -> u32 {
    if error.kind() == std::io::ErrorKind::WouldBlock {
        WOULD_BLOCK
    } else {
        10054
    }
}
impl TcpTransport {
    fn snapshot(&self) -> Result<TransportSnapshot, String> {
        let mut bytes = vec![0; 65536];
        match self.0.peek(&mut bytes) {
            Ok(n) => {
                bytes.truncate(n);
                TransportSnapshot::new(bytes, if n == 0 { Ok(()) } else { Err(10054) })
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                Ok(TransportSnapshot::interrupted(10054))
            }
            Err(error) => Err(error.to_string()),
        }
    }
}
impl Stream for TcpTransport {
    fn read(&mut self, out: &mut [u8]) -> BtResult<usize> {
        self.0.read(out).map_err(io_error)
    }
    fn write(&mut self, data: &[u8]) -> BtResult<usize> {
        self.0.write(data).map_err(io_error)
    }
    fn capture_received(&self) -> Result<TransportSnapshot, String> {
        self.snapshot()
    }
}
impl RfcommSocket for TcpTransport {
    fn bind(&mut self, _: &SocketAddress) -> BtResult<()> {
        Err(10045)
    }
    fn listen(&mut self, _: i32) -> BtResult<()> {
        Err(10045)
    }
    fn connect(&mut self, _: &SocketAddress) -> BtResult<()> {
        Err(10056)
    }
    fn accept(&mut self) -> BtResult<(Box<dyn RfcommSocket>, SocketAddress)> {
        Err(10045)
    }
    fn read(&mut self, out: &mut [u8], flags: i32) -> BtResult<usize> {
        if flags & 2 != 0 {
            self.0.peek(out).map_err(io_error)
        } else {
            self.0.read(out).map_err(io_error)
        }
    }
    fn write(&mut self, data: &[u8], _: i32) -> BtResult<usize> {
        self.0.write(data).map_err(io_error)
    }
    fn readiness(&mut self) -> BtResult<(bool, bool, bool)> {
        let mut b = [0];
        match self.0.peek(&mut b) {
            Ok(_) => Ok((true, true, false)),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok((false, true, false)),
            Err(e) => Err(io_error(e)),
        }
    }
    fn available(&mut self) -> BtResult<u32> {
        let mut bytes = [0; 65536];
        self.0.peek(&mut bytes).map(|n| n as u32).map_err(io_error)
    }
    fn set_option(&mut self, _: i32, _: i32, _: &[u8]) -> BtResult<()> {
        Err(10045)
    }
    fn get_option(&mut self, _: i32, _: i32, _: usize) -> BtResult<Vec<u8>> {
        Err(10045)
    }
    fn address(&mut self, _: bool) -> BtResult<SocketAddress> {
        Err(10045)
    }
    fn shutdown(&mut self, _: i32) -> BtResult<()> {
        self.0.shutdown(std::net::Shutdown::Both).map_err(io_error)
    }
    fn capture_received(&self) -> Result<TransportSnapshot, String> {
        self.snapshot()
    }
}
struct Provider(Arc<AtomicUsize>);
impl Backend for Provider {
    fn hostname(&self) -> BtResult<String> {
        Ok("loopback fixture".into())
    }
    fn scan(&self) -> BtResult<Vec<Device>> {
        panic!("restore must not restart an inquiry")
    }
    fn open(&self, _: &PortParams) -> BtResult<Box<dyn Stream>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        panic!("restore must not reconnect a port")
    }
    fn socket(&self) -> BtResult<Box<dyn RfcommSocket>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        panic!("restore must not reconnect a socket")
    }
}
fn fixture_service() -> (Service, Arc<AtomicUsize>) {
    let count = Arc::new(AtomicUsize::new(0));
    let service = Service::with_backend(Arc::new(Provider(count.clone())));
    service.set_allowed(true);
    service.start_driver().unwrap();
    (service, count)
}
fn pair() -> (TcpTransport, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let socket = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (peer, _) = listener.accept().unwrap();
    socket.set_nonblocking(true).unwrap();
    (TcpTransport(socket), peer)
}
fn received(socket: &mut dyn RfcommSocket, expected: usize) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if socket.available() == Ok(expected as u32) {
            return;
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
}
#[test]
fn live_socket_checkpoint_keeps_received_prefix_without_consuming_or_reconnecting() {
    let (service, _) = fixture_service();
    let mut state = State {
        service,
        ..Default::default()
    };
    let (socket, mut peer) = pair();
    let id = state.insert_socket(Box::new(socket), true).unwrap();
    peer.write_all(b"received").unwrap();
    received(state.sockets.get_mut(&id).unwrap().native.as_mut(), 8);
    let bytes = serde_json::to_vec(&state.capture_snapshot(Instant::now()).unwrap()).unwrap();
    peer.write_all(b" future").unwrap();
    received(state.sockets.get_mut(&id).unwrap().native.as_mut(), 15);
    let mut out = [0; 32];
    assert_eq!(
        state.sockets.get_mut(&id).unwrap().native.read(&mut out, 0),
        Ok(15)
    );
    assert_eq!(&out[..15], b"received future");
    drop(state);
    let (service, count) = fixture_service();
    let mut restored = State {
        service,
        ..Default::default()
    };
    restored
        .restore_snapshot(serde_json::from_slice(&bytes).unwrap())
        .unwrap();
    let socket = &mut restored.sockets.get_mut(&id).unwrap().native;
    assert_eq!(socket.read(&mut out, 2), Ok(8));
    assert_eq!(socket.available(), Ok(8));
    assert_eq!(socket.read(&mut out, 0), Ok(8));
    assert_eq!(&out[..8], b"received");
    assert_eq!(socket.read(&mut out, 0), Err(10054));
    assert_eq!(socket.write(b"must not send", 0), Err(10054));
    assert_eq!(count.load(Ordering::SeqCst), 0);
    let saved = restored.capture_snapshot(Instant::now()).unwrap();
    let (service, _) = fixture_service();
    let mut again = State {
        service,
        ..Default::default()
    };
    again.restore_snapshot(saved).unwrap();
    assert_eq!(
        again.sockets.get_mut(&id).unwrap().native.read(&mut out, 0),
        Err(10054)
    );
}
#[test]
fn registered_port_and_vfs_aliases_share_one_interrupted_receive_queue() {
    let mut source = crate::vfs::Vfs::new();
    let (service, _) = fixture_service();
    source.set_bluetooth_service(service.clone());
    let (transport, mut peer) = pair();
    let port = Arc::new(Port::new(Box::new(transport)));
    service
        .0
        .lock()
        .unwrap()
        .ports
        .insert(3, (0xb7000000, port.clone()));
    service.0.lock().unwrap().next = 0xb7000001;
    peer.write_all(b"port prefix").unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while !port.readable().unwrap() {
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    let mut source = source;
    let handle = source.open_file("COM3:", 3, 3, 3, false, 0).unwrap().handle;
    let object = source.export_handle(handle).unwrap();
    assert!(source.import_handle(handle + 1, object));
    let bytes = serde_json::to_vec(&source.capture_file_snapshot().unwrap()).unwrap();
    drop(source);
    drop(service);
    drop(port);
    let mut restored = crate::vfs::Vfs::new();
    let (service, count) = fixture_service();
    restored.set_bluetooth_service(service.clone());
    restored
        .restore_file_snapshot(serde_json::from_slice(&bytes).unwrap())
        .unwrap();
    let first = restored.bluetooth_open(handle).unwrap();
    let second = restored.bluetooth_open(handle + 1).unwrap();
    assert!(Arc::ptr_eq(&first, &second));
    assert!(Arc::ptr_eq(&first.port, &service.port(3).unwrap()));
    let mut out = [0; 32];
    let n = first.port.read(&mut out).unwrap();
    assert_eq!(&out[..n], b"port prefix");
    assert_eq!(second.port.read(&mut out), Err(10054));
    assert_eq!(count.load(Ordering::SeqCst), 0);
    service.set_allowed(false);
    assert_eq!(first.port.read(&mut out), Err(CANCELLED));
}

#[test]
fn shared_port_capture_uses_one_immutable_receive_boundary_per_epoch_without_retaining_history() {
    let (mut transport, mut peer) = pair();
    peer.write_all(b"first").unwrap();
    received(&mut transport, 5);
    let port = Port::new(Box::new(transport));
    let at = Instant::now();
    let first = port.capture_snapshot(at).unwrap();
    peer.write_all(b" later").unwrap();
    let until = Instant::now() + Duration::from_secs(2);
    loop {
        let n = port
            .stream
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .capture_received()
            .unwrap()
            .bytes
            .len();
        if n == 11 {
            break;
        }
        assert!(Instant::now() < until);
        std::thread::yield_now();
    }
    let alias = port.capture_snapshot(at).unwrap();
    assert!(Arc::ptr_eq(&first, &alias));
    assert_eq!(alias.received.bytes, b"first");
    let next = port.capture_snapshot(at + Duration::from_nanos(1)).unwrap();
    assert_eq!(next.received.bytes, b"first later");
    let weak = Arc::downgrade(&next);
    drop(next);
    assert!(weak.upgrade().is_none());
}

#[test]
fn pending_inquiry_is_cancelled_and_partial_write_state_is_not_replayed() {
    let (service, _) = fixture_service();
    let mut source = State {
        service,
        ..Default::default()
    };
    let (socket, mut peer) = pair();
    peer.set_nonblocking(true).unwrap();
    let id = source.insert_socket(Box::new(socket), false).unwrap();
    let inquiry = 0xb7100000;
    source.next_lookup = inquiry + 1;
    let producer = Arc::new(Mutex::new(None));
    source.lookups.insert(
        inquiry,
        Lookup {
            result: producer.clone(),
            index: 0,
        },
    );
    let key = (2, 0x70000100, 0x60000000);
    source.writes.insert(
        key,
        PendingWrite {
            handle: id,
            pointer: 0x20000,
            bytes: b"already-sent not-yet-sent".to_vec(),
            offset: 13,
        },
    );
    let bytes = serde_json::to_vec(&source.capture_snapshot(Instant::now()).unwrap()).unwrap();
    // A later source callback must not overwrite the saved cancellation.
    *producer.lock().unwrap() = Some(Ok(vec![Device {
        address: 1,
        name: "later peer".into(),
    }]));
    drop(source);
    let (service, count) = fixture_service();
    let mut restored = State {
        service,
        ..Default::default()
    };
    restored
        .restore_snapshot(serde_json::from_slice(&bytes).unwrap())
        .unwrap();
    assert_eq!(
        *restored.lookups[&inquiry].result.lock().unwrap(),
        Some(Err(CANCELLED))
    );
    let pending = restored.writes.remove(&key).unwrap();
    assert_eq!(pending.offset, 13);
    assert_eq!(pending.pointer, 0x20000);
    assert_eq!(
        restored
            .sockets
            .get_mut(&id)
            .unwrap()
            .native
            .write(&pending.bytes[pending.offset..], 0),
        Err(10054)
    );
    let mut data = [0; 64];
    match peer.read(&mut data) {
        Ok(0) => {}
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
        other => panic!("restore sent bytes to the old peer: {other:?}"),
    }
    assert_eq!(count.load(Ordering::SeqCst), 0);
}
