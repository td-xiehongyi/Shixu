#![cfg(target_os = "linux")]
use shixu_core::{
    contracts::{
        error::AppError,
        vault::{SecretBytes, VaultMutation},
    },
    vault::ports::VaultEngine,
};
use shixu_native::vault::engine::KdbxWebEngine;
use std::path::PathBuf;
fn secret(s: &str) -> SecretBytes {
    SecretBytes::new(s.as_bytes().to_vec())
}
fn setup() -> (KdbxWebEngine, PathBuf) {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let work = repo
        .join(".superpowers/sdd/shixu-v0.1")
        .join(format!("task-kdbxweb-engine-work-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&work).unwrap();
    (
        KdbxWebEngine::prepared(repo.join("resources/vault-linux-x64"), work.clone()).unwrap(),
        work,
    )
}
#[test]
fn real_create_crud_reopen_change_master_conflict() {
    let (mut e, work) = setup();
    e.create(secret(" synthetic master 密码 ")).unwrap();
    let a = e
        .apply(VaultMutation::Create {
            channel: " 渠道/\"\n名字 ".into(),
            account: " 用户/\" ".into(),
            password: secret(" 密码/\"🗝️ "),
        })
        .unwrap();
    let b = e
        .apply(VaultMutation::Create {
            channel: a.channel.clone(),
            account: a.account.clone(),
            password: secret(" 密码/\"🗝️ "),
        })
        .unwrap();
    assert_ne!(a.entry_id, b.entry_id);
    assert_eq!(a.revision, 1);
    assert_eq!(e.list().unwrap().len(), 2);
    assert_eq!(
        e.read_secret(&a.entry_id.to_string()).unwrap().expose(),
        " 密码/\"🗝️ ".as_bytes()
    );
    e.close().unwrap();
    assert!(matches!(
        e.read_secret(&a.entry_id.to_string()),
        Err(AppError::Locked)
    ));
    e.open(secret(" synthetic master 密码 ")).unwrap();
    assert_eq!(e.list().unwrap()[0], a);
    assert_eq!(
        e.apply(VaultMutation::Delete {
            id: a.entry_id,
            expected_revision: 0
        }),
        Err(AppError::Conflict)
    );
    e.open(secret(" synthetic master 密码 ")).unwrap();
    let updated = e
        .apply(VaultMutation::Update {
            id: a.entry_id,
            expected_revision: 1,
            channel: "新渠道".into(),
            account: "新账号 ".into(),
            password: secret("新密码 "),
        })
        .unwrap();
    assert_eq!(updated.revision, 2);
    e.apply(VaultMutation::Delete {
        id: b.entry_id,
        expected_revision: 1,
    })
    .unwrap();
    let old = std::fs::read(work.join("vault.kdbx")).unwrap();
    e.change_master(secret(" synthetic master 密码 "), secret(" next master "))
        .unwrap();
    assert_eq!(
        std::fs::read(work.join("vault.previous.kdbx")).unwrap(),
        old
    );
    e.close().unwrap();
    assert_eq!(
        e.open(secret(" synthetic master 密码 ")),
        Err(AppError::AuthFailed)
    );
    e.open(secret(" next master ")).unwrap();
    assert_eq!(e.list().unwrap(), vec![updated]);
    let data = std::fs::read(work.join("vault.kdbx")).unwrap();
    assert_eq!(&data[..8], &[3, 217, 162, 154, 103, 251, 75, 181]);
    for canary in ["新密码", "next master", "新账号"] {
        assert!(!data.windows(canary.len()).any(|w| w == canary.as_bytes()));
    }
    e.close().unwrap();
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let mut decoder =
        std::process::Command::new(repo.join("resources/vault-linux-x64/runtime/node"))
            .env_clear()
            .arg(repo.join("vault-helper/tests/upstream-decode.mjs"))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
    let input = serde_json::json!({"path":work.join("vault.kdbx"),"master":" next master ","entries":[{"id":a.entry_id.to_string(),"channel":"新渠道","account":"新账号 ","password":"新密码 "}]});
    std::io::Write::write_all(
        &mut decoder.stdin.take().unwrap(),
        &serde_json::to_vec(&input).unwrap(),
    )
    .unwrap();
    let result = decoder.wait_with_output().unwrap();
    assert!(result.status.success());
    assert_eq!(result.stdout, b"UPSTREAM_DECODE_OK");
    std::fs::remove_dir_all(work).unwrap();
}
#[test]
fn rejects_newlines_wrong_master_tamper_and_external_edit() {
    let (mut e, work) = setup();
    for nl in ["\r", "\n", "\u{85}", "\u{2028}", "\u{2029}"] {
        assert_eq!(
            e.create(secret(&format!("m{nl}x"))),
            Err(AppError::InvalidInput)
        );
        assert!(!work.join("vault.kdbx").exists());
    }
    e.create(secret("synthetic-master")).unwrap();
    let before = std::fs::read(work.join("vault.kdbx")).unwrap();
    for nl in ["\r", "\n", "\u{85}", "\u{2028}", "\u{2029}"] {
        e.open(secret("synthetic-master")).unwrap();
        assert_eq!(
            e.apply(VaultMutation::Create {
                channel: "c".into(),
                account: format!("a{nl}z"),
                password: secret("p")
            }),
            Err(AppError::InvalidInput)
        );
        assert_eq!(std::fs::read(work.join("vault.kdbx")).unwrap(), before);
        e.open(secret("synthetic-master")).unwrap();
        assert_eq!(
            e.apply(VaultMutation::Create {
                channel: "c".into(),
                account: "a".into(),
                password: secret(&format!("p{nl}z"))
            }),
            Err(AppError::InvalidInput)
        );
    }
    e.close().unwrap();
    assert_eq!(e.open(secret("wrong")), Err(AppError::AuthFailed));
    let mut tampered = before.clone();
    let last = tampered.len() - 1;
    tampered[last] ^= 1;
    std::fs::write(work.join("vault.kdbx"), &tampered).unwrap();
    assert_eq!(
        e.open(secret("synthetic-master")),
        Err(AppError::AuthFailed)
    );
    std::fs::write(work.join("vault.kdbx"), &before[..20]).unwrap();
    assert_eq!(
        e.open(secret("synthetic-master")),
        Err(AppError::AuthFailed)
    );
    std::fs::write(work.join("vault.kdbx"), &before).unwrap();
    e.open(secret("synthetic-master")).unwrap();
    std::fs::write(work.join("vault.kdbx"), tampered).unwrap();
    assert_eq!(e.list(), Err(AppError::Conflict));
    assert_eq!(e.list(), Err(AppError::Locked));
    e.close().unwrap();
    std::fs::remove_dir_all(work).unwrap();
}

#[test]
fn create_existing_wrong_current_storage_failure_and_cancellation_preserve_ciphertext() {
    let (mut e, work) = setup();
    e.create(secret("master")).unwrap();
    let before = std::fs::read(work.join("vault.kdbx")).unwrap();
    assert_eq!(e.create(secret("another")), Err(AppError::Conflict));
    assert_eq!(std::fs::read(work.join("vault.kdbx")).unwrap(), before);
    e.open(secret("master")).unwrap();
    assert_eq!(
        e.change_master(secret("wrong"), secret("next")),
        Err(AppError::AuthFailed)
    );
    assert_eq!(std::fs::read(work.join("vault.kdbx")).unwrap(), before);
    e.open(secret("master")).unwrap();
    std::fs::create_dir(work.join("vault.pending.kdbx")).unwrap();
    assert_eq!(
        e.apply(VaultMutation::Create {
            channel: "c".into(),
            account: "a".into(),
            password: secret("p")
        }),
        Err(AppError::AuthFailed)
    );
    assert_eq!(std::fs::read(work.join("vault.kdbx")).unwrap(), before);
    std::fs::remove_dir(work.join("vault.pending.kdbx")).unwrap();
    e.open(secret("master")).unwrap();
    std::fs::create_dir(work.join("vault.previous.kdbx")).unwrap();
    assert_eq!(
        e.apply(VaultMutation::Create {
            channel: "c".into(),
            account: "a".into(),
            password: secret("p")
        }),
        Err(AppError::AuthFailed)
    );
    assert_eq!(std::fs::read(work.join("vault.kdbx")).unwrap(), before);
    std::fs::remove_dir(work.join("vault.previous.kdbx")).unwrap();
    e.open(secret("master")).unwrap();
    let handle = e.cancellation().unwrap();
    handle.cancel();
    assert_eq!(e.list(), Err(AppError::AuthFailed));
    assert_eq!(e.list(), Err(AppError::Locked));
    assert_eq!(std::fs::read(work.join("vault.kdbx")).unwrap(), before);
    e.close().unwrap();
    std::fs::remove_dir_all(work).unwrap();
}
#[test]
fn missing_altered_resources_and_symlink_paths_fail_closed() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let work = repo.join(".superpowers/sdd/shixu-v0.1").join(format!(
        "task-kdbxweb-engine-invalid-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir(&work).unwrap();
    assert!(matches!(
        KdbxWebEngine::prepared(work.join("missing"), work.clone()),
        Err(AppError::Unsupported)
    ));
    let altered = work.join("altered");
    std::fs::create_dir(&altered).unwrap();
    let nested_work = work.join("work");
    std::fs::create_dir(&nested_work).unwrap();
    let manifest: serde_json::Value = serde_json::from_str(include_str!(
        "../../../vault-helper/resources-linux-x64.json"
    ))
    .unwrap();
    for relative in manifest["files"].as_object().unwrap().keys() {
        let target = altered.join(relative);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::hard_link(
            repo.join("resources/vault-linux-x64").join(relative),
            target,
        )
        .unwrap();
    }
    let changed = altered.join("helper/crypto.mjs");
    std::fs::remove_file(&changed).unwrap();
    std::fs::write(changed, b"altered public resource").unwrap();
    assert!(matches!(
        KdbxWebEngine::prepared(altered, nested_work),
        Err(AppError::Unsupported)
    ));
    std::os::unix::fs::symlink(repo.join("resources/vault-linux-x64"), work.join("link")).unwrap();
    assert!(matches!(
        KdbxWebEngine::prepared(work.join("link"), work.clone()),
        Err(AppError::Unsupported)
    ));
    std::fs::remove_dir_all(work).unwrap();
}

#[test]
fn aggregate_list_budget_rejects_escaped_create_and_update_without_write() {
    let (mut engine, work) = setup();
    engine.create(secret("synthetic-budget")).unwrap();
    let channel = "\u{1}".repeat(65536);
    let account = "\"".repeat(65536);
    let first = engine
        .apply(VaultMutation::Create {
            channel: channel.clone(),
            account: account.clone(),
            password: secret("synthetic"),
        })
        .unwrap();
    let second = engine
        .apply(VaultMutation::Create {
            channel: "small".into(),
            account: "small".into(),
            password: secret("synthetic"),
        })
        .unwrap();
    let accepted = vec![first, second.clone()];
    assert_eq!(engine.list().unwrap(), accepted);
    let before = std::fs::read(work.join("vault.kdbx")).unwrap();
    assert!(matches!(
        engine.apply(VaultMutation::Update {
            id: second.entry_id,
            expected_revision: second.revision,
            channel: channel.clone(),
            account: account.clone(),
            password: secret("next")
        }),
        Err(AppError::Unsupported)
    ));
    assert_eq!(std::fs::read(work.join("vault.kdbx")).unwrap(), before);
    assert!(!work.join("vault.pending.kdbx").exists());
    engine.open(secret("synthetic-budget")).unwrap();
    assert_eq!(engine.list().unwrap(), accepted);
    assert!(matches!(
        engine.apply(VaultMutation::Create {
            channel,
            account,
            password: secret("synthetic")
        }),
        Err(AppError::Unsupported)
    ));
    assert_eq!(std::fs::read(work.join("vault.kdbx")).unwrap(), before);
    assert!(!work.join("vault.pending.kdbx").exists());
    engine.open(secret("synthetic-budget")).unwrap();
    assert_eq!(engine.list().unwrap(), accepted);
    engine.close().unwrap();
    std::fs::remove_dir_all(work).unwrap();
}
