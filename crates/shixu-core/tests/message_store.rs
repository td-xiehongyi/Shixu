use shixu_core::{
    contracts::{AppResult, error::AppError, notification::*},
    notifications::{
        AppendOutcome, MessageStore, identity::message_identity,
        retention::NON_EVENT_RETENTION_MILLIS,
    },
    storage::{DataProtector, Database},
};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use uuid::Uuid;

// Deliberately insecure test double. Exists only in this integration test binary;
// never exported by a product module or usable as application encryption.
struct TestProtector {
    identity: u8,
    calls: AtomicUsize,
}
impl TestProtector {
    fn new(identity: u8) -> Self {
        Self {
            identity,
            calls: AtomicUsize::new(0),
        }
    }
}
impl DataProtector for TestProtector {
    fn protect(&self, plain: &[u8]) -> AppResult<Vec<u8>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(std::iter::once(self.identity)
            .chain(plain.iter().rev().map(|v| v ^ 0xa5))
            .collect())
    }
    fn unprotect(&self, sealed: &[u8]) -> AppResult<Vec<u8>> {
        if sealed.first() != Some(&self.identity) {
            return Err(AppError::AuthFailed);
        }
        Ok(sealed[1..].iter().rev().map(|v| v ^ 0xa5).collect())
    }
}
struct TempDir(PathBuf);
impl TempDir {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!("shixu-n1-{}", Uuid::new_v4()));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn db(&self) -> PathBuf {
        self.0.join("messages.db")
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn config() -> SourceConfig {
    SourceConfig {
        source_id: SourceId::from_uuid(Uuid::from_u128(10)),
        adapter_type: "synthetic".into(),
        account_id: "test-account-A".into(),
        allowed_group_ids: vec!["group-1".into()],
        timezone: "Etc/UTC".into(),
        enabled: true,
        capability_set: vec![
            SourceCapability::LiveMessages,
            SourceCapability::Attachments,
        ],
    }
}
fn message(c: &SourceConfig) -> MessageEnvelope {
    MessageEnvelope {
        message_key: MessageKey::from_uuid(Uuid::new_v4()),
        source_id: c.source_id,
        account_id: c.account_id.clone(),
        group_id: "group-1".into(),
        native_message_id: "native-1".into(),
        sent_at: 0,
        received_at: 0,
        sender_id: "SYNTHETIC-SENDER-SENSITIVE".into(),
        text: "SYNTHETIC-BODY-SENSITIVE".into(),
        reply_to: None,
        revision: 1,
        revoked: false,
        processing_state: ProcessingState::Persisted,
        parts: vec![],
    }
}
fn store(t: &TempDir) -> MessageStore {
    MessageStore::new(Arc::new(
        Database::open(&t.db(), Arc::new(TestProtector::new(42))).unwrap(),
    ))
}
fn count(t: &TempDir, table: &str) -> i64 {
    rusqlite::Connection::open(t.db())
        .unwrap()
        .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

#[test]
fn account_switch_keeps_cursor_namespace() {
    let t = TempDir::new();
    let s = store(&t);
    let a = config();
    let mut b = a.clone();
    b.account_id = "test-account-B".into();
    let ma = message(&a);
    let mb = message(&b);
    assert_ne!(
        message_identity(&a, &ma).unwrap().key,
        message_identity(&b, &mb).unwrap().key
    );
    s.append(&a, ma).unwrap();
    s.append(&b, mb).unwrap();
    assert_eq!(s.pending(10).unwrap().len(), 2);
    assert_eq!(count(&t, "sources"), 2);
}
#[test]
fn persist_before_processing() {
    if let Ok(path) = std::env::var("SHIXU_N1_CRASH_DB") {
        let s = MessageStore::new(Arc::new(
            Database::open(
                std::path::Path::new(&path),
                Arc::new(TestProtector::new(42)),
            )
            .unwrap(),
        ));
        let c = config();
        s.append(&c, message(&c)).unwrap();
        std::fs::write(format!("{path}.ready"), b"committed").unwrap();
        loop {
            std::thread::park();
        }
    }
    let t = TempDir::new();
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "persist_before_processing", "--nocapture"])
        .env("SHIXU_N1_CRASH_DB", t.db())
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let ready = t.db().with_extension("db.ready");
    let deadline = Instant::now() + Duration::from_secs(15);
    while !ready.exists() && Instant::now() < deadline {
        if child.try_wait().unwrap().is_some() {
            panic!("writer exited before commit");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let committed = ready.exists();
    child.kill().unwrap();
    let status = child.wait().unwrap();
    assert!(committed, "writer did not commit before timeout");
    assert!(!status.success());
    let p = store(&t).pending(10).unwrap();
    assert_eq!(p.len(), 1);
    assert_eq!(p[0].processing_state, ProcessingState::Persisted);
}
#[test]
fn duplicate_and_edited_message() {
    let t = TempDir::new();
    let s = store(&t);
    let c = config();
    let m = message(&c);
    assert_eq!(s.append(&c, m.clone()).unwrap(), AppendOutcome::Stored);
    let mut replay = m.clone();
    replay.message_key = MessageKey::from_uuid(Uuid::new_v4());
    replay.received_at = 99;
    assert_eq!(s.append(&c, replay).unwrap(), AppendOutcome::Duplicate);
    let mut edit = m;
    edit.text = "synthetic changed text".into();
    edit.revision = 2;
    assert_eq!(
        s.append(&c, edit.clone()).unwrap(),
        AppendOutcome::RevisionConflict
    );
    let mut capable = c;
    capable
        .capability_set
        .push(serde_json::from_str("\"edits\"").unwrap());
    assert_eq!(
        s.append(&capable, edit.clone()).unwrap(),
        AppendOutcome::Stored
    );
    edit.revision = 1;
    edit.text = "stale synthetic change".into();
    assert_eq!(
        s.append(&capable, edit).unwrap(),
        AppendOutcome::RevisionConflict
    );
    assert_eq!(s.pending(10).unwrap()[0].revision, 2);
}
#[test]
fn forbidden_group_zero_storage() {
    let t = TempDir::new();
    let protector = Arc::new(TestProtector::new(42));
    let s = MessageStore::new(Arc::new(
        Database::open(&t.db(), protector.clone()).unwrap(),
    ));
    let c = config();
    let mut m = message(&c);
    m.group_id = "forbidden".into();
    let before = protector.calls.load(Ordering::SeqCst);
    assert_eq!(s.append(&c, m).unwrap(), AppendOutcome::Filtered);
    assert_eq!(protector.calls.load(Ordering::SeqCst), before);
    assert_eq!(count(&t, "messages"), 0);
    assert_eq!(count(&t, "sources"), 0);
    assert_eq!(count(&t, "part_results"), 0);
}
#[test]
fn cleanup_keeps_source_and_tombstones() {
    let t = TempDir::new();
    let s = store(&t);
    let c = config();
    let m = message(&c);
    let key = message_identity(&c, &m).unwrap().key;
    s.append(&c, m.clone()).unwrap();
    let conn = rusqlite::Connection::open(t.db()).unwrap();
    conn.execute("UPDATE messages SET processing_state='non_event'", [])
        .unwrap();
    conn.execute(
        "INSERT INTO suppressions(message_key, reason) VALUES (?1,'user_removed')",
        [key.to_string()],
    )
    .unwrap();
    assert_eq!(s.cleanup(NON_EVENT_RETENTION_MILLIS - 1).unwrap(), 0);
    assert_eq!(s.cleanup(NON_EVENT_RETENTION_MILLIS).unwrap(), 1);
    assert_eq!(s.cleanup(NON_EVENT_RETENTION_MILLIS + 1).unwrap(), 0);
    assert_eq!(count(&t, "messages"), 1);
    assert_eq!(count(&t, "sources"), 1);
    assert_eq!(count(&t, "suppressions"), 1);
    assert_eq!(s.append(&c, m).unwrap(), AppendOutcome::Duplicate);
    let missing: bool = conn
        .query_row("SELECT payload IS NULL FROM messages", [], |r| r.get(0))
        .unwrap();
    assert!(missing);
}
#[test]
fn payloads_are_protected_and_wrong_identity_fails_closed() {
    let t = TempDir::new();
    let c = config();
    store(&t).append(&c, message(&c)).unwrap();
    for entry in std::fs::read_dir(&t.0).unwrap() {
        let bytes = std::fs::read(entry.unwrap().path()).unwrap();
        for marker in [
            b"SYNTHETIC-BODY-SENSITIVE".as_slice(),
            b"SYNTHETIC-SENDER-SENSITIVE".as_slice(),
        ] {
            assert!(!bytes.windows(marker.len()).any(|v| v == marker));
        }
    }
    assert!(matches!(
        Database::open(&t.db(), Arc::new(TestProtector::new(7))),
        Err(AppError::AuthFailed)
    ));
}
#[test]
fn missing_native_id_is_explicitly_degraded() {
    let c = config();
    let mut m = message(&c);
    m.native_message_id.clear();
    assert!(message_identity(&c, &m).unwrap().degraded);
}

#[test]
fn parts_and_evidence_are_protected_and_invalid_parts_are_atomic() {
    let t = TempDir::new();
    let s = store(&t);
    let c = config();
    let mut m = message(&c);
    let pid = PartId::from_uuid(Uuid::new_v4());
    m.parts.push(MessagePart {
        part_id: pid,
        message_key: m.message_key,
        kind: PartKind::Image,
        source_file_ref: Some("synthetic-ref".to_string().try_into().unwrap()),
        original_name: Some("SYNTHETIC-FILENAME-SENSITIVE.png".into()),
        declared_type: None,
        detected_type: None,
        byte_size: None,
        content_hash: None,
        fetch_state: FetchState::Pending,
        parse_state: PartStatus::PendingDownload,
        failure_code: None,
        encrypted_blob_ref: None,
        retained_until: None,
    });
    let key = message_identity(&c, &m).unwrap().key;
    s.append(&c, m).unwrap();
    let result = PartResult {
        part_id: pid,
        status: PartStatus::Success,
        blocks: vec![EvidenceBlock {
            part_id: pid,
            page_or_sheet: None,
            cell_range_or_bbox: None,
            text: "SYNTHETIC-EVIDENCE-SENSITIVE".into(),
            method: Method::Ocr,
            engine_version: "test".into(),
            quality_flags: vec![],
        }],
        reason_code: None,
    };
    s.record_parts(&key, 1, vec![result.clone()]).unwrap();
    let mut invalid = result.clone();
    invalid.part_id = PartId::from_uuid(Uuid::new_v4());
    assert_eq!(
        s.record_parts(&key, 1, vec![result, invalid]),
        Err(AppError::InvalidInput)
    );
    assert_eq!(
        s.pending(1).unwrap()[0].parts[0].parse_state,
        PartStatus::Success
    );
    assert_eq!(count(&t, "part_results"), 1);
    for e in std::fs::read_dir(&t.0).unwrap() {
        let bytes = std::fs::read(e.unwrap().path()).unwrap();
        for marker in [
            b"SYNTHETIC-FILENAME-SENSITIVE".as_slice(),
            b"SYNTHETIC-EVIDENCE-SENSITIVE".as_slice(),
        ] {
            assert!(!bytes.windows(marker.len()).any(|v| v == marker));
        }
    }
    assert_eq!(
        s.record_parts(&MessageKey::from_uuid(Uuid::new_v4()), 1, vec![]),
        Err(AppError::Conflict)
    );
}
#[test]
fn retention_preserves_pending_and_linked_event_payloads_and_cursor() {
    let t = TempDir::new();
    let s = store(&t);
    let c = config();
    let m = message(&c);
    s.append(&c, m.clone()).unwrap();
    let mut event = m.clone();
    event.native_message_id = "event-native".into();
    s.append(&c, event).unwrap();
    let conn = rusqlite::Connection::open(t.db()).unwrap();
    conn.execute(
        "UPDATE messages SET processing_state='committed' WHERE native_message_id='event-native'",
        [],
    )
    .unwrap();
    conn.execute("UPDATE sources SET cursor='N2-synthetic-cursor'", [])
        .unwrap();
    let mut next = m;
    next.native_message_id = "next-native".into();
    s.append(&c, next).unwrap();
    assert_eq!(s.cleanup(NON_EVENT_RETENTION_MILLIS).unwrap(), 0);
    let cursor: String = conn
        .query_row("SELECT cursor FROM sources", [], |r| r.get(0))
        .unwrap();
    assert_eq!(cursor, "N2-synthetic-cursor");
    assert_eq!(s.pending(0).unwrap().len(), 0);
    assert_eq!(s.pending(1).unwrap().len(), 1);
}
#[test]
fn revocations_require_capability_and_cannot_be_resurrected_by_edit() {
    let t = TempDir::new();
    let s = store(&t);
    let mut c = config();
    let m = message(&c);
    s.append(&c, m.clone()).unwrap();
    let mut revoked = m.clone();
    revoked.revoked = true;
    revoked.revision = 2;
    assert_eq!(
        s.append(&c, revoked.clone()).unwrap(),
        AppendOutcome::RevisionConflict
    );
    c.capability_set.push(SourceCapability::Revocations);
    assert_eq!(s.append(&c, revoked).unwrap(), AppendOutcome::Stored);
    c.capability_set.push(SourceCapability::Edits);
    let mut replay = m;
    replay.revision = 3;
    assert_eq!(
        s.append(&c, replay).unwrap(),
        AppendOutcome::RevisionConflict
    );
    assert_eq!(count(&t, "suppressions"), 1);
    assert!(s.pending(10).unwrap().is_empty());
}
#[test]
fn protector_failure_rolls_back_all_rows() {
    struct FailProtector {
        calls: AtomicUsize,
    }
    impl DataProtector for FailProtector {
        fn protect(&self, p: &[u8]) -> AppResult<Vec<u8>> {
            if self.calls.fetch_add(1, Ordering::SeqCst) > 0 {
                Err(AppError::AuthFailed)
            } else {
                Ok(p.to_vec())
            }
        }
        fn unprotect(&self, p: &[u8]) -> AppResult<Vec<u8>> {
            Ok(p.to_vec())
        }
    }
    let t = TempDir::new();
    let s = MessageStore::new(Arc::new(
        Database::open(
            &t.db(),
            Arc::new(FailProtector {
                calls: AtomicUsize::new(0),
            }),
        )
        .unwrap(),
    ));
    let c = config();
    assert_eq!(s.append(&c, message(&c)), Err(AppError::AuthFailed));
    assert_eq!(count(&t, "sources"), 0);
    assert_eq!(count(&t, "messages"), 0);
}
#[test]
fn credentials_use_separate_protected_source_account_records() {
    use shixu_core::contracts::vault::SecretBytes;
    let t = TempDir::new();
    let db = Database::open(&t.db(), Arc::new(TestProtector::new(42))).unwrap();
    let c = config();
    db.store_source_secret(
        &c,
        &SecretBytes::new(b"synthetic-not-a-real-token".to_vec()),
    )
    .unwrap();
    assert_eq!(
        db.source_secret(&c).unwrap().unwrap().expose(),
        b"synthetic-not-a-real-token"
    );
    let mut other = c.clone();
    other.account_id = "account-B".into();
    assert!(db.source_secret(&other).unwrap().is_none());
    other.enabled = false;
    assert_eq!(
        db.store_source_secret(&other, &SecretBytes::new(vec![1])),
        Err(AppError::InvalidInput)
    );
    assert_eq!(count(&t, "source_secrets"), 1);
    assert_eq!(count(&t, "messages"), 0);
}
#[test]
fn namespace_delimiters_cannot_alias_and_parallel_duplicates_are_atomic() {
    let t = TempDir::new();
    let c = config();
    let m = message(&c);
    let mut a = c.clone();
    a.adapter_type = "a|b".into();
    a.account_id = "c".into();
    let mut b = c.clone();
    b.adapter_type = "a".into();
    b.account_id = "b|c".into();
    assert_ne!(
        message_identity(&a, &message(&a)).unwrap().key,
        message_identity(&b, &message(&b)).unwrap().key
    );
    let db = Arc::new(Database::open(&t.db(), Arc::new(TestProtector::new(42))).unwrap());
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let db = db.clone();
            let c = c.clone();
            let m = m.clone();
            std::thread::spawn(move || MessageStore::new(db).append(&c, m).unwrap())
        })
        .collect();
    let outcomes: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(
        outcomes
            .iter()
            .filter(|o| **o == AppendOutcome::Stored)
            .count(),
        1
    );
    assert_eq!(count(&t, "messages"), 1);
}

