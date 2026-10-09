use shixu_core::{
    calendar::EventService,
    contracts::{AppResult, calendar::*, error::AppError, notification::*},
    notifications::{MessageStore, extract::extract},
    storage::{DataProtector, Database},
};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicIsize, Ordering},
    },
};
use uuid::Uuid;
// Insecure reversible protection exists only in this synthetic test binary.
struct Protector(AtomicIsize);
impl DataProtector for Protector {
    fn protect(&self, p: &[u8]) -> AppResult<Vec<u8>> {
        let n = self.0.load(Ordering::SeqCst);
        if n == 0 {
            return Err(AppError::StorageFull);
        }
        if n > 0 {
            self.0.fetch_sub(1, Ordering::SeqCst);
        }
        Ok(p.iter().map(|b| b ^ 0xa5).collect())
    }
    fn unprotect(&self, p: &[u8]) -> AppResult<Vec<u8>> {
        Ok(p.iter().map(|b| b ^ 0xa5).collect())
    }
}
struct Fixture {
    path: PathBuf,
    db: Arc<Database>,
    protector: Arc<Protector>,
    config: SourceConfig,
}
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("shixu-d1-{}.db", Uuid::new_v4()));
        let protector = Arc::new(Protector(AtomicIsize::new(-1)));
        let db = Arc::new(Database::open(&path, protector.clone()).unwrap());
        let config = SourceConfig {
            source_id: SourceId::from_uuid(Uuid::new_v4()),
            adapter_type: "synthetic".into(),
            account_id: "test".into(),
            allowed_group_ids: vec!["g1".into(), "g2".into()],
            timezone: "Asia/Shanghai".into(),
            enabled: true,
            capability_set: vec![
                SourceCapability::LiveMessages,
                SourceCapability::Edits,
                SourceCapability::Revocations,
                SourceCapability::Attachments,
            ],
        };
        MessageStore::new(db.clone()).bind_source(&config).unwrap();
        Self {
            path,
            db,
            protector,
            config,
        }
    }
    fn service(&self) -> EventService {
        EventService::new(self.db.clone())
    }
    fn msg(&self, text: &str, sent: i64, reply: Option<MessageKey>) -> MessageEnvelope {
        let m = MessageEnvelope {
            message_key: MessageKey::from_uuid(Uuid::new_v4()),
            source_id: self.config.source_id,
            account_id: self.config.account_id.clone(),
            group_id: "g1".into(),
            native_message_id: Uuid::new_v4().to_string(),
            sent_at: sent,
            received_at: sent,
            sender_id: "synthetic".into(),
            text: text.into(),
            reply_to: reply,
            revision: 1,
            revoked: false,
            processing_state: ProcessingState::Persisted,
            parts: vec![],
        };
        self.persist(m)
    }
    fn persist(&self, m: MessageEnvelope) -> MessageEnvelope {
        let key = shixu_core::notifications::identity::message_identity(&self.config, &m)
            .unwrap()
            .key;
        MessageStore::new(self.db.clone())
            .append(&self.config, m)
            .unwrap();
        let c = rusqlite::Connection::open(&self.path).unwrap();
        let sealed: Vec<u8> = c
            .query_row(
                "SELECT payload FROM messages WHERE message_key=?1",
                [key.to_string()],
                |r| r.get(0),
            )
            .unwrap();
        serde_json::from_slice(&self.protector.unprotect(&sealed).unwrap()).unwrap()
    }
    fn batch(&self, m: &MessageEnvelope, ctx: &[MessageEnvelope]) -> ExtractBatch {
        extract(m, &[], ctx, &self.config.timezone).unwrap()
    }
    fn events(&self) -> Vec<CalendarEvent> {
        self.service()
            .query(EventQuery {
                from_date: None,
                through_date: None,
                statuses: vec![],
                include_pending: true,
            })
            .unwrap()
    }
    fn create(&self) -> (MessageEnvelope, ApplySummary) {
        let m = self.msg("2026年10月12日9:00高数考试", 1791504000000, None);
        let summary = self.service().apply(self.batch(&m, &[])).unwrap();
        (m, summary)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_file(self.path.with_extension("db-wal"));
        let _ = std::fs::remove_file(self.path.with_extension("db-shm"));
    }
}
fn patch(e: &CalendarEvent) -> EventPatch {
    EventPatch {
        event_id: e.event_id,
        expected_revision: e.revision,
        title: None,
        time: None,
        location: None,
        status: None,
    }
}
#[test]
fn duplicate_replay_has_one_effect() {
    let f = Fixture::new();
    let (m, s) = f.create();
    assert_eq!(s.created, 1);
    assert_eq!(f.service().apply(f.batch(&m, &[])).unwrap().created, 0);
    assert_eq!(f.events().len(), 1);
}
#[test]
fn reply_reschedule_updates_one_event() {
    let f = Fixture::new();
    let (m, _) = f.create();
    let n = f.msg(
        "高数考试改至2026年10月13日10:00",
        m.sent_at + 1,
        Some(m.message_key),
    );
    assert_eq!(f.service().apply(f.batch(&n, &[m])).unwrap().updated, 1);
    assert_eq!(f.events()[0].local_date.as_deref(), Some("2026-10-13"));
}
#[test]
fn cancel_is_status_recall_is_not_cancel() {
    let f = Fixture::new();
    let (m, _) = f.create();
    let n = f.msg("取消高数考试", m.sent_at + 1, Some(m.message_key));
    assert_eq!(
        f.service()
            .apply(f.batch(&n, std::slice::from_ref(&m)))
            .unwrap()
            .cancelled,
        1
    );
    assert_eq!(f.events()[0].status, EventStatus::Cancelled);
    let mut recall = m.clone();
    recall.revoked = true;
    recall.revision = 2;
    recall.received_at += 10;
    let recall = f.persist(recall);
    assert_eq!(f.service().apply(f.batch(&recall, &[])).unwrap().created, 0);
    assert_eq!(f.events()[0].status, EventStatus::Cancelled);
}
#[test]
fn ambiguous_target_is_pending() {
    let f = Fixture::new();
    f.create();
    let m = f.msg("高数考试改至2026年10月13日", 1791504000010, None);
    assert_eq!(f.service().apply(f.batch(&m, &[])).unwrap().pending, 1);
    assert_eq!(f.events().len(), 1);
}
#[test]
fn user_override_survives_automatic_update() {
    let f = Fixture::new();
    let (m, _) = f.create();
    let e = f.events().remove(0);
    let mut p = patch(&e);
    p.time = Some(
        shixu_core::notifications::time::parse_time("2026年10月20日", m.sent_at, "Asia/Shanghai")
            .unwrap(),
    );
    f.service()
        .edit(&e.event_id.to_string(), e.revision, p)
        .unwrap();
    let n = f.msg(
        "高数考试改至2026年10月13日",
        m.sent_at + 1,
        Some(m.message_key),
    );
    assert_eq!(f.service().apply(f.batch(&n, &[m])).unwrap().conflicts, 1);
    assert_eq!(f.events()[0].local_date.as_deref(), Some("2026-10-20"));
}
#[test]
fn undo_revision_conflict_preserves_newer_value() {
    let f = Fixture::new();
    let (_, s) = f.create();
    let e = f.events().remove(0);
    let mut p = patch(&e);
    p.title = Some("人工高数考试".into());
    f.service()
        .edit(&e.event_id.to_string(), e.revision, p)
        .unwrap();
    assert_eq!(
        f.service().undo(UndoRequest {
            change_id: s.change_ids[0],
            expected_revision: 1
        }),
        Err(AppError::Conflict)
    );
    assert_eq!(f.events()[0].title, "人工高数考试");
}
#[test]
fn removed_event_does_not_resurrect() {
    let f = Fixture::new();
    let (m, s) = f.create();
    f.service()
        .undo(UndoRequest {
            change_id: s.change_ids[0],
            expected_revision: 1,
        })
        .unwrap();
    assert_eq!(f.service().apply(f.batch(&m, &[])).unwrap().created, 0);
    assert_eq!(f.events()[0].status, EventStatus::Removed);
}
#[test]
fn late_attachment_does_not_revert_newer_notice() {
    let f = Fixture::new();
    let (m, _) = f.create();
    let n = f.msg(
        "高数考试改至2026年10月13日",
        m.sent_at + 1,
        Some(m.message_key),
    );
    f.service()
        .apply(f.batch(&n, std::slice::from_ref(&m)))
        .unwrap();
    let mut edited = m.clone();
    edited.revision = 2;
    edited.text = "2026年10月14日高数考试".into();
    edited.received_at = n.received_at + 1;
    let edited = f.persist(edited);
    assert_eq!(
        f.service().apply(f.batch(&edited, &[])).unwrap().conflicts,
        1
    );
    assert_eq!(f.events()[0].local_date.as_deref(), Some("2026-10-13"));
}
#[test]
fn durable_revision_order_zone_and_utf8_are_authenticated() {
    let f = Fixture::new();
    let (m, _) = f.create();
    for mode in 0..5 {
        let mut b = f.batch(&m, &[]);
        match mode {
            0 => b.message_revision += 1,
            1 => b.source_order += 1,
            2 => b.candidates[0].time.timezone = "Etc/UTC".into(),
            3 => b.candidates[0].evidence[0].text = "伪造".into(),
            _ => b.candidates[0].title = "伪造考试".into(),
        };
        assert!(f.service().apply(b).is_err());
    }
    assert_eq!(f.events().len(), 1);
}
#[test]
fn transaction_fault_rolls_back_and_reopens() {
    for fail in 0..8 {
        let f = Fixture::new();
        let m = f.msg("2026年10月12日高数考试", 1791504000000, None);
        f.protector.0.store(fail, Ordering::SeqCst);
        let result = f.service().apply(f.batch(&m, &[]));
        f.protector.0.store(-1, Ordering::SeqCst);
        if result.is_err() {
            assert!(f.events().is_empty());
            let c = rusqlite::Connection::open(&f.path).unwrap();
            for table in ["calendar_sources", "calendar_changes"] {
                assert_eq!(
                    c.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                        .get::<_, i64>(0))
                        .unwrap(),
                    0
                );
            }
            assert_eq!(
                MessageStore::new(f.db.clone()).pending(10).unwrap().len(),
                1
            );
        }
        let reopened = Arc::new(Database::open(&f.path, f.protector.clone()).unwrap());
        let svc = EventService::new(reopened);
        let summary = svc.apply(f.batch(&m, &[])).unwrap();
        assert_eq!(summary.created, if result.is_err() { 1 } else { 0 });
    }
}
#[test]
fn manual_requires_explicit_id_time_and_patch_consistency() {
    let f = Fixture::new();
    let p = EventPatch {
        event_id: EventId::from_uuid(Uuid::new_v4()),
        expected_revision: 0,
        title: Some("手工".into()),
        time: Some(
            shixu_core::notifications::time::parse_time("2026年10月12日", 0, "Asia/Shanghai")
                .unwrap(),
        ),
        location: None,
        status: None,
    };
    let e = f.service().create_manual(p.clone()).unwrap();
    assert_eq!(e.kind, "manual");
    assert!(f.service().create_manual(p).is_err());
    let mut bad = patch(&e);
    bad.expected_revision += 1;
    assert_eq!(
        f.service().edit(&e.event_id.to_string(), e.revision, bad),
        Err(AppError::InvalidInput)
    );
}
#[test]
fn unsupported_body_stays_pending() {
    let f = Fixture::new();
    let m = f.msg("明天研究生答辩", 1791504000000, None);
    assert!(f.batch(&m, &[]).candidates.is_empty());
    f.service().apply(f.batch(&m, &[])).unwrap();
    assert_eq!(
        MessageStore::new(f.db.clone()).pending(10).unwrap()[0].processing_state,
        ProcessingState::Pending
    );
}
#[test]
fn equal_order_conflict_remains_observable_after_replay() {
    let f = Fixture::new();
    let (m, _) = f.create();
    let n = f.msg("高数考试改至2026年10月13日", m.sent_at, Some(m.message_key));
    let b = f.batch(&n, &[m]);
    assert_eq!(f.service().apply(b.clone()).unwrap().conflicts, 1);
    f.service().apply(b).unwrap();
    assert_eq!(
        MessageStore::new(f.db.clone()).pending(10).unwrap().len(),
        1
    );
    assert_eq!(
        f.service()
            .notices()
            .unwrap()
            .iter()
            .filter(|s| s.outcome == shixu_core::calendar::changes::SourceOutcome::Conflict)
            .count(),
        1
    );
}
#[test]
fn multiple_subjects_and_cross_group_same_title_are_separate() {
    let f = Fixture::new();
    let m = f.msg(
        "2026年10月12日高数考试；2026年10月13日英语考试",
        1791504000000,
        None,
    );
    assert_eq!(f.service().apply(f.batch(&m, &[])).unwrap().created, 2);
    let mut n = m.clone();
    n.group_id = "g2".into();
    n.native_message_id = "group2".into();
    let n = f.persist(n);
    assert_eq!(f.service().apply(f.batch(&n, &[])).unwrap().created, 2);
    assert_eq!(f.events().len(), 4);
}
#[test]
fn historical_year_evidence_is_durable_and_bound() {
    let f = Fixture::new();
    let (m, _) = f.create();
    let n = f.msg(
        "高数考试改至10月13日10:00",
        m.sent_at + 1,
        Some(m.message_key),
    );
    let b = f.batch(&n, std::slice::from_ref(&m));
    assert_eq!(b.candidates[0].time.precision, Precision::Exact);
    assert_eq!(f.service().apply(b.clone()).unwrap().updated, 1);
    let mut forged = b;
    let e = forged.candidates[0].evidence.last_mut().unwrap();
    e.text = "2027年10月12日9:00高数考试".into();
    assert_eq!(f.service().apply(forged), Err(AppError::InvalidInput));
}
#[test]
fn attachment_requires_stored_current_parser_result_and_conflict_is_preserved() {
    let f = Fixture::new();
    let mut m = f.msg("2026年10月12日高数考试", 1791504000000, None);
    let id = PartId::from_uuid(Uuid::new_v4());
    m.parts.push(MessagePart {
        part_id: id,
        message_key: m.message_key,
        kind: PartKind::Image,
        source_file_ref: Some("synthetic-file".to_string().try_into().unwrap()),
        original_name: Some("synthetic.png".into()),
        declared_type: Some("image/png".into()),
        detected_type: None,
        byte_size: None,
        content_hash: None,
        fetch_state: FetchState::Fetched,
        parse_state: PartStatus::Success,
        failure_code: None,
        encrypted_blob_ref: None,
        retained_until: None,
    });
    m.revision = 2;
    let m = f.persist(m);
    let block = EvidenceBlock {
        part_id: id,
        page_or_sheet: Some(PageOrSheet::Page { number: 1 }),
        cell_range_or_bbox: Some(EvidenceLocation::BoundingBox {
            x: 0,
            y: 0,
            width: 100,
            height: 100,
        }),
        text: "2026年10月13日高数考试".into(),
        method: Method::Ocr,
        engine_version: "test-parser-1".into(),
        quality_flags: vec![],
    };
    let b = extract(&m, std::slice::from_ref(&block), &[], &f.config.timezone).unwrap();
    assert_eq!(f.service().apply(b.clone()), Err(AppError::InvalidInput));
    MessageStore::new(f.db.clone())
        .record_parts(
            &m.message_key,
            m.revision,
            vec![PartResult {
                part_id: id,
                status: PartStatus::Success,
                blocks: vec![block],
                reason_code: None,
            }],
        )
        .unwrap();
    assert_eq!(f.service().apply(b).unwrap().created, 1);
    assert_eq!(f.events()[0].time_precision, Precision::UnknownDate);
    let mut forged = extract(&m, &[], &[], &f.config.timezone).unwrap();
    forged.candidates[0].time.local_date = Some("2026-10-15".into());
    assert!(f.service().apply(forged).is_err());
}
#[test]
fn sql_write_faults_roll_back_every_effect() {
    for table in [
        "calendar_events",
        "calendar_changes",
        "calendar_sources",
        "calendar_batches",
        "messages",
    ] {
        let f = Fixture::new();
        let m = f.msg("2026年10月12日高数考试", 1791504000000, None);
        let c = rusqlite::Connection::open(&f.path).unwrap();
        let op = if table == "messages" {
            "UPDATE"
        } else {
            "INSERT"
        };
        c.execute_batch(&format!("CREATE TRIGGER fail_write BEFORE {op} ON {table} BEGIN SELECT RAISE(ABORT,'synthetic fault'); END;")).unwrap();
        assert!(f.service().apply(f.batch(&m, &[])).is_err());
        assert!(f.events().is_empty());
        for t in ["calendar_changes", "calendar_sources", "calendar_batches"] {
            assert_eq!(
                c.query_row(&format!("SELECT count(*) FROM {t}"), [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                0
            );
        }
        assert_eq!(
            MessageStore::new(f.db.clone()).pending(10).unwrap().len(),
            1
        );
        c.execute_batch("DROP TRIGGER fail_write;").unwrap();
        assert_eq!(f.service().apply(f.batch(&m, &[])).unwrap().created, 1);
    }
}
#[test]
fn suppression_allow_is_explicit_and_history_is_retained() {
    let f = Fixture::new();
    let (m, s) = f.create();
    let key = f.batch(&m, &[]).candidates[0].candidate_key;
    let removed = f
        .service()
        .undo(UndoRequest {
            change_id: s.change_ids[0],
            expected_revision: 1,
        })
        .unwrap();
    assert_eq!(
        f.service().origin(&removed.event_id.to_string()).unwrap(),
        shixu_core::calendar::changes::EventOrigin::Source
    );
    assert_eq!(
        f.service()
            .history(&removed.event_id.to_string())
            .unwrap()
            .len(),
        2
    );
    f.service().allow_suppressed(&key.to_string()).unwrap();
    assert_eq!(f.events()[0].status, EventStatus::Active);
    assert_eq!(f.service().apply(f.batch(&m, &[])).unwrap().created, 0);
}
#[test]
fn edited_message_keeps_identity_and_stale_batch_cannot_apply() {
    let f = Fixture::new();
    let (m, _) = f.create();
    let mut edit = m.clone();
    edit.text = "2026年10月14日高数考试".into();
    edit.revision = 2;
    let edit = f.persist(edit);
    assert_eq!(f.service().apply(f.batch(&m, &[])), Err(AppError::Conflict));
    assert_eq!(f.service().apply(f.batch(&edit, &[])).unwrap().updated, 1);
    assert_eq!(f.events().len(), 1);
    assert_eq!(f.events()[0].local_date.as_deref(), Some("2026-10-14"));
}
#[test]
fn undo_update_restores_old_value_without_erasing_history() {
    let f = Fixture::new();
    let (m, _) = f.create();
    let n = f.msg(
        "高数考试改至2026年10月13日",
        m.sent_at + 1,
        Some(m.message_key),
    );
    let s = f.service().apply(f.batch(&n, &[m])).unwrap();
    let e = f.events().remove(0);
    let old = f
        .service()
        .undo(UndoRequest {
            change_id: s.change_ids[0],
            expected_revision: e.revision,
        })
        .unwrap();
    assert_eq!(old.local_date.as_deref(), Some("2026-10-12"));
    assert_eq!(old.revision, 3);
    assert_eq!(
        f.service()
            .history(&old.event_id.to_string())
            .unwrap()
            .len(),
        3
    );
}
#[test]
fn ambiguous_subject_edit_does_not_duplicate_original() {
    let f = Fixture::new();
    let (m, _) = f.create();
    let mut edit = m;
    edit.text = "2026年10月14日数学考试".into();
    edit.revision = 2;
    let edit = f.persist(edit);
    let s = f.service().apply(f.batch(&edit, &[])).unwrap();
    assert_eq!(s.created, 0);
    assert_eq!(s.conflicts, 1);
    assert_eq!(f.events().len(), 1);
}
#[test]
fn unknown_date_event_keeps_message_pending() {
    let f = Fixture::new();
    let m = f.msg("近期高数考试", 1791504000000, None);
    let s = f.service().apply(f.batch(&m, &[])).unwrap();
    assert_eq!(s.created, 1);
    assert_eq!(
        MessageStore::new(f.db.clone()).pending(10).unwrap()[0].processing_state,
        ProcessingState::Pending
    );
}
#[test]
fn reschedule_before_original_calendar_apply_is_retried() {
    let f = Fixture::new();
    let m = f.msg("2026年10月12日高数考试", 1791504000000, None);
    let n = f.msg(
        "高数考试改至2026年10月13日",
        m.sent_at + 1,
        Some(m.message_key),
    );
    assert_eq!(
        f.service()
            .apply(f.batch(&n, std::slice::from_ref(&m)))
            .unwrap()
            .pending,
        1
    );
    let s = f.service().apply(f.batch(&m, &[])).unwrap();
    assert_eq!(s.created, 1);
    assert_eq!(s.updated, 1);
    assert_eq!(f.events()[0].local_date.as_deref(), Some("2026-10-13"));
    assert_eq!(
        MessageStore::new(f.db.clone()).pending(10).unwrap().len(),
        0
    );
}
#[test]
fn pending_notice_revalidates_historical_year_after_target_edit() {
    let f = Fixture::new();
    let m = f.msg("2026年10月12日高数考试", 1791504000000, None);
    let n = f.msg("高数考试改至10月13日", m.sent_at + 1, Some(m.message_key));
    assert_eq!(
        f.service()
            .apply(f.batch(&n, std::slice::from_ref(&m)))
            .unwrap()
            .pending,
        1
    );
    let mut changed = m;
    changed.revision = 2;
    changed.text = "2027年10月12日高数考试".into();
    let changed = f.persist(changed);
    f.service().apply(f.batch(&changed, &[])).unwrap();
    assert_eq!(f.events()[0].local_date.as_deref(), Some("2027-10-12"));
    assert!(
        f.service()
            .notices()
            .unwrap()
            .iter()
            .any(|s| s.outcome == shixu_core::calendar::changes::SourceOutcome::Conflict)
    );
}
#[test]
fn legacy_schema_upgrades_preserve_protected_messages_and_parser_rows() {
    for version in 1..=4 {
        let f = Fixture::new();
        let m = f.msg("synthetic unsupported", 1791504000000, None);
        let c = rusqlite::Connection::open(&f.path).unwrap();
        let sealed = f
            .protector
            .protect(&serde_json::to_vec(&Vec::<PartResult>::new()).unwrap())
            .unwrap();
        c.execute(
            "INSERT INTO part_results VALUES (?1,1,?2)",
            rusqlite::params![m.message_key.to_string(), sealed],
        )
        .unwrap();
        c.execute_batch("DROP TABLE calendar_suppressions; DROP TABLE calendar_changes; DROP TABLE calendar_sources; DROP TABLE calendar_batches; DROP TABLE calendar_events;").unwrap();
        if version < 4 {
            c.execute_batch("ALTER TABLE attachment_tasks DROP COLUMN retry_limit;")
                .unwrap();
        }
        if version < 3 {
            c.execute_batch("DROP TABLE attachment_tasks;").unwrap();
        }
        if version < 2 {
            c.execute_batch("DROP TABLE source_recovery; DROP TABLE source_bindings;")
                .unwrap();
        }
        c.execute_batch(&format!("PRAGMA user_version={version};"))
            .unwrap();
        let db = Arc::new(Database::open(&f.path, f.protector.clone()).unwrap());
        assert_eq!(
            MessageStore::new(db.clone()).pending(10).unwrap()[0].text,
            m.text
        );
        assert_eq!(
            c.query_row::<i64, _, _>("PRAGMA user_version", [], |r| r.get(0))
                .unwrap(),
            5
        );
        assert_eq!(
            c.query_row::<i64, _, _>("SELECT count(*) FROM part_results", [], |r| r.get(0))
                .unwrap(),
            1
        );
        assert!(
            EventService::new(db)
                .query(EventQuery {
                    from_date: None,
                    through_date: None,
                    statuses: vec![],
                    include_pending: true
                })
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn immutable_change_history_retains_original_revision_evidence() {
    let f = Fixture::new();
    let (m, _) = f.create();
    let mut edit = m.clone();
    edit.revision = 2;
    edit.text = "2026年10月14日高数考试".into();
    let edit = f.persist(edit);
    f.service().apply(f.batch(&edit, &[])).unwrap();
    let id = f.events()[0].event_id;
    let history = f.service().history(&id.to_string()).unwrap();
    assert_eq!(history[0].source.as_ref().unwrap().message_revision, 1);
    assert_eq!(
        history[0].source.as_ref().unwrap().candidate.evidence[0].text,
        m.text
    );
    assert_eq!(history[1].source.as_ref().unwrap().message_revision, 2);
}
#[test]
fn durable_adapter_namespace_cannot_bypass_source_binding() {
    let mut f = Fixture::new();
    f.config.adapter_type = "different-synthetic-adapter".into();
    let m = f.msg("2026年10月12日高数考试", 1791504000000, None);
    assert_eq!(
        f.service().apply(f.batch(&m, &[])),
        Err(AppError::InvalidInput)
    );
    assert!(f.events().is_empty());
}
#[test]
fn suppression_write_failure_rolls_back_undo_and_history() {
    let f = Fixture::new();
    let (_, s) = f.create();
    let id = f.events()[0].event_id;
    let c = rusqlite::Connection::open(&f.path).unwrap();
    c.execute_batch("CREATE TRIGGER fail_suppression AFTER INSERT ON calendar_suppressions BEGIN SELECT RAISE(ABORT,'synthetic fault'); END;").unwrap();
    let request = UndoRequest {
        change_id: s.change_ids[0],
        expected_revision: 1,
    };
    assert!(f.service().undo(request.clone()).is_err());
    assert_eq!(f.events()[0].status, EventStatus::Active);
    assert_eq!(f.events()[0].revision, 1);
    assert!(!f.service().history(&id.to_string()).unwrap()[0].undone);
    assert_eq!(
        c.query_row::<i64, _, _>("SELECT count(*) FROM calendar_suppressions", [], |r| r
            .get(0))
            .unwrap(),
        0
    );
    c.execute_batch("DROP TRIGGER fail_suppression;").unwrap();
    assert_eq!(
        f.service().undo(request).unwrap().status,
        EventStatus::Removed
    );
}
#[test]
fn calendar_payloads_and_history_use_existing_protector() {
    let f = Fixture::new();
    let (_, s) = f.create();
    let e = f.events().remove(0);
    let mut p = patch(&e);
    p.title = Some("SYNTHETIC-CALENDAR-SENSITIVE-MARKER".into());
    f.service()
        .edit(&e.event_id.to_string(), e.revision, p)
        .unwrap();
    let c = rusqlite::Connection::open(&f.path).unwrap();
    c.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);").unwrap();
    let bytes = std::fs::read(&f.path).unwrap();
    for plain in ["SYNTHETIC-CALENDAR-SENSITIVE-MARKER", "高数考试"] {
        assert!(!bytes.windows(plain.len()).any(|w| w == plain.as_bytes()));
    }
    assert_eq!(
        f.service().history(&e.event_id.to_string()).unwrap()[0].change_id,
        s.change_ids[0]
    );
}
#[test]
fn stored_late_attachment_cannot_replace_newer_reply_notice() {
    let f = Fixture::new();
    let mut m = f.msg("2026年10月12日高数考试", 1791504000000, None);
    let id = PartId::from_uuid(Uuid::new_v4());
    m.parts.push(MessagePart {
        part_id: id,
        message_key: m.message_key,
        kind: PartKind::Image,
        source_file_ref: Some("synthetic-late-image".to_string().try_into().unwrap()),
        original_name: None,
        declared_type: None,
        detected_type: None,
        byte_size: None,
        content_hash: None,
        fetch_state: FetchState::Fetched,
        parse_state: PartStatus::Success,
        failure_code: None,
        encrypted_blob_ref: None,
        retained_until: None,
    });
    m.revision = 2;
    let m = f.persist(m);
    f.service().apply(f.batch(&m, &[])).unwrap();
    let n = f.msg(
        "高数考试改至2026年10月13日10:00",
        m.sent_at + 1,
        Some(m.message_key),
    );
    f.service()
        .apply(f.batch(&n, std::slice::from_ref(&m)))
        .unwrap();
    let block = EvidenceBlock {
        part_id: id,
        page_or_sheet: None,
        cell_range_or_bbox: Some(EvidenceLocation::BoundingBox {
            x: 0,
            y: 0,
            width: 100,
            height: 100,
        }),
        text: "2026年10月12日9:00高数考试".into(),
        method: Method::Ocr,
        engine_version: "synthetic-parser-1".into(),
        quality_flags: vec![],
    };
    MessageStore::new(f.db.clone())
        .record_parts(
            &m.message_key,
            m.revision,
            vec![PartResult {
                part_id: id,
                status: PartStatus::Success,
                blocks: vec![block.clone()],
                reason_code: None,
            }],
        )
        .unwrap();
    let late = extract(&m, &[block], &[], &f.config.timezone).unwrap();
    assert_eq!(f.service().apply(late).unwrap().conflicts, 1);
    assert_eq!(f.events()[0].local_date.as_deref(), Some("2026-10-13"));
}
