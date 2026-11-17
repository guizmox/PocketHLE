//! Logical WinINet handles reconnect through the current native HTTP backend.
//! Never reissue an in-flight request: it could be a non-idempotent POST.
use super::*;

#[derive(Serialize, Deserialize)]
pub(crate) struct InternetSnapshot {
    handles: HashMap<u32, SavedHandle>,
    next: u32,
    connected: bool,
}
#[derive(Serialize, Deserialize)]
enum SavedHandle {
    Session(SessionSpec),
    Connection(Connection),
    Request { parent: u32, spec: RequestSpec },
}
impl State {
    pub(crate) fn capture_checkpoint(&self) -> std::result::Result<InternetSnapshot, String> {
        let handles=self.handles.iter().map(|(&id,handle)| {
            Ok((id,match handle {
                Handle::Session(session)=>SavedHandle::Session(session.spec.clone()),
                Handle::Connection(connection)=>SavedHandle::Connection(connection.clone()),
                Handle::Request(request)=> {
                    if request.transfer.is_some() || request.sending.is_some() {
                        return Err("checkpoint contains an HTTP transfer; remote requests cannot be replayed safely".into());
                    }
                    SavedHandle::Request {parent:request.parent,spec:request.spec.clone()}
                }
            }))
        }).collect::<std::result::Result<_,String>>()?;
        let saved = InternetSnapshot {
            handles,
            next: self.next,
            connected: self.gprs_connected,
        };
        saved.validate()?;
        Ok(saved)
    }
}
impl InternetSnapshot {
    fn validate(&self) -> std::result::Result<(), String> {
        if self.next < 0xb7300000
            || self.handles.len() > 512
            || self
                .handles
                .keys()
                .any(|&id| id < 0xb7300000 || id >= self.next)
            || self
                .handles
                .values()
                .filter(|handle| matches!(handle, SavedHandle::Request { .. }))
                .count()
                > 64
        {
            return Err("invalid saved WinINet handle namespace".into());
        }
        for handle in self.handles.values() {
            match handle {
                SavedHandle::Connection(connection)
                    if !matches!(
                        self.handles.get(&connection.parent),
                        Some(SavedHandle::Session(_))
                    ) =>
                {
                    return Err("invalid saved WinINet session reference".into())
                }
                SavedHandle::Request { parent, .. }
                    if !matches!(
                        self.handles.get(parent),
                        Some(SavedHandle::Session(_) | SavedHandle::Connection(_))
                    ) =>
                {
                    return Err("invalid saved WinINet request parent".into())
                }
                _ => {}
            }
        }
        Ok(())
    }
    pub(crate) fn restore(self, state: &mut State) -> std::result::Result<(), String> {
        self.validate()?;
        if self.connected && (state.backend.is_none() || state.gprs_enabled == Some(false)) {
            return Err(
                "cannot restore connected GPRS session: current HTTP provider is disabled".into(),
            );
        }
        let mut sessions: HashMap<u32, Arc<dyn Client>> = HashMap::new();
        let mut handles = HashMap::new();
        for (&id, handle) in &self.handles {
            if let SavedHandle::Session(spec) = handle {
                let client = state
                    .backend
                    .as_ref()
                    .ok_or("cannot reopen WinINet session: no current native backend")?
                    .open(spec)
                    .map_err(|code| format!("cannot reopen WinINet session: WinCE error {code}"))?;
                sessions.insert(id, client.clone());
                handles.insert(
                    id,
                    Handle::Session(Session {
                        spec: spec.clone(),
                        client,
                    }),
                );
            }
        }
        for (id, handle) in &self.handles {
            match handle {
                SavedHandle::Session(_) => {}
                SavedHandle::Connection(connection) => {
                    handles.insert(*id, Handle::Connection(connection.clone()));
                }
                SavedHandle::Request { parent, spec } => {
                    let session = match self.handles.get(parent).unwrap() {
                        SavedHandle::Session(_) => *parent,
                        SavedHandle::Connection(connection) => connection.parent,
                        _ => unreachable!(),
                    };
                    handles.insert(
                        *id,
                        Handle::Request(Request {
                            parent: *parent,
                            client: sessions[&session].clone(),
                            spec: spec.clone(),
                            transfer: None,
                            sending: None,
                        }),
                    );
                }
            }
        }
        // The current endpoint routing and host access policy remain supplied
        // by launch configuration. No network request is sent during restore.
        state.handles = handles;
        state.next = self.next;
        state.gprs_connected = self.connected;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Provider {
        opens: Arc<AtomicUsize>,
        denied: bool,
    }
    struct TestClient;
    impl Client for TestClient {
        fn start(&self, _: RequestSpec, _: Vec<u8>) -> Result<Box<dyn Transfer>> {
            panic!("restoring a handle must never send an HTTP request");
        }
    }
    impl Backend for Provider {
        fn open(&self, _: &SessionSpec) -> Result<Arc<dyn Client>> {
            self.opens.fetch_add(1, Ordering::SeqCst);
            if self.denied {
                Err(5)
            } else {
                Ok(Arc::new(TestClient))
            }
        }
    }
    fn provider(denied: bool) -> (Arc<dyn Backend>, Arc<AtomicUsize>) {
        let opens = Arc::new(AtomicUsize::new(0));
        (
            Arc::new(Provider {
                opens: opens.clone(),
                denied,
            }),
            opens,
        )
    }
    fn fixture() -> (State, u32, u32) {
        let (provider, _) = provider(false);
        let mut state = State::with_backend(provider);
        let session = state
            .open(SessionSpec {
                agent: "checkpoint-test".into(),
                access: 1,
                proxy: String::new(),
                bypass: String::new(),
            })
            .unwrap();
        let client = match &state.handles[&session] {
            Handle::Session(session) => session.client.clone(),
            _ => unreachable!(),
        };
        let connection = state
            .insert(Handle::Connection(Connection {
                parent: session,
                server: "localhost".into(),
                port: 8080,
                user: String::new(),
                password: String::new(),
            }))
            .unwrap();
        let request = state
            .insert(Handle::Request(Request {
                parent: connection,
                client,
                spec: RequestSpec {
                    server: "localhost".into(),
                    port: 8080,
                    secure: false,
                    method: "POST".into(),
                    path: "/submit".into(),
                    version: "HTTP/1.1".into(),
                    user: String::new(),
                    password: String::new(),
                    headers: Vec::new(),
                    flags: 0,
                },
                transfer: None,
                sending: None,
            }))
            .unwrap();
        (state, session, request)
    }
    #[test]
    fn unsent_http_handles_reopen_current_backend_without_replaying_requests() {
        let (source, session, request) = fixture();
        let saved = source.capture_checkpoint().unwrap();
        let (backend, opens) = provider(false);
        let mut destination = State::with_backend(backend);
        saved.restore(&mut destination).unwrap();
        assert_eq!(opens.load(Ordering::SeqCst), 1);
        let client = destination.request(request).unwrap().client.clone();
        let Handle::Session(session_object) = &destination.handles[&session] else {
            panic!()
        };
        assert!(Arc::ptr_eq(&client, &session_object.client));
        let Handle::Session(original) = &source.handles[&session] else {
            panic!()
        };
        assert!(!Arc::ptr_eq(&client, &original.client));
        assert_eq!(destination.request(request).unwrap().spec.method, "POST");
        assert!(destination.close(session));
        assert!(destination.handles.is_empty());
    }
    #[test]
    fn pending_post_and_missing_backend_fail_explicitly() {
        let (mut source, _, request) = fixture();
        source.request(request).unwrap().sending = Some((0, 0x70001000, 0x10000));
        assert!(source
            .capture_checkpoint()
            .err()
            .unwrap()
            .contains("HTTP transfer"));
        source.request(request).unwrap().sending = None;
        let saved = source.capture_checkpoint().unwrap();
        let (backend, opens) = provider(true);
        let mut destination = State::with_backend(backend);
        assert!(saved
            .restore(&mut destination)
            .unwrap_err()
            .contains("WinCE error 5"));
        assert_eq!(opens.load(Ordering::SeqCst), 1);
        assert!(destination.handles.is_empty());
    }
    #[test]
    fn malformed_parent_is_rejected_before_opening_native_clients() {
        let (source, _, request) = fixture();
        let mut saved = source.capture_checkpoint().unwrap();
        let SavedHandle::Request { parent, .. } = saved.handles.get_mut(&request).unwrap() else {
            panic!()
        };
        *parent = request;
        let (backend, opens) = provider(false);
        let mut destination = State::with_backend(backend);
        assert!(saved.restore(&mut destination).is_err());
        assert_eq!(opens.load(Ordering::SeqCst), 0);
    }
}
