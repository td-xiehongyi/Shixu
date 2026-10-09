use shixu_core::contracts::error::AppError;
use shixu_desktop::{
    app_state::AppState,
    commands::{CallingContext, dispatch},
};
#[test]
fn d4_main_commands_reach_services_without_vault_privileges() {
    let ctx = CallingContext {
        label: "main",
        origin: "http://tauri.localhost",
    };
    for (command, payload) in [
        ("notification_list", serde_json::json!({"source_id":null})),
        (
            "notification_parts",
            serde_json::json!({"message_key":"11111111-1111-4111-8111-111111111111"}),
        ),
        ("settings_read", serde_json::json!({})),
        (
            "calendar_details",
            serde_json::json!({"id":"11111111-1111-4111-8111-111111111111"}),
        ),
    ] {
        assert_eq!(
            dispatch(&ctx, command, payload, &AppState::default()),
            Err(AppError::Unsupported),
            "{command}"
        );
    }
}
use shixu_core::{
    calendar::EventService,
    contracts::{AppResult, calendar::*, notification::*},
    notifications::{
        MessageStore, consent::ModelConsent, extract::extract, settings::SettingsStore,
    },
    storage::{DataProtector, Database},
};
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
fn database() -> Arc<Database> {
    Arc::new(Database::open(std::path::Path::new(":memory:"), Arc::new(Synthetic)).unwrap())
}
fn config() -> SourceConfig {
    serde_json::from_value(serde_json::json!({"source_id":"11111111-1111-4111-8111-111111111111","adapter_type":"synthetic","account_id":"synthetic","allowed_group_ids":["g1"],"timezone":"Asia/Shanghai","enabled":true,"capability_set":["live_messages","attachments","edits","revocations"]})).unwrap()
}
fn message(c: &SourceConfig, text: &str, native: &str) -> MessageEnvelope {
    serde_json::from_value(serde_json::json!({"message_key":"22222222-2222-4222-8222-222222222222","source_id":c.source_id,"account_id":c.account_id,"group_id":c.allowed_group_ids[0],"native_message_id":native,"sent_at":1791504000000i64,"received_at":1791504000000i64,"sender_id":"synthetic","text":text,"reply_to":null,"revision":1,"revoked":false,"processing_state":"persisted","parts":[]})).unwrap()
}
fn query() -> EventQuery {
    EventQuery {
        from_date: None,
        through_date: None,
        statuses: vec![],
        include_pending: true,
    }
}
#[test]
fn editable_settings_preserve_historical_timezone_identity_and_authorization() {
    let db = database();
    let store = MessageStore::new(db.clone());
    let settings = SettingsStore::new(db.clone());
    let mut c = config();
    settings.save_source(c.clone()).unwrap();
    let old = message(&c, "2026年10月12日9:00高数考试", "old");
    store.append(&c, old).unwrap();
    let old = store.list(None, 100).unwrap().remove(0);
    c.allowed_group_ids = vec!["g2".into()];
    c.timezone = "UTC".into();
    settings.save_source(c.clone()).unwrap();
    assert_eq!(settings.sources().unwrap()[0].epoch, 2);
    assert!(store.bind_source(&c).unwrap());
    assert_eq!(
        store.append(&config(), message(&config(), "旧授权", "rejected")),
        Err(AppError::Conflict)
    );
    let service = EventService::new(db.clone());
    service
        .apply(extract(&old, &[], &[], "Asia/Shanghai").unwrap())
        .unwrap();
    assert_eq!(service.query(query()).unwrap()[0].timezone, "Asia/Shanghai");
    store
        .append(&c, message(&c, "2026年10月13日9:00英语考试", "new"))
        .unwrap();
    let rows = store.list(None, 100).unwrap();
    let new = rows.iter().find(|m| m.native_message_id == "new").unwrap();
    service
        .apply(extract(new, &[], &[], "UTC").unwrap())
        .unwrap();
    assert_eq!(service.query(query()).unwrap().len(), 2);
    assert_eq!(store.list(None, 100).unwrap().len(), 2);
    assert!(store.list(None, 101).is_err());
    let mut changed = c.clone();
    changed.account_id = "other".into();
    assert_eq!(settings.save_source(changed), Err(AppError::Conflict));
}
#[test]
fn all_states_and_only_current_revision_parts_are_read() {
    let db = database();
    let store = MessageStore::new(db.clone());
    let c = config();
    store.bind_source(&c).unwrap();
    let mut m = message(&c, "2026年10月12日9:00高数考试", "parts");
    let pid: PartId = "33333333-3333-4333-8333-333333333333".parse().unwrap();
    m.parts=vec![serde_json::from_value(serde_json::json!({"part_id":pid,"message_key":m.message_key,"kind":"image","source_file_ref":null,"original_name":null,"declared_type":null,"detected_type":null,"byte_size":null,"content_hash":null,"fetch_state":"unavailable","parse_state":"download_failed","failure_code":"DOWNLOAD_UNAVAILABLE","encrypted_blob_ref":null,"retained_until":null})).unwrap()];
    store.append(&c, m).unwrap();
    let mut m = store.list(None, 100).unwrap().remove(0);
    store
        .record_parts(
            &m.message_key,
            1,
            vec![PartResult {
                part_id: pid,
                status: PartStatus::DownloadFailed,
                blocks: vec![],
                reason_code: Some(PartReason::DownloadUnavailable),
            }],
        )
        .unwrap();
    assert_eq!(store.current_parts(m.message_key).unwrap().len(), 1);
    let service = EventService::new(db.clone());
    let mut batch = extract(&m, &[], &[], &c.timezone).unwrap();
    batch.part_results = store.current_parts(m.message_key).unwrap();
    service.apply(batch).unwrap();
    assert!(!store.list(None, 100).unwrap().is_empty());
    let state = AppState::from_database(db.clone());
    let ctx = CallingContext {
        label: "main",
        origin: "http://tauri.localhost",
    };
    let list = dispatch(
        &ctx,
        "notification_list",
        serde_json::json!({"source_id":null}),
        &state,
    )
    .unwrap();
    assert_eq!(list[0]["calendar_applied"], true);

    m.revision = 2;
    m.text = "2026年10月13日9:00高数考试".into();
    store.append(&c, m.clone()).unwrap();
    assert!(store.current_parts(m.message_key).unwrap().is_empty());
    assert_eq!(
        store.record_parts(
            &m.message_key,
            1,
            vec![PartResult {
                part_id: pid,
                status: PartStatus::Success,
                blocks: vec![],
                reason_code: None
            }]
        ),
        Err(AppError::Conflict)
    );
}
#[test]
fn actual_core_long_source_has_bounded_wire_without_losing_stored_proof() {
    let db = database();
    let c = config();
    let store = MessageStore::new(db.clone());
    store.bind_source(&c).unwrap();
    let raw = format!("2026年10月12日9:00高数考试 {}", "说明".repeat(5000));
    store.append(&c, message(&c, &raw, "long")).unwrap();
    let m = store.list(None, 100).unwrap().remove(0);
    let batch = extract(&m, &[], &[], &c.timezone).unwrap();
    assert!(!batch.candidates.is_empty());
    let service = EventService::new(db.clone());
    service.apply(batch).unwrap();
    let e = service.query(query()).unwrap().remove(0);
    assert_eq!(e.raw_time_text, raw);
    let out = serde_json::to_value(shixu_desktop::wire::event(e).unwrap()).unwrap();
    assert!(out["raw_time_text"].as_str().unwrap().len() <= 4096);
    assert!(out["raw_time_text"].as_str().unwrap().ends_with('…'));
    assert_eq!(service.query(query()).unwrap()[0].raw_time_text, raw);
    if let Ok(path) = std::env::var("D4_WIRE_OUTPUT") {
        std::fs::write(path, serde_json::to_vec_pretty(&out).unwrap()).unwrap();
    }
}
#[test]
fn native_settings_share_live_consent_and_defaults_are_off() {
    let state = AppState::from_database(database());
    let ctx = CallingContext {
        label: "main",
        origin: "http://tauri.localhost",
    };
    let before = dispatch(&ctx, "settings_read", serde_json::json!({}), &state).unwrap();
    assert_eq!(before["autostart"], false);
    assert_eq!(before["model"]["enabled"], false);
    dispatch(&ctx,"set_model_consent",serde_json::json!({"consent":{"enabled":true,"provider_id":"synthetic","allowed_group_ids":["g1"],"allow_attachment_text":false,"revision":"0"}}),&state).unwrap();
    assert!(state.consent.snapshot().unwrap().enabled);
    assert_eq!(
        state.consent.replace(0, ModelConsent::default()),
        Err(AppError::Conflict)
    );
    assert_eq!(state.consent.snapshot().unwrap().revision, 1);
}
#[test]
fn bounded_retry_reuses_current_task_without_duplicate_event_or_override_loss() {
    use shixu_core::notifications::parts::TaskQueue;
    let db = database();
    let c = config();
    let store = MessageStore::new(db.clone());
    store.bind_source(&c).unwrap();
    let mut m = message(&c, "2026年10月12日9:00高数考试", "retry");
    let pid: PartId = "33333333-3333-4333-8333-333333333333".parse().unwrap();
    m.parts=vec![serde_json::from_value(serde_json::json!({"part_id":pid,"message_key":m.message_key,"kind":"image","source_file_ref":null,"original_name":null,"declared_type":null,"detected_type":null,"byte_size":null,"content_hash":null,"fetch_state":"pending","parse_state":"pending_download","failure_code":null,"encrypted_blob_ref":null,"retained_until":null})).unwrap()];
    store.append(&c, m).unwrap();
    let m = store.list(None, 100).unwrap().remove(0);
    let q = TaskQueue::new(db.clone());
    q.enqueue(&m.message_key, 1, 0, &ParserLimits::v01())
        .unwrap();
    let job = q.claim(0, &ParserLimits::v01()).unwrap().unwrap();
    q.finish(
        &job,
        PartResult {
            part_id: pid,
            status: PartStatus::DownloadFailed,
            blocks: vec![],
            reason_code: Some(PartReason::DownloadUnavailable),
        },
        false,
        0,
    )
    .unwrap();
    let service = EventService::new(db.clone());
    let mut batch = extract(&m, &[], &[], &c.timezone).unwrap();
    batch.part_results = store.current_parts(m.message_key).unwrap();
    service.apply(batch).unwrap();
    let e = service.query(query()).unwrap().remove(0);
    let edited = service
        .edit(
            &e.event_id.to_string(),
            e.revision,
            EventPatch {
                event_id: e.event_id,
                expected_revision: e.revision,
                title: Some("人工标题".into()),
                time: None,
                location: None,
                status: None,
            },
        )
        .unwrap();
    let state = AppState::from_database(db);
    let ctx = CallingContext {
        label: "main",
        origin: "http://tauri.localhost",
    };
    dispatch(
        &ctx,
        "retry_part",
        serde_json::json!({"part_id":pid}),
        &state,
    )
    .unwrap();
    dispatch(
        &ctx,
        "retry_part",
        serde_json::json!({"part_id":pid}),
        &state,
    )
    .unwrap();
    assert_eq!(service.query(query()).unwrap(), vec![edited]);
    let job = q.claim(i64::MAX, &ParserLimits::v01()).unwrap().unwrap();
    assert_eq!(job.part.part_id, pid);
    assert!(q.claim(i64::MAX, &ParserLimits::v01()).unwrap().is_none());
}
#[test]
fn legacy_notice_edit_after_timezone_change_keeps_first_interpretation() {
    let db = database();
    let store = MessageStore::new(db.clone());
    let mut c = config();
    store.bind_source(&c).unwrap();
    store
        .append(&c, message(&c, "2026年10月12日9:00高数考试", "legacy"))
        .unwrap();
    let mut m = store.list(None, 100).unwrap().remove(0);
    c.timezone = "UTC".into();
    SettingsStore::new(db.clone())
        .save_source(c.clone())
        .unwrap();
    m.revision = 2;
    m.text = "2026年10月13日9:00高数考试".into();
    store.append(&c, m).unwrap();
    let m = store.list(None, 100).unwrap().remove(0);
    let service = EventService::new(db);
    assert_eq!(
        service.apply(extract(&m, &[], &[], "UTC").unwrap()),
        Err(AppError::InvalidInput)
    );
    service
        .apply(extract(&m, &[], &[], "Asia/Shanghai").unwrap())
        .unwrap();
    assert_eq!(service.query(query()).unwrap()[0].timezone, "Asia/Shanghai");
}
#[test]
fn settings_cannot_invent_binding_for_retained_unbound_legacy_notices() {
    let db = database();
    let c = config();
    MessageStore::new(db.clone())
        .append(&c, message(&c, "legacy unbound", "legacy-unbound"))
        .unwrap();
    assert_eq!(
        SettingsStore::new(db).save_source(c),
        Err(AppError::Conflict)
    );
}
