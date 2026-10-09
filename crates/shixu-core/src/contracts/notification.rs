use super::{Revision, UtcMillis, backup::BlobId, calendar::TimeValue, error::AppError};
use serde::{Deserialize, Serialize};

uuid_id!(SourceId);
uuid_id!(MessageKey);
uuid_id!(PartId);
uuid_id!(CandidateKey);

/// A native adapter identifier. URLs and filesystem paths never persist here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct SourceFileRef(String);
impl TryFrom<String> for SourceFileRef {
    type Error = AppError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.is_empty()
            || value.contains(['/', '\\', ':'])
            || value.chars().any(char::is_control)
        {
            Err(AppError::InvalidInput)
        } else {
            Ok(Self(value))
        }
    }
}

/// SourceId identifies one adapter/account configuration. Message identity and
/// cursors must additionally be scoped by account/group/native ID in N1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceConfig {
    pub source_id: SourceId,
    pub adapter_type: String,
    pub account_id: String,
    pub allowed_group_ids: Vec<String>,
    pub timezone: String,
    pub enabled: bool,
    pub capability_set: Vec<SourceCapability>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceCapability {
    LiveMessages,
    Backfill,
    Attachments,
    Replies,
    Revocations,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessingState {
    Persisted,
    Parsing,
    Committed,
    RetryableFailure,
    Unparseable,
    NonEvent,
    Pending,
    SourceRevoked,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MessageEnvelope {
    pub message_key: MessageKey,
    pub source_id: SourceId,
    pub account_id: String,
    pub group_id: String,
    pub native_message_id: String,
    pub sent_at: UtcMillis,
    pub received_at: UtcMillis,
    pub sender_id: String,
    pub text: String,
    pub reply_to: Option<MessageKey>,
    pub revision: Revision,
    pub revoked: bool,
    pub processing_state: ProcessingState,
    pub parts: Vec<MessagePart>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartKind {
    Text,
    Image,
    File,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FetchState {
    Pending,
    Fetching,
    Fetched,
    Unavailable,
}
/// Exact ten per-part states from design 8.3, distinct from message processing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartStatus {
    PendingDownload,
    Downloading,
    Fetched,
    Parsing,
    Success,
    DownloadFailed,
    Unsupported,
    LimitExceeded,
    RecognitionFailed,
    PartialParse,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PartReason {
    DownloadUnavailable,
    FormatUnsupported,
    LimitExceeded,
    RecognitionFailed,
    PartialSource,
    TimedOut,
    MemoryLimit,
    AuthRequired,
    Expired,
    PermissionDenied,
    StorageFull,
}

/// source_file_ref is a stable native adapter reference, never a temporary URL.
/// Downloader URL/credential handling belongs to the transient N2 adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MessagePart {
    pub part_id: PartId,
    pub message_key: MessageKey,
    pub kind: PartKind,
    pub source_file_ref: Option<SourceFileRef>,
    pub original_name: Option<String>,
    pub declared_type: Option<String>,
    pub detected_type: Option<String>,
    pub byte_size: Option<u64>,
    pub content_hash: Option<String>,
    pub fetch_state: FetchState,
    pub parse_state: PartStatus,
    pub failure_code: Option<PartReason>,
    pub encrypted_blob_ref: Option<BlobId>,
    pub retained_until: Option<UtcMillis>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttachmentRef {
    pub part_id: PartId,
    pub message_key: MessageKey,
    pub source_file_ref: Option<SourceFileRef>,
    pub content_hash: Option<String>,
    pub encrypted_blob_ref: Option<BlobId>,
    pub fetch_state: FetchState,
    pub retained_until: Option<UtcMillis>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Method {
    NativeText,
    Ocr,
    Cell,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityFlag {
    UncertainDate,
    AmbiguousLayout,
    FormulaDerived,
    PartialSource,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PageOrSheet {
    Page { number: u32 },
    Sheet { name: String },
    Paragraph { number: u32 },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EvidenceLocation {
    CellRange {
        range: String,
    },
    BoundingBox {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    },
    TextSpan {
        start: u32,
        end: u32,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceBlock {
    pub part_id: PartId,
    pub page_or_sheet: Option<PageOrSheet>,
    pub cell_range_or_bbox: Option<EvidenceLocation>,
    pub text: String,
    pub method: Method,
    pub engine_version: String,
    pub quality_flags: Vec<QualityFlag>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartResult {
    pub part_id: PartId,
    pub status: PartStatus,
    pub blocks: Vec<EvidenceBlock>,
    pub reason_code: Option<PartReason>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateAction {
    Create,
    Reschedule,
    Cancel,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    pub candidate_key: CandidateKey,
    pub action: CandidateAction,
    pub title: String,
    pub kind: String,
    pub time: TimeValue,
    pub location: Option<String>,
    pub evidence: Vec<EvidenceBlock>,
    pub target_message_key: Option<MessageKey>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtractBatch {
    pub message_key: MessageKey,
    pub message_revision: Revision,
    pub source_order: u64,
    pub candidates: Vec<Candidate>,
    pub part_results: Vec<PartResult>,
    pub extractor_version: String,
}

/// Resource defaults only. Parsers and OS enforcement are implemented in N2–N5.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParserLimits {
    pub max_image_bytes: u64,
    pub max_image_pixels: u64,
    pub max_image_edge: u32,
    pub max_file_bytes: u64,
    pub max_parts_per_message: u32,
    pub max_message_bytes: u64,
    pub max_pdf_pages: u32,
    pub max_extracted_chars: u32,
    pub max_xlsx_sheets: u32,
    pub max_xlsx_rows_per_sheet: u32,
    pub max_xlsx_columns_per_sheet: u32,
    pub max_xlsx_nonempty_cells: u32,
    pub max_uncompressed_bytes: u64,
    pub max_archive_entries: u32,
    pub max_compression_ratio: u32,
    pub max_concurrent_parsers: u32,
    pub max_subprocess_memory_bytes: u64,
    pub image_timeout_secs: u32,
    pub file_timeout_secs: u32,
    pub max_pending_tasks: u32,
    pub attachment_cache_bytes: u64,
    pub max_download_retries: u32,
    pub download_retry_delays_secs: [u32; 3],
    pub non_event_retention_days: u32,
}
impl Default for ParserLimits {
    fn default() -> Self {
        const MIB: u64 = 1024 * 1024;
        Self {
            max_image_bytes: 10 * MIB,
            max_image_pixels: 20_000_000,
            max_image_edge: 10_000,
            max_file_bytes: 20 * MIB,
            max_parts_per_message: 5,
            max_message_bytes: 50 * MIB,
            max_pdf_pages: 20,
            max_extracted_chars: 200_000,
            max_xlsx_sheets: 10,
            max_xlsx_rows_per_sheet: 2_000,
            max_xlsx_columns_per_sheet: 50,
            max_xlsx_nonempty_cells: 20_000,
            max_uncompressed_bytes: 100 * MIB,
            max_archive_entries: 5_000,
            max_compression_ratio: 100,
            max_concurrent_parsers: 1,
            max_subprocess_memory_bytes: 512 * MIB,
            image_timeout_secs: 30,
            file_timeout_secs: 120,
            max_pending_tasks: 100,
            attachment_cache_bytes: 1024 * MIB,
            max_download_retries: 3,
            download_retry_delays_secs: [60, 300, 1800],
            non_event_retention_days: 30,
        }
    }
}