#[test]
fn cleanup_does_not_discard_unparseable_messages_that_may_contain_events() {
    let t = TempDir::new();
    let s = store(&t);
    let c = config();
    s.append(&c, message(&c)).unwrap();
    let conn = rusqlite::Connection::open(t.db()).unwrap();
    conn.execute("UPDATE messages SET processing_state='unparseable'", [])
        .unwrap();
    assert_eq!(s.cleanup(NON_EVENT_RETENTION_MILLIS).unwrap(), 0);
    let exists: bool = conn
        .query_row("SELECT payload IS NOT NULL FROM messages", [], |r| r.get(0))
        .unwrap();
    assert!(exists);
}
#[test]
fn replay_with_regenerated_part_ids_deduplicates_using_content() {
    let t = TempDir::new();
    let s = store(&t);
    let c = config();
    let mut m = message(&c);
    m.parts.push(MessagePart {
        part_id: PartId::from_uuid(Uuid::new_v4()),
        message_key: m.message_key,
        kind: PartKind::Image,
        source_file_ref: Some("ref-1".to_string().try_into().unwrap()),
        original_name: Some("test.png".into()),
        declared_type: None,
        detected_type: None,
        byte_size: None,
        content_hash: Some("untrusted-1".into()),
        fetch_state: FetchState::Pending,
        parse_state: PartStatus::PendingDownload,
        failure_code: None,
        encrypted_blob_ref: None,
        retained_until: None,
    });
    assert_eq!(s.append(&c, m.clone()).unwrap(), AppendOutcome::Stored);
    m.message_key = MessageKey::from_uuid(Uuid::new_v4());
    m.parts[0].part_id = PartId::from_uuid(Uuid::new_v4());
    m.parts[0].message_key = m.message_key;
    m.parts[0].content_hash = Some("untrusted-2".into());
    assert_eq!(s.append(&c, m).unwrap(), AppendOutcome::Duplicate);
}

