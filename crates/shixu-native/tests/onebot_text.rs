use shixu_core::{
    calendar::EventService,
    contracts::{
        AppResult, calendar::EventQuery, error::AppError, notification::*, vault::SecretBytes,
    },
    notifications::{
        MessageStore,
        consent::{ConsentStore, ModelConsent},
    },
    runtime::{Supervisor, workers::WorkerPorts},
    storage::{DataProtector, Database},
};
use shixu_native::qq::{onebot::connected_onebot_receiver, transport::LoopbackEndpoint};
use std::{
    net::TcpListener,
    sync::Arc,
    time::{Duration, Instant},
};
use tungstenite::{Message, accept_hdr};
struct TestProtector;
impl DataProtector for TestProtector {
    fn protect(&self, p: &[u8]) -> AppResult<Vec<u8>> {
        Ok(p.iter().map(|b| b ^ 0xa5).collect())
    }
    fn unprotect(&self, p: &[u8]) -> AppResult<Vec<u8>> {
        self.protect(p)
    }
}
fn config() -> SourceConfig {
    SourceConfig {
        source_id: SourceId::from_uuid(uuid::Uuid::new_v4()),
        adapter_type: "onebot11-text".into(),
        account_id: "42".into(),
        allowed_group_ids: vec!["7".into()],
        timezone: "Asia/Shanghai".into(),
        enabled: true,
        capability_set: vec![SourceCapability::LiveMessages],
    }
}
fn event(group: i64, message: serde_json::Value) -> serde_json::Value {
    serde_json::json!({"time":1791504000i64,"self_id":42,"post_type":"message","message_type":"group","message_id":-123,"group_id":group,"user_id":88,"message":message})
}
// tungstenite server callback requires its unboxed HTTP error response type.
#[allow(clippy::result_large_err)]
fn server(frames: Vec<String>) -> (LoopbackEndpoint, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = LoopbackEndpoint::parse(&listener.local_addr().unwrap().to_string()).unwrap();
    let thread = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut ws = accept_hdr(
            stream,
            |request: &tungstenite::handshake::server::Request, response| {
                assert_eq!(request.uri().to_string(), "/event");
                assert_eq!(
                    request.headers()["authorization"],
                    "Bearer synthetic-test-token"
                );
                Ok(response)
            },
        )
        .unwrap();
        for frame in frames {
            if let Err(error) = ws.send(Message::Text(frame.into())) {
                // A client may reject an oversized frame before the server finishes writing.
                match error {
                    tungstenite::Error::Io(e)
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::ConnectionReset
                        ) =>
                    {
                        return;
                    }
                    other => panic!("synthetic frame send failed: {other}"),
                }
            }
        }
        loop {
            match ws.read() {
                Ok(Message::Close(_)) | Err(_) => break,
                Ok(Message::Pong(_)) => (),
                Ok(other) => panic!("unexpected outbound business frame: {other:?}"),
            }
        }
    });
    (endpoint, thread)
}
fn token() -> SecretBytes {
    SecretBytes::new(b"synthetic-test-token".to_vec())
}
fn db() -> Arc<Database> {
    Arc::new(Database::open(std::path::Path::new(":memory:"), Arc::new(TestProtector)).unwrap())
}
#[test]
fn real_socket_factory_worker_persists_duplicate_once_and_applies_calendar() {
    let text = " 2026年10月12日9:00高数考试\n ";
    let allowed = event(7, serde_json::json!([{"type":"text","data":{"text":text}}])).to_string();
    let forbidden = event(8, serde_json::json!([{"type":"image","data":{}}])).to_string();
    let mut marker = event(
        7,
        serde_json::json!([{"type":"text","data":{"text":"普通说明"}}]),
    );
    marker["message_id"] = (-124).into();
    let (endpoint, server) = server(vec![
        forbidden,
        allowed.clone(),
        allowed,
        marker.to_string(),
    ]);
    let c = config();
    let db = db();
    let store = MessageStore::new(db.clone());
    let receiver = connected_onebot_receiver(
        endpoint,
        c.clone(),
        token(),
        MessageStore::new(db.clone()),
        || 1791504000000,
    )
    .unwrap();
    let supervisor = Arc::new(Supervisor::new(
        db.clone(),
        Arc::new(ConsentStore::new(ModelConsent::default())),
    ));
    supervisor.start(vec![c.clone()]).unwrap();
    let workers = supervisor
        .spawn_workers(WorkerPorts {
            receiver: Some(receiver),
            ..Default::default()
        })
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    let events = loop {
        let events = EventService::new(db.clone())
            .query(EventQuery {
                from_date: None,
                through_date: None,
                statuses: vec![],
                include_pending: true,
            })
            .unwrap();
        if !events.is_empty()
            && supervisor.status().sources[0].1.last_persisted_at.is_some()
            && store.cursor(&c, "7").unwrap().as_deref() == Some("-124")
        {
            break events;
        }
        assert!(Instant::now() < deadline, "socket-to-calendar deadline");
        std::thread::sleep(Duration::from_millis(10));
    };
    // The marker cursor proves the preceding duplicate passed the serialized worker.
    assert_eq!(events.len(), 1);
    let messages = store.list(None, 100).unwrap();
    assert_eq!(messages.len(), 2);
    let notification = messages
        .iter()
        .find(|m| m.native_message_id == "-123")
        .unwrap();
    assert_eq!(notification.text, text);
    assert!(messages.iter().all(|m| m.group_id == "7"));
    assert_eq!(store.cursor(&c, "7").unwrap().as_deref(), Some("-124"));
    let at = Instant::now();
    workers.stop().unwrap();
    assert!(at.elapsed() < Duration::from_secs(1));
    server.join().unwrap();
}
#[test]
fn socket_rejects_wrong_account_malformed_overflow_cq_and_nontext() {
    let base = event(
        7,
        serde_json::json!([{"type":"text","data":{"text":"hello"}}]),
    );
    let mut wrong = base.clone();
    wrong["self_id"] = 43.into();
    let mut overflow = base.clone();
    overflow["time"] = i64::MAX.into();
    for (frame, expected) in [
        (wrong.to_string(), AppError::AuthFailed),
        ("{broken".into(), AppError::ParseFailed),
        (overflow.to_string(), AppError::InvalidInput),
        (
            event(7, "[CQ:image,file=x]".into()).to_string(),
            AppError::Unsupported,
        ),
        (
            event(7, serde_json::json!([{"type":"image","data":{}}])).to_string(),
            AppError::Unsupported,
        ),
        (" ".repeat(1024 * 1024 + 1), AppError::InvalidInput),
    ] {
        let (endpoint, server) = server(vec![frame]);
        let result =
            connected_onebot_receiver(endpoint, config(), token(), MessageStore::new(db()), || 0);
        assert_eq!(result.err(), Some(expected));
        server.join().unwrap();
    }
}

