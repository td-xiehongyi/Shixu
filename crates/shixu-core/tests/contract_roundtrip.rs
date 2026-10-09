mod support;
use serde_json::{Value, json};
use shixu_core::contracts::{
    backup::BackupManifest,
    calendar::{CalendarEvent, TimeValue},
    vault::{SecretBytes, VaultRecord, VaultSummary},
};
use support::{fixture_bytes, fixture_json};

#[test]
fn known_event_roundtrips_without_inventing_a_clock_time() {
    let value = fixture_json("contracts/event.json");
    let event: CalendarEvent = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(event).unwrap(), value);
    assert!(value["start_at"].is_null());
    assert!(value["end_at"].is_null());
}

#[test]
fn invalid_precision_is_rejected() {
    let bad_precision = json!({"precision":"guess_midnight","local_date":null,"start_at":null,"end_at":null,"timezone":"Asia/Shanghai","raw_time_text":"下周"});
    assert!(serde_json::from_value::<TimeValue>(bad_precision).is_err());
}

#[test]
fn unknown_backup_versions_are_rejected() {
    let mut value = backup_value();
    value["schema_version"] = json!(999);
    assert!(serde_json::from_value::<BackupManifest>(value).is_err());
}

#[test]
fn known_backup_manifest_roundtrips() {
    let value = backup_value();
    let manifest: BackupManifest = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(manifest).unwrap(), value);
}

#[test]
fn secrets_never_enter_vault_summary() {
    let record = VaultRecord {
        entry_id: "00000000-0000-4000-8000-000000000001".parse().unwrap(),
        channel: "虚构渠道".into(),
        account: "synthetic-account".into(),
        password: SecretBytes::new(b"SYNTHETIC-ONLY".to_vec()),
        revision: 2,
        created_at: 1,
        updated_at: 2,
    };
    let summary_json = serde_json::to_string(&VaultSummary::from(&record)).unwrap();
    assert!(!summary_json.contains("password"));
    assert!(!summary_json.contains("SYNTHETIC-ONLY"));
    assert!(summary_json.contains("synthetic-account"));
    assert_eq!(record.password.expose(), b"SYNTHETIC-ONLY");
}

#[test]
fn fixture_bytes_share_the_read_only_allowlist() {
    let bytes = fixture_bytes("contracts/event.json");
    assert_eq!(
        serde_json::from_slice::<Value>(&bytes).unwrap(),
        fixture_json("contracts/event.json")
    );
    for name in [
        "/etc/passwd",
        "../manifest.json",
        "contracts/../../README.md",
        "manifest.json",
        ".env",
        "unknown.json",
    ] {
        assert!(std::panic::catch_unwind(|| fixture_bytes(name)).is_err());
    }
}

fn backup_value() -> Value {
    json!({"schema_version":1,"created_at":1791504000000i64,"entity_counts":{"events":1,"messages":1,"sources":1,"changes":1,"suppressions":0,"vault_records":0},"blobs":[]})
}

#[test]
fn secret_clear_releases_exposed_contents_without_normalizing_bytes() {
    let mut secret = SecretBytes::new(vec![0, 255, b' ', b'\n']);
    assert_eq!(secret.expose(), &[0, 255, b' ', b'\n']);
    secret.clear();
    assert!(secret.expose().is_empty());
    fn requires_zeroize_on_drop<T: zeroize::ZeroizeOnDrop>() {}
    requires_zeroize_on_drop::<SecretBytes>();
}

#[test]
fn persistent_attachment_references_reject_transient_urls_and_disk_paths() {
    use shixu_core::contracts::notification::MessagePart;
    for reference in [
        "https://synthetic.invalid/file?token=synthetic-only",
        "file:///synthetic",
        "/synthetic/private/file",
        "C:\\synthetic\\file",
    ] {
        let mut value = part_value();
        value["source_file_ref"] = json!(reference);
        assert!(serde_json::from_value::<MessagePart>(value).is_err());
    }
}

