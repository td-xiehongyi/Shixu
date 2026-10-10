//! Synthetic FakeVaultEngine policy coverage. Does not validate encryption.
#[allow(dead_code)]
mod support;
use shixu_core::{
    contracts::{
        error::AppError,
        vault::{EntryId, LockReason, SecretBytes, SessionId, VaultMutation, VaultStatus},
    },
    vault::VaultService,
};
use std::{cell::Cell, rc::Rc};
use support::fake_vault::FakeVaultEngine;

fn secret(bytes: &[u8]) -> SecretBytes {
    SecretBytes::new(bytes.to_vec())
}
fn master() -> SecretBytes {
    secret(b"SYNTHETIC-MASTER-ONLY")
}
fn create_entry(password: &[u8]) -> VaultMutation {
    VaultMutation::Create {
        channel: "synthetic-channel".into(),
        account: "synthetic-account".into(),
        password: secret(password),
    }
}
fn created() -> (VaultService<FakeVaultEngine>, SessionId) {
    let mut service = VaultService::new(FakeVaultEngine::default());
    assert_eq!(service.status(), VaultStatus::NotCreated);
    let session = service.create(master(), 0).unwrap();
    assert_eq!(service.status(), VaultStatus::Unlocked);
    (service, session)
}
fn assert_error<T>(result: Result<T, AppError>, expected: AppError) {
    // Never Debug-format secret-bearing success values, even on assertion failure.
    assert!(matches!(result, Err(error) if error == expected));
}

#[test]
fn locked_rejects_every_secret_operation() {
    let (mut service, session) = created();
    let row = service
        .apply(&session, create_entry(b"SYNTHETIC-ONLY"), 0)
        .unwrap();
    service.lock(LockReason::Manual).unwrap();
    assert_eq!(service.list(&session, 1), Err(AppError::Locked));
    assert_eq!(
        service.apply(&session, create_entry(b"SYNTHETIC-ONLY"), 1),
        Err(AppError::Locked)
    );
    assert_error(
        service.reveal(&session, &row.entry_id.to_string(), 1),
        AppError::Locked,
    );
    assert_eq!(
        service.change_master(&session, master(), secret(b"SYNTHETIC-NEXT")),
        Err(AppError::Locked)
    );
    assert_eq!(service.status(), VaultStatus::Locked);
}

#[test]
fn duplicate_channel_account_uses_distinct_ids() {
    let (mut service, session) = created();
    let first = service
        .apply(&session, create_entry(b"SYNTHETIC-A"), 1)
        .unwrap();
    let second = service
        .apply(&session, create_entry(b"SYNTHETIC-B"), 2)
        .unwrap();
    assert_ne!(first.entry_id, second.entry_id);
    assert_eq!(service.list(&session, 3).unwrap().len(), 2);
}

#[test]
fn password_is_not_trimmed() {
    let (mut service, session) = created();
    for bytes in [&b"  SYNTHETIC-ONLY "[..], &b" \t"[..], &[0, 255, b' '][..]] {
        let row = service.apply(&session, create_entry(bytes), 1).unwrap();
        let revealed = service
            .reveal(&session, &row.entry_id.to_string(), 1)
            .unwrap();
        assert!(revealed.expose() == bytes);
    }
}

#[test]
fn background_does_not_extend_timeout() {
    let (mut service, session) = created();
    service.tick(299_999).unwrap();
    assert_eq!(service.status(), VaultStatus::Unlocked);
    service.tick(300_000).unwrap();
    assert_eq!(service.status(), VaultStatus::Locked);
    assert_eq!(service.list(&session, 300_000), Err(AppError::Locked));
}