#[test]
fn unsupported_frame_is_visible_then_next_text_can_be_received() {
    let heartbeat =
        serde_json::json!({"self_id":42,"post_type":"meta_event","meta_event_type":"heartbeat"})
            .to_string();
    let (endpoint, server) = server(vec![
        heartbeat,
        event(7, serde_json::json!([{"type":"image","data":{}}])).to_string(),
        event(
            7,
            serde_json::json!([{"type":"text","data":{"text":"next text"}}]),
        )
        .to_string(),
    ]);
    let mut receiver =
        connected_onebot_receiver(endpoint, config(), token(), MessageStore::new(db()), || 0)
            .unwrap();
    assert_eq!(receiver.poll().err(), Some(AppError::Unsupported));
    assert_eq!(receiver.poll().unwrap().unwrap().message.text, "next text");
    receiver.disconnect().unwrap();
    server.join().unwrap();
}

#[test]
fn idle_handshake_times_out_and_invalid_token_never_connects() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = LoopbackEndpoint::parse(&listener.local_addr().unwrap().to_string()).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let server = std::thread::spawn(move || {
        let (_stream, _) = listener.accept().unwrap();
        rx.recv_timeout(Duration::from_secs(3)).unwrap();
    });
    let at = Instant::now();
    assert_eq!(
        connected_onebot_receiver(endpoint, config(), token(), MessageStore::new(db()), || 0).err(),
        Some(AppError::Disconnected)
    );
    assert!(at.elapsed() < Duration::from_secs(2));
    tx.send(()).unwrap();
    server.join().unwrap();
    for bytes in [
        b"abc\r\ninjected: x".to_vec(),
        vec![0xff],
        vec![b'a'; 4097],
        vec![],
    ] {
        let endpoint = LoopbackEndpoint::parse("127.0.0.1:1").unwrap();
        assert_eq!(
            connected_onebot_receiver(
                endpoint,
                config(),
                SecretBytes::new(bytes),
                MessageStore::new(db()),
                || 0
            )
            .err(),
            Some(AppError::InvalidInput)
        );
    }
}