fn part_value() -> Value {
    json!({"part_id":"00000000-0000-4000-8000-000000000003","message_key":"00000000-0000-4000-8000-000000000004","kind":"file","source_file_ref":"synthetic-native-file-1","original_name":"synthetic.xlsx","declared_type":"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet","detected_type":null,"byte_size":128,"content_hash":null,"fetch_state":"pending","parse_state":"pending_download","failure_code":null,"encrypted_blob_ref":null,"retained_until":null})
}

fn roundtrip<T>(value: Value)
where
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    let decoded: T = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), value);
}

#[test]
fn source_and_message_roundtrips_preserve_adapter_account_group_namespace() {
    use shixu_core::contracts::notification::{MessageEnvelope, SourceConfig};
    roundtrip::<SourceConfig>(source_value());
    let message = json!({"message_key":"00000000-0000-4000-8000-000000000004","source_id":"00000000-0000-4000-8000-000000000005","account_id":"synthetic-account-A","group_id":"synthetic-group-B","native_message_id":"synthetic-msg-1","sent_at":1791504000000i64,"received_at":1791504000100i64,"sender_id":"synthetic-sender","text":"合成考试通知","reply_to":null,"revision":1,"revoked":false,"processing_state":"persisted","parts":[part_value()]});
    roundtrip::<MessageEnvelope>(message);
}
fn source_value() -> Value {
    json!({"source_id":"00000000-0000-4000-8000-000000000005","adapter_type":"synthetic","account_id":"synthetic-account-A","allowed_group_ids":["synthetic-group-B"],"timezone":"Asia/Shanghai","enabled":false,"capability_set":["live_messages","attachments","backfill","replies","revocations"]})
}

#[test]
fn dto_payloads_reject_credentials_and_unstructured_parser_errors() {
    use shixu_core::contracts::notification::{MessagePart, PartResult, SourceConfig};
    let mut source = source_value();
    source["access_token"] = json!("SYNTHETIC-ONLY");
    assert!(serde_json::from_value::<SourceConfig>(source).is_err());
    let mut part = part_value();
    part["download_url"] = json!("https://synthetic.invalid");
    assert!(serde_json::from_value::<MessagePart>(part).is_err());
    let result = json!({"part_id":"00000000-0000-4000-8000-000000000003","status":"recognition_failed","blocks":[],"reason_code":"arbitrary exception with payload"});
    assert!(serde_json::from_value::<PartResult>(result).is_err());
    let record_summary = json!({"entry_id":"00000000-0000-4000-8000-000000000001","channel":"synthetic","account":"synthetic","revision":1,"created_at":0,"updated_at":0,"password":"SYNTHETIC-ONLY"});
    assert!(serde_json::from_value::<VaultSummary>(record_summary).is_err());
}

#[test]
fn extraction_roundtrips_preserve_order_revision_and_locatable_quality_evidence() {
    use shixu_core::contracts::notification::{AttachmentRef, ExtractBatch};
    let evidence = json!({"part_id":"00000000-0000-4000-8000-000000000003","page_or_sheet":{"kind":"sheet","name":"合成安排"},"cell_range_or_bbox":{"kind":"cell_range","range":"A1:C2"},"text":"下周考试","method":"cell","engine_version":"synthetic-1","quality_flags":["uncertain_date","ambiguous_layout","formula_derived","partial_source"]});
    let time = json!({"precision":"unknown_date","local_date":null,"start_at":null,"end_at":null,"timezone":"Asia/Shanghai","raw_time_text":"下周"});
    let candidate = json!({"candidate_key":"00000000-0000-4000-8000-000000000006","action":"reschedule","title":"合成高数考试","kind":"exam","time":time,"location":null,"evidence":[evidence.clone()],"target_message_key":"00000000-0000-4000-8000-000000000004"});
    roundtrip::<ExtractBatch>(
        json!({"message_key":"00000000-0000-4000-8000-000000000004","message_revision":3,"source_order":7,"candidates":[candidate],"part_results":[{"part_id":"00000000-0000-4000-8000-000000000003","status":"partial_parse","blocks":[evidence],"reason_code":"PARTIAL_SOURCE"}],"extractor_version":"synthetic-1"}),
    );
    roundtrip::<AttachmentRef>(
        json!({"part_id":"00000000-0000-4000-8000-000000000003","message_key":"00000000-0000-4000-8000-000000000004","source_file_ref":"synthetic-native-file-1","content_hash":null,"encrypted_blob_ref":null,"fetch_state":"unavailable","retained_until":null}),
    );
}

