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

fn image_part(message: &MessageEnvelope) -> MessagePart {
    MessagePart {
        part_id: PartId::from_uuid(Uuid::new_v4()),
        message_key: message.message_key,
        kind: PartKind::Image,
        source_file_ref: Some("synthetic-review-image".to_string().try_into().unwrap()),
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
    }
}
fn parser_block(part_id: PartId, text: &str) -> EvidenceBlock {
    EvidenceBlock {
        part_id,
        page_or_sheet: None,
        cell_range_or_bbox: Some(EvidenceLocation::BoundingBox {
            x: 0,
            y: 0,
            width: 100,
            height: 100,
        }),
        text: text.into(),
        method: Method::Ocr,
        engine_version: "synthetic-review-parser".into(),
        quality_flags: vec![],
    }
}
fn save_parser(f: &Fixture, m: &MessageEnvelope, block: EvidenceBlock) {
    MessageStore::new(f.db.clone())
        .record_parts(
            &m.message_key,
            m.revision,
            vec![PartResult {
                part_id: block.part_id,
                status: PartStatus::Success,
                blocks: vec![block],
                reason_code: None,
            }],
        )
        .unwrap();
}
#[test]
fn review1_f1_omitted_current_candidate_cannot_revive_stale_precise_notice() {
    let f = Fixture::new();
    let original = f.msg("2026年10月12日高数考试", 1791504000000, None);
    let mut notice = f.msg(
        "高数考试改至2026年10月13日",
        original.sent_at + 1,
        Some(original.message_key),
    );
    let part = image_part(&notice);
    let id = part.part_id;
    notice.parts.push(part);
    notice.revision = 2;
    let notice = f.persist(notice);
    assert_eq!(
        f.service()
            .apply(f.batch(&notice, std::slice::from_ref(&original)))
            .unwrap()
            .pending,
        1
    );
    let block = parser_block(id, "2026年10月14日高数考试改至2026年10月14日");
    save_parser(&f, &notice, block.clone());
    let mut omitted = extract(
        &notice,
        &[block],
        std::slice::from_ref(&original),
        &f.config.timezone,
    )
    .unwrap();
    assert_eq!(omitted.candidates[0].time.precision, Precision::UnknownDate);
    omitted.candidates.clear();
    f.service().apply(omitted).unwrap();
    let summary = f.service().apply(f.batch(&original, &[])).unwrap();
    assert_eq!(summary.updated, 0);
    assert_eq!(f.events()[0].local_date.as_deref(), Some("2026-10-12"));
    assert!(
        f.service()
            .notices()
            .unwrap()
            .iter()
            .any(|s| s.outcome == shixu_core::calendar::changes::SourceOutcome::Conflict)
    );
}
#[test]
fn review1_f2_cleanup_removes_non_event_body_and_attachment_duplicates() {
    let f = Fixture::new();
    let mut m = f.msg("谢谢老师", 1791504000000, None);
    let part = image_part(&m);
    let id = part.part_id;
    m.parts.push(part);
    m.revision = 2;
    let m = f.persist(m);
    let block = parser_block(id, "谢谢老师");
    save_parser(&f, &m, block.clone());
    f.service()
        .apply(extract(&m, &[block], &[], &f.config.timezone).unwrap())
        .unwrap();
    let store = MessageStore::new(f.db.clone());
    let deadline = m.received_at + shixu_core::notifications::retention::NON_EVENT_RETENTION_MILLIS;
    assert_eq!(store.cleanup(deadline - 1).unwrap(), 0);
    assert_eq!(store.cleanup(deadline).unwrap(), 1);
    let c = rusqlite::Connection::open(&f.path).unwrap();
    assert!(
        c.query_row::<bool, _, _>(
            "SELECT payload IS NULL FROM messages WHERE message_key=?1",
            [m.message_key.to_string()],
            |r| r.get(0)
        )
        .unwrap()
    );
    for table in ["part_results", "calendar_batches"] {
        assert_eq!(
            c.query_row::<i64, _, _>(
                &format!("SELECT count(*) FROM {table} WHERE message_key=?1"),
                [m.message_key.to_string()],
                |r| r.get(0)
            )
            .unwrap(),
            0
        );
    }
    let db = Arc::new(Database::open(&f.path, f.protector.clone()).unwrap());
    assert_eq!(MessageStore::new(db).cleanup(deadline).unwrap(), 0);
}
#[test]
fn review1_f2_linked_chat_edit_is_not_disposable_non_event() {
    let f = Fixture::new();
    let (original, s) = f.create();
    f.service()
        .undo(UndoRequest {
            change_id: s.change_ids[0],
            expected_revision: 1,
        })
        .unwrap();
    let mut chat = original.clone();
    chat.revision = 2;
    chat.text = "谢谢老师".into();
    let chat = f.persist(chat);
    f.service().apply(f.batch(&chat, &[])).unwrap();
    let c = rusqlite::Connection::open(&f.path).unwrap();
    let state: String = c
        .query_row(
            "SELECT processing_state FROM messages WHERE message_key=?1",
            [chat.message_key.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_ne!(state, "non_event");
    assert_eq!(
        MessageStore::new(f.db.clone())
            .cleanup(
                chat.received_at + shixu_core::notifications::retention::NON_EVENT_RETENTION_MILLIS
            )
            .unwrap(),
        0
    );
    assert_eq!(f.events()[0].status, EventStatus::Removed);
    assert!(
        f.service()
            .history(&f.events()[0].event_id.to_string())
            .unwrap()[0]
            .undone
    );
    assert_eq!(
        c.query_row::<i64, _, _>("SELECT count(*) FROM calendar_suppressions", [], |r| r
            .get(0))
            .unwrap(),
        1
    );
}
#[test]
fn review1_f3_edit_cannot_change_original_anchor_before_first_apply_or_after_newer_reply() {
    for apply_first in [false, true] {
        let f = Fixture::new();
        let original = f.msg("2026年10月12日高数考试", 1791504000000, None);
        if apply_first {
            f.service().apply(f.batch(&original, &[])).unwrap();
            let n = f.msg(
                "高数考试改至2026年10月13日",
                original.sent_at + 1,
                Some(original.message_key),
            );
            f.service()
                .apply(f.batch(&n, std::slice::from_ref(&original)))
                .unwrap();
        }
        let mut edit = original.clone();
        edit.revision = 2;
        edit.sent_at += 2;
        edit.text = "2026年10月14日高数考试".into();
        let store = MessageStore::new(f.db.clone());
        assert_eq!(
            store.append(&f.config, edit.clone()).unwrap(),
            shixu_core::notifications::AppendOutcome::RevisionConflict
        );
        let persisted = f.persist(original.clone());
        assert_eq!(persisted.sent_at, original.sent_at);
        assert_eq!(persisted.revision, 1);
        if apply_first {
            assert_eq!(f.events()[0].local_date.as_deref(), Some("2026-10-13"));
        } else {
            f.service().apply(f.batch(&persisted, &[])).unwrap();
            assert_eq!(f.events()[0].local_date.as_deref(), Some("2026-10-12"));
        }
    }
}
#[test]
fn review1_f4_explicit_identifier_original_date_updates_unique_same_group_event() {
    let f = Fixture::new();
    let original = f.msg(
        "2026年10月12日9:00事件编号MATH101高数考试",
        1791504000000,
        None,
    );
    f.service().apply(f.batch(&original, &[])).unwrap();
    let n = f.msg(
        "原定2026年10月12日9:00事件编号MATH101高数考试改至2026年10月13日10:00",
        original.sent_at + 1,
        None,
    );
    let s = f.service().apply(f.batch(&n, &[])).unwrap();
    assert_eq!(s.updated, 1);
    assert_eq!(s.pending, 0);
    assert_eq!(f.events()[0].local_date.as_deref(), Some("2026-10-13"));
}
#[test]
fn review1_f5_actual_original_message_arrives_after_its_reply() {
    let f = Fixture::new();
    let seed = f.msg("synthetic template", 1791504000000, None);
    let mut original = seed;
    original.native_message_id = Uuid::new_v4().to_string();
    original.text = "2026年10月12日高数考试".into();
    original.message_key =
        shixu_core::notifications::identity::message_identity(&f.config, &original)
            .unwrap()
            .key;
    let n = f.msg(
        "高数考试改至2026年10月13日",
        original.sent_at + 1,
        Some(original.message_key),
    );
    assert_eq!(f.service().apply(f.batch(&n, &[])).unwrap().pending, 1);
    let original = f.persist(original);
    let s = f.service().apply(f.batch(&original, &[])).unwrap();
    assert_eq!(s.created, 1);
    assert_eq!(s.updated, 1);
    assert_eq!(f.events()[0].local_date.as_deref(), Some("2026-10-13"));
    assert_eq!(
        f.service()
            .notices()
            .unwrap()
            .iter()
            .filter(|s| s.outcome == shixu_core::calendar::changes::SourceOutcome::Pending)
            .count(),
        0
    );
}
#[test]
fn review1_f6_reply_uniquely_matches_one_subject_in_multi_event_original() {
    let f = Fixture::new();
    let original = f.msg(
        "2026年10月12日高数考试；2026年10月14日英语考试",
        1791504000000,
        None,
    );
    assert_eq!(
        f.service().apply(f.batch(&original, &[])).unwrap().created,
        2
    );
    let n = f.msg(
        "高数考试改至2026年10月13日",
        original.sent_at + 1,
        Some(original.message_key),
    );
    let s = f.service().apply(f.batch(&n, &[original])).unwrap();
    assert_eq!(s.updated, 1);
    let e = f.events();
    assert_eq!(
        e.iter()
            .find(|e| e.title == "高数考试")
            .unwrap()
            .local_date
            .as_deref(),
        Some("2026-10-13")
    );
    assert_eq!(
        e.iter()
            .find(|e| e.title == "英语考试")
            .unwrap()
            .local_date
            .as_deref(),
        Some("2026-10-14")
    );
}

#[test]
fn review1_f4_fallback_needs_unique_identifier_original_date_and_namespace() {
    for (originals, notice, group) in [
        (
            vec![
                "2026年10月12日事件编号MATH101高数考试",
                "2026年10月12日事件编号MATH101高数考试",
            ],
            "原定2026年10月12日事件编号MATH101高数考试改至2026年10月13日",
            "g1",
        ),
        (
            vec!["2026年10月12日事件编号MATH101高数考试"],
            "原定2026年10月12日事件编号OTHER高数考试改至2026年10月13日",
            "g1",
        ),
        (
            vec!["2026年10月12日事件编号MATH101高数考试"],
            "事件编号MATH101高数考试改至2026年10月12日",
            "g1",
        ),
        (
            vec!["2026年10月12日事件编号MATH101高数考试"],
            "原定2026年10月11日事件编号MATH101高数考试改至2026年10月12日",
            "g1",
        ),
        (
            vec!["2026年10月12日事件编号MATH101高数考试"],
            "原定2026年10月12日事件编号MATH101高数考试改至2026年10月13日",
            "g2",
        ),
        (
            vec!["2026年10月12日事件编号MATH101高数考试"],
            "原定10月12日事件编号MATH101高数考试改至2026年10月13日",
            "g1",
        ),
    ] {
        let f = Fixture::new();
        for text in originals {
            let m = f.msg(text, 1791504000000, None);
            f.service().apply(f.batch(&m, &[])).unwrap();
        }
        let mut n = f.msg(notice, 1791504000001, None);
        if group != "g1" {
            n.group_id = group.into();
            n.native_message_id = Uuid::new_v4().to_string();
            n = f.persist(n);
        }
        let summary = f.service().apply(f.batch(&n, &[])).unwrap();
        assert_eq!(summary.updated, 0, "{notice}");
        assert_eq!(summary.pending, 1, "{notice}");
        assert!(
            f.events()
                .iter()
                .all(|e| e.local_date.as_deref() == Some("2026-10-12"))
        );
    }
}
#[test]
fn review1_f4_fallback_retains_order_overrides_and_cancellation_guards() {
    for mode in ["override", "equal", "cancel", "unclear"] {
        let f = Fixture::new();
        let original = f.msg("2026年10月12日事件编号MATH101高数考试", 1791504000000, None);
        f.service().apply(f.batch(&original, &[])).unwrap();
        let e = f.events().remove(0);
        if mode == "override" {
            let mut p = patch(&e);
            p.time = Some(
                shixu_core::notifications::time::parse_time(
                    "2026年10月20日",
                    original.sent_at,
                    "Asia/Shanghai",
                )
                .unwrap(),
            );
            f.service()
                .edit(&e.event_id.to_string(), e.revision, p)
                .unwrap();
        }
        let text = match mode {
            "cancel" => "取消原定2026年10月12日事件编号MATH101高数考试",
            "unclear" => "原定2026年10月12日事件编号MATH101高数考试改至近期",
            _ => "原定2026年10月12日事件编号MATH101高数考试改至2026年10月13日",
        };
        let n = f.msg(text, original.sent_at + i64::from(mode != "equal"), None);
        let s = f.service().apply(f.batch(&n, &[])).unwrap();
        match mode {
            "override" => {
                assert_eq!(s.conflicts, 1);
                assert_eq!(f.events()[0].local_date.as_deref(), Some("2026-10-20"));
            }
            "equal" => {
                assert_eq!(s.conflicts, 1);
                assert_eq!(f.events()[0].local_date.as_deref(), Some("2026-10-12"));
            }
            "cancel" => {
                assert_eq!(s.cancelled, 1);
                assert_eq!(f.events()[0].status, EventStatus::Cancelled);
            }
            _ => {
                assert_eq!(s.updated, 1);
                assert_eq!(f.events()[0].time_precision, Precision::UnknownDate);
            }
        }
    }
}

#[test]
fn review1_f1_retry_uses_newly_supplied_uncertain_candidate_after_parser_conflict() {
    let f = Fixture::new();
    let original = f.msg("2026年10月12日高数考试", 1791504000000, None);
    let mut notice = f.msg(
        "高数考试改至2026年10月13日",
        original.sent_at + 1,
        Some(original.message_key),
    );
    let part = image_part(&notice);
    let id = part.part_id;
    notice.parts.push(part);
    notice.revision = 2;
    let notice = f.persist(notice);
    f.service()
        .apply(f.batch(&notice, std::slice::from_ref(&original)))
        .unwrap();
    let block = parser_block(id, "2026年10月14日高数考试改至2026年10月14日");
    save_parser(&f, &notice, block.clone());
    let current = extract(
        &notice,
        &[block],
        std::slice::from_ref(&original),
        &f.config.timezone,
    )
    .unwrap();
    assert_eq!(current.candidates[0].time.precision, Precision::UnknownDate);
    f.service().apply(current).unwrap();
    f.service().apply(f.batch(&original, &[])).unwrap();
    assert_eq!(f.events()[0].time_precision, Precision::UnknownDate);
    assert_eq!(f.events()[0].local_date, None);
    let source = f
        .service()
        .notices()
        .unwrap()
        .into_iter()
        .find(|s| s.message_key == notice.message_key)
        .unwrap();
    assert_eq!(source.candidate.time.precision, Precision::UnknownDate);
}
#[test]
fn review1_f2_cleanup_failure_rolls_back_all_content_copies_and_preserves_linked_history() {
    let f = Fixture::new();
    let mut m = f.msg("谢谢老师", 1791504000000, None);
    let part = image_part(&m);
    let id = part.part_id;
    m.parts.push(part);
    m.revision = 2;
    let m = f.persist(m);
    let block = parser_block(id, "谢谢老师");
    save_parser(&f, &m, block.clone());
    f.service()
        .apply(extract(&m, &[block], &[], &f.config.timezone).unwrap())
        .unwrap();
    let c = rusqlite::Connection::open(&f.path).unwrap();
    c.execute_batch("CREATE TRIGGER fail_cleanup AFTER UPDATE OF payload ON messages BEGIN SELECT RAISE(ABORT,'synthetic fault'); END;").unwrap();
    assert!(
        MessageStore::new(f.db.clone())
            .cleanup(
                m.received_at + shixu_core::notifications::retention::NON_EVENT_RETENTION_MILLIS
            )
            .is_err()
    );
    for table in ["calendar_batches", "part_results"] {
        assert_eq!(
            c.query_row::<i64, _, _>(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
                .unwrap(),
            1
        );
    }
    assert!(
        c.query_row::<bool, _, _>("SELECT payload IS NOT NULL FROM messages", [], |r| r.get(0))
            .unwrap()
    );
    c.execute_batch("DROP TRIGGER fail_cleanup;").unwrap();
    let linked = Fixture::new();
    let (original, _) = linked.create();
    let conn = rusqlite::Connection::open(&linked.path).unwrap();
    conn.execute(
        "UPDATE messages SET processing_state='non_event' WHERE message_key=?1",
        [original.message_key.to_string()],
    )
    .unwrap();
    assert_eq!(
        MessageStore::new(linked.db.clone())
            .cleanup(
                original.received_at
                    + shixu_core::notifications::retention::NON_EVENT_RETENTION_MILLIS
            )
            .unwrap(),
        0
    );
    assert_eq!(linked.events().len(), 1);
    assert_eq!(
        linked
            .service()
            .history(&linked.events()[0].event_id.to_string())
            .unwrap()
            .len(),
        1
    );
}
#[test]
fn review1_f5_later_original_proves_year_without_reusing_targetless_time() {
    let f = Fixture::new();
    let mut original = f.msg("synthetic template", 1791504000000, None);
    original.native_message_id = Uuid::new_v4().to_string();
    original.text = "2026年10月12日高数考试".into();
    original.message_key =
        shixu_core::notifications::identity::message_identity(&f.config, &original)
            .unwrap()
            .key;
    let notice = f.msg(
        "高数考试改至10月13日9:00",
        original.sent_at + 1,
        Some(original.message_key),
    );
    let old = f.batch(&notice, &[]);
    assert_eq!(old.candidates[0].time.precision, Precision::UnknownDate);
    assert_eq!(old.candidates[0].target_message_key, None);
    f.service().apply(old).unwrap();
    let original = f.persist(original);
    assert_eq!(
        f.service().apply(f.batch(&original, &[])).unwrap().updated,
        1
    );
    assert_eq!(f.events()[0].time_precision, Precision::Exact);
    assert_eq!(f.events()[0].local_date.as_deref(), Some("2026-10-13"));
    let source = f
        .service()
        .notices()
        .unwrap()
        .into_iter()
        .find(|s| s.message_key == notice.message_key)
        .unwrap();
    assert_eq!(
        source.candidate.target_message_key,
        Some(original.message_key)
    );
    assert_eq!(source.candidate.evidence.len(), 2);
}
#[test]
fn review1_f6_duplicate_subject_reply_remains_pending() {
    let f = Fixture::new();
    let original = f.msg(
        "2026年10月12日高数考试；2026年10月14日高数考试",
        1791504000000,
        None,
    );
    let n = f.msg(
        "高数考试改至2026年10月13日",
        original.sent_at + 1,
        Some(original.message_key),
    );
    let s = f.service().apply(f.batch(&n, &[original])).unwrap();
    assert_eq!(s.pending, 1);
    assert_eq!(s.updated, 0);
    assert!(f.events().is_empty());
}

#[test]
fn review1_f2_cleanup_also_expires_batches_left_by_previous_schema5_cleanup() {
    let f = Fixture::new();
    let m = f.msg("谢谢老师", 1791504000000, None);
    f.service().apply(f.batch(&m, &[])).unwrap();
    let c = rusqlite::Connection::open(&f.path).unwrap();
    c.execute(
        "UPDATE messages SET payload=NULL WHERE message_key=?1",
        [m.message_key.to_string()],
    )
    .unwrap();
    assert_eq!(
        MessageStore::new(f.db.clone())
            .cleanup(
                m.received_at + shixu_core::notifications::retention::NON_EVENT_RETENTION_MILLIS
            )
            .unwrap(),
        0
    );
    assert_eq!(
        c.query_row::<i64, _, _>("SELECT count(*) FROM calendar_batches", [], |r| r.get(0))
            .unwrap(),
        0
    );
}

fn review2_apply_identifier_pair(
    original_id: &str,
    notice_id: &str,
) -> (ApplySummary, CalendarEvent) {
    let f = Fixture::new();
    let original = f.msg(
        &format!("2026年10月12日9:00事件编号{original_id}高数考试"),
        1791504000000,
        None,
    );
    f.service().apply(f.batch(&original, &[])).unwrap();
    let notice = f.msg(
        &format!("原定2026年10月12日9:00事件编号{notice_id}高数考试改至2026年10月13日10:00"),
        original.sent_at + 1,
        None,
    );
    let result = f.service().apply(f.batch(&notice, &[])).unwrap();
    (result, f.events().remove(0))
}
#[test]
fn review2_r1_unsupported_identifier_continuations_in_notice_never_match_prefix() {
    let mut incorrectly_matched = vec![];
    for id in [
        "MATH101é",
        "MATH101/OTHER",
        "MATH101β",
        "MATH101\u{301}",
        "MATH101.OTHER",
        "MATH101\\OTHER",
        "MATH101~OTHER",
    ] {
        let (summary, event) = review2_apply_identifier_pair("MATH101", id);
        if summary.updated != 0
            || summary.pending != 1
            || event.local_date.as_deref() != Some("2026-10-12")
        {
            incorrectly_matched.push(id);
        }
    }
    assert!(
        incorrectly_matched.is_empty(),
        "unsupported references matched a prefix: {incorrectly_matched:?}"
    );
}
#[test]
fn review2_r1_unsupported_identifier_continuations_in_original_never_match_prefix() {
    let mut incorrectly_matched = vec![];
    for id in [
        "MATH101é",
        "MATH101/OTHER",
        "MATH101β",
        "MATH101\u{301}",
        "MATH101.OTHER",
        "MATH101\\OTHER",
        "MATH101~OTHER",
    ] {
        let (summary, event) = review2_apply_identifier_pair(id, "MATH101");
        if summary.updated != 0
            || summary.pending != 1
            || event.local_date.as_deref() != Some("2026-10-12")
        {
            incorrectly_matched.push(id);
        }
    }
    assert!(
        incorrectly_matched.is_empty(),
        "unsupported originals matched a prefix: {incorrectly_matched:?}"
    );
}
#[test]
fn review2_r1_complete_ascii_identifiers_boundaries_and_exact_case_are_preserved() {
    for id in [
        "A".to_owned(),
        "A_9-z".to_owned(),
        "MATH101".to_owned(),
        "A".repeat(64),
    ] {
        let (summary, event) = review2_apply_identifier_pair(&id, &id);
        assert_eq!(summary.updated, 1, "{id}");
        assert_eq!(event.local_date.as_deref(), Some("2026-10-13"));
    }
    for (old, new) in [
        ("MATH101", "MATH101X"),
        ("MATH101", "math101"),
        ("MATH101X", "MATH101"),
    ] {
        let (summary, event) = review2_apply_identifier_pair(old, new);
        assert_eq!(summary.updated, 0);
        assert_eq!(summary.pending, 1);
        assert_eq!(event.local_date.as_deref(), Some("2026-10-12"));
    }
    let maximum = "A".repeat(64);
    let oversized = "A".repeat(65);
    for (old, new) in [
        (&maximum, &oversized),
        (&oversized, &maximum),
        (&oversized, &oversized),
    ] {
        let (summary, event) = review2_apply_identifier_pair(old, new);
        assert_eq!(summary.updated, 0);
        assert_eq!(summary.pending, 1);
        assert_eq!(event.local_date.as_deref(), Some("2026-10-12"));
    }
    for suffix in [
        " 高数考试",
        "\t高数考试",
        ",高数考试",
        "，高数考试",
        ":高数考试",
        "：高数考试",
    ] {
        let f = Fixture::new();
        let original = f.msg(
            &format!("2026年10月12日9:00事件编号：MATH101{suffix}"),
            1791504000000,
            None,
        );
        f.service().apply(f.batch(&original, &[])).unwrap();
        let n = f.msg(
            &format!("原定2026年10月12日9:00事件编号 MATH101{suffix}改至2026年10月13日10:00"),
            original.sent_at + 1,
            None,
        );
        let s = f.service().apply(f.batch(&n, &[])).unwrap();
        assert_eq!(s.updated, 1, "separator {suffix:?}");
        assert_eq!(f.events()[0].local_date.as_deref(), Some("2026-10-13"));
    }
}
#[test]
fn review2_r2_supplied_original_end_must_agree_but_missing_old_clocks_are_optional() {
    let mut violations = vec![];
    for (original_clock, reference_clock, should_update) in [
        ("9:00—11:00", "9:00—12:00", false),
        ("9:00", "9:00—11:00", false),
        ("9:00—11:00", "9:00—11:00", true),
        ("9:00—11:00", "9:00", true),
        ("9:00—11:00", "", true),
    ] {
        let f = Fixture::new();
        let original = f.msg(
            &format!("2026年10月12日{original_clock}事件编号MATH101高数考试"),
            1791504000000,
            None,
        );
        f.service().apply(f.batch(&original, &[])).unwrap();
        let n = f.msg(
            &format!(
                "原定2026年10月12日{reference_clock}事件编号MATH101高数考试改至2026年10月13日10:00"
            ),
            original.sent_at + 1,
            None,
        );
        let result = f.service().apply(f.batch(&n, &[])).unwrap();
        let event = f.events().remove(0);
        if result.updated != u64::from(should_update)
            || result.pending != u64::from(!should_update)
            || event.local_date.as_deref()
                != Some(if should_update {
                    "2026-10-13"
                } else {
                    "2026-10-12"
                })
        {
            violations.push((original_clock, reference_clock, should_update, result));
        }
    }
    assert!(
        violations.is_empty(),
        "incorrect old endpoint proof: {violations:?}"
    );
}
#[test]
fn review2_r2_multiple_evidence_cannot_hide_supplied_or_contradictory_old_endpoints() {
    let mut violations = vec![];
    for (body_clock, image_clock, should_update) in [
        ("9:00—11:00", "9:00—12:00", false),
        ("9:00—12:00", "9:00—11:00", false),
        ("9:00", "9:00—12:00", false),
        ("9:00—12:00", "9:00", false),
        ("", "9:00—11:00", true),
        ("9:00", "9:00—11:00", true),
        ("9:00—11:00", "9:00", true),
    ] {
        let f = Fixture::new();
        let original = f.msg(
            "2026年10月12日9:00—11:00事件编号MATH101高数考试",
            1791504000000,
            None,
        );
        f.service().apply(f.batch(&original, &[])).unwrap();
        let text = |clock: &str| {
            format!("原定2026年10月12日{clock}事件编号MATH101高数考试改至2026年10月13日10:00")
        };
        let mut notice = f.msg(&text(body_clock), original.sent_at + 1, None);
        let part = image_part(&notice);
        let part_id = part.part_id;
        notice.parts.push(part);
        notice.revision = 2;
        let notice = f.persist(notice);
        let block = parser_block(part_id, &text(image_clock));
        save_parser(&f, &notice, block.clone());
        let batch = extract(&notice, &[block], &[], &f.config.timezone).unwrap();
        assert_eq!(batch.candidates.len(), 1);
        assert_eq!(batch.candidates[0].evidence.len(), 2);
        let result = f.service().apply(batch).unwrap();
        let event = f.events().remove(0);
        if result.updated != u64::from(should_update)
            || result.pending != u64::from(!should_update)
            || event.local_date.as_deref()
                != Some(if should_update {
                    "2026-10-13"
                } else {
                    "2026-10-12"
                })
        {
            violations.push((body_clock, image_clock, should_update, result));
        }
    }
    assert!(
        violations.is_empty(),
        "contradictory/omitted cross-evidence endpoints: {violations:?}"
    );
}

// N7 uses synthetic captured responses through the actual guarded calendar API.
fn n7_model(f: &Fixture, m: &MessageEnvelope) -> ExtractBatch {
    use shixu_core::notifications::{
        consent::{ConsentStore, ModelConsent},
        model::{ModelRequest, ModelService, ModelTransport},
    };
    struct Model(MessageEnvelope);
    impl ModelTransport for Model {
        fn send(&self, r: &ModelRequest) -> AppResult<String> {
            let e = EvidenceBlock {
                part_id: shixu_core::notifications::extract::body_part_id(&self.0),
                page_or_sheet: None,
                cell_range_or_bbox: Some(EvidenceLocation::TextSpan {
                    start: 0,
                    end: self.0.text.len() as u32,
                }),
                text: self.0.text.clone(),
                method: Method::NativeText,
                engine_version: "n6.body.utf8-bytes.1".into(),
                quality_flags: vec![],
            };
            let c = Candidate {
                candidate_key: CandidateKey::from_uuid(Uuid::new_v4()),
                action: CandidateAction::Create,
                title: "高数测验".into(),
                kind: "exam".into(),
                time: shixu_core::notifications::time::parse_time(&e.text, r.sent_at, &r.timezone)?,
                location: None,
                evidence: vec![e],
                target_message_key: None,
            };
            Ok(serde_json::to_string(&vec![c]).unwrap())
        }
    }
    let consent = ConsentStore::new(ModelConsent {
        enabled: true,
        provider_id: Some("synthetic".into()),
        allowed_group_ids: vec!["g1".into()],
        allow_attachment_text: false,
        revision: 1,
    });
    let transport = Model(m.clone());
    let svc = ModelService {
        consent: &consent,
        transport: &transport,
    };
    svc.dispatch(svc.prepare(m, &[], &[], &f.config).unwrap())
        .unwrap()
        .batch
}
#[test]
fn n7_grounded_fake_model_reaches_actual_calendar_once() {
    let f = Fixture::new();
    let m = f.msg("2026年10月12日9:00高数测验", 1791504000000, None);
    let batch = n7_model(&f, &m);
    assert_eq!(batch.candidates.len(), 1);
    assert_eq!(
        f.service().apply(batch.clone()),
        Err(AppError::InvalidInput)
    );
    assert_eq!(f.service().apply_model(batch.clone()).unwrap().created, 1);
    assert_eq!(f.service().apply_model(batch).unwrap().created, 0);
    assert_eq!(f.events().len(), 1);
    assert_eq!(f.events()[0].local_date.as_deref(), Some("2026-10-12"));
}
#[test]
fn n7_calendar_revalidates_forgery_and_durable_revision() {
    let f = Fixture::new();
    let m = f.msg("2026年10月12日9:00高数测验", 1791504000000, None);
    let batch = n7_model(&f, &m);
    assert_eq!(batch.candidates.len(), 1);
    for mutation in 0..5 {
        let mut b = batch.clone();
        match mutation {
            0 => b.candidates[0].time.local_date = Some("2026-10-13".into()),
            1 => b.candidates[0].evidence[0].part_id = PartId::from_uuid(Uuid::new_v4()),
            2 => b.candidates[0].title = "伪造测验".into(),
            3 => b.source_order += 1,
            _ => b.candidates[0].evidence[0].engine_version = "n6.reply-year.fake".into(),
        };
        assert!(f.service().apply_model(b).is_err());
    }
    let mut edit = m.clone();
    edit.revision += 1;
    edit.text = "2026年10月14日9:00高数测验".into();
    f.persist(edit);
    assert_eq!(f.service().apply_model(batch), Err(AppError::Conflict));
    assert!(f.events().is_empty());
}
#[test]
fn n7_model_cannot_strip_negation_using_a_durable_subspan() {
    let f = Fixture::new();
    let m = f.msg("不举行2026年10月12日9:00高数测验", 1791504000000, None);
    let mut cropped = m.clone();
    cropped.text = "2026年10月12日9:00高数测验".into();
    let mut batch = n7_model(&f, &cropped);
    batch.part_results = f.batch(&m, &[]).part_results;
    assert_eq!(batch.candidates.len(), 1);
    let evidence = &mut batch.candidates[0].evidence[0];
    evidence.cell_range_or_bbox = Some(EvidenceLocation::TextSpan {
        start: "不举行".len() as u32,
        end: m.text.len() as u32,
    });
    assert_eq!(f.service().apply_model(batch), Err(AppError::InvalidInput));
    assert!(f.events().is_empty());
}
fn n7_from_block(
    f: &Fixture,
    m: &MessageEnvelope,
    blocks: &[EvidenceBlock],
    selected: EvidenceBlock,
    title: &str,
) -> ExtractBatch {
    use shixu_core::notifications::{
        consent::{ConsentStore, ModelConsent},
        model::{ModelRequest, ModelService, ModelTransport},
    };
    struct Model {
        selected: EvidenceBlock,
        title: String,
    }
    impl ModelTransport for Model {
        fn send(&self, r: &ModelRequest) -> AppResult<String> {
            let mut time = shixu_core::notifications::time::parse_time(
                &self.selected.text,
                r.sent_at,
                &r.timezone,
            )?;
            if !self.selected.quality_flags.is_empty() {
                time.precision = Precision::UnknownDate;
                time.local_date = None;
                time.start_at = None;
                time.end_at = None;
            }
            let c = Candidate {
                candidate_key: CandidateKey::from_uuid(Uuid::new_v4()),
                action: CandidateAction::Create,
                title: self.title.clone(),
                kind: if self.title.ends_with("测验") {
                    "exam"
                } else {
                    "activity"
                }
                .into(),
                time,
                location: None,
                evidence: vec![self.selected.clone()],
                target_message_key: None,
            };
            Ok(serde_json::to_string(&vec![c]).unwrap())
        }
    }
    let store = ConsentStore::new(ModelConsent {
        enabled: true,
        provider_id: Some("synthetic".into()),
        allowed_group_ids: vec!["g1".into()],
        allow_attachment_text: true,
        revision: 1,
    });
    let transport = Model {
        selected,
        title: title.into(),
    };
    let svc = ModelService {
        consent: &store,
        transport: &transport,
    };
    svc.dispatch(svc.prepare(m, blocks, &[], &f.config).unwrap())
        .unwrap()
        .batch
}
#[test]
fn n7_durable_attachment_and_partial_quality_are_required() {
    for partial in [false, true] {
        let f = Fixture::new();
        let mut m = f.msg("见附件", 1791504000000, None);
        let mut part = image_part(&m);
        if partial {
            part.parse_state = PartStatus::PartialParse;
        }
        let id = part.part_id;
        m.parts.push(part);
        m.revision = 2;
        let m = f.persist(m);
        let mut block = parser_block(id, "明天9:00高数测验");
        if partial {
            block.quality_flags.push(QualityFlag::PartialSource);
        }
        let batch = n7_from_block(
            &f,
            &m,
            std::slice::from_ref(&block),
            block.clone(),
            "高数测验",
        );
        assert_eq!(batch.candidates.len(), 1);
        assert!(f.service().apply_model(batch.clone()).is_err());
        MessageStore::new(f.db.clone())
            .record_parts(
                &m.message_key,
                m.revision,
                vec![PartResult {
                    part_id: id,
                    status: if partial {
                        PartStatus::PartialParse
                    } else {
                        PartStatus::Success
                    },
                    blocks: vec![block],
                    reason_code: None,
                }],
            )
            .unwrap();
        assert_eq!(f.service().apply_model(batch.clone()).unwrap().created, 1);
        assert_eq!(
            f.events()[0].time_precision,
            if partial {
                Precision::UnknownDate
            } else {
                Precision::Exact
            }
        );
        if partial {
            assert!(f.events()[0].local_date.is_none());
            let mut forged = batch;
            for e in &mut forged.candidates[0].evidence {
                e.quality_flags.clear();
            }
            assert!(f.service().apply_model(forged).is_err());
        }
    }
}
#[test]
fn n7_negated_same_subject_attachment_cannot_be_ignored() {
    let f = Fixture::new();
    let mut m = f.msg("2026年10月12日9:00高数测验", 1791504000000, None);
    let part = image_part(&m);
    let id = part.part_id;
    m.parts.push(part);
    m.revision = 2;
    let m = f.persist(m);
    let baseline = n7_model(&f, &m);
    let block = parser_block(id, "不举行2026年10月12日9:00高数测验");
    save_parser(&f, &m, block.clone());
    let mut batch = baseline;
    batch.part_results = extract(&m, &[block], &[], &f.config.timezone)
        .unwrap()
        .part_results;
    assert_eq!(f.service().apply_model(batch), Err(AppError::InvalidInput));
    assert!(f.events().is_empty());
}
#[test]
fn n7_model_checks_durable_timezone_namespace_original_anchor_and_utf8() {
    for mutation in 0..5 {
        let f = Fixture::new();
        let m = f.msg("2026年10月12日9:00高数测验", 1791504000000, None);
        let mut batch = n7_model(&f, &m);
        let c = rusqlite::Connection::open(&f.path).unwrap();
        match mutation {
            0 => {
                c.execute("UPDATE source_bindings SET timezone='UTC'", [])
                    .unwrap();
            }
            1 => {
                c.execute("UPDATE sources SET group_id='g2'", []).unwrap();
            }
            2 => {
                batch.source_order = m.received_at as u64 + 1;
            }
            3 => {
                batch.candidates[0].evidence[0].cell_range_or_bbox =
                    Some(EvidenceLocation::TextSpan {
                        start: 1,
                        end: m.text.len() as u32,
                    });
            }
            _ => {
                batch.candidates[0].target_message_key =
                    Some(MessageKey::from_uuid(Uuid::new_v4()));
            }
        }
        assert!(f.service().apply_model(batch).is_err());
        assert!(f.events().is_empty());
    }
}
#[test]
fn n7_retry_timeout_keeps_actual_rule_event_unique_and_undo_suppresses_model() {
    use shixu_core::notifications::{
        consent::{ConsentStore, ModelConsent},
        model::{ModelRequest, ModelService, ModelTransport},
    };
    struct Timeout;
    impl ModelTransport for Timeout {
        fn send(&self, _: &ModelRequest) -> AppResult<String> {
            Err(AppError::Disconnected)
        }
    }
    let f = Fixture::new();
    let m = f.msg("明天9:00高数考试", 1791504000000, None);
    let store = ConsentStore::new(ModelConsent {
        enabled: true,
        provider_id: Some("synthetic".into()),
        allowed_group_ids: vec!["g1".into()],
        allow_attachment_text: false,
        revision: 1,
    });
    let svc = ModelService {
        consent: &store,
        transport: &Timeout,
    };
    let first = svc
        .dispatch(svc.prepare(&m, &[], &[], &f.config).unwrap())
        .unwrap();
    assert!(first.retryable);
    assert_eq!(f.service().apply_model(first.batch).unwrap().created, 1);
    let retry = svc
        .dispatch(svc.prepare(&m, &[], &[], &f.config).unwrap())
        .unwrap();
    assert_eq!(first.request_id, retry.request_id);
    assert_eq!(f.service().apply_model(retry.batch).unwrap().created, 0);
    let n = f.msg("明天9:00高数测验", m.sent_at + 1, None);
    let batch = n7_model(&f, &n);
    let applied = f.service().apply_model(batch.clone()).unwrap();
    let e = f
        .events()
        .into_iter()
        .find(|e| e.title == "高数测验")
        .unwrap();
    f.service()
        .undo(UndoRequest {
            change_id: applied.change_ids[0],
            expected_revision: e.revision,
        })
        .unwrap();
    assert_eq!(f.service().apply_model(batch).unwrap().created, 0);
    assert_eq!(f.events().len(), 2);
    assert!(
        f.events()
            .iter()
            .any(|e| e.title == "高数测验" && e.status == EventStatus::Removed)
    );
}
#[test]
fn n7_missing_year_and_competing_dates_remain_pending() {
    for text in [
        "10月12日9:00高数测验",
        "2026年10月12日2026年10月13日高数测验",
    ] {
        let f = Fixture::new();
        let m = f.msg(text, 1791504000000, None);
        let batch = n7_model(&f, &m);
        assert_eq!(batch.candidates.len(), 1);
        assert_eq!(batch.candidates[0].time.precision, Precision::UnknownDate);
        assert_eq!(f.service().apply_model(batch).unwrap().created, 1);
        assert!(f.events()[0].local_date.is_none());
        assert_eq!(
            MessageStore::new(f.db.clone()).pending(10).unwrap()[0].processing_state,
            ProcessingState::Pending
        );
    }
}
#[test]
fn n7_bound_historical_role_cannot_authenticate_a_create() {
    let f = Fixture::new();
    let original = f.msg("2025年10月12日9:00高数测验", 1791504000000, None);
    let m = f.msg(
        "2026年10月12日9:00高数测验",
        original.sent_at + 1,
        Some(original.message_key),
    );
    let batch = n7_model(&f, &m);
    assert_eq!(batch.candidates.len(), 1);
    let mut forged = batch.clone();
    let evidence = &mut forged.candidates[0].evidence[0];
    evidence.engine_version = format!(
        "n6.body.utf8-bytes.1;n6_context_year_target={}",
        original.message_key
    );
    forged.candidates[0].target_message_key = Some(original.message_key);
    assert!(f.service().apply_model(forged).is_err());
    let mut forged = batch;
    let e = &mut forged.candidates[0].evidence[0];
    e.part_id = shixu_core::notifications::extract::body_part_id(&original);
    e.text = original.text.clone();
    e.engine_version = format!(
        "n6.body.utf8-bytes.1;n6_context_year_target={}",
        original.message_key
    );
    e.cell_range_or_bbox = Some(EvidenceLocation::TextSpan {
        start: 0,
        end: original.text.len() as u32,
    });
    forged.candidates[0].target_message_key = Some(original.message_key);
    assert!(f.service().apply_model(forged).is_err());
    assert!(f.events().is_empty());
}
#[test]
fn n7_literal_defense_create_reaches_calendar_with_original_relative_anchor() {
    let f = Fixture::new();
    let m = f.msg("明天9:00研究生答辩", 1791504000000, None);
    let body = EvidenceBlock {
        part_id: shixu_core::notifications::extract::body_part_id(&m),
        page_or_sheet: None,
        cell_range_or_bbox: Some(EvidenceLocation::TextSpan {
            start: 0,
            end: m.text.len() as u32,
        }),
        text: m.text.clone(),
        method: Method::NativeText,
        engine_version: "n6.body.utf8-bytes.1".into(),
        quality_flags: vec![],
    };
    let batch = n7_from_block(&f, &m, &[], body, "研究生答辩");
    assert_eq!(batch.candidates.len(), 1);
    let expected =
        shixu_core::notifications::time::parse_time(&m.text, m.sent_at, &f.config.timezone)
            .unwrap();
    assert_eq!(f.service().apply_model(batch).unwrap().created, 1);
    assert_eq!(f.events()[0].kind, "activity");
    assert_eq!(f.events()[0].local_date, expected.local_date);
    assert_eq!(f.events()[0].start_at, expected.start_at);
}
#[test]
fn n7_protected_history_records_model_provenance_without_changing_character_origin() {
    let f = Fixture::new();
    let m = f.msg("2026年10月12日9:00高数测验", 1791504000000, None);
    let batch = n7_model(&f, &m);
    let result = f.service().apply_model(batch.clone()).unwrap();
    let e = f.events()[0].clone();
    let source = f.service().sources(&e.event_id.to_string()).unwrap();
    let source_json = serde_json::to_value(&source[0]).unwrap();
    assert_eq!(
        source_json.get("extractor_version"),
        Some(&serde_json::json!(
            "n7.guarded-create.1;timezone=Asia/Shanghai"
        ))
    );
    assert_eq!(
        batch.extractor_version,
        "n6.rules.1+n7.guarded-create.1;timezone=Asia/Shanghai"
    );
    assert_eq!(
        source[0].candidate.evidence[0].engine_version,
        "n6.body.utf8-bytes.1"
    );
    assert_eq!(source[0].candidate.evidence[0].method, Method::NativeText);
    assert_eq!(
        serde_json::to_value(
            f.service().history(&e.event_id.to_string()).unwrap()[0]
                .source
                .as_ref()
                .unwrap()
        )
        .unwrap()["extractor_version"],
        source_json["extractor_version"]
    );
    assert_eq!(result.created, 1);
    let mut metadata_only = batch;
    metadata_only.candidates[0].time.local_date = Some("2026-10-13".into());
    assert!(f.service().apply_model(metadata_only).is_err());
}
#[test]
fn n7_mixed_sources_keep_rule_priority_and_legacy_source_payloads_readable() {
    let f = Fixture::new();
    let mut m = f.msg("2026年10月12日9:00高数考试", 1791504000000, None);
    let part = image_part(&m);
    let id = part.part_id;
    m.parts.push(part);
    m.revision = 2;
    let m = f.persist(m);
    let block = parser_block(id, "2026年10月13日9:00高数测验");
    save_parser(&f, &m, block.clone());
    let rules = extract(&m, std::slice::from_ref(&block), &[], &f.config.timezone).unwrap();
    let batch = n7_from_block(
        &f,
        &m,
        std::slice::from_ref(&block),
        block.clone(),
        "高数测验",
    );
    assert_eq!(batch.candidates[0], rules.candidates[0]);
    assert_eq!(batch.candidates.len(), 2);
    assert_eq!(f.service().apply_model(batch).unwrap().created, 2);
    for e in f.events() {
        let source = f
            .service()
            .sources(&e.event_id.to_string())
            .unwrap()
            .remove(0);
        let mut payload = serde_json::to_value(&source).unwrap();
        assert_eq!(
            payload["extractor_version"],
            serde_json::json!(if e.title == "高数考试" {
                "n6.rules.1;timezone=Asia/Shanghai"
            } else {
                "n7.guarded-create.1;timezone=Asia/Shanghai"
            })
        );
        payload.as_object_mut().unwrap().remove("extractor_version");
        let legacy: shixu_core::calendar::changes::EventSource =
            serde_json::from_value(payload).unwrap();
        assert!(legacy.extractor_version.is_none());
    }
    assert_eq!(
        f.events()
            .iter()
            .find(|e| e.title == "高数考试")
            .unwrap()
            .local_date
            .as_deref(),
        Some("2026-10-12")
    );
}

fn n7_fix1_body(m: &MessageEnvelope) -> EvidenceBlock {
    EvidenceBlock {
        part_id: shixu_core::notifications::extract::body_part_id(m),
        page_or_sheet: None,
        cell_range_or_bbox: Some(EvidenceLocation::TextSpan {
            start: 0,
            end: m.text.len() as u32,
        }),
        text: m.text.clone(),
        method: Method::NativeText,
        engine_version: "n6.body.utf8-bytes.1".into(),
        quality_flags: vec![],
    }
}
fn n7_fix1_pair(contrary: &str, negative_body: bool, should_block: bool) {
    let f = Fixture::new();
    let positive = "2026年10月12日9:00高数测验";
    let mut m = f.msg(
        if negative_body { contrary } else { positive },
        1791504000000,
        None,
    );
    let part = image_part(&m);
    let id = part.part_id;
    m.parts.push(part);
    m.revision = 2;
    let m = f.persist(m);
    let block = parser_block(id, if negative_body { positive } else { contrary });
    save_parser(&f, &m, block.clone());
    let selected = if negative_body {
        block.clone()
    } else {
        n7_fix1_body(&m)
    };
    // Construct the candidate without contrary evidence, then supply honest
    // current part results. Calendar must independently inspect all durable text.
    let mut neutral = m.clone();
    if negative_body {
        neutral.text = "见附件".into();
    }
    let mut direct = if negative_body {
        n7_from_block(
            &f,
            &neutral,
            std::slice::from_ref(&block),
            selected.clone(),
            "高数测验",
        )
    } else {
        n7_model(&f, &m)
    };
    assert_eq!(direct.candidates.len(), 1);
    direct.part_results = extract(&m, std::slice::from_ref(&block), &[], &f.config.timezone)
        .unwrap()
        .part_results;
    let applied = f.service().apply_model(direct);
    if should_block {
        assert_eq!(
            applied,
            Err(AppError::InvalidInput),
            "durable: {contrary}, negative_body={negative_body}"
        );
        assert!(f.events().is_empty());
    } else {
        assert_eq!(applied.unwrap().created, 1);
    }
    let dispatched = n7_from_block(&f, &m, std::slice::from_ref(&block), selected, "高数测验");
    assert_eq!(
        dispatched.candidates.len(),
        usize::from(!should_block),
        "dispatch: {contrary}, negative_body={negative_body}"
    );
}
#[test]
fn n7_fix1_normalized_contrary_current_identity_blocks_both_evidence_orders() {
    for subject in [
        "高数测验",
        "高数 测验",
        "高数-测验",
        "高数·测验",
        "高数，测验",
    ] {
        for prefix in [
            "不举行",
            "未安排",
            "可能",
            "听说",
            "已完成",
            "已经完成",
            "已结束",
        ] {
            for contrary in [
                format!("{prefix}2026年10月12日9:00{subject}"),
                format!("2026年10月12日9:00{prefix}{subject}"),
            ] {
                for negative_body in [false, true] {
                    n7_fix1_pair(&contrary, negative_body, true);
                }
            }
        }
    }
}
#[test]
fn n7_fix1_unrelated_subjects_are_not_fuzzy_matched() {
    for subject in ["英语测验", "英语高数测验"] {
        for negative_body in [false, true] {
            n7_fix1_pair(
                &format!("不举行2026年10月12日9:00{subject}"),
                negative_body,
                false,
            );
        }
    }
}
#[test]
fn n7_fix1_unresolved_contrary_identity_fails_closed() {
    for negative_body in [false, true] {
        n7_fix1_pair("2026年10月12日9:00我听说高数 测验", negative_body, true);
    }
}
#[test]
fn n7_fix1_non_event_suffixes_cannot_reach_calendar() {
    for (title, suffix) in [
        ("高数测验", "答案已公布"),
        ("高数测验", "题库发布"),
        ("高数测验", "相关说明"),
        ("研究生答辩", "评分表已上传"),
    ] {
        let f = Fixture::new();
        let m = f.msg(
            &format!("2026年10月12日9:00{title}{suffix}"),
            1791504000000,
            None,
        );
        let mut clean = m.clone();
        clean.text = format!("2026年10月12日9:00{title}");
        let mut direct = n7_from_block(&f, &clean, &[], n7_fix1_body(&clean), title);
        assert_eq!(direct.candidates.len(), 1);
        direct.candidates[0].evidence = vec![n7_fix1_body(&m)];
        direct.candidates[0].time =
            shixu_core::notifications::time::parse_time(&m.text, m.sent_at, &f.config.timezone)
                .unwrap();
        direct.part_results = f.batch(&m, &[]).part_results;
        assert_eq!(
            f.service().apply_model(direct),
            Err(AppError::InvalidInput),
            "{title}{suffix}"
        );
        let dispatched = n7_from_block(&f, &m, &[], n7_fix1_body(&m), title);
        assert!(dispatched.candidates.is_empty());
        f.service().apply_model(dispatched).unwrap();
        assert!(f.events().is_empty());
    }
}
#[test]
fn n7_fix1_plain_literal_affirmative_control() {
    for (text, title) in [
        ("2026年10月12日9:00高数测验", "高数测验"),
        ("明天9:00研究生答辩", "研究生答辩"),
    ] {
        let f = Fixture::new();
        let m = f.msg(text, 1791504000000, None);
        let batch = n7_from_block(&f, &m, &[], n7_fix1_body(&m), title);
        assert_eq!(batch.candidates.len(), 1);
        assert_eq!(f.service().apply_model(batch).unwrap().created, 1);
        assert_eq!(f.events()[0].time_precision, Precision::Exact);
    }
}

fn n7_fix2_location_batches(
    f: &Fixture,
    m: &MessageEnvelope,
    location: &str,
) -> (ExtractBatch, ExtractBatch) {
    use shixu_core::notifications::{
        consent::{ConsentStore, ModelConsent},
        model::{ModelRequest, ModelService, ModelTransport},
    };
    // Obtain the canonical N7 candidate identity without the location tail,
    // then present the complete actual source and faithful model fields to
    // calendar directly. This path must not rely on dispatch rejecting first.
    let mut clean = m.clone();
    clean.text = m.text.split('，').next().unwrap().into();
    let mut direct = n7_model(f, &clean);
    assert_eq!(direct.candidates.len(), 1);
    let candidate = &mut direct.candidates[0];
    candidate.time =
        shixu_core::notifications::time::parse_time(&m.text, m.sent_at, &f.config.timezone)
            .unwrap();
    candidate.location = Some(location.into());
    candidate.evidence = vec![n7_fix1_body(m)];
    direct.part_results = f.batch(m, &[]).part_results;
    struct Fake(Candidate);
    impl ModelTransport for Fake {
        fn send(&self, _: &ModelRequest) -> AppResult<String> {
            Ok(serde_json::to_string(&vec![self.0.clone()]).unwrap())
        }
    }
    let fake = Fake(direct.candidates[0].clone());
    let consent = ConsentStore::new(ModelConsent {
        enabled: true,
        provider_id: Some("synthetic".into()),
        allowed_group_ids: vec!["g1".into()],
        allow_attachment_text: false,
        revision: 1,
    });
    let service = ModelService {
        consent: &consent,
        transport: &fake,
    };
    let dispatched = service
        .dispatch(service.prepare(m, &[], &[], &f.config).unwrap())
        .unwrap()
        .batch;
    (direct, dispatched)
}
#[test]
fn n7_fix2_semantic_location_qualifiers_block_dispatch_and_durable_calendar() {
    let mut unexpected = vec![];
    for qualifier in [
        "只是示例",
        "仅举例",
        "例如",
        "似乎",
        "大概",
        "估计",
        "传闻",
        "已完成",
        "已经完成",
        "已结束",
        "已经结束",
    ] {
        let f = Fixture::new();
        let location = format!("A301{qualifier}");
        let m = f.msg(
            &format!("2026年10月12日9:00高数测验，地点：{location}"),
            1791504000000,
            None,
        );
        let (direct, dispatched) = n7_fix2_location_batches(&f, &m, &location);
        let applied = f.service().apply_model(direct);
        if applied != Err(AppError::InvalidInput) || !f.events().is_empty() {
            unexpected.push(format!("durable {qualifier}: {applied:?}"));
        }
        if !dispatched.candidates.is_empty() {
            unexpected.push(format!(
                "dispatch {qualifier}: accepted {}",
                dispatched.candidates.len()
            ));
        }
        f.service().apply_model(dispatched).unwrap();
    }
    assert!(unexpected.is_empty(), "{}", unexpected.join("; "));
}
#[test]
fn n7_fix2_plain_locations_keep_exact_and_unknown_dates() {
    for (date, precision, expected_date) in [
        ("2026年10月12日9:00", Precision::Exact, Some("2026-10-12")),
        ("10月12日9:00", Precision::UnknownDate, None),
    ] {
        for location in ["A301", "协和楼A301"] {
            let f = Fixture::new();
            let m = f.msg(
                &format!("{date}高数测验，地点：{location}"),
                1791504000000,
                None,
            );
            let (direct, dispatched) = n7_fix2_location_batches(&f, &m, location);
            assert_eq!(direct.candidates.len(), 1);
            assert_eq!(dispatched.candidates, direct.candidates);
            assert_eq!(f.service().apply_model(direct).unwrap().created, 1);
            assert_eq!(f.service().apply_model(dispatched).unwrap().created, 0);
            let events = f.events();
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].location.as_deref(), Some(location));
            assert_eq!(events[0].time_precision, precision);
            assert_eq!(events[0].local_date.as_deref(), expected_date);
            if precision == Precision::UnknownDate {
                assert!(events[0].start_at.is_none());
                assert!(events[0].end_at.is_none());
            }
        }
    }
}
