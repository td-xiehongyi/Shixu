use shixu_core::{
    backup::CalendarBackup,
    calendar::EventService,
    contracts::{AppResult, calendar::*, error::AppError, notification::*},
    notifications::{
        consent::{ConsentStore, ModelConsent},
        settings::SettingsStore,
    },
    storage::{DataProtector, Database},
};
use std::{path::PathBuf, sync::Arc};
struct Protector;
impl DataProtector for Protector {
    fn protect(&self, p: &[u8]) -> AppResult<Vec<u8>> {
        Ok(p.iter().map(|b| b ^ 0xa5).collect())
    }
    fn unprotect(&self, p: &[u8]) -> AppResult<Vec<u8>> {
        self.protect(p)
    }
}
struct F {
    root: PathBuf,
    db: Arc<Database>,
    consent: Arc<ConsentStore>,
    backup: CalendarBackup,
}
impl F {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("shixu-d6-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let db = Arc::new(Database::open(&root.join("live.sqlite"), Arc::new(Protector)).unwrap());
        let consent = Arc::new(ConsentStore::new(ModelConsent::default()));
        let backup =
            CalendarBackup::new(db.clone(), consent.clone(), &root.join("backups")).unwrap();
        Self {
            root,
            db,
            consent,
            backup,
        }
    }
    fn event(&self, title: &str) -> CalendarEvent {
        EventService::new(self.db.clone())
            .create_manual(EventPatch {
                event_id: EventId::from_uuid(uuid::Uuid::new_v4()),
                expected_revision: 0,
                title: Some(title.into()),
                time: Some(TimeValue {
                    precision: Precision::DateOnly,
                    local_date: Some("2026-10-12".into()),
                    start_at: None,
                    end_at: None,
                    timezone: "Asia/Shanghai".into(),
                    raw_time_text: "2026年10月12日".into(),
                }),
                location: None,
                status: None,
            })
            .unwrap()
    }
    fn count(&self) -> usize {
        EventService::new(self.db.clone())
            .query(EventQuery {
                from_date: None,
                through_date: None,
                statuses: vec![],
                include_pending: true,
            })
            .unwrap()
            .len()
    }
}
impl Drop for F {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
#[test]
fn wal_snapshot_includes_committed_rows_and_keeps_live_owner() {
    let f = F::new();
    f.event("one");
    let p = f.db.pause_writes().unwrap();
    let m = f.backup.snapshot(0, &p).unwrap();
    assert_eq!(m.entity_counts.events, 1);
    drop(p);
    f.event("two");
    let v = f.backup.preview(0, 1).unwrap();
    let p = f.db.pause_writes().unwrap();
    f.backup.restore(v.preview_id, true, 2, &p).unwrap();
    drop(p);
    assert_eq!(f.count(), 1);
    f.event("three");
    assert_eq!(f.count(), 2);
}
#[test]
fn backup_retention_seven_daily_and_ten_vault_unavailable() {
    let f = F::new();
    for day in 0..10 {
        let p = f.db.pause_writes().unwrap();
        let a = f.backup.snapshot(day * 86400000, &p).unwrap();
        let b = f.backup.snapshot(day * 86400000 + 1, &p).unwrap();
        assert_eq!(a, b);
    }
    assert_eq!(f.backup.list().unwrap().len(), 7);
    assert_eq!(f.backup.vault_snapshot(), Err(AppError::Unsupported));
}
#[test]
fn same_database_guard_required() {
    let f = F::new();
    let other = F::new();
    let p = other.db.pause_writes().unwrap();
    assert_eq!(f.backup.snapshot(0, &p), Err(AppError::Conflict));
}
#[test]
fn export_confirmation_and_migration_disable_authority() {
    let f = F::new();
    f.event("exported");
    assert!(f.backup.export_json(false, false).is_err());
    let bytes = f.backup.export_json(false, true).unwrap();
    let v = f.backup.import_json(&bytes, 1).unwrap();
    assert_eq!(v.entity_counts.events, 1);
    let p = f.db.pause_writes().unwrap();
    assert!(f.backup.restore(v.preview_id, false, 2, &p).is_err());
    f.backup.restore(v.preview_id, true, 2, &p).unwrap();
    drop(p);
    assert!(!f.consent.snapshot().unwrap().enabled);
    assert!(!SettingsStore::new(f.db.clone()).autostart().unwrap());
}
fn config() -> SourceConfig {
    SourceConfig {
        source_id: SourceId::from_uuid(uuid::Uuid::new_v4()),
        adapter_type: "synthetic".into(),
        account_id: "synthetic".into(),
        allowed_group_ids: vec!["g".into()],
        timezone: "Asia/Shanghai".into(),
        enabled: true,
        capability_set: vec![
            SourceCapability::LiveMessages,
            SourceCapability::Attachments,
        ],
    }
}
fn notice(c: &SourceConfig, text: &str) -> MessageEnvelope {
    MessageEnvelope {
        message_key: MessageKey::from_uuid(uuid::Uuid::new_v4()),
        source_id: c.source_id,
        account_id: c.account_id.clone(),
        group_id: "g".into(),
        native_message_id: uuid::Uuid::new_v4().to_string(),
        sent_at: 1791504000000,
        received_at: 1791504000000,
        sender_id: "synthetic".into(),
        text: text.into(),
        reply_to: None,
        revision: 1,
        revoked: false,
        processing_state: ProcessingState::Persisted,
        parts: vec![],
    }
}
#[test]
fn credential_redaction_isolated_and_migration_preserves_source_proof() {
    use shixu_core::{contracts::vault::SecretBytes, notifications::MessageStore};
    let f = F::new();
    let c = config();
    SettingsStore::new(f.db.clone())
        .save_source(c.clone())
        .unwrap();
    f.db.store_source_secret(
        &c,
        &SecretBytes::new(b"SYNTHETIC-D6-CREDENTIAL-MARKER".to_vec()),
    )
    .unwrap();
    let store = MessageStore::new(f.db.clone());
    store
        .append(&c, notice(&c, "FULL-RAW-PRIVATE-SYNTHETIC-NOTICE"))
        .unwrap();
    let p = f.db.pause_writes().unwrap();
    f.backup.snapshot(0, &p).unwrap();
    drop(p);
    assert!(f.db.source_secret(&c).unwrap().is_some());
    let bytes = std::fs::read(f.root.join("backups/day-0/calendar.sqlite")).unwrap();
    let marker: Vec<_> = b"SYNTHETIC-D6-CREDENTIAL-MARKER"
        .iter()
        .map(|b| b ^ 0xa5)
        .collect();
    assert!(!bytes.windows(marker.len()).any(|w| w == marker));
    let export = f.backup.export_json(false, true).unwrap();
    assert!(
        !String::from_utf8(export.clone())
            .unwrap()
            .contains("FULL-RAW-PRIVATE")
    );
    let v = f.backup.import_json(&export, 1).unwrap();
    let p = f.db.pause_writes().unwrap();
    f.backup.restore(v.preview_id, true, 2, &p).unwrap();
    drop(p);
    assert!(
        !SettingsStore::new(f.db.clone()).sources().unwrap()[0]
            .config
            .enabled
    );
    assert!(f.db.source_secret(&c).unwrap().is_none());
}
#[test]
fn trusted_preview_rechecks_digest_rejects_forgery_and_reuse() {
    let f = F::new();
    f.event("one");
    let p = f.db.pause_writes().unwrap();
    f.backup.snapshot(0, &p).unwrap();
    drop(p);
    let v = f.backup.preview(0, 1).unwrap();
    let p = f.db.pause_writes().unwrap();
    assert_eq!(
        f.backup.restore(
            shixu_core::contracts::backup::RestorePreviewId::from_uuid(uuid::Uuid::new_v4()),
            true,
            2,
            &p
        ),
        Err(AppError::Conflict)
    );
    drop(p);
    let path = f.root.join("backups/day-0/manifest.json");
    let old = std::fs::read(&path).unwrap();
    let mut altered = old.clone();
    altered.push(b' ');
    std::fs::write(&path, &altered).unwrap();
    let p = f.db.pause_writes().unwrap();
    assert_eq!(
        f.backup.restore(v.preview_id, true, 2, &p),
        Err(AppError::Conflict)
    );
    drop(p);
    assert_eq!(f.count(), 1);
    std::fs::write(path, old).unwrap();
    let p = f.db.pause_writes().unwrap();
    f.backup.restore(v.preview_id, true, 2, &p).unwrap();
    assert_eq!(
        f.backup.restore(v.preview_id, true, 2, &p),
        Err(AppError::Conflict)
    );
}
#[test]
fn malformed_json_and_path_traversal_rejected() {
    let f = F::new();
    for bad in [b"{}".to_vec(), vec![b' '; 20 * 1024 * 1024 + 1]] {
        assert!(f.backup.import_json(&bad, 0).is_err())
    }
    let bytes = f.backup.export_json(false, true).unwrap();
    for field in ["transfer_schema_version", "physical_schema_version"] {
        let mut v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        v[field] = 999.into();
        assert!(
            f.backup
                .import_json(&serde_json::to_vec(&v).unwrap(), 0)
                .is_err()
        )
    }
    assert!(
        CalendarBackup::new(f.db.clone(), f.consent.clone(), &f.root.join("../escape")).is_err()
    );
}
#[test]
fn restore_replays_without_resurrection() {
    use shixu_core::{notifications::MessageStore, runtime::Supervisor};
    let f = F::new();
    let c = config();
    SettingsStore::new(f.db.clone())
        .save_source(c.clone())
        .unwrap();
    let runtime = Supervisor::new(f.db.clone(), f.consent.clone());
    runtime.start(vec![c.clone()]).unwrap();
    let m = notice(&c, "2026年10月12日9:00高数考试");
    runtime.receive(m.clone(), "cursor").unwrap();
    runtime.process_pending(0).unwrap();
    let service = EventService::new(f.db.clone());
    let event = service
        .query(EventQuery {
            from_date: None,
            through_date: None,
            statuses: vec![],
            include_pending: true,
        })
        .unwrap()
        .remove(0);
    let change = service
        .history(&event.event_id.to_string())
        .unwrap()
        .remove(0);
    service
        .undo(UndoRequest {
            change_id: change.change_id,
            expected_revision: event.revision,
        })
        .unwrap();
    let pending = notice(&c, "2026年10月13日9:00物理考试");
    runtime.receive(pending, "cursor2").unwrap();
    let p = f.db.pause_writes().unwrap();
    f.backup.snapshot(0, &p).unwrap();
    drop(p);
    runtime.process_pending(1).unwrap();
    let v = f.backup.preview(0, 2).unwrap();
    let p = f.db.pause_writes().unwrap();
    f.backup.restore(v.preview_id, true, 3, &p).unwrap();
    drop(p);
    SettingsStore::new(f.db.clone())
        .save_source(c.clone())
        .unwrap();
    runtime.process_pending(4).unwrap();
    let before = service
        .query(EventQuery {
            from_date: None,
            through_date: None,
            statuses: vec![],
            include_pending: true,
        })
        .unwrap();
    MessageStore::new(f.db.clone()).append(&c, m).unwrap();
    runtime.process_pending(5).unwrap();
    let after = service
        .query(EventQuery {
            from_date: None,
            through_date: None,
            statuses: vec![],
            include_pending: true,
        })
        .unwrap();
    assert_eq!(after, before);
    assert_eq!(
        after
            .iter()
            .filter(|e| e.status == EventStatus::Active)
            .count(),
        1
    );
    assert_eq!(
        after
            .iter()
            .filter(|e| e.status == EventStatus::Removed)
            .count(),
        1
    );
}
#[test]
fn default_migration_retains_missing_original_metadata() {
    use shixu_core::notifications::MessageStore;
    let f = F::new();
    let c = config();
    SettingsStore::new(f.db.clone())
        .save_source(c.clone())
        .unwrap();
    let mut m = notice(&c, "RAW-NOT-EXPORTED");
    let part = MessagePart {
        part_id: PartId::from_uuid(uuid::Uuid::new_v4()),
        message_key: m.message_key,
        kind: PartKind::File,
        source_file_ref: None,
        original_name: Some("fixture.pdf".into()),
        declared_type: None,
        detected_type: None,
        byte_size: Some(3),
        content_hash: Some("a".repeat(64)),
        fetch_state: FetchState::Fetched,
        parse_state: PartStatus::Success,
        failure_code: None,
        encrypted_blob_ref: None,
        retained_until: None,
    };
    m.parts.push(part);
    MessageStore::new(f.db.clone()).append(&c, m).unwrap();
    let export = f.backup.export_json(false, true).unwrap();
    let v = f.backup.import_json(&export, 1).unwrap();
    assert_eq!(v.blobs.len(), 1);
    assert_eq!(
        v.blobs[0].state,
        shixu_core::contracts::backup::BlobState::NotMigrated
    );
    assert!(
        !String::from_utf8(export)
            .unwrap()
            .contains("RAW-NOT-EXPORTED")
    );
}
struct Blobs {
    coordinator: Arc<shixu_core::storage::coordinator::WriteCoordinator>,
    data:
        std::sync::Mutex<std::collections::HashMap<shixu_core::contracts::backup::BlobId, Vec<u8>>>,
    fail: std::sync::atomic::AtomicBool,
}
impl shixu_core::backup::calendar::BackupBlobs for Blobs {
    fn coordinator(&self) -> Arc<shixu_core::storage::coordinator::WriteCoordinator> {
        self.coordinator.clone()
    }
    fn read(
        &self,
        p: &MessagePart,
        g: &shixu_core::storage::coordinator::PauseGuard,
    ) -> AppResult<Option<Vec<u8>>> {
        assert!(g.authenticates(&self.coordinator));
        Ok(self
            .data
            .lock()
            .unwrap()
            .get(&p.encrypted_blob_ref.unwrap())
            .cloned())
    }
    fn publish(
        &self,
        p: &MessagePart,
        b: &[u8],
        g: &shixu_core::storage::coordinator::PauseGuard,
    ) -> AppResult<()> {
        assert!(g.authenticates(&self.coordinator));
        if self.fail.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(AppError::StorageFull);
        }
        self.data
            .lock()
            .unwrap()
            .insert(p.encrypted_blob_ref.unwrap(), b.to_vec());
        Ok(())
    }
}
#[test]
fn blob_manifest_is_consistent_and_failed_publication_keeps_old_database() {
    use shixu_core::{
        contracts::backup::{BlobId, BlobState},
        notifications::MessageStore,
    };
    let mut f = F::new();
    let c = config();
    SettingsStore::new(f.db.clone())
        .save_source(c.clone())
        .unwrap();
    let mut m = notice(&c, "synthetic parts");
    let present = BlobId::from_uuid(uuid::Uuid::new_v4());
    let missing = BlobId::from_uuid(uuid::Uuid::new_v4());
    for id in [Some(present), Some(missing), None] {
        m.parts.push(MessagePart {
            part_id: PartId::from_uuid(uuid::Uuid::new_v4()),
            message_key: m.message_key,
            kind: PartKind::File,
            source_file_ref: None,
            original_name: None,
            declared_type: None,
            detected_type: None,
            byte_size: Some(3),
            content_hash: id.map(|_| "a".repeat(64)),
            fetch_state: if id.is_some() {
                FetchState::Fetched
            } else {
                FetchState::Pending
            },
            parse_state: PartStatus::PendingDownload,
            failure_code: None,
            encrypted_blob_ref: id,
            retained_until: None,
        });
    }
    MessageStore::new(f.db.clone()).append(&c, m).unwrap();
    let blobs = Arc::new(Blobs {
        coordinator: f.db.coordinator(),
        data: std::sync::Mutex::new(std::collections::HashMap::from([(present, vec![1, 2, 3])])),
        fail: false.into(),
    });
    f.backup = CalendarBackup::new(
        f.db.clone(),
        f.consent.clone(),
        &f.root.join("blob-backups"),
    )
    .unwrap()
    .with_blobs(blobs.clone())
    .unwrap();
    let p = f.db.pause_writes().unwrap();
    let manifest = f.backup.snapshot(0, &p).unwrap();
    assert!(manifest.blobs.iter().any(|b| b.state == BlobState::Present));
    assert!(manifest.blobs.iter().any(|b| b.state == BlobState::Cleaned));
    assert!(
        manifest
            .blobs
            .iter()
            .any(|b| b.state == BlobState::NeverFetched)
    );
    drop(p);
    f.event("retain on failure");
    let v = f.backup.preview(0, 1).unwrap();
    blobs.fail.store(true, std::sync::atomic::Ordering::SeqCst);
    let p = f.db.pause_writes().unwrap();
    assert_eq!(
        f.backup.restore(v.preview_id, true, 2, &p),
        Err(AppError::StorageFull)
    );
    drop(p);
    assert_eq!(f.count(), 1);
    blobs.fail.store(false, std::sync::atomic::Ordering::SeqCst);
    let blob_path = f.root.join("blob-backups/day-0").join(present.to_string());
    std::fs::write(&blob_path, [9, 9, 9]).unwrap();
    let p = f.db.pause_writes().unwrap();
    assert!(f.backup.restore(v.preview_id, true, 3, &p).is_err());
    drop(p);
    assert_eq!(f.count(), 1);
    std::fs::write(blob_path, [1, 2, 3]).unwrap();
    let p = f.db.pause_writes().unwrap();
    f.backup.restore(v.preview_id, true, 4, &p).unwrap();
    drop(p);
    assert_eq!(f.count(), 0);
    let exp = f.backup.export_json(false, true).unwrap();
    let v = f.backup.import_json(&exp, 5).unwrap();
    assert_eq!(
        v.blobs
            .iter()
            .filter(|b| b.state == BlobState::Cleaned)
            .count(),
        1
    );
    assert_eq!(
        v.blobs
            .iter()
            .filter(|b| b.state == BlobState::NotMigrated)
            .count(),
        1
    );
    assert_eq!(
        v.blobs
            .iter()
            .filter(|b| b.state == BlobState::NeverFetched)
            .count(),
        1
    );
}
#[test]
fn expired_preview_symlink_and_wrong_identity_fail_closed() {
    let f = F::new();
    f.event("safe");
    let p = f.db.pause_writes().unwrap();
    f.backup.snapshot(0, &p).unwrap();
    drop(p);
    let v = f.backup.preview(0, 1).unwrap();
    let p = f.db.pause_writes().unwrap();
    assert_eq!(
        f.backup.restore(v.preview_id, true, 600002, &p),
        Err(AppError::Conflict)
    );
    drop(p);
    assert_eq!(f.count(), 1);
    #[cfg(unix)]
    {
        let original = f.root.join("backups/day-0/calendar.sqlite");
        let moved = f.root.join("moved.sqlite");
        std::fs::rename(&original, &moved).unwrap();
        std::os::unix::fs::symlink(&moved, &original).unwrap();
        assert!(f.backup.preview(0, 2).is_err());
        std::fs::remove_file(&original).unwrap();
        std::fs::rename(&moved, &original).unwrap();
    }
    struct Different;
    impl DataProtector for Different {
        fn protect(&self, p: &[u8]) -> AppResult<Vec<u8>> {
            Ok(p.iter().map(|b| b ^ 0x22).collect())
        }
        fn unprotect(&self, p: &[u8]) -> AppResult<Vec<u8>> {
            self.protect(p)
        }
    }
    let db =
        Arc::new(Database::open(std::path::Path::new(":memory:"), Arc::new(Different)).unwrap());
    let backup = CalendarBackup::new(
        db,
        Arc::new(ConsentStore::new(ModelConsent::default())),
        &f.root.join("other"),
    )
    .unwrap();
    std::fs::create_dir(f.root.join("other/day-0")).unwrap();
    for file in ["manifest.json", "calendar.sqlite"] {
        std::fs::copy(
            f.root.join("backups/day-0").join(file),
            f.root.join("other/day-0").join(file),
        )
        .unwrap();
    }
    assert_eq!(backup.preview(0, 1), Err(AppError::AuthFailed));
}
#[test]
fn automatic_worker_writes_daily_snapshot_and_stops() {
    let f = F::new();
    let service = Arc::new(
        CalendarBackup::new(f.db.clone(), f.consent.clone(), &f.root.join("scheduled")).unwrap(),
    );
    let runtime = Arc::new(shixu_core::runtime::Supervisor::new(
        f.db.clone(),
        f.consent.clone(),
    ));
    runtime.start(vec![]).unwrap();
    let workers = runtime
        .spawn_workers(shixu_core::runtime::workers::WorkerPorts {
            backup: Some(service.clone()),
            ..Default::default()
        })
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while service.list().unwrap_or_default().is_empty() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    workers.stop().unwrap();
    assert_eq!(service.list().unwrap().len(), 1);
    assert!(!runtime.status().running);
}
#[cfg(unix)]
#[test]
fn dangling_owner_link_is_rejected_without_creating_target() {
    let f = F::new();
    let root = f.root.join("unsafe-backups");
    std::fs::create_dir(&root).unwrap();
    let target = f.root.join("must-not-be-created");
    std::os::unix::fs::symlink(&target, root.join(".owner")).unwrap();
    assert!(CalendarBackup::new(f.db.clone(), f.consent.clone(), &root).is_err());
    assert!(!target.exists());
}