#[test]
fn all_time_precisions_roundtrip_nulls_without_default_midnight() {
    for (precision, date, start) in [
        ("unknown_date", Value::Null, Value::Null),
        ("date_only", json!("2026-10-12"), Value::Null),
        ("explicit_all_day", json!("2026-10-12"), Value::Null),
        ("exact", json!("2026-10-12"), json!(1791766800000i64)),
    ] {
        roundtrip::<TimeValue>(
            json!({"precision":precision,"local_date":date,"start_at":start,"end_at":null,"timezone":"Asia/Shanghai","raw_time_text":"合成时间表述"}),
        );
    }
}

#[test]
fn ipc_errors_have_only_fixed_uppercase_codes() {
    use shixu_core::contracts::error::AppError;
    for (error, code) in [
        (AppError::Locked, "LOCKED"),
        (AppError::AuthFailed, "AUTH_FAILED"),
        (AppError::Unsupported, "UNSUPPORTED"),
        (AppError::Conflict, "CONFLICT"),
        (AppError::StorageFull, "STORAGE_FULL"),
        (AppError::Disconnected, "DISCONNECTED"),
        (AppError::ParseFailed, "PARSE_FAILED"),
        (AppError::InvalidInput, "INVALID_INPUT"),
    ] {
        assert_eq!(error.code(), code);
        assert_eq!(error.to_string(), code);
        roundtrip::<AppError>(json!(code));
    }
}

#[test]
fn parser_defaults_match_every_authorized_resource_and_retry_bound() {
    use shixu_core::contracts::notification::ParserLimits;
    let limits = serde_json::to_value(ParserLimits::default()).unwrap();
    let expected = json!({"max_image_bytes":10485760,"max_image_pixels":20000000,"max_image_edge":10000,"max_file_bytes":20971520,"max_parts_per_message":5,"max_message_bytes":52428800,"max_pdf_pages":20,"max_extracted_chars":200000,"max_xlsx_sheets":10,"max_xlsx_rows_per_sheet":2000,"max_xlsx_columns_per_sheet":50,"max_xlsx_nonempty_cells":20000,"max_uncompressed_bytes":104857600,"max_archive_entries":5000,"max_compression_ratio":100,"max_concurrent_parsers":1,"max_subprocess_memory_bytes":536870912,"image_timeout_secs":30,"file_timeout_secs":120,"max_pending_tasks":100,"attachment_cache_bytes":1073741824u64,"max_download_retries":3,"download_retry_delays_secs":[60,300,1800],"non_event_retention_days":30});
    assert_eq!(limits, expected);
    roundtrip::<ParserLimits>(expected);
}

#[test]
fn opaque_ids_reject_non_uuid_values_with_safe_errors() {
    use shixu_core::contracts::{calendar::EventId, error::AppError};
    assert_eq!(
        "not-a-uuid".parse::<EventId>().unwrap_err(),
        AppError::InvalidInput
    );
    let mut value = fixture_json("contracts/event.json");
    value["event_id"] = json!("not-a-uuid");
    assert!(serde_json::from_value::<CalendarEvent>(value).is_err());
    let mut value = source_value();
    value["source_id"] = json!(42);
    assert!(
        serde_json::from_value::<shixu_core::contracts::notification::SourceConfig>(value).is_err()
    );
}

