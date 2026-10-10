#![cfg(target_os = "linux")]
use serde_json::{Value, json};
use shixu_core::contracts::error::AppError;
use shixu_desktop::{
    app_state::AppState,
    commands::{CallingContext, dispatch, dispatch_published},
};
const VAULT: CallingContext<'static> = CallingContext {
    label: "vault",
    origin: "http://tauri.localhost",
};
#[test]
fn real_dispatcher_create_crud_reopen_change_master_wrong_master_tamper() {
    let repo = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .canonicalize()
        .unwrap();
    let work = repo.join(format!(
        ".superpowers/sdd/shixu-v0.1/task-windows-vault-wiring-dispatch-work-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir(&work).unwrap();
    let state =
        AppState::default().with_vault(repo.join("resources/vault-linux-x64"), work.clone());
    let call = |command, payload| dispatch(&VAULT, command, payload, &state);
    let master = json!({"master":" synthetic master 密码 ".as_bytes()});
    assert_eq!(call("vault_create", master.clone()), Ok(Value::Null));
    assert_eq!(call("vault_list", json!({})), Ok(json!([])));
    let row = call("vault_apply",json!({"mutation":{"operation":"create","channel":" Channel\n含换行\u{0000} ","account":" 用户\r单独\r\n行\n\u{85}\u{2028}\u{2029}/\"\u{0000} ","password":" pass/\"密钥\u{0000} ".as_bytes()}})).unwrap();
    assert_eq!(
        row["account"],
        " 用户\r单独\r\n行\n\u{85}\u{2028}\u{2029}/\"\u{0000} "
    );
    assert_eq!(row["revision"], "1");
    assert_eq!(row.as_object().unwrap().len(), 6);
    assert!(row.get("password").is_none());
    assert_eq!(
        call("vault_reveal", json!({"id":row["entry_id"]})),
        Ok(json!(" pass/\"密钥\u{0000} ".as_bytes()))
    );
    let before = std::fs::read(work.join("vault.kdbx")).unwrap();
    for bad in ["a\r\nb", "a\u{85}b", "a\u{2028}b", "a\u{2029}b"] {
        assert_eq!(
            call(
                "vault_apply",
                json!({"mutation":{"operation":"create","channel":"ch","account":" account\r\n ","password":bad.as_bytes()}})
            ),
            Err(AppError::InvalidInput)
        );
    }
    assert_eq!(std::fs::read(work.join("vault.kdbx")).unwrap(), before);
    let mut submitted = 0;
    assert_eq!(
        dispatch_published(
            &VAULT,
            "vault_reveal",
            json!({"id":row["entry_id"]}),
            &state,
            |value| {
                submitted += 1;
                assert_eq!(value, json!(" pass/\"密钥\u{0000} ".as_bytes()));
                Err::<(), _>(AppError::Disconnected)
            }
        ),
        Err(AppError::Disconnected)
    );
    assert_eq!(submitted, 1);
    assert_eq!(call("vault_list", json!({})), Err(AppError::Locked));
    assert_eq!(call("vault_unlock", master.clone()), Ok(Value::Null));

    assert_eq!(call("vault_activity", json!({})), Ok(Value::Null));
    assert_eq!(
        call(
            "vault_apply",
            json!({"mutation":{"operation":"delete","id":row["entry_id"],"expected_revision":"0"}})
        ),
        Err(AppError::Conflict)
    );
    assert_eq!(call("vault_list", json!({})), Err(AppError::Locked));
    assert_eq!(call("vault_unlock", master.clone()), Ok(Value::Null));
    assert_eq!(call("vault_list", json!({})).unwrap(), json!([row]));
    let updated = call("vault_apply",json!({"mutation":{"operation":"update","id":row["entry_id"],"expected_revision":"1","channel":"new","account":" duplicate ","password":b"new secret"}})).unwrap();
    assert_eq!(updated["revision"], "2");
    let second = call("vault_apply",json!({"mutation":{"operation":"create","channel":"other","account":" duplicate ","password":b"other secret"}})).unwrap();
    assert_eq!(
        call("vault_reveal", json!({"id":row["entry_id"]})),
        Ok(json!(b"new secret"))
    );
    assert_eq!(call("vault_apply",json!({"mutation":{"operation":"delete","id":second["entry_id"],"expected_revision":"1"}})).unwrap()["entry_id"],second["entry_id"]);
    assert_eq!(
        call(
            "vault_change_master",
            json!({"current":" synthetic master 密码 ".as_bytes(),"next":b"new master"})
        ),
        Ok(Value::Null)
    );
    assert_eq!(
        call(
            "vault_copy",
            json!({"id":row["entry_id"],"field":"password"})
        ),
        Err(AppError::Unsupported)
    );
    assert_eq!(call("vault_lock", json!({})), Ok(Value::Null));
    assert_eq!(call("vault_unlock", master), Err(AppError::AuthFailed));
    let next = json!({"master":b"new master"});
    assert_eq!(call("vault_unlock", next.clone()), Ok(Value::Null));
    assert_eq!(call("vault_list", json!({})).unwrap(), json!([updated]));
    // External encrypted-file replacement revokes the service; reveal then denies.
    let valid = std::fs::read(work.join("vault.kdbx")).unwrap();
    let mut tampered = valid.clone();
    let end = tampered.len() - 1;
    tampered[end] ^= 1;
    std::fs::write(work.join("vault.kdbx"), &tampered).unwrap();
    assert_eq!(call("vault_list", json!({})), Err(AppError::Conflict));
    assert_eq!(
        call("vault_reveal", json!({"id":row["entry_id"]})),
        Err(AppError::Locked)
    );
    assert_eq!(call("vault_unlock", next), Err(AppError::AuthFailed));
    std::fs::write(work.join("vault.kdbx"), valid).unwrap();
    drop(state);
    std::fs::remove_dir_all(work).unwrap();
}
#[test]
fn native_newlines_and_invalid_utf8_reject_before_unavailable_engine() {
    let state = AppState::default();
    for bytes in [
        b"a\rb".to_vec(),
        b"a\nb".to_vec(),
        "a\u{85}b".as_bytes().to_vec(),
        "a\u{2028}b".as_bytes().to_vec(),
        "a\u{2029}b".as_bytes().to_vec(),
        vec![255],
    ] {
        assert_eq!(
            dispatch(&VAULT, "vault_create", json!({"master":bytes}), &state),
            Err(AppError::InvalidInput)
        );
    }
}
#[test]
fn main_remote_unknown_denied_before_secret_decode_or_engine() {
    let state = AppState::default();
    for (label, origin) in [
        ("main", VAULT.origin),
        ("vault", "https://remote.example"),
        ("unknown", VAULT.origin),
    ] {
        for command in [
            "vault_create",
            "vault_unlock",
            "vault_list",
            "vault_apply",
            "vault_reveal",
            "vault_change_master",
            "vault_copy",
            "vault_activity",
        ] {
            assert_eq!(
                dispatch(
                    &CallingContext { label, origin },
                    command,
                    json!({"master":"malformed"}),
                    &state
                ),
                Err(AppError::AuthFailed)
            );
        }
    }
}

#[test]
fn unavailable_vault_preserves_calendar_and_qq_runtime_state() {
    use shixu_core::{
        contracts::AppResult,
        storage::{DataProtector, Database},
    };
    use std::sync::Arc;
    struct Synthetic;
    impl DataProtector for Synthetic {
        fn protect(&self, bytes: &[u8]) -> AppResult<Vec<u8>> {
            Ok(bytes.iter().map(|b| b ^ 0xa5).collect())
        }
        fn unprotect(&self, bytes: &[u8]) -> AppResult<Vec<u8>> {
            self.protect(bytes)
        }
    }
    let db =
        Arc::new(Database::open(std::path::Path::new(":memory:"), Arc::new(Synthetic)).unwrap());
    let state = AppState::from_database(db)
        .with_vault("/nonexistent/resources".into(), "/nonexistent/work".into());
    state.runtime().unwrap().resume().unwrap();
    assert!(state.qq().is_ok());
    assert_eq!(
        dispatch(
            &VAULT,
            "vault_create",
            json!({"master":b"synthetic"}),
            &state
        ),
        Err(AppError::Unsupported)
    );
    assert_eq!(
        dispatch(&VAULT, "vault_lock", json!({}), &state),
        Ok(Value::Null)
    );
    assert!(state.runtime().unwrap().status().running);
    assert!(
        state
            .calendar()
            .unwrap()
            .query(shixu_core::contracts::calendar::EventQuery {
                from_date: None,
                through_date: None,
                statuses: vec![],
                include_pending: true
            })
            .unwrap()
            .is_empty()
    );
    state.runtime().unwrap().stop().unwrap();
}