fn source_transfer(f: &F) -> serde_json::Value {
    let c = config();
    SettingsStore::new(f.db.clone())
        .save_source(c.clone())
        .unwrap();
    let runtime = shixu_core::runtime::Supervisor::new(f.db.clone(), f.consent.clone());
    runtime.start(vec![c.clone()]).unwrap();
    runtime
        .receive(notice(&c, "2026年10月12日9:00高数考试"), "cursor")
        .unwrap();
    runtime.process_pending(0).unwrap();
    serde_json::from_slice(&f.backup.export_json(true, true).unwrap()).unwrap()
}
fn assert_rejected_mutations(
    f: &F,
    original: &serde_json::Value,
    mutations: &[(&str, serde_json::Value)],
) {
    f.backup
        .import_json(&serde_json::to_vec(original).unwrap(), 1)
        .unwrap();
    let before = f.backup.export_json(true, true).unwrap();
    let mut accepted = vec![];
    for (path, value) in mutations {
        let mut altered = original.clone();
        *altered
            .pointer_mut(path)
            .unwrap_or_else(|| panic!("missing fixture {path}")) = value.clone();
        if f.backup
            .import_json(&serde_json::to_vec(&altered).unwrap(), 2)
            .is_ok()
        {
            accepted.push(*path);
        }
        assert_eq!(
            f.backup.export_json(true, true).unwrap(),
            before,
            "live data changed for {path}"
        );
    }
    assert!(
        accepted.is_empty(),
        "accepted malformed imports: {accepted:?}"
    );
    f.event("live owner remains usable");
}
#[test]
fn migration_rejects_key_revision_and_source_relationship_mismatches() {
    let f = F::new();
    let original = source_transfer(&f);
    let id = serde_json::json!(uuid::Uuid::new_v4().to_string());
    assert_rejected_mutations(
        &f,
        &original,
        &[
            ("/tables/calendar_events/0/1/event/event_id", id.clone()),
            (
                "/tables/calendar_events/0/1/event/revision",
                serde_json::json!(0),
            ),
            ("/tables/calendar_events/0/1/last_message", id.clone()),
            ("/tables/calendar_changes/0/2/change_id", id.clone()),
            ("/tables/calendar_changes/0/2/event_id", id.clone()),
            ("/tables/calendar_changes/0/2/after/event_id", id.clone()),
            (
                "/tables/calendar_sources/0/3/candidate/candidate_key",
                id.clone(),
            ),
            ("/tables/calendar_sources/0/3/message_key", id.clone()),
            ("/tables/calendar_sources/0/3/event_id", id.clone()),
            ("/tables/calendar_sources/0/3/source_id", id.clone()),
            (
                "/tables/calendar_sources/0/3/message_revision",
                serde_json::json!(2),
            ),
            (
                "/tables/calendar_changes/0/2/source/account_id",
                serde_json::json!("other"),
            ),
            ("/tables/calendar_batches/0/1/batch/message_key", id.clone()),
            (
                "/tables/calendar_batches/0/1/batch/message_revision",
                serde_json::json!(2),
            ),
            ("/tables/messages/0/8/message_key", id.clone()),
            ("/tables/messages/0/8/source_id", id.clone()),
            ("/tables/messages/0/8/revision", serde_json::json!(2)),
            ("/tables/source_settings/0/2/source_id", id.clone()),
            ("/tables/message_source_proof/0/2/source_id", id),
        ],
    );
}
#[test]
fn migration_rejects_semantically_invalid_current_and_nested_times() {
    let f = F::new();
    let mut original = source_transfer(&f);
    let event = EventService::new(f.db.clone())
        .query(EventQuery {
            from_date: None,
            through_date: None,
            statuses: vec![],
            include_pending: true,
        })
        .unwrap()
        .remove(0);
    EventService::new(f.db.clone())
        .edit(
            &event.event_id.to_string(),
            event.revision,
            EventPatch {
                event_id: event.event_id,
                expected_revision: event.revision,
                title: Some("edited".into()),
                time: None,
                location: None,
                status: None,
            },
        )
        .unwrap();
    original =
        serde_json::from_slice(&f.backup.export_json(true, true).unwrap()).unwrap_or(original);
    let historical_index = original["tables"]["calendar_changes"]
        .as_array()
        .unwrap()
        .iter()
        .position(|r| !r[2]["before"].is_null())
        .unwrap();
    let source_index = original["tables"]["calendar_changes"]
        .as_array()
        .unwrap()
        .iter()
        .position(|r| !r[2]["source"].is_null())
        .unwrap();
    let before = format!("/tables/calendar_changes/{historical_index}/2/before/local_date");
    let nested =
        format!("/tables/calendar_changes/{source_index}/2/source/candidate/time/local_date");
    assert_rejected_mutations(
        &f,
        &original,
        &[
            (
                "/tables/calendar_events/0/1/event/local_date",
                serde_json::json!("2026-02-30"),
            ),
            (
                "/tables/calendar_events/0/1/event/timezone",
                serde_json::json!("Nowhere/Invalid"),
            ),
            (
                "/tables/calendar_events/0/1/event/end_at",
                serde_json::json!(1),
            ),
            (
                "/tables/calendar_events/0/1/event/time_precision",
                serde_json::json!("date_only"),
            ),
            (
                "/tables/calendar_changes/0/2/after/local_date",
                serde_json::json!("2026-02-30"),
            ),
            (&before, serde_json::json!("2026-02-30")),
            (&nested, serde_json::json!("2026-02-30")),
            (
                "/tables/calendar_sources/0/3/candidate/time/local_date",
                serde_json::json!("2026-02-30"),
            ),
            (
                "/tables/calendar_batches/0/1/batch/candidates/0/time/local_date",
                serde_json::json!("2026-02-30"),
            ),
        ],
    );
}
#[test]
fn migration_preserves_all_valid_time_precisions() {
    let f = F::new();
    let service = EventService::new(f.db.clone());
    for precision in [
        Precision::UnknownDate,
        Precision::DateOnly,
        Precision::ExplicitAllDay,
        Precision::Exact,
    ] {
        let e = f.event("valid precision");
        service
            .edit(
                &e.event_id.to_string(),
                e.revision,
                EventPatch {
                    event_id: e.event_id,
                    expected_revision: e.revision,
                    title: None,
                    time: Some(TimeValue {
                        precision,
                        local_date: (precision != Precision::UnknownDate)
                            .then(|| "2026-10-12".into()),
                        start_at: (precision == Precision::Exact).then_some(1791766800000),
                        end_at: None,
                        timezone: "Asia/Shanghai".into(),
                        raw_time_text: "synthetic".into(),
                    }),
                    location: None,
                    status: None,
                },
            )
            .unwrap();
    }
    let data = f.backup.export_json(false, true).unwrap();
    let preview = f.backup.import_json(&data, 1).unwrap();
    let pause = f.db.pause_writes().unwrap();
    f.backup
        .restore(preview.preview_id, true, 2, &pause)
        .unwrap();
    drop(pause);
    assert_eq!(f.count(), 4);
}
#[test]
fn migration_preserves_121_change_chronology_ids_and_undo() {
    let f = F::new();
    let service = EventService::new(f.db.clone());
    let mut e = f.event("initial");
    for n in 0..120 {
        e = service
            .edit(
                &e.event_id.to_string(),
                e.revision,
                EventPatch {
                    event_id: e.event_id,
                    expected_revision: e.revision,
                    title: Some(format!("edit {n}")),
                    time: None,
                    location: None,
                    status: None,
                },
            )
            .unwrap();
    }
    let before = service.history(&e.event_id.to_string()).unwrap();
    assert_eq!(before.len(), 121);
    let data = f.backup.export_json(false, true).unwrap();
    let preview = f.backup.import_json(&data, 1).unwrap();
    let pause = f.db.pause_writes().unwrap();
    f.backup
        .restore(preview.preview_id, true, 2, &pause)
        .unwrap();
    drop(pause);
    assert_eq!(service.history(&e.event_id.to_string()).unwrap(), before);
    let restored = service
        .undo(UndoRequest {
            change_id: before.last().unwrap().change_id,
            expected_revision: e.revision,
        })
        .unwrap();
    assert_eq!(restored.title, "edit 118");
    assert_eq!(restored.revision, 122);
}