#[test]
fn late_reply_after_lock_is_discarded() {
    let (mut service, old_session) = created();
    let row = service
        .apply(&old_session, create_entry(b"SYNTHETIC-ONLY"), 0)
        .unwrap();
    // A fake completion holds a real owned secret and a test-only destruction spy.
    struct DelayedReply {
        _secret: SecretBytes,
        dropped: Rc<Cell<bool>>,
    }
    impl Drop for DelayedReply {
        fn drop(&mut self) {
            self.dropped.set(true);
        }
    }
    let dropped = Rc::new(Cell::new(false));
    let late_reply = DelayedReply {
        _secret: service
            .reveal(&old_session, &row.entry_id.to_string(), 1)
            .unwrap(),
        dropped: dropped.clone(),
    };
    service.lock(LockReason::Manual).unwrap();
    let new_session = service.unlock(master(), 2).unwrap();
    assert_error(
        service.accept_reply(&old_session, Ok(late_reply)),
        AppError::Locked,
    );
    assert!(dropped.get());
    assert!(old_session != new_session);
    assert_eq!(service.status(), VaultStatus::Unlocked);
    assert_eq!(service.list(&old_session, 3), Err(AppError::Locked));
    assert_eq!(service.accept_reply(&new_session, Ok(7)), Ok(7));
    assert_eq!(service.list(&new_session, 3).unwrap().len(), 1);
}

#[test]
fn late_reply_in_locked_state_is_discarded() {
    let (mut service, session) = created();
    service.lock(LockReason::Manual).unwrap();
    assert_error(
        service.accept_reply(&session, Ok(secret(b"SYNTHETIC-ONLY"))),
        AppError::Locked,
    );
    assert_eq!(service.status(), VaultStatus::Locked);
}

#[test]
fn wrong_password_stays_locked_and_does_not_reuse_session() {
    let (mut service, old_session) = created();
    service.lock(LockReason::Manual).unwrap();
    assert_error(
        service.unlock(secret(b"SYNTHETIC-WRONG"), 10),
        AppError::AuthFailed,
    );
    assert_eq!(service.status(), VaultStatus::Locked);
    assert_eq!(service.list(&old_session, 11), Err(AppError::Locked));
    let session = service.unlock(master(), 12).unwrap();
    assert!(session != old_session);
}

#[test]
fn master_and_password_empty_bytes_are_invalid() {
    let mut service = VaultService::new(FakeVaultEngine::default());
    assert_error(service.create(secret(b""), 0), AppError::InvalidInput);
    assert_eq!(service.status(), VaultStatus::Locked);
    let session = service.create(master(), 1).unwrap();
    assert_eq!(
        service.apply(&session, create_entry(b""), 2),
        Err(AppError::InvalidInput)
    );
    assert_eq!(
        service.change_master(&session, master(), secret(b"")),
        Err(AppError::InvalidInput)
    );
    assert_eq!(
        service.change_master(&session, secret(b""), master()),
        Err(AppError::InvalidInput)
    );
    assert!(service.list(&session, 2).unwrap().is_empty());
    service.lock(LockReason::Manual).unwrap();
    assert_error(service.unlock(secret(b""), 3), AppError::InvalidInput);
    assert_eq!(service.status(), VaultStatus::Locked);
}

#[test]
fn empty_channel_and_account_are_rejected_before_engine_mutation() {
    let (mut service, session) = created();
    for (channel, account) in [
        ("", "synthetic"),
        (" \n", "synthetic"),
        ("synthetic", ""),
        ("synthetic", " \t"),
    ] {
        let mutation = VaultMutation::Create {
            channel: channel.into(),
            account: account.into(),
            password: secret(b"SYNTHETIC-ONLY"),
        };
        assert_eq!(
            service.apply(&session, mutation, 1),
            Err(AppError::InvalidInput)
        );
    }
    assert!(service.list(&session, 2).unwrap().is_empty());
}

#[test]
fn revision_conflicts_cannot_update_or_delete_records() {
    let (mut service, session) = created();
    let row = service
        .apply(&session, create_entry(b"SYNTHETIC-ORIGINAL"), 0)
        .unwrap();
    let mutation = VaultMutation::Update {
        id: row.entry_id,
        expected_revision: 0,
        channel: "synthetic-new".into(),
        account: "synthetic-new".into(),
        password: secret(b"SYNTHETIC-REPLACEMENT"),
    };
    assert_eq!(
        service.apply(&session, mutation, 1),
        Err(AppError::Conflict)
    );
    assert_eq!(
        service.apply(
            &session,
            VaultMutation::Delete {
                id: row.entry_id,
                expected_revision: 0
            },
            1
        ),
        Err(AppError::Conflict)
    );
    assert_eq!(service.list(&session, 2).unwrap(), vec![row.clone()]);
    assert!(
        service
            .reveal(&session, &row.entry_id.to_string(), 2)
            .unwrap()
            .expose()
            == b"SYNTHETIC-ORIGINAL"
    );
    let updated = service
        .apply(
            &session,
            VaultMutation::Update {
                id: row.entry_id,
                expected_revision: row.revision,
                channel: "synthetic-new".into(),
                account: "synthetic-new".into(),
                password: secret(b" SYNTHETIC-REPLACEMENT "),
            },
            3,
        )
        .unwrap();
    assert_eq!(updated.revision, row.revision + 1);
    assert!(
        service
            .reveal(&session, &row.entry_id.to_string(), 3)
            .unwrap()
            .expose()
            == b" SYNTHETIC-REPLACEMENT "
    );
    let deleted = service
        .apply(
            &session,
            VaultMutation::Delete {
                id: updated.entry_id,
                expected_revision: updated.revision,
            },
            4,
        )
        .unwrap();
    assert_eq!(deleted, updated);
    assert!(service.list(&session, 5).unwrap().is_empty());
}

