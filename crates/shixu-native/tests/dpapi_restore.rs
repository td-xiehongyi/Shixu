use shixu_core::{
    backup::calendar::BackupBlobs,
    contracts::{AppResult, backup::BlobId, error::AppError, notification::*},
    storage::{DataProtector, Database},
};
use shixu_native::attachments::download::ProtectedCache;
use std::sync::Arc;
struct Synthetic;
impl DataProtector for Synthetic {
    fn protect(&self, p: &[u8]) -> AppResult<Vec<u8>> {
        Ok(p.iter().map(|b| b ^ 0xa5).collect())
    }
    fn unprotect(&self, p: &[u8]) -> AppResult<Vec<u8>> {
        self.protect(p)
    }
}
#[test]
fn actual_cache_snapshot_adapter_shares_live_guard_and_immutable_content() {
    use sha2::{Digest, Sha256};
    let root = std::env::temp_dir().join(format!("shixu-d6-native-{}", uuid::Uuid::new_v4()));
    let db = Database::open(std::path::Path::new(":memory:"), Arc::new(Synthetic)).unwrap();
    let cache = ProtectedCache::create_coordinated(&root, Arc::new(Synthetic), &db).unwrap();
    let plain = b"%PDF-synthetic-only";
    let part = MessagePart {
        part_id: PartId::from_uuid(uuid::Uuid::new_v4()),
        message_key: MessageKey::from_uuid(uuid::Uuid::new_v4()),
        kind: PartKind::File,
        source_file_ref: Some("synthetic-ref".to_string().try_into().unwrap()),
        original_name: None,
        declared_type: None,
        detected_type: None,
        byte_size: Some(plain.len() as u64),
        content_hash: Some(
            Sha256::digest(plain)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
        ),
        fetch_state: FetchState::Fetched,
        parse_state: PartStatus::Success,
        failure_code: None,
        encrypted_blob_ref: Some(BlobId::from_uuid(uuid::Uuid::new_v4())),
        retained_until: None,
    };
    let pause = db.pause_writes().unwrap();
    let bytes = Synthetic.protect(plain).unwrap();
    assert!(cache.read(&part, &pause).unwrap().is_none());
    cache.publish(&part, &bytes, &pause).unwrap();
    assert_eq!(cache.read(&part, &pause).unwrap(), Some(bytes.clone()));
    cache.publish(&part, &bytes, &pause).unwrap();
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 2);
    let other = Database::open(std::path::Path::new(":memory:"), Arc::new(Synthetic)).unwrap();
    assert_eq!(
        cache.read(&part, &other.pause_writes().unwrap()),
        Err(AppError::Conflict)
    );
    let mut bad = bytes;
    bad[0] ^= 1;
    assert_eq!(
        cache.publish(&part, &bad, &pause),
        Err(AppError::InvalidInput)
    );
    drop(cache);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
#[ignore = "requires actual Windows users, DPAPI identities, ACLs and restore runtime"]
fn wrong_windows_identity_fails_closed() {
    panic!(
        "BLOCKED: actual Windows same-user restore and second-user rejection have not been executed; synthetic protector tests are not identity proof"
    );
}
