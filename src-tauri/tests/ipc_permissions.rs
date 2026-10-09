use shixu_core::contracts::{calendar::CalendarEvent, error::AppError, vault::VaultSummary};
use shixu_desktop::{
    commands::{CallingContext, authorize, validate_id, validate_size},
    wire,
};
const MAIN: CallingContext<'static> = CallingContext {
    label: "main",
    origin: "http://tauri.localhost",
};
#[test]
fn main_window_cannot_call_vault() {
    for command in [
        "vault_unlock",
        "vault_list",
        "vault_apply",
        "vault_reveal",
        "vault_lock",
    ] {
        assert_eq!(authorize(&MAIN, command), Err(AppError::AuthFailed));
    }
    assert_eq!(authorize(&MAIN, "show_vault_window"), Ok(()));
    assert_eq!(authorize(&MAIN, "shell"), Err(AppError::AuthFailed));
}
#[test]
fn remote_origin_cannot_invoke() {
    for origin in [
        "https://evil.example",
        "http://tauri.localhost.evil",
        "http://tauri.localhost:80",
        "http://tauri.localhost@evil",
        "http://localhost:1420",
        "tauri://localhost",
    ] {
        assert_eq!(
            authorize(
                &CallingContext {
                    label: "vault",
                    origin
                },
                "vault_reveal"
            ),
            Err(AppError::AuthFailed)
        );
    }
    assert_eq!(
        authorize(
            &CallingContext {
                label: "forged",
                origin: MAIN.origin
            },
            "calendar_query"
        ),
        Err(AppError::AuthFailed)
    );
}
#[test]
fn payload_limits_and_revision_are_checked() {
    for value in ["01", "+1", "-1", " 1", "1.0", "18446744073709551616"] {
        assert_eq!(wire::revision(value), Err(AppError::InvalidInput));
    }
    assert_eq!(wire::revision("18446744073709551615"), Ok(u64::MAX));
    assert_eq!(validate_size(65537), Err(AppError::InvalidInput));
    assert_eq!(validate_id("../../secret"), Err(AppError::InvalidInput));
}
#[test]
fn dto_roundtrip_matches_rust_fixture() {
    let raw = include_str!("fixtures/event.json");
    let mut numeric: serde_json::Value = serde_json::from_str(raw).unwrap();
    numeric["revision"] = serde_json::json!(9007199254740993u64);
    let core: CalendarEvent = serde_json::from_value(numeric).unwrap();
    let serialized = serde_json::to_value(wire::event(core).unwrap()).unwrap();
    let expected: serde_json::Value = serde_json::from_str(raw).unwrap();
    assert_eq!(serialized, expected);
    let core_summary: VaultSummary = serde_json::from_value(serde_json::json!({"entry_id":"11111111-1111-4111-8111-111111111111","channel":"演示","account":"synthetic","revision":9007199254740993u64,"created_at":0,"updated_at":1})).unwrap();
    let serialized = serde_json::to_value(wire::summary(core_summary).unwrap()).unwrap();
    assert_eq!(serialized["revision"], "9007199254740993");
    assert!(serialized.get("password").is_none());
}
#[test]
fn actual_vault_is_unsupported_until_engine_verified() {
    use shixu_desktop::{app_state::AppState, commands::dispatch};
    let vault = CallingContext {
        label: "vault",
        origin: MAIN.origin,
    };
    for (command, payload) in [
        ("vault_unlock", serde_json::json!({"master":[7,8,9]})),
        ("vault_list", serde_json::json!({})),
        (
            "vault_reveal",
            serde_json::json!({"id":"11111111-1111-4111-8111-111111111111"}),
        ),
        (
            "vault_apply",
            serde_json::json!({"mutation":{"operation":"create","channel":"演示","account":"synthetic","password":[7,8,9]}}),
        ),
        ("vault_lock", serde_json::json!({})),
    ] {
        assert_eq!(
            dispatch(&vault, command, payload, &AppState::default()),
            Err(AppError::Unsupported)
        );
    }
}