#[test]
fn event_patch_distinguishes_keep_set_clear_and_requires_revision() {
    use shixu_core::contracts::calendar::{
        ApplySummary, EventPatch, EventQuery, LocationPatch, UndoRequest,
    };
    let patch = json!({"event_id":"00000000-0000-4000-8000-000000000002","expected_revision":3,"title":null,"time":null,"location":null,"status":null});
    let unchanged: EventPatch = serde_json::from_value(patch.clone()).unwrap();
    assert_eq!(unchanged.location, None);
    roundtrip::<EventPatch>(patch.clone());
    let mut clear = patch.clone();
    clear["location"] = json!({"operation":"clear"});
    assert_eq!(
        serde_json::from_value::<EventPatch>(clear.clone())
            .unwrap()
            .location,
        Some(LocationPatch::Clear)
    );
    roundtrip::<EventPatch>(clear);
    let mut set = patch.clone();
    set["location"] = json!({"operation":"set","value":"合成教室"});
    roundtrip::<EventPatch>(set);
    let mut prohibited = patch.clone();
    prohibited["kind"] = json!("exam");
    assert!(serde_json::from_value::<EventPatch>(prohibited).is_err());
    let mut missing = patch;
    missing.as_object_mut().unwrap().remove("expected_revision");
    assert!(serde_json::from_value::<EventPatch>(missing).is_err());
    roundtrip::<EventQuery>(
        json!({"from_date":null,"through_date":null,"statuses":["active","cancelled","removed"],"include_pending":true}),
    );
    roundtrip::<UndoRequest>(
        json!({"change_id":"00000000-0000-4000-8000-000000000007","expected_revision":4}),
    );
    roundtrip::<ApplySummary>(
        json!({"created":1,"updated":2,"cancelled":3,"pending":4,"conflicts":5,"change_ids":["00000000-0000-4000-8000-000000000007"]}),
    );
}

#[test]
fn backup_preview_preserves_missing_blobs_without_credentials_or_restore_action() {
    use shixu_core::contracts::backup::RestorePreview;
    let mut manifest = backup_value();
    manifest["blobs"] = json!([{"blob_id":"00000000-0000-4000-8000-000000000008","content_hash":null,"byte_size":null,"state":"never_fetched"},{"blob_id":"00000000-0000-4000-8000-000000000009","content_hash":"synthetic-sha256","byte_size":128,"state":"cleaned"}]);
    roundtrip::<BackupManifest>(manifest.clone());
    let mut preview = manifest;
    preview["preview_id"] = json!("00000000-0000-4000-8000-000000000010");
    roundtrip::<RestorePreview>(preview.clone());
    preview["schema_version"] = json!(2);
    assert!(serde_json::from_value::<RestorePreview>(preview).is_err());
    for invalid_version in [json!(0), json!(-1), json!("1"), json!(1.5), Value::Null] {
        let mut value = backup_value();
        value["schema_version"] = invalid_version;
        assert!(serde_json::from_value::<BackupManifest>(value).is_err());
    }
}

#[test]
fn part_and_vault_states_are_typed_wire_values() {
    use shixu_core::contracts::{
        notification::{CandidateAction, Method, PartStatus},
        vault::{LockReason, VaultStatus},
    };
    for value in [
        "pending_download",
        "downloading",
        "fetched",
        "parsing",
        "success",
        "download_failed",
        "unsupported",
        "limit_exceeded",
        "recognition_failed",
        "partial_parse",
    ] {
        roundtrip::<PartStatus>(json!(value));
    }
    for value in ["native_text", "ocr", "cell"] {
        roundtrip::<Method>(json!(value));
    }
    for value in ["create", "reschedule", "cancel"] {
        roundtrip::<CandidateAction>(json!(value));
    }
    for value in ["not_created", "locked", "unlocking", "unlocked"] {
        roundtrip::<VaultStatus>(json!(value));
    }
    for value in ["manual", "timeout", "session_lock", "suspend", "exit"] {
        roundtrip::<LockReason>(json!(value));
    }
    assert!(serde_json::from_value::<PartStatus>(json!("pretend_success")).is_err());
}