#[test]
fn query_summaries_do_not_contain_password() {
    let (mut service, session) = created();
    service
        .apply(&session, create_entry(b"SYNTHETIC-ONLY"), 0)
        .unwrap();
    let summary = serde_json::to_value(service.list(&session, 1).unwrap()).unwrap();
    assert!(summary[0].get("password").is_none());
    assert!(!summary.to_string().contains("SYNTHETIC-ONLY"));
}

#[test]
fn successful_interaction_refreshes_timeout() {
    let (mut service, session) = created();
    service.list(&session, 299_999).unwrap();
    service.tick(300_000).unwrap();
    assert_eq!(service.status(), VaultStatus::Unlocked);
    service.tick(599_999).unwrap();
    assert_eq!(service.status(), VaultStatus::Locked);
}

#[test]
fn every_timed_operation_checks_expiry_without_a_prior_tick() {
    for operation in 0..3 {
        let (mut service, session) = created();
        let row = service
            .apply(&session, create_entry(b"SYNTHETIC-ONLY"), 0)
            .unwrap();
        match operation {
            0 => assert_eq!(service.list(&session, 300_000), Err(AppError::Locked)),
            1 => assert_eq!(
                service.apply(&session, create_entry(b"SYNTHETIC-ONLY"), 300_000),
                Err(AppError::Locked)
            ),
            _ => assert_error(
                service.reveal(&session, &row.entry_id.to_string(), 300_000),
                AppError::Locked,
            ),
        }
        assert_eq!(service.status(), VaultStatus::Locked);
    }
}

#[test]
fn failed_close_is_fail_closed_and_requires_cleanup_before_reopen() {
    let engine = FakeVaultEngine::default();
    let faults = engine.faults.clone();
    let mut service = VaultService::new(engine);
    let session = service.create(master(), 0).unwrap();
    faults.borrow_mut().close_error = Some(AppError::Disconnected);
    assert_eq!(
        service.lock(LockReason::Manual),
        Err(AppError::Disconnected)
    );
    assert_eq!(service.status(), VaultStatus::Locked);
    assert_eq!(service.list(&session, 1), Err(AppError::Locked));
    assert_error(
        service.accept_reply(&session, Ok(secret(b"SYNTHETIC-ONLY"))),
        AppError::Locked,
    );
    assert_error(service.unlock(master(), 2), AppError::Disconnected);
    assert_eq!(service.status(), VaultStatus::Locked);
    faults.borrow_mut().close_error = None;
    let new_session = service.unlock(master(), 3).unwrap();
    assert!(session != new_session);
}

#[test]
fn every_lock_reason_revokes_session() {
    for reason in [LockReason::Manual, LockReason::Timeout, LockReason::Exit] {
        let (mut service, session) = created();
        service.lock(reason).unwrap();
        service.lock(reason).unwrap();
        assert_eq!(service.list(&session, 1), Err(AppError::Locked));
        assert_eq!(service.status(), VaultStatus::Locked);
    }
}

#[test]
fn change_master_preserves_entry_secrets_and_rejects_old_master() {
    let (mut service, session) = created();
    let row = service
        .apply(&session, create_entry(b" SYNTHETIC-ONLY "), 0)
        .unwrap();
    service
        .change_master(&session, master(), secret(b" SYNTHETIC-NEXT "))
        .unwrap();
    service.lock(LockReason::Manual).unwrap();
    assert_error(service.unlock(master(), 1), AppError::AuthFailed);
    let session = service.unlock(secret(b" SYNTHETIC-NEXT "), 2).unwrap();
    assert!(
        service
            .reveal(&session, &row.entry_id.to_string(), 2)
            .unwrap()
            .expose()
            == b" SYNTHETIC-ONLY "
    );
}

