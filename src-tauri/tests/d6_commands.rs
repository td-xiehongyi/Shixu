use shixu_core::{
    contracts::{AppResult, error::AppError},
    storage::{DataProtector, Database},
};
use shixu_desktop::{
    app_state::AppState,
    commands::{CallingContext, dispatch},
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
#[test]
fn main_only_typed_backup_roundtrip_and_confirmed_output() {
    let root = std::env::temp_dir().join(format!(
        "shixu-d6-ipc-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let db =
        Arc::new(Database::open(std::path::Path::new(":memory:"), Arc::new(Synthetic)).unwrap());
    let state = AppState::from_database(db).with_backups(&root).unwrap();
    let main = CallingContext {
        label: "main",
        origin: "http://tauri.localhost",
    };
    let vault = CallingContext {
        label: "vault",
        origin: main.origin,
    };
    assert_eq!(
        dispatch(&vault, "backup_snapshot", serde_json::json!({}), &state),
        Err(AppError::AuthFailed)
    );
    assert_eq!(
        dispatch(
            &main,
            "backup_export",
            serde_json::json!({"confirmed":false,"include_raw_messages":false}),
            &state
        ),
        Err(AppError::InvalidInput)
    );
    assert_eq!(
        dispatch(
            &main,
            "backup_preview",
            serde_json::json!({"path":"../../live.sqlite"}),
            &state
        ),
        Err(AppError::InvalidInput)
    );
    let m = dispatch(&main, "backup_snapshot", serde_json::json!({}), &state).unwrap();
    assert_eq!(m["events"], "0");
    let data = dispatch(
        &main,
        "backup_export",
        serde_json::json!({"confirmed":true,"include_raw_messages":false}),
        &state,
    )
    .unwrap();
    let preview = dispatch(
        &main,
        "backup_import",
        serde_json::json!({"data":data}),
        &state,
    )
    .unwrap();
    assert_eq!(
        dispatch(
            &main,
            "backup_restore",
            serde_json::json!({"preview_id":preview["preview_id"],"confirmed":false}),
            &state
        ),
        Err(AppError::InvalidInput)
    );
    dispatch(
        &main,
        "backup_restore",
        serde_json::json!({"preview_id":preview["preview_id"],"confirmed":true}),
        &state,
    )
    .unwrap();
    assert!(dispatch(&main, "backup_previous", serde_json::json!({}), &state).is_ok());
    assert_eq!(
        dispatch(&main, "backup_vault", serde_json::json!({}), &state),
        Err(AppError::Unsupported)
    );
    drop(state);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn migration_keeps_actual_details_newest_100_changes() {
    use shixu_core::contracts::calendar::*;
    let root = std::env::temp_dir().join(format!(
        "shixu-d6-history-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let db =
        Arc::new(Database::open(std::path::Path::new(":memory:"), Arc::new(Synthetic)).unwrap());
    let state = AppState::from_database(db.clone())
        .with_backups(&root)
        .unwrap();
    let service = state.calendar().unwrap();
    let mut event = service
        .create_manual(EventPatch {
            event_id: "00000000-0000-4000-8000-000000000006".parse().unwrap(),
            expected_revision: 0,
            title: Some("initial".into()),
            time: Some(TimeValue {
                precision: Precision::UnknownDate,
                local_date: None,
                start_at: None,
                end_at: None,
                timezone: "UTC".into(),
                raw_time_text: String::new(),
            }),
            location: None,
            status: None,
        })
        .unwrap();
    for n in 0..120 {
        event = service
            .edit(
                &event.event_id.to_string(),
                event.revision,
                EventPatch {
                    event_id: event.event_id,
                    expected_revision: event.revision,
                    title: Some(format!("edit {n}")),
                    time: None,
                    location: None,
                    status: None,
                },
            )
            .unwrap();
    }
    let main = CallingContext {
        label: "main",
        origin: "http://tauri.localhost",
    };
    let args = serde_json::json!({"id":event.event_id});
    let before = dispatch(&main, "calendar_details", args.clone(), &state).unwrap();
    assert_eq!(before["history"].as_array().unwrap().len(), 100);
    assert_eq!(before["history"][0]["after"]["revision"], "121");
    assert_eq!(before["history"][99]["after"]["revision"], "22");
    let backup = state.backup().unwrap();
    let data = backup.export_json(false, true).unwrap();
    let preview = backup.import_json(&data, 1).unwrap();
    let pause = db.pause_writes().unwrap();
    backup.restore(preview.preview_id, true, 2, &pause).unwrap();
    drop(pause);
    let after = dispatch(&main, "calendar_details", args, &state).unwrap();
    assert_eq!(after, before);
    drop(backup);
    drop(state);
    drop(db);
    std::fs::remove_dir_all(root).unwrap();
}