#[test]
fn forged_payload_identity_and_nested_revision_fail_closed() {
    use shixu_desktop::{app_state::AppState, commands::dispatch};
    let state = AppState::default();
    assert_eq!(
        dispatch(
            &MAIN,
            "vault_reveal",
            serde_json::json!({"id":"11111111-1111-4111-8111-111111111111","label":"vault","origin":MAIN.origin}),
            &state
        ),
        Err(AppError::AuthFailed)
    );
    assert_eq!(
        dispatch(
            &MAIN,
            "calendar_undo",
            serde_json::json!({"request":{"change_id":"11111111-1111-4111-8111-111111111111","expected_revision":"01"}}),
            &state
        ),
        Err(AppError::InvalidInput)
    );
    assert_eq!(
        dispatch(
            &MAIN,
            "show_vault_window",
            serde_json::json!({"label":"vault"}),
            &state
        ),
        Err(AppError::InvalidInput)
    );
    let vault = CallingContext {
        label: "vault",
        origin: MAIN.origin,
    };
    assert_eq!(
        dispatch(
            &vault,
            "vault_unlock",
            serde_json::json!({"master":vec![7u8;65537]}),
            &state
        ),
        Err(AppError::InvalidInput)
    );
}
#[test]
fn safe_millis_and_all_summary_counts_stay_lossless() {
    use shixu_core::contracts::calendar::ApplySummary;
    let s = wire::WireApplySummary::from(ApplySummary {
        created: u64::MAX,
        updated: 9007199254740993,
        cancelled: 0,
        pending: 1,
        conflicts: 2,
        change_ids: vec![],
    });
    let v = serde_json::to_value(s).unwrap();
    assert_eq!(v["created"], "18446744073709551615");
    assert_eq!(v["updated"], "9007199254740993");
    assert_eq!(v["cancelled"], "0");
    let mut value: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/event.json")).unwrap();
    value["revision"] = serde_json::json!(1);
    value["start_at"] = serde_json::json!(9007199254740992i64);
    let core: CalendarEvent = serde_json::from_value(value).unwrap();
    assert!(matches!(wire::event(core), Err(AppError::InvalidInput)));
}

#[test]
fn d3_commands_are_vault_only_bounded_and_honestly_unsupported() {
    use shixu_desktop::{app_state::AppState, commands::dispatch};
    let vault = CallingContext {
        label: "vault",
        origin: MAIN.origin,
    };
    for (command, payload) in [
        ("vault_create", serde_json::json!({"master":[7,8]})),
        (
            "vault_change_master",
            serde_json::json!({"current":[7],"next":[8]}),
        ),
        (
            "vault_copy",
            serde_json::json!({"id":"11111111-1111-4111-8111-111111111111","field":"password"}),
        ),
    ] {
        assert_eq!(
            dispatch(&MAIN, command, payload.clone(), &AppState::default()),
            Err(AppError::AuthFailed)
        );
        assert_eq!(
            dispatch(&vault, command, payload, &AppState::default()),
            Err(AppError::Unsupported)
        );
    }
    for (command, payload) in [
        ("vault_create", serde_json::json!({"master":[]})),
        (
            "vault_change_master",
            serde_json::json!({"current":[7],"next":[]}),
        ),
        (
            "vault_copy",
            serde_json::json!({"id":"11111111-1111-4111-8111-111111111111","field":"url"}),
        ),
        (
            "vault_copy",
            serde_json::json!({"id":"../secret","field":"account"}),
        ),
        (
            "vault_copy",
            serde_json::json!({"id":"11111111-1111-4111-8111-111111111111","field":"password","session":"forged"}),
        ),
    ] {
        assert_eq!(
            dispatch(&vault, command, payload, &AppState::default()),
            Err(AppError::InvalidInput)
        );
    }
}