#[test]
fn failed_master_change_stays_locked_and_preserves_original_master() {
    let (mut service, session) = created();
    assert_eq!(
        service.change_master(
            &session,
            secret(b"SYNTHETIC-WRONG"),
            secret(b"SYNTHETIC-NEXT")
        ),
        Err(AppError::AuthFailed)
    );
    assert_eq!(service.status(), VaultStatus::Locked);
    assert!(service.unlock(master(), 1).is_ok());
}

#[test]
fn engine_error_invalidates_session_and_keeps_fixed_error_code() {
    let engine = FakeVaultEngine::default();
    let faults = engine.faults.clone();
    let mut service = VaultService::new(engine);
    let session = service.create(master(), 0).unwrap();
    faults.borrow_mut().read_error = Some(AppError::Disconnected);
    assert_error(
        service.reveal(&session, "00000000-0000-0000-0000-000000000001", 1),
        AppError::Disconnected,
    );
    assert_eq!(service.status(), VaultStatus::Locked);
    assert_eq!(AppError::Disconnected.to_string(), "DISCONNECTED");
}

#[test]
fn invalid_ids_and_missing_revisions_do_not_mutate_records() {
    let (mut service, session) = created();
    assert_error(
        service.reveal(&session, "invalid", 1),
        AppError::InvalidInput,
    );
    let id = EntryId::from_uuid(uuid::Uuid::nil());
    assert_eq!(
        service.apply(
            &session,
            VaultMutation::Delete {
                id,
                expected_revision: 1
            },
            2
        ),
        Err(AppError::InvalidInput)
    );
    assert!(service.list(&session, 3).unwrap().is_empty());
}

#[test]
fn backward_and_extreme_clocks_do_not_extend_session() {
    let (mut service, session) = created();
    service.list(&session, 10).unwrap();
    assert_eq!(service.list(&session, 9), Err(AppError::InvalidInput));
    assert_eq!(service.tick(9), Err(AppError::InvalidInput));
    service.tick(300_010).unwrap();
    assert_eq!(service.status(), VaultStatus::Locked);
    let mut service = VaultService::new(FakeVaultEngine::default());
    service.create(master(), i64::MIN).unwrap();
    service.tick(i64::MAX).unwrap();
    assert_eq!(service.status(), VaultStatus::Locked);
}

#[test]
fn unsuccessful_interactions_do_not_refresh_timeout() {
    let (mut service, session) = created();
    assert_eq!(
        service.apply(&session, create_entry(b""), 299_999),
        Err(AppError::InvalidInput)
    );
    assert_error(
        service.reveal(&session, "invalid", 299_999),
        AppError::InvalidInput,
    );
    service.tick(300_000).unwrap();
    assert_eq!(service.status(), VaultStatus::Locked);
}

#[test]
fn failed_unlock_from_unlocked_state_revokes_previous_session() {
    let (mut service, session) = created();
    assert_error(
        service.unlock(secret(b"SYNTHETIC-WRONG"), 1),
        AppError::AuthFailed,
    );
    assert_eq!(service.status(), VaultStatus::Locked);
    assert_eq!(service.list(&session, 2), Err(AppError::Locked));
}

#[test]
fn different_services_cannot_accept_each_others_sessions() {
    let (mut first, first_session) = created();
    let (mut second, second_session) = created();
    assert!(first_session != second_session);
    assert_eq!(first.list(&second_session, 1), Err(AppError::Locked));
    assert_eq!(second.list(&first_session, 1), Err(AppError::Locked));
}

#[test]
fn tick_observation_cannot_be_rewound_by_vault_interaction() {
    let (mut service, session) = created();
    service.tick(200_000).unwrap();
    assert_eq!(service.list(&session, 100_000), Err(AppError::InvalidInput));
    service.tick(300_000).unwrap();
    assert_eq!(service.status(), VaultStatus::Locked);
}

