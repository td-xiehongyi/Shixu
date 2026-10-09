use super::{SchemaVersion, UtcMillis};
use serde::{Deserialize, Serialize};

uuid_id!(BlobId);
uuid_id!(RestorePreviewId);
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntityCounts {
    pub events: u64,
    pub messages: u64,
    pub sources: u64,
    pub changes: u64,
    pub suppressions: u64,
    pub vault_records: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlobState {
    Present,
    NeverFetched,
    Cleaned,
    NotMigrated,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlobManifest {
    pub blob_id: BlobId,
    pub content_hash: Option<String>,
    pub byte_size: Option<u64>,
    pub state: BlobState,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackupManifest {
    pub schema_version: SchemaVersion,
    pub created_at: UtcMillis,
    pub entity_counts: EntityCounts,
    pub blobs: Vec<BlobManifest>,
}
/// This is data only, never permission to overwrite. Restore requires a
/// separate user confirmation action bound to preview_id (implemented in D6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestorePreview {
    pub preview_id: RestorePreviewId,
    pub schema_version: SchemaVersion,
    pub created_at: UtcMillis,
    pub entity_counts: EntityCounts,
    pub blobs: Vec<BlobManifest>,
}
