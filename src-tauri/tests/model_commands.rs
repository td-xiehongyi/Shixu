use shixu_core::contracts::error::AppError;
use shixu_desktop::{
    app_state::AppState,
    commands::{CallingContext, authorize, dispatch},
};
#[test]
fn vault_cannot_call_model_commands() {
    for command in [
        "model_key_save",
        "model_key_delete",
        "model_settings_read",
        "model_status",
        "model_retry",
        "model_test_connection",
        "model_reviews",
        "model_review_resolve",
    ] {
        assert_eq!(
            authorize(
                &CallingContext {
                    label: "vault",
                    origin: "http://tauri.localhost"
                },
                command
            ),
            Err(AppError::AuthFailed)
        );
        assert!(
            authorize(
                &CallingContext {
                    label: "main",
                    origin: "http://tauri.localhost"
                },
                command
            )
            .is_ok()
        );
    }
}
#[cfg(windows)]
#[test]
fn key_is_cleared_and_never_returned() {
    let root = std::env::temp_dir().join(format!("shixu-key-command-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let s = AppState::default().with_model(&root).unwrap();
    let c = CallingContext {
        label: "main",
        origin: "http://tauri.localhost",
    };
    dispatch(
        &c,
        "model_key_save",
        serde_json::json!({"key":"synthetic-token"}),
        &s,
    )
    .unwrap();
    let v = dispatch(&c, "model_settings_read", serde_json::json!({}), &s).unwrap();
    assert_eq!(v["configured"], true);
    assert!(!v.to_string().contains("synthetic-token"));
    assert!(v.get("key").is_none());
    std::fs::remove_dir_all(root).unwrap();
}