#[test]
fn successful_apply_and_reveal_refresh_timeout() {
    let (mut service, session) = created();
    let row = service
        .apply(&session, create_entry(b"SYNTHETIC-ONLY"), 299_999)
        .unwrap();
    service.tick(300_000).unwrap();
    assert_eq!(service.status(), VaultStatus::Unlocked);
    let revealed = service
        .reveal(&session, &row.entry_id.to_string(), 599_998)
        .unwrap();
    assert!(revealed.expose() == b"SYNTHETIC-ONLY");
    service.tick(599_999).unwrap();
    assert_eq!(service.status(), VaultStatus::Unlocked);
    service.tick(899_998).unwrap();
    assert_eq!(service.status(), VaultStatus::Locked);
}

#[test]
fn apply_engine_failure_cannot_publish_success_or_keep_session() {
    let engine = FakeVaultEngine::default();
    let faults = engine.faults.clone();
    let mut service = VaultService::new(engine);
    let session = service.create(master(), 0).unwrap();
    faults.borrow_mut().apply_error = Some(AppError::StorageFull);
    assert_eq!(
        service.apply(&session, create_entry(b"SYNTHETIC-ONLY"), 1),
        Err(AppError::StorageFull)
    );
    assert_eq!(service.status(), VaultStatus::Locked);
    faults.borrow_mut().apply_error = None;
    let session = service.unlock(master(), 2).unwrap();
    assert!(service.list(&session, 3).unwrap().is_empty());
}

#[test]
fn timeout_close_error_still_revokes_authority() {
    let engine = FakeVaultEngine::default();
    let faults = engine.faults.clone();
    let mut service = VaultService::new(engine);
    let session = service.create(master(), 0).unwrap();
    faults.borrow_mut().close_error = Some(AppError::Disconnected);
    assert_eq!(service.tick(300_000), Err(AppError::Disconnected));
    assert_eq!(service.status(), VaultStatus::Locked);
    assert_eq!(service.list(&session, 300_001), Err(AppError::Locked));
    assert_error(
        service.accept_reply(&session, Ok(secret(b"SYNTHETIC-ONLY"))),
        AppError::Locked,
    );
}

#[test]
fn newlines_allowed_in_account_but_rejected_in_master_password_without_trimming() {
    for nl in ["\r", "\n", "\u{85}", "\u{2028}", "\u{2029}"] {
        let mut service = VaultService::new(FakeVaultEngine::default());
        assert_error(
            service.create(secret(format!("master{nl}x").as_bytes()), 0),
            AppError::InvalidInput,
        );
        let session = service.create(master(), 0).unwrap();
        assert_eq!(
            service.apply(
                &session,
                VaultMutation::Create {
                    channel: "channel\nallowed".into(),
                    account: " account ".into(),
                    password: secret(format!("p{nl}x").as_bytes())
                },
                1
            ),
            Err(AppError::InvalidInput)
        );
        assert_eq!(
            service.change_master(&session, master(), secret(format!("m{nl}x").as_bytes())),
            Err(AppError::InvalidInput)
        );
        assert!(service.list(&session, 2).unwrap().is_empty());
        let row = service
            .apply(
                &session,
                VaultMutation::Create {
                    channel: "channel\nallowed".into(),
                    account: format!(" account{nl}x "),
                    password: secret(b" password "),
                },
                3,
            )
            .unwrap();
        assert_eq!(row.channel, "channel\nallowed");
        assert_eq!(row.account, format!(" account{nl}x "));
        assert!(
            service
                .reveal(&session, &row.entry_id.to_string(), 4)
                .unwrap()
                .expose()
                == b" password "
        );
    }
}

#[test]
fn explicit_authorized_activity_refreshes_but_background_tick_does_not() {
    let (mut service, session) = created();
    service.activity(&session, 299_999).unwrap();
    service.tick(300_000).unwrap();
    assert_eq!(service.status(), VaultStatus::Unlocked);
    service.tick(599_999).unwrap();
    assert_eq!(service.status(), VaultStatus::Locked);
}
#[test]
fn old_session_activity_cannot_refresh_reopened_session() {
    let (mut service, old) = created();
    service.lock(LockReason::Manual).unwrap();
    let _new = service.unlock(master(), 1).unwrap();
    assert_eq!(service.activity(&old, 299_999), Err(AppError::Locked));
    service.tick(300_001).unwrap();
    assert_eq!(service.status(), VaultStatus::Locked);
}