#[test]
fn fragmented_text_survives_poll_deadline_and_ping_has_only_protocol_pong() {
    use tungstenite::protocol::frame::{
        Frame,
        coding::{Data, OpCode},
    };
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = LoopbackEndpoint::parse(&listener.local_addr().unwrap().to_string()).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut ws = tungstenite::accept(stream).unwrap();
        ws.send(Message::Text(serde_json::json!({"self_id":42,"post_type":"meta_event","meta_event_type":"heartbeat"}).to_string().into())).unwrap();
        ws.send(Message::Ping(b"ping".to_vec().into())).unwrap();
        let text = event(
            7,
            serde_json::json!([{"type":"text","data":{"text":"fragmented"}}]),
        )
        .to_string();
        let split = text.len() / 2;
        ws.send(Message::Frame(Frame::message(
            text.as_bytes()[..split].to_vec(),
            OpCode::Data(Data::Text),
            false,
        )))
        .unwrap();
        rx.recv_timeout(Duration::from_secs(3)).unwrap();
        ws.send(Message::Frame(Frame::message(
            text.as_bytes()[split..].to_vec(),
            OpCode::Data(Data::Continue),
            true,
        )))
        .unwrap();
        assert_eq!(ws.read().unwrap(), Message::Pong(b"ping".to_vec().into()));
        assert!(matches!(ws.read(), Ok(Message::Close(_))));
    });
    let mut receiver =
        connected_onebot_receiver(endpoint, config(), token(), MessageStore::new(db()), || 0)
            .unwrap();
    assert!(receiver.poll().unwrap().is_none());
    tx.send(()).unwrap();
    assert_eq!(receiver.poll().unwrap().unwrap().message.text, "fragmented");
    receiver.disconnect().unwrap();
    server.join().unwrap();
}

#[test]
fn unauthorized_http_and_oversized_handshake_fail_with_fixed_errors() {
    use std::io::{Read, Write};
    for (response, expected) in [
        (
            b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\n\r\n".to_vec(),
            AppError::AuthFailed,
        ),
        (
            [
                b"HTTP/1.1 101 Switching Protocols\r\nX-Fill: ".as_slice(),
                &vec![b'a'; 70000],
            ]
            .concat(),
            AppError::ParseFailed,
        ),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint =
            LoopbackEndpoint::parse(&listener.local_addr().unwrap().to_string()).unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut request = [0; 8192];
            assert!(stream.read(&mut request).unwrap() > 0);
            let _ = stream.write_all(&response);
        });
        assert_eq!(
            connected_onebot_receiver(endpoint, config(), token(), MessageStore::new(db()), || 0)
                .err(),
            Some(expected)
        );
        server.join().unwrap();
    }
}

#[test]
fn trace_enabled_logger_never_receives_bearer_or_message_body() {
    use std::sync::atomic::{AtomicBool, Ordering};
    struct Capture;
    static LEAKED: AtomicBool = AtomicBool::new(false);
    impl log::Log for Capture {
        fn enabled(&self, _: &log::Metadata<'_>) -> bool {
            true
        }
        fn log(&self, record: &log::Record<'_>) {
            let rendered = record.args().to_string();
            if rendered.contains("synthetic-test-token") || rendered.contains("unique-private-body")
            {
                LEAKED.store(true, Ordering::SeqCst);
            }
        }
        fn flush(&self) {}
    }
    static LOGGER: Capture = Capture;
    log::set_logger(&LOGGER).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    let (endpoint, server) = server(vec![
        event(
            7,
            serde_json::json!([{"type":"text","data":{"text":"unique-private-body"}}]),
        )
        .to_string(),
    ]);
    let mut receiver =
        connected_onebot_receiver(endpoint, config(), token(), MessageStore::new(db()), || 0)
            .unwrap();
    assert_eq!(
        receiver.poll().unwrap().unwrap().message.text,
        "unique-private-body"
    );
    receiver.disconnect().unwrap();
    server.join().unwrap();
    assert!(
        !LEAKED.load(Ordering::SeqCst),
        "library logging exposed a bearer or private body"
    );
}

