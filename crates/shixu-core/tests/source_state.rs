use shixu_core::{
    contracts::{AppResult, error::AppError, notification::*},
    notifications::{AppendOutcome, MessageStore, reconnect::next_retry, source::*},
    storage::{DataProtector, Database},
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use uuid::Uuid;
struct SyntheticProtector(AtomicBool);
impl DataProtector for SyntheticProtector {
    fn protect(&self, p: &[u8]) -> AppResult<Vec<u8>> {
        if self.0.load(Ordering::SeqCst) {
            Err(AppError::StorageFull)
        } else {
            Ok(p.to_vec())
        }
    }
    fn unprotect(&self, p: &[u8]) -> AppResult<Vec<u8>> {
        Ok(p.to_vec())
    }
}
fn config() -> SourceConfig {
    SourceConfig {
        source_id: SourceId::from_uuid(Uuid::from_u128(10)),
        adapter_type: "synthetic".into(),
        account_id: "synthetic-A".into(),
        allowed_group_ids: vec!["g".into()],
        timezone: "Etc/UTC".into(),
        enabled: true,
        capability_set: vec![SourceCapability::LiveMessages],
    }
}
fn message(c: &SourceConfig) -> MessageEnvelope {
    MessageEnvelope {
        message_key: MessageKey::from_uuid(Uuid::from_u128(20)),
        source_id: c.source_id,
        account_id: c.account_id.clone(),
        group_id: "g".into(),
        native_message_id: "n".into(),
        sent_at: 1,
        received_at: 2,
        sender_id: "synthetic-sender".into(),
        text: "synthetic text".into(),
        reply_to: None,
        revision: 1,
        revoked: false,
        processing_state: ProcessingState::Persisted,
        parts: vec![],
    }
}
fn store() -> (MessageStore, Arc<SyntheticProtector>) {
    let p = Arc::new(SyntheticProtector(AtomicBool::new(false)));
    let db = Database::open(std::path::Path::new(":memory:"), p.clone()).unwrap();
    (MessageStore::new(Arc::new(db)), p)
}
#[test]
fn retry_boundaries() {
    for (i, b) in [1000, 2000, 4000, 8000, 16000, 32000, 60000]
        .into_iter()
        .enumerate()
    {
        assert_eq!(next_retry(i as u32, 0), b);
        assert_eq!(next_retry(i as u32, u32::MAX), (b + 1000).min(60000));
    }
    assert_eq!(next_retry(30, 1000), 60000);
    assert_eq!(next_retry(u32::MAX, 0), 60000);
}
#[test]
fn heartbeat_is_not_group_verification() {
    let mut h = SourceHealth::default();
    h.connected(10, vec![SourceCapability::LiveMessages]);
    assert_eq!(h.connection_state, ConnectionState::Connected);
    assert_eq!(h.last_connected_at, Some(10));
    assert_eq!(h.last_received_at, None);
    assert!(!h.ordinary_group_verified);
    h.received(20);
    h.persisted(21);
    h.applied(22);
    assert_eq!(
        (h.last_received_at, h.last_persisted_at, h.last_applied_at),
        (Some(20), Some(21), Some(22))
    );
    assert_eq!(h.last_connected_at, Some(10));
}
#[test]
fn auth_failure_waits_for_login() {
    let mut h = SourceHealth::default();
    h.connected(1, vec![]);
    h.failed(AppError::AuthFailed, 2);
    assert_eq!(h.connection_state, ConnectionState::WaitingForLogin);
    assert_eq!(h.retry_delay(0, 0), None);
    assert!(!h.gaps.is_empty());
    h.failed(AppError::Disconnected, 3);
    assert_eq!(h.connection_state, ConnectionState::WaitingForLogin);
    h.connected(4, vec![]);
    assert_eq!(h.connection_state, ConnectionState::Connected);
    assert!(!h.gaps.is_empty());
}
#[test]
fn backfill_keeps_gap_when_unsupported() {
    let mut h = SourceHealth::default();
    h.failed(AppError::Disconnected, 10);
    assert_eq!(h.retry_delay(0, 0), Some(1000));
    h.failed(AppError::Unsupported, 11);
    assert_eq!(h.connection_state, ConnectionState::Incompatible);
    assert_eq!(h.retry_delay(0, 0), None);
    assert!(!h.gaps.is_empty());
}
#[test]
fn overlap_replay_is_idempotent() {
    let (s, _) = store();
    let c = config();
    assert_eq!(
        s.append_with_cursor(&c, message(&c), "c1"),
        Ok(AppendOutcome::Stored)
    );
    assert_eq!(
        s.append_with_cursor(&c, message(&c), "c2"),
        Ok(AppendOutcome::Duplicate)
    );
    assert_eq!(s.pending(10).unwrap().len(), 1);
    assert_eq!(s.cursor(&c, "g").unwrap().as_deref(), Some("c2"));
}
#[test]
fn account_switch_keeps_cursor_namespace() {
    let (s, _) = store();
    let c = config();
    s.append_with_cursor(&c, message(&c), "a").unwrap();
    for changed in 0..3 {
        let mut b = c.clone();
        match changed {
            0 => b.source_id = SourceId::from_uuid(Uuid::from_u128(11)),
            1 => b.account_id = "synthetic-B".into(),
            _ => b.adapter_type = "synthetic-v2".into(),
        };
        assert_eq!(s.cursor(&b, "g").unwrap(), None);
        s.append_with_cursor(&b, message(&b), "b").unwrap();
        assert_eq!(s.cursor(&c, "g").unwrap().as_deref(), Some("a"));
    }
}
#[test]
fn failed_filtered_conflicting_append_never_advances_cursor() {
    let (s, p) = store();
    let c = config();
    s.append_with_cursor(&c, message(&c), "a").unwrap();
    let mut m = message(&c);
    m.text = "changed".into();
    assert_eq!(
        s.append_with_cursor(&c, m, "b"),
        Ok(AppendOutcome::RevisionConflict)
    );
    let mut m = message(&c);
    m.group_id = "forbidden".into();
    assert_eq!(
        s.append_with_cursor(&c, m, "b"),
        Ok(AppendOutcome::Filtered)
    );
    p.0.store(true, Ordering::SeqCst);
    let mut m = message(&c);
    m.native_message_id = "new".into();
    assert_eq!(s.append_with_cursor(&c, m, "b"), Err(AppError::StorageFull));
    p.0.store(false, Ordering::SeqCst);
    assert_eq!(s.cursor(&c, "g").unwrap().as_deref(), Some("a"));
    assert_eq!(s.pending(10).unwrap().len(), 1);
    assert_eq!(s.cursor(&c, "forbidden"), Err(AppError::InvalidInput));
}
#[test]
fn cursor_validation_and_transaction_rollback() {
    let (s, _) = store();
    let c = config();
    for cursor in ["", "bad\n", "https://synthetic.invalid/token"] {
        assert_eq!(
            s.append_with_cursor(&c, message(&c), cursor),
            Err(AppError::InvalidInput)
        );
    }
    assert!(s.pending(10).unwrap().is_empty());
}
#[test]
fn cursor_update_failure_rolls_back_message_insert() {
    let path = std::env::temp_dir().join(format!("shixu-n2-{}.db", Uuid::new_v4()));
    {
        let p = Arc::new(SyntheticProtector(AtomicBool::new(false)));
        let s = MessageStore::new(Arc::new(Database::open(&path, p).unwrap()));
        let sql = rusqlite::Connection::open(&path).unwrap();
        sql.execute_batch("CREATE TRIGGER synthetic_reject_cursor BEFORE UPDATE OF cursor ON sources BEGIN SELECT RAISE(ABORT,'synthetic'); END;").unwrap();
        let c = config();
        assert_eq!(
            s.append_with_cursor(&c, message(&c), "c1"),
            Err(AppError::Conflict)
        );
        assert!(s.pending(10).unwrap().is_empty());
        assert_eq!(s.cursor(&c, "g").unwrap(), None);
        sql.execute_batch("DROP TRIGGER synthetic_reject_cursor;")
            .unwrap();
        assert_eq!(
            s.append_with_cursor(&c, message(&c), "c1"),
            Ok(AppendOutcome::Stored)
        );
    }
    std::fs::remove_file(path).unwrap();
}
#[test]
fn cursor_size_exact_boundary() {
    let (s, _) = store();
    let c = config();
    assert_eq!(
        s.append_with_cursor(&c, message(&c), &"x".repeat(4096)),
        Ok(AppendOutcome::Stored)
    );
    assert_eq!(
        s.append_with_cursor(&c, message(&c), &"x".repeat(4097)),
        Err(AppError::InvalidInput)
    );
    assert_eq!(s.cursor(&c, "g").unwrap(), Some("x".repeat(4096)));
}
#[test]
fn durable_source_binding_rejects_reconfiguration_and_accepts_group_reordering() {
    let path = std::env::temp_dir().join(format!("shixu-n2-binding-{}.db", Uuid::new_v4()));
    let mut c = config();
    c.allowed_group_ids.push("second".into());
    {
        let s = MessageStore::new(Arc::new(
            Database::open(&path, Arc::new(SyntheticProtector(AtomicBool::new(false)))).unwrap(),
        ));
        assert_eq!(s.bind_source(&c), Ok(false));
    }
    let s = MessageStore::new(Arc::new(
        Database::open(&path, Arc::new(SyntheticProtector(AtomicBool::new(false)))).unwrap(),
    ));
    let mut reordered = c.clone();
    reordered.allowed_group_ids.reverse();
    assert_eq!(s.bind_source(&reordered), Ok(true));
    for i in 0..4 {
        let mut changed = c.clone();
        match i {
            0 => changed.account_id = "synthetic-B".into(),
            1 => changed.adapter_type = "other".into(),
            2 => changed.allowed_group_ids.push("third".into()),
            _ => changed.timezone = "Asia/Shanghai".into(),
        };
        assert_eq!(s.bind_source(&changed), Err(AppError::InvalidInput));
    }
    drop(s);
    std::fs::remove_file(path).unwrap();
}
#[test]
fn durable_recovery_anchor_progress_and_epoch_are_independent_of_live_cursor() {
    let (s, _) = store();
    let c = config();
    s.bind_source(&c).unwrap();
    s.append_with_cursor(&c, message(&c), "c1").unwrap();
    s.begin_recovery(&c, 10).unwrap();
    let first = s.recoveries(&c).unwrap().remove(0);
    let mut live = message(&c);
    live.native_message_id = "n3".into();
    s.append_with_cursor(&c, live, "c3").unwrap();
    assert_eq!(s.recoveries(&c).unwrap()[0], first);
    s.advance_recovery(&c, "g", first.epoch, "c1", "c2", false)
        .unwrap();
    s.begin_recovery(&c, 20).unwrap();
    let resumed = s.recoveries(&c).unwrap().remove(0);
    assert_eq!(resumed.anchor.as_deref(), Some("c1"));
    assert_eq!(resumed.recovery_cursor.as_deref(), Some("c1"));
    assert_eq!(resumed.since, 10);
    assert!(resumed.epoch > first.epoch);
    assert_eq!(
        s.advance_recovery(&c, "g", first.epoch, "c1", "c4", true),
        Err(AppError::Conflict)
    );
    assert_eq!(s.cursor(&c, "g").unwrap().as_deref(), Some("c3"));
    s.advance_recovery(&c, "g", resumed.epoch, "c1", "c4", true)
        .unwrap();
    s.begin_recovery(&c, 30).unwrap();
    let next = s.recoveries(&c).unwrap().remove(0);
    assert_eq!(next.anchor.as_deref(), Some("c3"));
    assert_eq!(next.since, 30);
    assert!(!next.complete);
}
#[test]
fn legacy_unbound_identity_requires_explicit_new_source() {
    let (s, _) = store();
    let c = config();
    s.append(&c, message(&c)).unwrap();
    assert_eq!(s.bind_source(&c), Err(AppError::Conflict));
    let mut next = c;
    next.source_id = SourceId::from_uuid(Uuid::from_u128(11));
    assert_eq!(s.bind_source(&next), Ok(false));
}
