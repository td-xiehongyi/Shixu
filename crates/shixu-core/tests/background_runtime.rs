use shixu_core::{
    contracts::{AppResult, error::AppError, notification::*},
    notifications::{MessageStore, settings::SettingsStore},
    storage::{DataProtector, Database},
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
struct Protector(AtomicBool);
impl DataProtector for Protector {
    fn protect(&self, p: &[u8]) -> AppResult<Vec<u8>> {
        if self.0.load(Ordering::SeqCst) {
            Err(AppError::StorageFull)
        } else {
            Ok(p.iter().map(|b| b ^ 0xa5).collect())
        }
    }
    fn unprotect(&self, p: &[u8]) -> AppResult<Vec<u8>> {
        Ok(p.iter().map(|b| b ^ 0xa5).collect())
    }
}
fn database() -> Arc<Database> {
    Arc::new(
        Database::open(
            std::path::Path::new(":memory:"),
            Arc::new(Protector(AtomicBool::new(false))),
        )
        .unwrap(),
    )
}
#[test]
fn pause_blocks_existing_settings_message_and_retention_writers_and_drop_resumes() {
    let db = database();
    let settings = SettingsStore::new(db.clone());
    let pause = db.pause_writes().unwrap();
    assert_eq!(settings.set_autostart(true), Err(AppError::Conflict));
    assert_eq!(
        MessageStore::new(db.clone()).cleanup(3_000_000_000),
        Err(AppError::Conflict)
    );
    assert!(matches!(db.pause_writes(), Err(AppError::Conflict)));
    drop(pause);
    settings.set_autostart(true).unwrap();
    assert!(settings.autostart().unwrap());
}
#[test]
fn pause_drains_live_publication_and_rejects_new_permits() {
    let db = database();
    let permit = db.coordinator().enter().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let other = db.clone();
    let worker = std::thread::spawn(move || {
        let guard = other.pause_writes().unwrap();
        tx.send(()).unwrap();
        guard
    });
    assert!(
        rx.recv_timeout(std::time::Duration::from_millis(80))
            .is_err()
    );
    drop(permit);
    rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
    let guard = worker.join().unwrap();
    assert!(matches!(db.coordinator().enter(), Err(AppError::Conflict)));
    drop(guard);
    assert!(db.coordinator().enter().is_ok());
}
use shixu_core::{
    calendar::EventService,
    contracts::calendar::*,
    notifications::{
        AppendOutcome,
        consent::{ConsentStore, ModelConsent},
    },
    runtime::Supervisor,
};
fn config() -> SourceConfig {
    SourceConfig {
        source_id: SourceId::from_uuid(uuid::Uuid::new_v4()),
        adapter_type: "synthetic".into(),
        account_id: "test".into(),
        allowed_group_ids: vec!["g".into()],
        timezone: "Asia/Shanghai".into(),
        enabled: true,
        capability_set: vec![
            SourceCapability::LiveMessages,
            SourceCapability::Edits,
            SourceCapability::Attachments,
            SourceCapability::Revocations,
        ],
    }
}
fn message(c: &SourceConfig) -> MessageEnvelope {
    MessageEnvelope {
        message_key: MessageKey::from_uuid(uuid::Uuid::new_v4()),
        source_id: c.source_id,
        account_id: c.account_id.clone(),
        group_id: "g".into(),
        native_message_id: uuid::Uuid::new_v4().to_string(),
        sent_at: 1791504000000,
        received_at: 1791504000000,
        sender_id: "synthetic".into(),
        text: "2026年10月12日9:00高数考试".into(),
        reply_to: None,
        revision: 1,
        revoked: false,
        processing_state: ProcessingState::Persisted,
        parts: vec![],
    }
}
fn consent() -> Arc<ConsentStore> {
    Arc::new(ConsentStore::new(ModelConsent::default()))
}
fn events(db: &Arc<Database>) -> Vec<CalendarEvent> {
    EventService::new(db.clone())
        .query(EventQuery {
            from_date: None,
            through_date: None,
            statuses: vec![],
            include_pending: true,
        })
        .unwrap()
}
#[test]
fn persist_before_cursor_advance_and_restart_pending() {
    let db = database();
    let c = config();
    let store = MessageStore::new(db.clone());
    let s = Supervisor::new(db.clone(), consent());
    s.start(vec![c.clone()]).unwrap();
    let m = message(&c);
    assert_eq!(s.receive(m.clone(), "c1").unwrap(), AppendOutcome::Stored);
    assert_eq!(store.cursor(&c, "g").unwrap(), Some("c1".into()));
    assert_eq!(store.list(None, 100).unwrap().len(), 1);
    s.stop().unwrap();
    let restarted = Supervisor::new(db.clone(), consent());
    restarted.start(vec![c.clone()]).unwrap();
    restarted.process_pending(1000).unwrap();
    assert_eq!(events(&db).len(), 1);
    assert_eq!(
        restarted.receive(m, "c2").unwrap(),
        AppendOutcome::Duplicate
    );
    restarted.process_pending(2000).unwrap();
    assert_eq!(events(&db).len(), 1);
    restarted.stop().unwrap();
}
#[test]
fn single_database_runtime_owner_and_stopped_receive() {
    let db = database();
    let c = config();
    let a = Supervisor::new(db.clone(), consent());
    let b = Supervisor::new(db, consent());
    a.start(vec![c.clone()]).unwrap();
    assert_eq!(b.start(vec![c.clone()]), Err(AppError::Conflict));
    a.stop().unwrap();
    assert_eq!(a.receive(message(&c), "c"), Err(AppError::Disconnected));
    b.start(vec![c]).unwrap();
    b.stop().unwrap();
    b.stop().unwrap();
}
use shixu_core::notifications::model::{ModelRequest, ModelTransport};
use std::sync::Mutex;
struct TimeoutTransport(Mutex<Vec<String>>);
impl ModelTransport for TimeoutTransport {
    fn send(&self, r: &ModelRequest) -> AppResult<String> {
        self.0.lock().unwrap().push(r.request_id.clone());
        Err(AppError::Disconnected)
    }
}
fn enabled_consent() -> Arc<ConsentStore> {
    Arc::new(ConsentStore::new(ModelConsent {
        enabled: true,
        provider_id: Some("synthetic".into()),
        allowed_group_ids: vec!["g".into()],
        allow_attachment_text: false,
        revision: 1,
    }))
}
#[test]
fn durable_model_retries_are_bounded_and_keep_stable_identity_and_rules() {
    let db = database();
    let c = config();
    let consent = enabled_consent();
    let s = Supervisor::new(db.clone(), consent.clone());
    s.start(vec![c.clone()]).unwrap();
    s.receive(message(&c), "c1").unwrap();
    s.process_pending(1000).unwrap();
    let t = TimeoutTransport(Mutex::new(vec![]));
    assert!(s.process_model(1000, &t).unwrap());
    assert_eq!(events(&db).len(), 1);
    assert!(!s.process_model(60999, &t).unwrap());
    s.stop().unwrap();
    let s = Supervisor::new(db.clone(), consent);
    s.start(vec![c]).unwrap();
    assert!(s.process_model(61000, &t).unwrap());
    assert!(!s.process_model(360999, &t).unwrap());
    assert!(s.process_model(361000, &t).unwrap());
    assert!(!s.process_model(9_000_000, &t).unwrap());
    let ids = t.0.lock().unwrap();
    assert_eq!(ids.len(), 3);
    assert!(ids.iter().all(|id| id == &ids[0]));
    assert_eq!(events(&db).len(), 1);
    assert_eq!(s.status().model_queue, 0);
}
#[test]
fn model_admission_drops_revoked_consent_and_current_source_and_stale_revision() {
    for change in 0..3 {
        let db = database();
        let mut c = config();
        let consent = enabled_consent();
        let s = Supervisor::new(db.clone(), consent.clone());
        s.start(vec![c.clone()]).unwrap();
        let mut m = message(&c);
        s.receive(m.clone(), "c1").unwrap();
        s.process_pending(1000).unwrap();
        s.schedule_models(1000).unwrap();
        assert_eq!(s.status().model_queue, 1);
        match change {
            0 => {
                consent.replace(1, ModelConsent::default()).unwrap();
            }
            1 => {
                c.enabled = false;
                SettingsStore::new(db.clone())
                    .save_source(c.clone())
                    .unwrap();
            }
            _ => {
                m.revision = 2;
                m.text = "2026年10月13日9:00高数考试".into();
                s.receive(m, "c2").unwrap();
            }
        }
        let t = TimeoutTransport(Mutex::new(vec![]));
        s.process_model(1000, &t).unwrap();
        assert!(t.0.lock().unwrap().is_empty());
        assert_eq!(events(&db).len(), 1);
    }
}
struct UnsupportedParser;
impl shixu_core::runtime::supervisor::AttachmentParser for UnsupportedParser {
    fn parse(&self, _p: &MessagePart, _l: &ParserLimits) -> AppResult<PartResult> {
        Err(AppError::Unsupported)
    }
}
fn with_part(mut m: MessageEnvelope) -> MessageEnvelope {
    m.parts.push(MessagePart {
        part_id: PartId::from_uuid(uuid::Uuid::new_v4()),
        message_key: m.message_key,
        kind: PartKind::Image,
        source_file_ref: Some("synthetic-file".to_string().try_into().unwrap()),
        original_name: None,
        declared_type: None,
        detected_type: None,
        byte_size: Some(100),
        content_hash: None,
        fetch_state: FetchState::Pending,
        parse_state: PartStatus::PendingDownload,
        failure_code: None,
        encrypted_blob_ref: None,
        retained_until: None,
    });
    m
}
#[test]
fn unavailable_parser_is_visible_while_text_rules_commit() {
    let db = database();
    let c = config();
    let s = Supervisor::new(db.clone(), consent());
    s.start(vec![c.clone()]).unwrap();
    s.receive(with_part(message(&c)), "c1").unwrap();
    s.process_pending(1000).unwrap();
    assert_eq!(events(&db).len(), 1);
    assert!(s.process_attachment(1000, &UnsupportedParser).unwrap());
    let store = MessageStore::new(db);
    let m = store.list(None, 100).unwrap().remove(0);
    assert_eq!(
        store.current_parts(m.message_key).unwrap()[0].status,
        PartStatus::Unsupported
    );
    assert_eq!(s.status().attachment_queue, 0);
}
#[test]
fn pause_covers_consent_and_manual_calendar_and_task_writes() {
    let db = database();
    let c = config();
    let consent = consent();
    let s = Supervisor::new(db.clone(), consent.clone());
    s.start(vec![c.clone()]).unwrap();
    s.receive(with_part(message(&c)), "c1").unwrap();
    s.process_pending(1).unwrap();
    let e = events(&db).remove(0);
    let pause = s.pause_writes().unwrap();
    assert_eq!(
        consent.replace(0, ModelConsent::default()),
        Err(AppError::Conflict)
    );
    assert_eq!(
        EventService::new(db.clone()).edit(
            &e.event_id.to_string(),
            e.revision,
            EventPatch {
                event_id: e.event_id,
                expected_revision: e.revision,
                title: Some("manual".into()),
                time: None,
                location: None,
                status: None
            }
        ),
        Err(AppError::Conflict)
    );
    assert!(matches!(
        shixu_core::notifications::parts::TaskQueue::new(db).claim(2, &ParserLimits::v01()),
        Err(AppError::Conflict)
    ));
    drop(pause);
    consent.replace(0, ModelConsent::default()).unwrap();
}
#[test]
fn disk_full_stops_false_healthy_state_and_never_acknowledges_failed_persist() {
    let p = Arc::new(Protector(AtomicBool::new(false)));
    let db = Arc::new(Database::open(std::path::Path::new(":memory:"), p.clone()).unwrap());
    let c = config();
    let s = Supervisor::new(db.clone(), consent());
    s.start(vec![c.clone()]).unwrap();
    p.0.store(true, Ordering::SeqCst);
    assert_eq!(s.receive(message(&c), "c1"), Err(AppError::StorageFull));
    let h = s.status();
    assert_eq!(h.last_error, Some(AppError::StorageFull));
    assert_eq!(h.sources[0].1.last_persisted_at, None);
    assert_eq!(h.sources[0].1.last_applied_at, None);
    assert!(!h.sources[0].1.gaps.is_empty());
    assert_eq!(MessageStore::new(db.clone()).cursor(&c, "g").unwrap(), None);
    assert!(events(&db).is_empty());
}
struct BlockingParser {
    entered: std::sync::mpsc::Sender<()>,
    release: Mutex<std::sync::mpsc::Receiver<()>>,
}
impl shixu_core::runtime::supervisor::AttachmentParser for BlockingParser {
    fn parse(&self, p: &MessagePart, _: &ParserLimits) -> AppResult<PartResult> {
        self.entered.send(()).unwrap();
        self.release
            .lock()
            .unwrap()
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        Ok(PartResult {
            part_id: p.part_id,
            status: PartStatus::Unsupported,
            blocks: vec![],
            reason_code: Some(PartReason::FormatUnsupported),
        })
    }
}
fn wait_for(mut f: impl FnMut() -> bool) -> bool {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while std::time::Instant::now() < until {
        if f() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    false
}
#[test]
fn slow_attachment_does_not_block_text_persistence_or_calendar() {
    let db = database();
    let c = config();
    let s = Arc::new(Supervisor::new(db.clone(), consent()));
    s.start(vec![c.clone()]).unwrap();
    let mut attachment = with_part(message(&c));
    attachment.text.clear();
    s.receive(attachment, "c1").unwrap();
    s.process_pending(1).unwrap();
    let (entered, rx) = std::sync::mpsc::channel();
    let (release, unblock) = std::sync::mpsc::channel();
    let workers = s
        .spawn_workers(shixu_core::runtime::workers::WorkerPorts {
            parser: Some(Arc::new(BlockingParser {
                entered,
                release: Mutex::new(unblock),
            })),
            ..Default::default()
        })
        .unwrap();
    let entered = rx.recv_timeout(std::time::Duration::from_secs(2)).is_ok();
    s.receive(message(&c), "c2").unwrap();
    let committed = wait_for(|| events(&db).len() == 1);
    let cursor = MessageStore::new(db).cursor(&c, "g").unwrap();
    let _ = release.send(());
    workers.stop().unwrap();
    assert!(entered, "attachment worker must actually enter its parser");
    assert!(committed, "text must commit while parser is blocked");
    assert_eq!(cursor, Some("c2".into()));
}
#[test]
fn full_model_queue_does_not_hold_rule_backlog() {
    let db = database();
    let c = config();
    let s = Supervisor::new(db.clone(), enabled_consent());
    s.start(vec![c.clone()]).unwrap();
    for n in 0..105 {
        let mut m = message(&c);
        m.native_message_id = format!("notice-{n}");
        s.receive(m, &format!("c{n}")).unwrap();
        assert_eq!(
            s.process_pending(n),
            Ok(1),
            "optional model admission cannot block committed rules"
        );
    }
    s.schedule_models(106).unwrap();
    s.schedule_models(107).unwrap();
    assert_eq!(s.status().model_queue, 100);
    assert_eq!(s.status().pending_rules, 0);
    assert_eq!(events(&db).len(), 105);
}
#[test]
fn disabled_sources_cannot_starve_current_authorized_rules() {
    let db = database();
    let mut c = config();
    let s = Supervisor::new(db.clone(), consent());
    s.start(vec![c.clone()]).unwrap();
    for _ in 0..100 {
        s.receive(message(&c), "c").unwrap();
    }
    c.enabled = false;
    SettingsStore::new(db.clone()).save_source(c).unwrap();
    let second = config();
    SettingsStore::new(db.clone())
        .save_source(second.clone())
        .unwrap();
    MessageStore::new(db.clone())
        .append(&second, message(&second))
        .unwrap();
    s.process_pending(1).unwrap();
    s.process_pending(2).unwrap();
    assert_eq!(events(&db).len(), 1);
    assert_eq!(s.status().pending_rules, 0);
}
#[test]
fn paused_parser_completion_is_recoverable_without_restart() {
    let db = database();
    let c = config();
    let s = Arc::new(Supervisor::new(db.clone(), consent()));
    s.start(vec![c.clone()]).unwrap();
    s.receive(with_part(message(&c)), "c1").unwrap();
    s.process_pending(1).unwrap();
    let (entered, rx) = std::sync::mpsc::channel();
    let (release, unblock) = std::sync::mpsc::channel();
    let parser = BlockingParser {
        entered,
        release: Mutex::new(unblock),
    };
    let worker = s.clone();
    let worker = std::thread::spawn(move || worker.process_attachment(1, &parser));
    rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
    let pause = s.pause_writes().unwrap();
    release.send(()).unwrap();
    assert_eq!(worker.join().unwrap(), Err(AppError::Conflict));
    drop(pause);
    assert!(s.process_attachment(2, &UnsupportedParser).unwrap());
    assert_eq!(s.status().attachment_queue, 0);
}
struct BlockingModel {
    entered: std::sync::mpsc::Sender<()>,
    release: Mutex<std::sync::mpsc::Receiver<()>>,
}
impl ModelTransport for BlockingModel {
    fn send(&self, _: &ModelRequest) -> AppResult<String> {
        self.entered.send(()).unwrap();
        self.release
            .lock()
            .unwrap()
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        Err(AppError::Disconnected)
    }
}
#[test]
fn slow_model_does_not_block_text_and_pause_drains_admitted_send() {
    let db = database();
    let c = config();
    let s = Arc::new(Supervisor::new(db.clone(), enabled_consent()));
    s.start(vec![c.clone()]).unwrap();
    s.receive(message(&c), "c1").unwrap();
    s.process_pending(1).unwrap();
    let (entered, rx) = std::sync::mpsc::channel();
    let (release, unblock) = std::sync::mpsc::channel();
    let transport = BlockingModel {
        entered,
        release: Mutex::new(unblock),
    };
    let worker = s.clone();
    let worker = std::thread::spawn(move || worker.process_model(1, &transport));
    rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
    // Persistence is independent of the consent/transport lock.
    s.receive(message(&c), "c2").unwrap();
    s.process_pending(2).unwrap();
    assert_eq!(events(&db).len(), 2);
    assert_eq!(
        MessageStore::new(db.clone()).list(None, 100).unwrap().len(),
        2
    );
    let (paused, ready) = std::sync::mpsc::channel();
    let pauser = s.clone();
    let pauser = std::thread::spawn(move || {
        let guard = pauser.pause_writes().unwrap();
        paused.send(()).unwrap();
        guard
    });
    assert!(
        ready
            .recv_timeout(std::time::Duration::from_millis(80))
            .is_err()
    );
    release.send(()).unwrap();
    ready
        .recv_timeout(std::time::Duration::from_secs(2))
        .unwrap();
    let pause = pauser.join().unwrap();
    let _ = worker.join().unwrap();
    drop(pause);
    let t = TimeoutTransport(Mutex::new(vec![]));
    assert!(s.process_model(100000, &t).unwrap());
    assert_eq!(t.0.lock().unwrap().len(), 1);
}
#[test]
fn startup_default_off_cannot_resurrect_old_consent_epoch() {
    let db = database();
    let c = config();
    let first = Supervisor::new(db.clone(), enabled_consent());
    first.start(vec![c.clone()]).unwrap();
    first.receive(message(&c), "c").unwrap();
    first.process_pending(1).unwrap();
    let t = TimeoutTransport(Mutex::new(vec![]));
    first.process_model(1, &t).unwrap();
    first.stop().unwrap();
    let consent = consent();
    let next = Supervisor::new(db, consent.clone());
    next.start(vec![c]).unwrap();
    consent
        .replace(
            0,
            ModelConsent {
                enabled: true,
                provider_id: Some("different-provider".into()),
                allowed_group_ids: vec!["g".into()],
                allow_attachment_text: false,
                revision: 0,
            },
        )
        .unwrap();
    next.process_model(100000, &t).unwrap();
    assert_eq!(t.0.lock().unwrap().len(), 1);
}
struct CountingReceive(Arc<std::sync::atomic::AtomicUsize>);
impl shixu_core::runtime::workers::ReceivePort for CountingReceive {
    fn poll(&mut self) -> AppResult<Option<shixu_core::runtime::workers::Delivery>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(None)
    }
    fn acknowledge(&mut self, _: &str) -> AppResult<()> {
        Ok(())
    }
    fn disconnect(&mut self) -> AppResult<()> {
        Ok(())
    }
}
#[test]
fn worker_lanes_resume_after_suspend_and_explicit_stop_joins() {
    let db = database();
    let c = config();
    let s = Arc::new(Supervisor::new(db, consent()));
    s.start(vec![c]).unwrap();
    let polls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let workers = s
        .spawn_workers(shixu_core::runtime::workers::WorkerPorts {
            receiver: Some(Box::new(CountingReceive(polls.clone()))),
            ..Default::default()
        })
        .unwrap();
    assert!(wait_for(|| polls.load(Ordering::SeqCst) > 0));
    s.suspend(10).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(150));
    let paused = polls.load(Ordering::SeqCst);
    s.resume().unwrap();
    let resumed = wait_for(|| polls.load(Ordering::SeqCst) > paused);
    workers.stop().unwrap();
    assert!(resumed);
    assert!(!s.status().running);
}
#[test]
fn database_reopen_replays_persisted_work_without_duplicate_calendar() {
    let path = std::env::temp_dir().join(format!("shixu-d5-{}.sqlite", uuid::Uuid::new_v4()));
    let p = Arc::new(Protector(AtomicBool::new(false)));
    let c = config();
    let m = message(&c);
    {
        let db = Arc::new(Database::open(&path, p.clone()).unwrap());
        let s = Supervisor::new(db, consent());
        s.start(vec![c.clone()]).unwrap();
        s.receive(m.clone(), "saved").unwrap();
        s.stop().unwrap();
    }
    {
        let db = Arc::new(Database::open(&path, p).unwrap());
        let s = Supervisor::new(db.clone(), consent());
        s.start(vec![c.clone()]).unwrap();
        assert_eq!(
            MessageStore::new(db.clone()).cursor(&c, "g").unwrap(),
            Some("saved".into())
        );
        s.process_pending(2).unwrap();
        assert_eq!(events(&db).len(), 1);
        s.receive(m, "again").unwrap();
        s.process_pending(3).unwrap();
        assert_eq!(events(&db).len(), 1);
    }
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("sqlite-wal"));
    let _ = std::fs::remove_file(path.with_extension("sqlite-shm"));
}
#[test]
fn attachment_queue_pressure_is_visible_and_does_not_block_text_rules() {
    let db = database();
    let c = config();
    let s = Supervisor::new(db.clone(), consent());
    s.start(vec![c.clone()]).unwrap();
    for n in 0..101 {
        let mut notice = with_part(message(&c));
        notice.received_at += n;
        s.receive(notice, &format!("c{n}")).unwrap();
        s.process_pending(n).unwrap();
    }
    s.process_pending(102).unwrap();
    assert_eq!(s.status().attachment_queue, 100);
    assert_eq!(s.status().pending_rules, 0);
    assert_eq!(events(&db).len(), 101);
    assert!(
        MessageStore::new(db.clone())
            .list(None, 100)
            .unwrap()
            .iter()
            .any(|m| m.parts[0].parse_state == PartStatus::LimitExceeded)
    );
}
