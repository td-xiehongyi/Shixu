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
