/** Native wire values: every Rust u64 is a canonical decimal string. IDs are opaque UUIDs. */
export type Revision = string;
export type U64 = string;
export type UtcMillis = number;
export type AppErrorCode =
  | "LOCKED"
  | "AUTH_FAILED"
  | "UNSUPPORTED"
  | "CONFLICT"
  | "STORAGE_FULL"
  | "DISCONNECTED"
  | "PARSE_FAILED"
  | "INVALID_INPUT";
export type Precision =
  "unknown_date" | "date_only" | "exact" | "explicit_all_day";
export interface TimeValue {
  precision: Precision;
  local_date: string | null;
  start_at: UtcMillis | null;
  end_at: UtcMillis | null;
  timezone: string;
  raw_time_text: string;
}
export type EventStatus = "active" | "cancelled" | "removed";
export type EventField = "title" | "time" | "location" | "status";
export interface CalendarEvent {
  event_id: string;
  title: string;
  kind: string;
  time_precision: Precision;
  local_date: string | null;
  start_at: UtcMillis | null;
  end_at: UtcMillis | null;
  timezone: string;
  raw_time_text: string;
  location: string | null;
  status: EventStatus;
  revision: Revision;
  user_overrides: EventField[];
}
export interface ApplySummary {
  created: U64;
  updated: U64;
  cancelled: U64;
  pending: U64;
  conflicts: U64;
  change_ids: string[];
}
export type LocationPatch =
  { operation: "set"; value: string } | { operation: "clear" };
export interface EventPatch {
  event_id: string;
  expected_revision: Revision;
  title: string | null;
  time: TimeValue | null;
  location: LocationPatch | null;
  status: EventStatus | null;
}
export interface EventQuery {
  from_date: string | null;
  through_date: string | null;
  statuses: EventStatus[];
  include_pending: boolean;
}
export interface UndoRequest {
  change_id: string;
  expected_revision: Revision;
}
export interface VaultSummary {
  entry_id: string;
  channel: string;
  account: string;
  revision: Revision;
  created_at: UtcMillis;
  updated_at: UtcMillis;
}
/** Secret-bearing values only exist inside the dedicated vault bridge/view. No SessionId DTO. */
export type VaultMutation =
  | {
      operation: "create";
      channel: string;
      account: string;
      password: Uint8Array;
    }
  | {
      operation: "update";
      id: string;
      expected_revision: Revision;
      channel: string;
      account: string;
      password: Uint8Array;
    }
  | { operation: "delete"; id: string; expected_revision: Revision };
export type LockReason =
  "manual" | "timeout" | "session_lock" | "suspend" | "exit";
export type VaultStatus = "not_created" | "locked" | "unlocking" | "unlocked";
export type SourceCapability =
  | "live_messages"
  | "backfill"
  | "attachments"
  | "replies"
  | "revocations"
  | "edits";
export interface SourceConfig {
  source_id: string;
  adapter_type: string;
  account_id: string;
  allowed_group_ids: string[];
  timezone: string;
  enabled: boolean;
  capability_set: SourceCapability[];
}
export type ProcessingState =
  | "persisted"
  | "parsing"
  | "committed"
  | "retryable_failure"
  | "unparseable"
  | "non_event"
  | "pending"
  | "source_revoked";
export type FetchState = "pending" | "fetching" | "fetched" | "unavailable";
export type PartStatus =
  | "pending_download"
  | "downloading"
  | "fetched"
  | "parsing"
  | "success"
  | "download_failed"
  | "unsupported"
  | "limit_exceeded"
  | "recognition_failed"
  | "partial_parse";
export type PartReason =
  | "DOWNLOAD_UNAVAILABLE"
  | "FORMAT_UNSUPPORTED"
  | "LIMIT_EXCEEDED"
  | "RECOGNITION_FAILED"
  | "PARTIAL_SOURCE"
  | "TIMED_OUT"
  | "MEMORY_LIMIT"
  | "AUTH_REQUIRED"
  | "EXPIRED"
  | "PERMISSION_DENIED"
  | "STORAGE_FULL";
export interface MessagePart {
  part_id: string;
  message_key: string;
  kind: "text" | "image" | "file";
  source_file_ref: string | null;
  original_name: string | null;
  declared_type: string | null;
  detected_type: string | null;
  byte_size: U64 | null;
  content_hash: string | null;
  fetch_state: FetchState;
  parse_state: PartStatus;
  failure_code: PartReason | null;
  encrypted_blob_ref: string | null;
  retained_until: UtcMillis | null;
}
export interface MessageEnvelope {
  /** Read projection only: current revision has an applied calendar source. */
  calendar_applied?: boolean;
  message_key: string;
  source_id: string;
  account_id: string;
  group_id: string;
  native_message_id: string;
  sent_at: UtcMillis;
  received_at: UtcMillis;
  sender_id: string;
  text: string;
  reply_to: string | null;
  revision: Revision;
  revoked: boolean;
  processing_state: ProcessingState;
  parts: MessagePart[];
}
export interface AttachmentRef {
  part_id: string;
  message_key: string;
  source_file_ref: string | null;
  content_hash: string | null;
  encrypted_blob_ref: string | null;
  fetch_state: FetchState;
  retained_until: UtcMillis | null;
}
export type Method = "native_text" | "ocr" | "cell";
export type QualityFlag =
  "uncertain_date" | "ambiguous_layout" | "formula_derived" | "partial_source";
