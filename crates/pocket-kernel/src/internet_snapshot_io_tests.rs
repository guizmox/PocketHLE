//! A real local HTTP peer verifies that capture neither cancels nor resends.
use super::*;
use std::{
    io::{BufRead, Read, Write},
    net::{Shutdown, TcpListener, TcpStream},
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};
struct LocalBackend {
    address: std::net::SocketAddr,
    starts: Arc<AtomicUsize>,
    workers: Arc<Mutex<Vec<std::thread::JoinHandle<()>>>>,
}
impl Backend for LocalBackend {
    fn open(&self, _: &SessionSpec) -> Result<Arc<dyn Client>> {
        Ok(Arc::new(LocalBackend {
            address: self.address,
            starts: self.starts.clone(),
            workers: self.workers.clone(),
        }))
    }
}
impl Client for LocalBackend {
    fn start(&self, spec: RequestSpec, body: Vec<u8>) -> Result<Box<dyn Transfer>> {
        self.starts.fetch_add(1, Ordering::SeqCst);
        let mut socket = TcpStream::connect(self.address).map_err(|_| 12029u32)?;
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        write!(
            socket,
            "{} {} HTTP/1.0\r\nContent-Length: {}\r\n\r\n",
            spec.method,
            spec.path,
            body.len()
        )
        .unwrap();
        socket.write_all(&body).unwrap();
        let cancel = socket.try_clone().unwrap();
        let channel = Channel::default();
        let output = channel.clone();
        let worker = std::thread::spawn(move || {
            let mut input = std::io::BufReader::new(socket);
            let mut line = String::new();
            if input.read_line(&mut line).is_err() {
                output.finish(Err(12030));
                return;
            }
            let mut fields = line.trim().splitn(3, ' ');
            let version = fields.next().unwrap().to_owned();
            let status = fields.next().unwrap().parse().unwrap();
            let reason = fields.next().unwrap().to_owned();
            let mut headers = Vec::new();
            loop {
                line.clear();
                if input.read_line(&mut line).is_err() {
                    output.finish(Err(12030));
                    return;
                }
                if line == "\r\n" {
                    break;
                }
                let (n, v) = line.trim().split_once(':').unwrap();
                headers.push((n.into(), v.trim().into()));
            }
            output.publish(ResponseHead {
                status,
                version,
                reason,
                headers,
            });
            let mut bytes = [0; 256];
            loop {
                match input.read(&mut bytes) {
                    Ok(0) => {
                        output.finish(Ok(()));
                        break;
                    }
                    Ok(n) => {
                        if output.push(&bytes[..n]).is_err() {
                            break;
                        }
                    }
                    Err(_) => {
                        output.finish(Err(12030));
                        break;
                    }
                }
            }
        });
        self.workers.lock().unwrap().push(worker);
        Ok(channel.transfer(move || {
            let _ = cancel.shutdown(Shutdown::Both);
        }))
    }
}
#[test]
fn in_flight_http_snapshots_keep_real_received_bytes_and_never_send_a_second_post() {
    for before_headers in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (received, ready) = std::sync::mpsc::channel();
        let (release, continue_server) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let (mut peer, _) = listener.accept().unwrap();
            peer.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut request = std::io::BufReader::new(peer.try_clone().unwrap());
            let mut line = String::new();
            request.read_line(&mut line).unwrap();
            assert_eq!(line, "POST /submit HTTP/1.0\r\n");
            loop {
                line.clear();
                request.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
            }
            let mut body = [0; 5];
            request.read_exact(&mut body).unwrap();
            assert_eq!(&body, b"money");
            if !before_headers {
                peer.write_all(b"HTTP/1.0 200 OK\r\nX-Test: received\r\n\r\nsaved")
                    .unwrap();
            }
            received.send(()).unwrap();
            if continue_server.recv_timeout(Duration::from_secs(5)).is_ok() {
                if before_headers {
                    peer.write_all(b"HTTP/1.0 200 OK\r\n\r\nsaved").unwrap();
                }
                peer.write_all(b" future").unwrap();
            }
        });
        let starts = Arc::new(AtomicUsize::new(0));
        let workers = Arc::new(Mutex::new(Vec::new()));
        let backend: Arc<dyn Backend> = Arc::new(LocalBackend {
            address,
            starts: starts.clone(),
            workers: workers.clone(),
        });
        let mut source = State::with_backend(backend.clone());
        let session = source
            .open(SessionSpec {
                agent: "snapshot fixture".into(),
                access: 1,
                proxy: String::new(),
                bypass: String::new(),
            })
            .unwrap();
        let client = match &source.handles[&session] {
            Handle::Session(s) => s.client.clone(),
            _ => unreachable!(),
        };
        let spec = RequestSpec {
            server: "127.0.0.1".into(),
            port: address.port(),
            secure: false,
            method: "POST".into(),
            path: "/submit".into(),
            version: "HTTP/1.0".into(),
            user: String::new(),
            password: String::new(),
            headers: vec![],
            flags: 0,
        };
        let transfer = client.start(spec.clone(), b"money".to_vec()).unwrap();
        let request = source
            .insert(Handle::Request(Request {
                parent: session,
                client,
                spec,
                transfer: Some(transfer),
                sending: before_headers.then_some((0, 0x70001000, 0x60000000)),
            }))
            .unwrap();
        ready.recv_timeout(Duration::from_secs(5)).unwrap();
        if !before_headers {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if source
                    .request(request)
                    .unwrap()
                    .transfer
                    .as_mut()
                    .unwrap()
                    .available()
                    .unwrap()
                    .0
                    == 5
                {
                    break;
                }
                assert!(Instant::now() < deadline);
                std::thread::yield_now();
            }
        }
        let encoded = serde_json::to_vec(&source.capture_checkpoint().unwrap()).unwrap();
        let mut destination = State::with_backend(backend);
        serde_json::from_slice::<InternetSnapshot>(&encoded)
            .unwrap()
            .restore(&mut destination)
            .unwrap();
        let stream = destination
            .request(request)
            .unwrap()
            .transfer
            .as_mut()
            .unwrap();
        let mut out = [0; 64];
        if before_headers {
            assert_eq!(stream.head().unwrap_err(), 12030);
        } else {
            assert_eq!(stream.head().unwrap().unwrap().status, 200);
            assert_eq!(stream.read(&mut out), Ok(Some(5)));
            assert_eq!(&out[..5], b"saved");
            assert_eq!(stream.read(&mut out), Err(12030));
        }
        assert_eq!(starts.load(Ordering::SeqCst), 1);
        release.send(()).unwrap();
        server.join().unwrap();
        for worker in workers.lock().unwrap().drain(..) {
            worker.join().unwrap();
        }
        let n = source
            .request(request)
            .unwrap()
            .transfer
            .as_mut()
            .unwrap()
            .read(&mut out)
            .unwrap()
            .unwrap();
        assert_eq!(&out[..n], b"saved future");
        assert_eq!(
            source
                .request(request)
                .unwrap()
                .transfer
                .as_mut()
                .unwrap()
                .read(&mut out),
            Ok(Some(0))
        );
        assert_eq!(starts.load(Ordering::SeqCst), 1);
    }
}