// Mature library encodes the valid nonfinal empty frames. The raw stream merely
// sends batches faster than a frame parser can consume them, keeping TCP readable.
fn fragment_flood_server(
    identity_first: bool,
) -> (
    LoopbackEndpoint,
    Arc<std::sync::atomic::AtomicBool>,
    std::thread::JoinHandle<()>,
) {
    use std::{
        io::Write,
        sync::atomic::{AtomicBool, Ordering},
    };
    use tungstenite::protocol::frame::{
        Frame,
        coding::{Data, OpCode},
    };
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = LoopbackEndpoint::parse(&listener.local_addr().unwrap().to_string()).unwrap();
    let running = Arc::new(AtomicBool::new(true));
    let signal = running.clone();
    let thread = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut ws = tungstenite::accept(stream).unwrap();
        if identity_first {
            ws.send(Message::Text(serde_json::json!({"self_id":42,"post_type":"meta_event","meta_event_type":"heartbeat"}).to_string().into())).unwrap();
        }
        let mut first = Vec::new();
        Frame::message(Vec::new(), OpCode::Data(Data::Text), false)
            .format(&mut first)
            .unwrap();
        let mut frame = Vec::new();
        Frame::message(Vec::new(), OpCode::Data(Data::Continue), false)
            .format(&mut frame)
            .unwrap();
        let batch = frame.repeat(32768);
        let stream = ws.get_mut();
        if stream.write_all(&first).is_err() {
            return;
        }
        while signal.load(Ordering::SeqCst) {
            if stream.write_all(&batch).is_err() {
                break;
            }
        }
    });
    (endpoint, running, thread)
}

#[test]
fn continuous_empty_fragments_cannot_extend_first_event_deadline() {
    use std::sync::atomic::Ordering;
    let (endpoint, running, server) = fragment_flood_server(false);
    let (tx, rx) = std::sync::mpsc::channel();
    let client = std::thread::spawn(move || {
        let at = Instant::now();
        let error =
            connected_onebot_receiver(endpoint, config(), token(), MessageStore::new(db()), || 0)
                .err();
        tx.send((error, at.elapsed())).unwrap();
    });
    let outcome = rx.recv_timeout(Duration::from_millis(1500));
    running.store(false, Ordering::SeqCst);
    server.join().unwrap();
    client.join().unwrap();
    let (error, elapsed) =
        outcome.expect("first-event deadline must interrupt continuously readable fragments");
    assert_eq!(error, Some(AppError::Disconnected));
    assert!(elapsed < Duration::from_millis(1500));
}

#[test]
fn continuous_empty_fragments_cannot_extend_poll_deadline() {
    use std::sync::atomic::Ordering;
    let (endpoint, running, server) = fragment_flood_server(true);
    let (tx, rx) = std::sync::mpsc::channel();
    let client = std::thread::spawn(move || {
        let mut receiver =
            connected_onebot_receiver(endpoint, config(), token(), MessageStore::new(db()), || 0)
                .unwrap();
        let at = Instant::now();
        let outcome = receiver.poll().map(|d| d.is_none());
        tx.send((outcome, at.elapsed())).unwrap();
        receiver.disconnect().unwrap();
    });
    let outcome = rx.recv_timeout(Duration::from_millis(500));
    running.store(false, Ordering::SeqCst);
    server.join().unwrap();
    client.join().unwrap();
    let (poll, elapsed) =
        outcome.expect("poll deadline must interrupt continuously readable fragments");
    assert_eq!(poll, Ok(true));
    assert!(elapsed < Duration::from_millis(300));
}

#[test]
fn continuous_empty_fragments_cannot_block_worker_stop() {
    use std::sync::atomic::Ordering;
    let (endpoint, running, server) = fragment_flood_server(true);
    let db = db();
    let c = config();
    let receiver = connected_onebot_receiver(
        endpoint,
        c.clone(),
        token(),
        MessageStore::new(db.clone()),
        || 0,
    )
    .unwrap();
    let supervisor = Arc::new(Supervisor::new(
        db,
        Arc::new(ConsentStore::new(ModelConsent::default())),
    ));
    supervisor.start(vec![c]).unwrap();
    let workers = supervisor
        .spawn_workers(WorkerPorts {
            receiver: Some(receiver),
            ..Default::default()
        })
        .unwrap();
    std::thread::sleep(Duration::from_millis(200));
    let (tx, rx) = std::sync::mpsc::channel();
    let stop = std::thread::spawn(move || {
        let at = Instant::now();
        tx.send((workers.stop(), at.elapsed())).unwrap();
    });
    let outcome = rx.recv_timeout(Duration::from_millis(500));
    running.store(false, Ordering::SeqCst);
    server.join().unwrap();
    stop.join().unwrap();
    let (result, elapsed) =
        outcome.expect("worker stop must complete while the fragment flood is still running");
    assert_eq!(result, Ok(()));
    assert!(elapsed < Duration::from_millis(500));
}