export type PageOrSheet =
  | { kind: "page"; number: number }
  | { kind: "sheet"; name: string }
  | { kind: "paragraph"; number: number };
export type EvidenceLocation =
  | { kind: "cell_range"; range: string }
  | {
      kind: "bounding_box";
      x: number;
      y: number;
      width: number;
      height: number;
    }
  | { kind: "text_span"; start: number; end: number };
export interface EvidenceBlock {
  part_id: string;
  page_or_sheet: PageOrSheet | null;
  cell_range_or_bbox: EvidenceLocation | null;
  text: string;
  method: Method;
  engine_version: string;
  quality_flags: QualityFlag[];
}
export interface PartResult {
  part_id: string;
  status: PartStatus;
  blocks: EvidenceBlock[];
  reason_code: PartReason | null;
}
export interface Candidate {
  candidate_key: string;
  action: "create" | "reschedule" | "cancel";
  title: string;
  kind: string;
  time: TimeValue;
  location: string | null;
  evidence: EvidenceBlock[];
  target_message_key: string | null;
}
export interface ExtractBatch {
  message_key: string;
  message_revision: Revision;
  source_order: U64;
  candidates: Candidate[];
  part_results: PartResult[];
  extractor_version: string;
}
export interface ParserLimits {
  max_image_bytes: U64;
  max_image_pixels: U64;
  max_image_edge: number;
  max_file_bytes: U64;
  max_parts_per_message: number;
  max_message_bytes: U64;
  max_pdf_pages: number;
  max_extracted_chars: number;
  max_xlsx_sheets: number;
  max_xlsx_rows_per_sheet: number;
  max_xlsx_columns_per_sheet: number;
  max_xlsx_nonempty_cells: number;
  max_uncompressed_bytes: U64;
  max_archive_entries: number;
  max_compression_ratio: number;
  max_concurrent_parsers: number;
  max_subprocess_memory_bytes: U64;
  image_timeout_secs: number;
  file_timeout_secs: number;
  max_pending_tasks: number;
  attachment_cache_bytes: U64;
  max_download_retries: number;
  download_retry_delays_secs: [number, number, number];
  non_event_retention_days: number;
}
export interface EntityCounts {
  events: U64;
  messages: U64;
  sources: U64;
  changes: U64;
  suppressions: U64;
  vault_records: U64;
}
export type BlobState =
  "present" | "never_fetched" | "cleaned" | "not_migrated";
export interface BlobManifest {
  blob_id: string;
  content_hash: string | null;
  byte_size: U64 | null;
  state: BlobState;
}
export interface BackupManifest {
  schema_version: 1;
  created_at: UtcMillis;
  entity_counts: EntityCounts;
  blobs: BlobManifest[];
}
export interface RestorePreview extends BackupManifest {
  preview_id: string;
}

export interface ModelConsent {
  enabled: boolean;
  provider_id: string | null;
  allowed_group_ids: string[];
  allow_attachment_text: boolean;
  revision: Revision;
}
export interface SourceSetting {
  config: SourceConfig;
  epoch: Revision;
}
export interface RuntimeSnapshot {
  running: boolean;
  pending_rules: number;
  attachment_queue: number;
  model_queue: number;
  last_calendar_commit: number | null;
  last_error: AppErrorCode | null;
  sources: {
    source_id: string;
    connection_state:
      "connected" | "disconnected" | "waiting_for_login" | "incompatible";
    last_received_at: number | null;
    last_persisted_at: number | null;
    last_applied_at: number | null;
    gap: boolean;
  }[];
}
export interface SettingsSnapshot {
  runtime: RuntimeSnapshot;
  sources: SourceSetting[];
  model: ModelConsent;
  autostart: boolean;
  transport_supported: boolean;
}
export interface EventChange {
  change_id: string;
  before: CalendarEvent | null;
  after: CalendarEvent;
  undone: boolean;
}
export interface EventSource {
  message_key: string;
  message_revision: Revision;
  group_id: string;
  outcome: "applied" | "pending" | "conflict" | "suppressed" | "revoked";
  evidence: EvidenceBlock[];
}
export interface CalendarDetails {
  origin: "manual" | "source";
  history: EventChange[];
  sources: EventSource[];
}

export interface BackupSummary {
  created_at: number;
  events: string;
  messages: string;
  present: string;
  never_fetched: string;
  cleaned: string;
  not_migrated: string;
}
export interface BackupPreview extends BackupSummary {
  preview_id: string;
}