#[test]
fn degraded_attachment_identity_survives_regenerated_local_keys() {
    let c = config();
    let mut a = message(&c);
    a.native_message_id.clear();
    a.parts.push(MessagePart {
        part_id: PartId::from_uuid(Uuid::new_v4()),
        message_key: a.message_key,
        kind: PartKind::Image,
        source_file_ref: Some("ref-1".to_string().try_into().unwrap()),
        original_name: None,
        declared_type: None,
        detected_type: None,
        byte_size: None,
        content_hash: None,
        fetch_state: FetchState::Pending,
        parse_state: PartStatus::PendingDownload,
        failure_code: None,
        encrypted_blob_ref: None,
        retained_until: None,
    });
    let mut b = a.clone();
    b.message_key = MessageKey::from_uuid(Uuid::new_v4());
    b.parts[0].message_key = b.message_key;
    b.parts[0].part_id = PartId::from_uuid(Uuid::new_v4());
    assert_eq!(
        message_identity(&c, &a).unwrap().key,
        message_identity(&c, &b).unwrap().key
    );
}

fn message_with_two_parts(c: &SourceConfig) -> MessageEnvelope {
    let mut m = message(c);
    for name in ["part-A", "part-B"] {
        m.parts.push(MessagePart {
            part_id: PartId::from_uuid(Uuid::new_v4()),
            message_key: m.message_key,
            kind: PartKind::Image,
            source_file_ref: Some(name.to_string().try_into().unwrap()),
            original_name: Some(format!("{name}.png")),
            declared_type: None,
            detected_type: None,
            byte_size: None,
            content_hash: None,
            fetch_state: FetchState::Pending,
            parse_state: PartStatus::PendingDownload,
            failure_code: None,
            encrypted_blob_ref: None,
            retained_until: None,
        });
    }
    m
}
fn successful_result(part_id: PartId, text: &str) -> PartResult {
    PartResult {
        part_id,
        status: PartStatus::Success,
        blocks: vec![EvidenceBlock {
            part_id,
            page_or_sheet: None,
            cell_range_or_bbox: None,
            text: text.into(),
            method: Method::Ocr,
            engine_version: "synthetic".into(),
            quality_flags: vec![],
        }],
        reason_code: None,
    }
}
fn saved_message_and_results(t: &TempDir) -> (MessageEnvelope, i64, Vec<PartResult>) {
    let conn = rusqlite::Connection::open(t.db()).unwrap();
    let payload: Vec<u8> = conn
        .query_row("SELECT payload FROM messages", [], |r| r.get(0))
        .unwrap();
    let message =
        serde_json::from_slice(&TestProtector::new(42).unprotect(&payload).unwrap()).unwrap();
    let (revision, payload): (i64, Vec<u8>) = conn
        .query_row("SELECT revision,payload FROM part_results", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    let results =
        serde_json::from_slice(&TestProtector::new(42).unprotect(&payload).unwrap()).unwrap();
    (message, revision, results)
}

#[test]
fn equal_content_advances_trusted_revision_and_rejects_stale_edit_after_reopen() {
    let t = TempDir::new();
    let mut c = config();
    c.capability_set.push(SourceCapability::Edits);
    let m = message_with_two_parts(&c);
    let key = message_identity(&c, &m).unwrap().key;
    let s = store(&t);
    s.append(&c, m.clone()).unwrap();
    let evidence = successful_result(m.parts[0].part_id, "synthetic original evidence");
    s.record_parts(&key, 1, vec![evidence.clone()]).unwrap();
    let mut newer = m.clone();
    newer.revision = 3;
    newer.received_at = 123;
    assert_eq!(s.append(&c, newer).unwrap(), AppendOutcome::Duplicate);
    drop(s);
    let s = store(&t);
    let mut stale = m;
    stale.revision = 2;
    stale.text = "synthetic stale content B".into();
    assert_eq!(
        s.append(&c, stale.clone()).unwrap(),
        AppendOutcome::RevisionConflict
    );
    let p = s.pending(10).unwrap();
    assert_eq!(p.len(), 1);
    assert_eq!(p[0].revision, 3);
    assert_ne!(p[0].text, stale.text);
    assert_eq!(p[0].received_at, 0);
    let (saved, revision, results) = saved_message_and_results(&t);
    assert_eq!(saved.revision, 3);
    assert_eq!(revision, 3);
    assert_eq!(results, vec![evidence]);
    assert_eq!(saved.parts[0].parse_state, PartStatus::Success);
    assert_eq!(count(&t, "messages"), 1);
}
#[test]
fn equal_content_revision_advance_preserves_completed_state_and_cleaned_tombstone() {
    for cleaned in [false, true] {
        let t = TempDir::new();
        let mut c = config();
        c.capability_set.push(SourceCapability::Edits);
        let s = store(&t);
        let m = message(&c);
        let key = message_identity(&c, &m).unwrap().key;
        s.append(&c, m.clone()).unwrap();
        let conn = rusqlite::Connection::open(t.db()).unwrap();
        let state = if cleaned { "non_event" } else { "committed" };
        conn.execute("UPDATE messages SET processing_state=?1", [state])
            .unwrap();
        conn.execute(
            "INSERT INTO suppressions VALUES (?1,'user_removed')",
            [key.to_string()],
        )
        .unwrap();
        if cleaned {
            assert_eq!(s.cleanup(NON_EVENT_RETENTION_MILLIS).unwrap(), 1);
        }
        let mut newer = m.clone();
        newer.revision = 3;
        assert_eq!(s.append(&c, newer).unwrap(), AppendOutcome::Duplicate);
        drop(s);
        let s = store(&t);
        let mut stale = m;
        stale.revision = 2;
        stale.text = "synthetic stale B".into();
        assert_eq!(
            s.append(&c, stale).unwrap(),
            AppendOutcome::RevisionConflict
        );
        let (revision, saved_state, missing): (i64, String, bool) = conn
            .query_row(
                "SELECT revision,processing_state,payload IS NULL FROM messages",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(revision, 3);
        assert_eq!(saved_state, state);
        assert_eq!(missing, cleaned);
        assert!(s.pending(10).unwrap().is_empty());
        assert_eq!(count(&t, "messages"), 1);
        assert_eq!(count(&t, "suppressions"), 1);
    }
}
#[test]
fn independent_part_results_merge_retries_and_empty_updates_across_reopen() {
    let t = TempDir::new();
    let c = config();
    let m = message_with_two_parts(&c);
    let key = message_identity(&c, &m).unwrap().key;
    let s = store(&t);
    s.append(&c, m.clone()).unwrap();
    let a = successful_result(m.parts[0].part_id, "synthetic evidence A");
    let b = successful_result(m.parts[1].part_id, "synthetic evidence B");
    s.record_parts(&key, 1, vec![a.clone()]).unwrap();
    s.record_parts(&key, 1, vec![b.clone()]).unwrap();
    drop(s);
    let s = store(&t);
    let (envelope, revision, results) = saved_message_and_results(&t);
    assert_eq!(revision, 1);
    assert_eq!(results, vec![a.clone(), b.clone()]);
    assert!(
        envelope
            .parts
            .iter()
            .all(|p| p.parse_state == PartStatus::Success)
    );
    let mut retry_a = a;
    retry_a.status = PartStatus::PartialParse;
    retry_a.reason_code = Some(PartReason::PartialSource);
    retry_a.blocks[0].text = "synthetic retry evidence A".into();
    s.record_parts(&key, 1, vec![retry_a.clone()]).unwrap();
    s.record_parts(&key, 1, vec![]).unwrap();
    drop(s);
    let reopened = store(&t);
    let (envelope, revision, results) = saved_message_and_results(&t);
    assert_eq!(revision, 1);
    assert_eq!(results, vec![retry_a.clone(), b]);
    assert_eq!(envelope.parts[0].parse_state, retry_a.status);
    assert_eq!(envelope.parts[0].failure_code, retry_a.reason_code);
    assert_eq!(envelope.parts[1].parse_state, PartStatus::Success);
    assert_eq!(envelope.parts[1].failure_code, None);
    assert_eq!(reopened.pending(1).unwrap()[0].parts, envelope.parts);
}
#[test]
fn empty_part_update_does_not_erase_saved_evidence() {
    let t = TempDir::new();
    let c = config();
    let m = message_with_two_parts(&c);
    let key = message_identity(&c, &m).unwrap().key;
    let s = store(&t);
    s.append(&c, m.clone()).unwrap();
    let result = successful_result(m.parts[0].part_id, "synthetic retained evidence");
    s.record_parts(&key, 1, vec![result.clone()]).unwrap();
    s.record_parts(&key, 1, vec![]).unwrap();
    drop(s);
    let _reopened = store(&t);
    let (envelope, _, results) = saved_message_and_results(&t);
    assert_eq!(results, vec![result]);
    assert_eq!(envelope.parts[0].parse_state, PartStatus::Success);
    assert_eq!(envelope.parts[1].parse_state, PartStatus::PendingDownload);
}

#[test]
fn stale_worker_cannot_write_reused_part_ids_after_message_edit() {
    let t = TempDir::new();
    let s = store(&t);
    let mut c = config();
    c.capability_set.push(SourceCapability::Edits);
    let m = message_with_two_parts(&c);
    let key = message_identity(&c, &m).unwrap().key;
    s.append(&c, m.clone()).unwrap();
    let captured = s.pending(1).unwrap().remove(0);
    let stale = successful_result(captured.parts[0].part_id, "synthetic stale event evidence");
    let mut edit = m;
    edit.revision = 2;
    edit.text = "synthetic changed date".into();
    s.append(&c, edit).unwrap();
    assert_eq!(
        s.record_parts(&key, captured.revision, vec![stale]),
        Err(AppError::Conflict)
    );
    assert_eq!(count(&t, "part_results"), 0);
    let current = s.pending(1).unwrap().remove(0);
    assert_eq!(current.revision, 2);
    assert_eq!(current.processing_state, ProcessingState::Persisted);
    assert_eq!(current.parts[0].parse_state, PartStatus::PendingDownload);
}

#[test]
fn attachment_jobs_persist_retry_with_virtual_clock_and_auth_is_terminal() {
    use shixu_core::notifications::parts::TaskQueue;
    let t = TempDir::new();
    let s = store(&t);
    let c = config();
    let m = message_with_two_parts(&c);
    let key = message_identity(&c, &m).unwrap().key;
    s.append(&c, m).unwrap();
    let db = Arc::new(Database::open(&t.db(), Arc::new(TestProtector::new(42))).unwrap());
    let q = TaskQueue::new(db);
    let limits = ParserLimits::v01();
    assert_eq!(q.enqueue(&key, 1, 0, &limits), Ok(2));
    let a = q.claim(0, &limits).unwrap().unwrap();
    assert!(q.claim(0, &limits).unwrap().is_none());
    q.finish(
        &a,
        PartResult {
            part_id: a.part.part_id,
            status: PartStatus::DownloadFailed,
            blocks: vec![],
            reason_code: Some(PartReason::AuthRequired),
        },
        true,
        0,
    )
    .unwrap();
    let mut now = 0;
    for delay in [60_000, 300_000, 1_800_000] {
        let job = q.claim(now, &limits).unwrap().unwrap();
        q.finish(
            &job,
            PartResult {
                part_id: job.part.part_id,
                status: PartStatus::DownloadFailed,
                blocks: vec![],
                reason_code: Some(PartReason::DownloadUnavailable),
            },
            true,
            now,
        )
        .unwrap();
        now += delay;
        assert!(q.claim(now - 1, &limits).unwrap().is_none());
    }
    drop(q);
    let q = TaskQueue::new(Arc::new(
        Database::open(&t.db(), Arc::new(TestProtector::new(42))).unwrap(),
    ));
    let job = q.claim(now, &limits).unwrap().unwrap();
    q.finish(
        &job,
        PartResult {
            part_id: job.part.part_id,
            status: PartStatus::DownloadFailed,
            blocks: vec![],
            reason_code: Some(PartReason::DownloadUnavailable),
        },
        true,
        now,
    )
    .unwrap();
    assert!(q.claim(i64::MAX, &limits).unwrap().is_none());
    assert_eq!(
        s.pending(1).unwrap()[0].processing_state,
        ProcessingState::Persisted
    );
}
#[test]
fn attachment_jobs_enforce_100_capacity_and_stale_completion_is_atomic() {
    use shixu_core::notifications::parts::TaskQueue;
    let t = TempDir::new();
    let s = store(&t);
    let mut c = config();
    c.capability_set.push(SourceCapability::Edits);
    let q = TaskQueue::new(Arc::new(
        Database::open(&t.db(), Arc::new(TestProtector::new(42))).unwrap(),
    ));
    let limits = ParserLimits::v01();
    for n in 0..50 {
        let mut m = message_with_two_parts(&c);
        m.native_message_id = format!("job-{n}");
        let key = message_identity(&c, &m).unwrap().key;
        s.append(&c, m).unwrap();
        assert_eq!(q.enqueue(&key, 1, 0, &limits), Ok(2));
    }
    let mut extra = message_with_two_parts(&c);
    extra.native_message_id = "extra".into();
    let key = message_identity(&c, &extra).unwrap().key;
    s.append(&c, extra).unwrap();
    assert_eq!(q.enqueue(&key, 1, 0, &limits), Err(AppError::StorageFull));
    let job = q.claim(0, &limits).unwrap().unwrap();
    let mut edit = s
        .pending(101)
        .unwrap()
        .into_iter()
        .find(|m| m.message_key == job.message_key)
        .unwrap();
    edit.revision = 2;
    edit.text = "synthetic edit".into();
    s.append(&c, edit).unwrap();
    assert_eq!(
        q.finish(&job, successful_result(job.part.part_id, "stale"), false, 0),
        Err(AppError::Conflict)
    );
    assert_eq!(count(&t, "part_results"), 0);
}

#[test]
fn attachment_startup_recovery_invalidates_old_lease_and_preserves_new_completion() {
    use shixu_core::notifications::parts::TaskQueue;
    let t = TempDir::new();
    let s = store(&t);
    let c = config();
    let m = message_with_two_parts(&c);
    let key = message_identity(&c, &m).unwrap().key;
    s.append(&c, m).unwrap();
    let q = TaskQueue::new(Arc::new(
        Database::open(&t.db(), Arc::new(TestProtector::new(42))).unwrap(),
    ));
    let l = ParserLimits::v01();
    assert_eq!(q.enqueue(&key, 1, 0, &l), Ok(2));
    assert_eq!(q.enqueue(&key, 1, 0, &l), Ok(0));
    let old = q.claim(0, &l).unwrap().unwrap();
    assert_eq!(q.recover_after_children_stopped(), Ok(1));
    let new = q.claim(0, &l).unwrap().unwrap();
    assert_eq!(old.part.part_id, new.part.part_id);
    assert_eq!(
        q.finish(
            &old,
            successful_result(old.part.part_id, "old lease"),
            false,
            0
        ),
        Err(AppError::Conflict)
    );
    q.finish(
        &new,
        successful_result(new.part.part_id, "new lease"),
        false,
        0,
    )
    .unwrap();
    let (_, revision, results) = saved_message_and_results(&t);
    assert_eq!(revision, 1);
    assert_eq!(results[0].blocks[0].text, "new lease");
}
