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
fn settings_bridge_uses_authoritative_runtime_and_same_pause() {
    let db =
        Arc::new(Database::open(std::path::Path::new(":memory:"), Arc::new(Synthetic)).unwrap());
    let state = AppState::from_database(db.clone());
    let runtime = state.runtime().unwrap();
    runtime.start(vec![]).unwrap();
    let context = CallingContext {
        label: "main",
        origin: "http://tauri.localhost",
    };
    let config = serde_json::json!({"source_id":"11111111-1111-4111-8111-111111111111","adapter_type":"synthetic","account_id":"synthetic","allowed_group_ids":["g"],"timezone":"Etc/UTC","enabled":true,"capability_set":["live_messages"]});
    dispatch(
        &context,
        "save_source_config",
        serde_json::json!({"config":config}),
        &state,
    )
    .unwrap();
    let status = dispatch(&context, "settings_read", serde_json::json!({}), &state).unwrap();
    assert_eq!(status["runtime"]["running"], true);
    assert_eq!(status["runtime"]["sources"].as_array().unwrap().len(), 1);
    assert_eq!(
        status["runtime"]["sources"][0]["connection_state"],
        "disconnected"
    );
    assert_eq!(status["transport_supported"], false);
    let pause = runtime.pause_writes().unwrap();
    assert_eq!(
        dispatch(
            &context,
            "set_autostart",
            serde_json::json!({"enabled":true}),
            &state
        ),
        Err(AppError::Conflict)
    );
    assert_eq!(
        state.consent.replace(0, Default::default()),
        Err(AppError::Conflict)
    );
    drop(pause);
    dispatch(
        &context,
        "set_autostart",
        serde_json::json!({"enabled":true}),
        &state,
    )
    .unwrap();
    assert!(state.settings().unwrap().autostart().unwrap());
}
