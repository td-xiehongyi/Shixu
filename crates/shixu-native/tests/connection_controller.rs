use shixu_core::{
    contracts::{AppResult, error::AppError, notification::*, vault::SecretBytes},
    notifications::{MessageStore, settings::SettingsStore},
    runtime::workers::ReceivePort,
    storage::{DataProtector, Database},
};
use shixu_native::qq::connection::ConnectionController;
use std::{
    net::TcpListener,
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};
struct Synthetic;
impl DataProtector for Synthetic {
    fn protect(&self, p: &[u8]) -> AppResult<Vec<u8>> {
        Ok(p.iter().map(|b| b ^ 0xa5).collect())
    }
    fn unprotect(&self, p: &[u8]) -> AppResult<Vec<u8>> {
        self.protect(p)
    }
}
fn setup() -> (
    Arc<Database>,
    SourceConfig,
    Arc<ConnectionController>,
    Box<dyn ReceivePort>,
) {
    let db =
        Arc::new(Database::open(std::path::Path::new(":memory:"), Arc::new(Synthetic)).unwrap());
    let c = SourceConfig {
        source_id: SourceId::from_uuid(uuid::Uuid::new_v4()),
        adapter_type: "onebot11-text".into(),
        account_id: "42".into(),
        allowed_group_ids: vec!["7".into()],
        timezone: "Asia/Shanghai".into(),
        enabled: true,
        capability_set: vec![SourceCapability::LiveMessages],
    };
    SettingsStore::new(db.clone())
        .save_source(c.clone())
        .unwrap();
    let (ctrl, port) = ConnectionController::new(db.clone());
    (db, c, ctrl, port)
}
#[allow(clippy::result_large_err)]
fn server() -> (String, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = listener.local_addr().unwrap().to_string();
    let thread = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut ws = tungstenite::accept(stream).unwrap();
        ws.send(tungstenite::Message::Text(serde_json::json!({"self_id":42,"post_type":"message","message_type":"group","group_id":7,"message_id":123,"user_id":88,"time":1791504000i64,"message":[{"type":"text","data":{"text":"2026年10月12日9:00高数考试"}}]}).to_string().into())).unwrap();
        while ws.read().is_ok() {}
    });
    (endpoint, thread)
}
fn connect(ctrl: Arc<ConnectionController>, port: &mut dyn ReceivePort, id: SourceId) {
    let (reply, result) = mpsc::channel();
    let t = std::thread::spawn(move || reply.send(ctrl.connect(id)).unwrap());
    let until = Instant::now() + Duration::from_secs(2);
    loop {
        port.service_controls().unwrap();
        if let Ok(r) = result.try_recv() {
            r.unwrap();
            break;
        }
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(2));
    }
    t.join().unwrap();
}
#[test]
fn revoked_epoch_discards_uncommitted_pending_without_false_ack_and_allows_switch() {
    for durable in [false, true] {
        let (db, c, ctrl, mut port) = setup();
        let (endpoint, server_thread) = server();
        ctrl.save(
            c.source_id,
            &endpoint,
            SecretBytes::new(b"synthetic-token".to_vec()),
        )
        .unwrap();
        connect(ctrl.clone(), port.as_mut(), c.source_id);
        let delivery = port.poll().unwrap().unwrap();
        if durable {
            MessageStore::new(db.clone())
                .append_with_cursor(&c, delivery.message.clone(), &delivery.cursor)
                .unwrap();
        }
        let mut disabled = c.clone();
        disabled.enabled = false;
        SettingsStore::new(db.clone())
            .save_source(disabled)
            .unwrap();
        port.service_controls().unwrap();
        assert!(port.poll().unwrap().is_none());
        let metadata = ctrl.read(c.source_id).unwrap();
        assert!(!metadata.active);
        assert_eq!(metadata.last_error, Some(AppError::Conflict));
        assert_eq!(
            MessageStore::new(db.clone())
                .list(Some(c.source_id), 100)
                .unwrap()
                .len(),
            usize::from(durable)
        );
        SettingsStore::new(db.clone())
            .save_source(c.clone())
            .unwrap();
        let (next, server2) = server();
        ctrl.save(
            c.source_id,
            &next,
            SecretBytes::new(b"synthetic-token".to_vec()),
        )
        .unwrap();
        connect(ctrl.clone(), port.as_mut(), c.source_id);
        assert!(ctrl.read(c.source_id).unwrap().active);
        port.disconnect().unwrap();
        server_thread.join().unwrap();
        server2.join().unwrap();
    }
}
#[test]
fn competing_source_connect_conflicts_and_active_save_does_not_replace_record() {
    let (db, c, ctrl, mut port) = setup();
    let (endpoint, server_thread) = server();
    ctrl.save(c.source_id, &endpoint, SecretBytes::new(vec![65]))
        .unwrap();
    connect(ctrl.clone(), port.as_mut(), c.source_id);
    let mut other = c.clone();
    other.source_id = SourceId::from_uuid(uuid::Uuid::new_v4());
    other.account_id = "43".into();
    SettingsStore::new(db).save_source(other.clone()).unwrap();
    assert_eq!(
        ctrl.save(c.source_id, "127.0.0.1:32123", SecretBytes::new(vec![66])),
        Err(AppError::Conflict)
    );
    assert_eq!(ctrl.read(c.source_id).unwrap().endpoint, Some(endpoint));
    let ctl = ctrl.clone();
    let t = std::thread::spawn(move || ctl.connect(other.source_id));
    std::thread::sleep(Duration::from_millis(10));
    port.service_controls().unwrap();
    assert_eq!(t.join().unwrap(), Err(AppError::Conflict));
    assert!(ctrl.read(c.source_id).unwrap().active);
    port.disconnect().unwrap();
    server_thread.join().unwrap();
}
#[test]
fn stopped_receiver_rejects_pending_and_future_control_requests() {
    let (_, c, ctrl, mut port) = setup();
    let ctl = ctrl.clone();
    let t = std::thread::spawn(move || ctl.connect(c.source_id));
    std::thread::sleep(Duration::from_millis(10));
    port.disconnect().unwrap();
    assert_eq!(t.join().unwrap(), Err(AppError::Disconnected));
    let before = Instant::now();
    assert_eq!(ctrl.disconnect(c.source_id), Err(AppError::Disconnected));
    assert!(before.elapsed() < Duration::from_millis(100));
}
#[test]
fn expired_request_cannot_later_connect_when_receiver_resumes() {
    let (_, c, ctrl, mut port) = setup();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    ctrl.save(
        c.source_id,
        &listener.local_addr().unwrap().to_string(),
        SecretBytes::new(vec![65]),
    )
    .unwrap();
    let ctl = ctrl.clone();
    let t = std::thread::spawn(move || ctl.connect(c.source_id));
    assert_eq!(t.join().unwrap(), Err(AppError::Disconnected));
    port.service_controls().unwrap();
    assert!(!ctrl.read(c.source_id).unwrap().active);
    assert!(listener.accept().is_err());
    port.disconnect().unwrap();
}
#[test]
fn save_cannot_race_connect_reading_an_older_credential_record() {
    let (_, c, ctrl, mut port) = setup();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = listener.local_addr().unwrap().to_string();
    ctrl.save(c.source_id, &endpoint, SecretBytes::new(vec![65]))
        .unwrap();
    let (accepted, accept) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        accepted.send(()).unwrap();
        wait.recv_timeout(Duration::from_secs(2)).unwrap();
        drop(stream);
    });
    let ctl = ctrl.clone();
    let request = std::thread::spawn(move || ctl.connect(c.source_id));
    let worker = std::thread::spawn(move || {
        let until = Instant::now() + Duration::from_secs(2);
        loop {
            port.service_controls().unwrap();
            if Instant::now() > until {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        port
    });
    accept.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(
        ctrl.save(c.source_id, "127.0.0.1:32123", SecretBytes::new(vec![66])),
        Err(AppError::Conflict)
    );
    assert_eq!(ctrl.read(c.source_id).unwrap().endpoint, Some(endpoint));
    release.send(()).unwrap();
    assert_eq!(request.join().unwrap(), Err(AppError::Disconnected));
    worker.join().unwrap().disconnect().unwrap();
    server.join().unwrap();
}
#[test]
fn protected_record_and_calendar_backup_never_include_plain_token_or_restore_credentials() {
    use shixu_core::{
        backup::CalendarBackup,
        notifications::consent::{ConsentStore, ModelConsent},
    };
    let root =
        std::env::temp_dir().join(format!("shixu-connection-protect-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let db = Arc::new(Database::open(&root.join("calendar.sqlite"), Arc::new(Synthetic)).unwrap());
    let (_, c, _, _) = setup();
    SettingsStore::new(db.clone())
        .save_source(c.clone())
        .unwrap();
    let (ctrl, mut port) = ConnectionController::new(db.clone());
    ctrl.save(
        c.source_id,
        "127.0.0.1:12345",
        SecretBytes::new(b"synthetic-sensitive-token".to_vec()),
    )
    .unwrap();
    let connection = rusqlite::Connection::open(root.join("calendar.sqlite")).unwrap();
    let sealed: Vec<u8> = connection
        .query_row("SELECT payload FROM source_secrets", [], |r| r.get(0))
        .unwrap();
    assert!(
        !sealed
            .windows(25)
            .any(|w| w == b"synthetic-sensitive-token")
    );
    let consent = Arc::new(ConsentStore::new(ModelConsent::default()));
    let backup = CalendarBackup::new(db.clone(), consent, &root.join("backups")).unwrap();
    backup.automatic_snapshot(0).unwrap();
    let snap = rusqlite::Connection::open(root.join("backups/day-0/calendar.sqlite")).unwrap();
    assert_eq!(
        snap.query_row("SELECT count(*) FROM source_secrets", [], |r| r
            .get::<_, u32>(0))
            .unwrap(),
        0
    );
    let export = backup.export_json(true, true).unwrap();
    assert!(
        !export
            .windows(25)
            .any(|w| w == b"synthetic-sensitive-token")
    );
    let preview = backup.preview(0, 0).unwrap();
    let pause = db.pause_writes().unwrap();
    backup.restore(preview.preview_id, true, 0, &pause).unwrap();
    drop(pause);
    assert!(!db.has_source_secret(&c).unwrap());
    assert!(
        !SettingsStore::new(db.clone()).sources().unwrap()[0]
            .config
            .enabled
    );
    port.disconnect().unwrap();
    drop(snap);
    drop(connection);
    drop(backup);
    drop(ctrl);
    drop(db);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn protection_failure_does_not_fallback_to_plaintext_or_connection() {
    use std::sync::atomic::{AtomicBool, Ordering};
    struct Switch(Arc<AtomicBool>);
    impl DataProtector for Switch {
        fn protect(&self, p: &[u8]) -> AppResult<Vec<u8>> {
            if self.0.load(Ordering::SeqCst) {
                Err(AppError::AuthFailed)
            } else {
                Ok(p.iter().map(|b| b ^ 0xa5).collect())
            }
        }
        fn unprotect(&self, p: &[u8]) -> AppResult<Vec<u8>> {
            self.protect(p)
        }
    }
    let fail = Arc::new(AtomicBool::new(false));
    let db = Arc::new(
        Database::open(
            std::path::Path::new(":memory:"),
            Arc::new(Switch(fail.clone())),
        )
        .unwrap(),
    );
    let (_, c, _, _) = setup();
    SettingsStore::new(db.clone())
        .save_source(c.clone())
        .unwrap();
    let (ctrl, mut port) = ConnectionController::new(db.clone());
    fail.store(true, Ordering::SeqCst);
    assert_eq!(
        ctrl.save(c.source_id, "127.0.0.1:12345", SecretBytes::new(vec![65])),
        Err(AppError::AuthFailed)
    );
    assert!(!db.has_source_secret(&c).unwrap());
    assert!(matches!(ctrl.read(c.source_id), Err(AppError::AuthFailed)));
    port.disconnect().unwrap();
}
#[test]
fn expired_conflicting_request_does_not_disconnect_an_existing_active_source() {
    let (_, c, ctrl, mut port) = setup();
    let (endpoint, server_thread) = server();
    ctrl.save(c.source_id, &endpoint, SecretBytes::new(vec![65]))
        .unwrap();
    connect(ctrl.clone(), port.as_mut(), c.source_id);
    let ctl = ctrl.clone();
    let request = std::thread::spawn(move || ctl.connect(c.source_id));
    assert_eq!(request.join().unwrap(), Err(AppError::Disconnected));
    port.service_controls().unwrap();
    assert!(ctrl.read(c.source_id).unwrap().active);
    port.disconnect().unwrap();
    server_thread.join().unwrap();
}
#[test]
fn legacy_noncanonical_protected_settings_connect_real_socket_and_calendar() {
    use shixu_core::{
        calendar::EventService,
        contracts::calendar::EventQuery,
        notifications::consent::{ConsentStore, ModelConsent},
        runtime::{Supervisor, workers::WorkerPorts},
    };
    let root = std::env::temp_dir().join(format!("shixu-legacy-groups-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let db = Arc::new(Database::open(&root.join("calendar.sqlite"), Arc::new(Synthetic)).unwrap());
    let (_, mut c, _, _) = setup();
    SettingsStore::new(db.clone())
        .save_source(c.clone())
        .unwrap();
    c.allowed_group_ids = vec!["9".into(), "7".into(), "7".into()];
    // Simulate an existing protected noncanonical setting without any resave/epoch advance.
    let raw = rusqlite::Connection::open(root.join("calendar.sqlite")).unwrap();
    let payload: Vec<u8> = serde_json::to_vec(&c)
        .unwrap()
        .iter()
        .map(|b| b ^ 0xa5)
        .collect();
    raw.execute("UPDATE source_settings SET payload=?1", [payload])
        .unwrap();
    let (ctrl, port) = ConnectionController::new(db.clone());
    let (endpoint, server_thread) = server();
    ctrl.save(c.source_id, &endpoint, SecretBytes::new(vec![65]))
        .unwrap();
    let runtime = Arc::new(Supervisor::new(
        db.clone(),
        Arc::new(ConsentStore::new(ModelConsent::default())),
    ));
    runtime.resume().unwrap();
    let workers = runtime
        .spawn_workers(WorkerPorts {
            receiver: Some(port),
            ..Default::default()
        })
        .unwrap();
    ctrl.connect(c.source_id).unwrap();
    let until = Instant::now() + Duration::from_secs(3);
    loop {
        if EventService::new(db.clone())
            .query(EventQuery {
                from_date: None,
                through_date: None,
                statuses: vec![shixu_core::contracts::calendar::EventStatus::Active],
                include_pending: true,
            })
            .unwrap()
            .len()
            == 1
        {
            break;
        }
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(10));
    }
    let setting = SettingsStore::new(db.clone()).sources().unwrap().remove(0);
    assert_eq!(setting.config.allowed_group_ids, ["7", "9"]);
    assert_eq!(setting.epoch, 1, "legacy read does not rewrite epoch");
    workers.stop().unwrap();
    server_thread.join().unwrap();
    drop(runtime);
    drop(ctrl);
    drop(raw);
    drop(db);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn retirement_persists_current_authority_gaps_across_whitelist_disable_restore_and_reopen() {
    use shixu_core::{
        backup::CalendarBackup,
        notifications::consent::{ConsentStore, ModelConsent},
    };
    for mode in ["whitelist", "disable", "restore"] {
        for durable in [false, true] {
            let root =
                std::env::temp_dir().join(format!("shixu-retire-{mode}-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&root).unwrap();
            let path = root.join("calendar.sqlite");
            let db = Arc::new(Database::open(&path, Arc::new(Synthetic)).unwrap());
            let (_, c, _, _) = setup();
            SettingsStore::new(db.clone())
                .save_source(c.clone())
                .unwrap();
            let prior_message = MessageEnvelope {
                message_key: MessageKey::from_uuid(uuid::Uuid::nil()),
                source_id: c.source_id,
                account_id: c.account_id.clone(),
                group_id: "7".into(),
                native_message_id: "122".into(),
                sent_at: 1791504000000,
                received_at: 1791504000000,
                sender_id: "88".into(),
                text: "此前普通说明".into(),
                reply_to: None,
                revision: 1,
                revoked: false,
                processing_state: ProcessingState::Persisted,
                parts: vec![],
            };
            MessageStore::new(db.clone())
                .append_with_cursor(&c, prior_message, "122")
                .unwrap();
            let (ctrl, mut port) = ConnectionController::new(db.clone());
            let (endpoint, server_thread) = server();
            ctrl.save(c.source_id, &endpoint, SecretBytes::new(vec![65]))
                .unwrap();
            connect(ctrl.clone(), port.as_mut(), c.source_id);
            let delivery = port.poll().unwrap().unwrap();
            if durable {
                MessageStore::new(db.clone())
                    .append_with_cursor(&c, delivery.message, &delivery.cursor)
                    .unwrap();
            }
            let backup = CalendarBackup::new(
                db.clone(),
                Arc::new(ConsentStore::new(ModelConsent::default())),
                &root.join("backups"),
            )
            .unwrap();
            if mode == "restore" {
                backup.automatic_snapshot(0).unwrap();
                let preview = backup.preview(0, 0).unwrap();
                let pause = db.pause_writes().unwrap();
                backup.restore(preview.preview_id, true, 0, &pause).unwrap();
                drop(pause);
            } else {
                let mut current = c.clone();
                if mode == "disable" {
                    current.enabled = false;
                } else {
                    current.allowed_group_ids = vec!["9".into()];
                }
                SettingsStore::new(db.clone()).save_source(current).unwrap();
            }
            let raw = rusqlite::Connection::open(&path).unwrap();
            let before: (i64, i64, Option<String>) = raw
                .query_row(
                    "SELECT epoch,since,anchor FROM source_recovery WHERE group_id='7'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .unwrap();
            port.service_controls().unwrap();
            assert!(!ctrl.read(c.source_id).unwrap().active);
            assert_eq!(
                ctrl.read(c.source_id).unwrap().last_error,
                Some(AppError::Conflict)
            );
            let after: (i64, i64, Option<String>, bool) = raw
                .query_row(
                    "SELECT epoch,since,anchor,complete FROM source_recovery WHERE group_id='7'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .unwrap();
            assert!(
                after.0 > before.0,
                "{mode}/{durable}: retirement must durably invalidate old recovery handles"
            );
            assert_eq!(before.2.as_deref(), Some("122"));
            assert_eq!(after.1, before.1);
            assert_eq!(after.2, before.2);
            assert!(!after.3);
            if mode == "whitelist" {
                assert_eq!(
                    raw.query_row(
                        "SELECT epoch FROM source_recovery WHERE group_id='9'",
                        [],
                        |r| r.get::<_, i64>(0)
                    )
                    .unwrap(),
                    after.0
                );
            }
            assert_eq!(
                raw.query_row("SELECT count(*) FROM messages", [], |r| r.get::<_, u32>(0))
                    .unwrap(),
                1 + u32::from(durable)
            );
            assert_eq!(
                raw.query_row("SELECT count(*) FROM runtime_work", [], |r| r
                    .get::<_, u32>(0))
                    .unwrap(),
                1 + u32::from(durable)
            );
            let recovery_cursor: Option<String> = raw
                .query_row(
                    "SELECT recovery_cursor FROM source_recovery WHERE group_id='7'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(
                recovery_cursor.as_deref(),
                Some("122"),
                "unresolved proof cursor retains original anchor"
            );
            let live_cursor: String = raw
                .query_row("SELECT cursor FROM sources WHERE group_id='7'", [], |r| {
                    r.get(0)
                })
                .unwrap();
            assert_eq!(live_cursor, if durable { "123" } else { "122" });
            let current = SettingsStore::new(db.clone()).sources().unwrap().remove(0);
            assert_eq!(current.config.enabled, mode == "whitelist");
            if mode == "whitelist" {
                assert_eq!(current.config.allowed_group_ids, ["9"]);
            }
            port.disconnect().unwrap();
            server_thread.join().unwrap();
            drop(port);
            drop(ctrl);
            drop(raw);
            drop(backup);
            drop(db);
            let reopened = Arc::new(Database::open(&path, Arc::new(Synthetic)).unwrap());
            let raw = rusqlite::Connection::open(&path).unwrap();
            assert_eq!(
                raw.query_row(
                    "SELECT epoch FROM source_recovery WHERE group_id='7'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
                after.0
            );
            assert_eq!(
                MessageStore::new(reopened.clone())
                    .list(Some(c.source_id), 100)
                    .unwrap()
                    .len(),
                1 + usize::from(durable)
            );
            drop(raw);
            drop(reopened);
            std::fs::remove_dir_all(root).unwrap();
        }
    }
}
#[test]
fn retirement_gap_persistence_failure_is_returned_and_socket_still_stops() {
    let root = std::env::temp_dir().join(format!("shixu-retire-fail-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("calendar.sqlite");
    let db = Arc::new(Database::open(&path, Arc::new(Synthetic)).unwrap());
    let (_, c, _, _) = setup();
    SettingsStore::new(db.clone())
        .save_source(c.clone())
        .unwrap();
    let (ctrl, mut port) = ConnectionController::new(db.clone());
    let (endpoint, server_thread) = server();
    ctrl.save(c.source_id, &endpoint, SecretBytes::new(vec![65]))
        .unwrap();
    connect(ctrl.clone(), port.as_mut(), c.source_id);
    let mut disabled = c.clone();
    disabled.enabled = false;
    SettingsStore::new(db.clone())
        .save_source(disabled)
        .unwrap();
    let raw = rusqlite::Connection::open(&path).unwrap();
    raw.execute_batch("CREATE TRIGGER deny_retirement BEFORE UPDATE OF recovery_epoch ON source_bindings BEGIN SELECT RAISE(ABORT,'synthetic storage failure'); END;").unwrap();
    assert_eq!(
        port.service_controls(),
        Err(AppError::Conflict),
        "failed durable gap write must propagate"
    );
    assert!(!ctrl.read(c.source_id).unwrap().active);
    assert_eq!(
        ctrl.read(c.source_id).unwrap().last_error,
        Some(AppError::Conflict)
    );
    port.disconnect().unwrap();
    server_thread.join().unwrap();
    drop(port);
    drop(ctrl);
    drop(raw);
    drop(db);
    std::fs::remove_dir_all(root).unwrap();
}
