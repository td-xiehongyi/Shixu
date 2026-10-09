use shixu_core::{
    contracts::{AppResult, error::AppError, notification::*, vault::SecretBytes},
    notifications::{AppendOutcome, MessageStore, source::*},
    storage::{DataProtector, Database},
};
use shixu_native::qq::{adapter::*, transport::*};
use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
use uuid::Uuid;
struct SyntheticProtection(Arc<AtomicBool>);
impl DataProtector for SyntheticProtection {
    fn protect(&self, p: &[u8]) -> AppResult<Vec<u8>> {
        if self.0.load(Ordering::SeqCst) {
            Err(AppError::StorageFull)
        } else {
            Ok(p.to_vec())
        }
    }
    fn unprotect(&self, p: &[u8]) -> AppResult<Vec<u8>> {
        Ok(p.to_vec())
    }
}
// TEST ONLY normalized transport: never performs network I/O, login or download.
struct SyntheticTransport {
    caps: Vec<SourceCapability>,
    events: VecDeque<AppResult<Option<Delivery>>>,
    batches: VecDeque<AppResult<BackfillBatch>>,
    calls: Arc<AtomicUsize>,
    auth_fail: bool,
}
impl ReceiveTransport for SyntheticTransport {
    fn connect(
        &mut self,
        _: &LoopbackEndpoint,
        _: &SourceConfig,
        token: SecretBytes,
    ) -> AppResult<Vec<SourceCapability>> {
        assert_eq!(token.expose(), b"synthetic-only-token");
        if self.auth_fail {
            Err(AppError::AuthFailed)
        } else {
            Ok(self.caps.clone())
        }
    }
    fn disconnect(&mut self) -> AppResult<()> {
        Ok(())
    }
    fn next(&mut self) -> AppResult<Option<Delivery>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.events.pop_front().unwrap_or(Ok(None))
    }
    fn backfill(&mut self, _: &str, _: &str) -> AppResult<BackfillBatch> {
        self.batches
            .pop_front()
            .unwrap_or(Err(AppError::Unsupported))
    }
}
fn config() -> SourceConfig {
    SourceConfig {
        source_id: SourceId::from_uuid(Uuid::from_u128(16)),
        adapter_type: "synthetic-test-only".into(),
        account_id: "synthetic-A".into(),
        allowed_group_ids: vec!["synthetic-group".into()],
        timezone: "Etc/UTC".into(),
        enabled: true,
        capability_set: serde_json::from_str(include_str!(
            "../../../tests/fixtures/qq/capabilities.json"
        ))
        .unwrap(),
    }
}
fn message() -> MessageEnvelope {
    serde_json::from_str(include_str!("../../../tests/fixtures/qq/messages.json")).unwrap()
}
fn delivery(cursor: &str) -> Delivery {
    Delivery {
        message: message(),
        cursor: cursor.into(),
    }
}
fn token() -> SecretBytes {
    SecretBytes::new(b"synthetic-only-token".to_vec())
}
fn setup(
    events: Vec<AppResult<Option<Delivery>>>,
    batches: Vec<AppResult<BackfillBatch>>,
    caps: Vec<SourceCapability>,
    auth_fail: bool,
) -> (
    NativeQQAdapter<SyntheticTransport>,
    Arc<AtomicBool>,
    Arc<AtomicUsize>,
) {
    let fail = Arc::new(AtomicBool::new(false));
    let calls = Arc::new(AtomicUsize::new(0));
    let db = Database::open(
        std::path::Path::new(":memory:"),
        Arc::new(SyntheticProtection(fail.clone())),
    )
    .unwrap();
    let t = SyntheticTransport {
        caps,
        events: events.into(),
        batches: batches.into(),
        calls: calls.clone(),
        auth_fail,
    };
    (
        NativeQQAdapter::new(
            t,
            LoopbackEndpoint::parse("127.0.0.1:3001").unwrap(),
            MessageStore::new(Arc::new(db)),
            || 100,
        ),
        fail,
        calls,
    )
}
fn connected(
    events: Vec<AppResult<Option<Delivery>>>,
    batches: Vec<AppResult<BackfillBatch>>,
) -> (
    NativeQQAdapter<SyntheticTransport>,
    Arc<AtomicBool>,
    Arc<AtomicUsize>,
) {
    let (mut a, f, c) = setup(events, batches, config().capability_set, false);
    a.connect(config(), token()).unwrap();
    (a, f, c)
}
#[test]
fn loopback_only_and_no_credentials_in_endpoint() {
    for v in ["127.0.0.1:1", "127.255.0.1:65535", "[::1]:3001"] {
        assert!(
            LoopbackEndpoint::parse(v)
                .unwrap()
                .address()
                .ip()
                .is_loopback()
        );
    }
    for v in [
        "0.0.0.0:1",
        "192.0.2.1:1",
        "[::]:1",
        "localhost:1",
        "127.0.0.1:0",
        "http://127.0.0.1:1",
        "user:token@127.0.0.1:1",
        "127.0.0.1:65536",
    ] {
        assert!(matches!(
            LoopbackEndpoint::parse(v),
            Err(AppError::InvalidInput)
        ));
    }
}
#[test]
fn heartbeat_is_not_group_verification() {
    let (mut a, _, _) = connected(vec![Ok(None)], vec![]);
    assert_eq!(a.next_message(), Ok(None));
    let h = a.health();
    assert_eq!(h.connection_state, ConnectionState::Connected);
    assert_eq!(h.last_connected_at, Some(100));
    assert_eq!(h.last_received_at, None);
    assert!(!h.ordinary_group_verified);
}
#[test]
fn auth_failure_waits_for_login() {
    let (mut a, _, calls) = setup(
        vec![Ok(Some(delivery("c1")))],
        vec![],
        config().capability_set,
        true,
    );
    assert_eq!(a.connect(config(), token()), Err(AppError::AuthFailed));
    assert_eq!(
        a.health().connection_state,
        ConnectionState::WaitingForLogin
    );
    assert_eq!(a.next_message(), Err(AppError::AuthFailed));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(a.health().retry_delay(0, 0), None);
}
#[test]
fn overlap_replay_is_idempotent() {
    let (mut a, _, _) = connected(
        vec![Ok(Some(delivery("c1"))), Ok(Some(delivery("c2")))],
        vec![],
    );
    a.next_message().unwrap().unwrap();
    assert_eq!(a.health().last_received_at, Some(100));
    assert_eq!(a.health().last_persisted_at, None);
    assert_eq!(a.next_message(), Err(AppError::Conflict));
    assert_eq!(a.persist_pending(), Ok(AppendOutcome::Stored));
    assert_eq!(a.health().last_persisted_at, Some(100));
    assert_eq!(a.health().last_applied_at, None);
    a.next_message().unwrap();
    assert_eq!(a.persist_pending(), Ok(AppendOutcome::Duplicate));
}
#[test]
fn persistence_failure_retains_delivery_and_gap() {
    let (mut a, fail, calls) = connected(vec![Ok(Some(delivery("c1")))], vec![]);
    a.next_message().unwrap();
    fail.store(true, Ordering::SeqCst);
    assert_eq!(a.persist_pending(), Err(AppError::StorageFull));
    assert_eq!(a.health().last_persisted_at, None);
    assert!(!a.health().gaps.is_empty());
    assert_eq!(a.next_message(), Err(AppError::Conflict));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    fail.store(false, Ordering::SeqCst);
    assert_eq!(a.persist_pending(), Ok(AppendOutcome::Stored));
}
#[test]
fn whitelist_and_account_filtered_before_persistence() {
    let mut m = message();
    m.account_id = "synthetic-other".into();
    let mut g = message();
    g.group_id = "forbidden".into();
    let (mut a, _, _) = connected(
        vec![
            Ok(Some(Delivery {
                message: m,
                cursor: "x".into(),
            })),
            Ok(Some(Delivery {
                message: g,
                cursor: "y".into(),
            })),
            Ok(Some(delivery("z"))),
        ],
        vec![],
    );
    assert_eq!(a.next_message(), Ok(None));
    assert_eq!(a.persist_pending(), Err(AppError::Conflict));
    assert_eq!(a.next_message(), Ok(None));
    assert_eq!(a.health().last_persisted_at, None);
    assert!(a.next_message().unwrap().is_some());
    assert_eq!(a.persist_pending(), Ok(AppendOutcome::Stored));
}
#[test]
fn backfill_keeps_gap_when_unsupported() {
    let (mut a, _, _) = setup(
        vec![Ok(Some(delivery("c1"))), Err(AppError::Disconnected)],
        vec![],
        vec![SourceCapability::LiveMessages],
        false,
    );
    a.connect(config(), token()).unwrap();
    a.next_message().unwrap();
    a.persist_pending().unwrap();
    assert_eq!(a.next_message(), Err(AppError::Disconnected));
    a.connect(config(), token()).unwrap();
    assert_eq!(a.backfill("c1"), Err(AppError::Unsupported));
    assert!(!a.health().gaps.is_empty());
}
#[test]
fn only_complete_scoped_backfill_clears_gap() {
    let batches = vec![
        Ok(BackfillBatch {
            group_id: "synthetic-group".into(),
            deliveries: vec![delivery("c2")],
            complete: false,
        }),
        Ok(BackfillBatch {
            group_id: "synthetic-group".into(),
            deliveries: vec![delivery("c3")],
            complete: true,
        }),
    ];
    let (mut a, _, _) = connected(
        vec![Ok(Some(delivery("c1"))), Err(AppError::Disconnected)],
        batches,
    );
    a.next_message().unwrap();
    a.persist_pending().unwrap();
    a.next_message().unwrap_err();
    a.connect(config(), token()).unwrap();
    assert_eq!(a.backfill("foreign"), Err(AppError::InvalidInput));
    assert_eq!(a.backfill("c1").unwrap().len(), 1);
    assert!(!a.health().gaps.is_empty());
    assert_eq!(a.backfill("c2"), Err(AppError::InvalidInput));
    assert_eq!(a.backfill("c1").unwrap().len(), 1);
    assert!(a.health().gaps.is_empty());
    assert!(!a.health().ordinary_group_verified);
}
#[test]
fn account_switch_keeps_cursor_namespace() {
    let (mut a, _, _) = connected(vec![Ok(Some(delivery("c1")))], vec![]);
    a.next_message().unwrap();
    a.persist_pending().unwrap();
    let mut b = config();
    b.account_id = "synthetic-B".into();
    assert_eq!(a.connect(b.clone(), token()), Err(AppError::InvalidInput));
    b.source_id = SourceId::from_uuid(Uuid::from_u128(17));
    a.connect(b, token()).unwrap();
    assert_eq!(a.health().last_persisted_at, None);
    assert_eq!(a.backfill("c1"), Err(AppError::InvalidInput));
}
#[test]
fn bad_config_token_and_capability_mismatch_are_rejected() {
    let (mut a, _, _) = setup(vec![], vec![], vec![SourceCapability::Edits], false);
    assert_eq!(
        a.connect(config(), SecretBytes::new(vec![])),
        Err(AppError::InvalidInput)
    );
    let mut c = config();
    c.enabled = false;
    assert_eq!(a.connect(c, token()), Err(AppError::InvalidInput));
    assert_eq!(a.connect(config(), token()), Err(AppError::Unsupported));
    assert_eq!(a.health().connection_state, ConnectionState::Incompatible);
}
#[test]
fn stable_native_attachment_refs_are_scoped_and_f0_valid() {
    let c = config();
    let r = stable_attachment_ref(&c, "synthetic-group", "native/file-id").unwrap();
    assert_eq!(
        r,
        stable_attachment_ref(&c, "synthetic-group", "native/file-id").unwrap()
    );
    let mut b = c.clone();
    b.account_id = "synthetic-B".into();
    assert_ne!(
        r,
        stable_attachment_ref(&b, "synthetic-group", "native/file-id").unwrap()
    );
    assert_ne!(
        r,
        stable_attachment_ref(&c, "synthetic-group", "native/other-id").unwrap()
    );
    assert_eq!(
        stable_attachment_ref(&c, "synthetic-group", "https://synthetic.invalid/temp"),
        Err(AppError::InvalidInput)
    );
    assert_eq!(
        stable_attachment_ref(&c, "synthetic-group", ""),
        Err(AppError::InvalidInput)
    );
}
#[test]
fn wrong_group_backfill_never_clears_gap() {
    let (mut a, _, _) = connected(
        vec![Ok(Some(delivery("c1"))), Err(AppError::Disconnected)],
        vec![Ok(BackfillBatch {
            group_id: "foreign".into(),
            deliveries: vec![],
            complete: true,
        })],
    );
    a.next_message().unwrap();
    a.persist_pending().unwrap();
    a.next_message().unwrap_err();
    a.connect(config(), token()).unwrap();
    assert_eq!(a.backfill("c1"), Err(AppError::InvalidInput));
    assert!(!a.health().gaps.is_empty());
}
#[test]
fn received_envelope_has_canonical_store_identity() {
    let (mut a, _, _) = connected(vec![Ok(Some(delivery("c1")))], vec![]);
    let m = a.next_message().unwrap().unwrap();
    assert_eq!(
        m.message_key,
        shixu_core::notifications::identity::message_identity(&config(), &m)
            .unwrap()
            .key
    );
    assert_eq!(m.received_at, 100);
    a.persist_pending().unwrap();
}
#[test]
fn applied_time_is_explicit_and_scoped() {
    let (mut a, _, _) = connected(vec![Ok(Some(delivery("c1")))], vec![]);
    a.next_message().unwrap();
    a.persist_pending().unwrap();
    assert_eq!(a.health().last_applied_at, None);
    assert_eq!(
        a.report_applied(SourceId::from_uuid(Uuid::from_u128(17)), 200),
        Err(AppError::InvalidInput)
    );
    a.report_applied(config().source_id, 200).unwrap();
    assert_eq!(a.health().last_applied_at, Some(200));
    assert_eq!(a.health().last_persisted_at, Some(100));
}
#[test]
fn disconnect_clears_partial_recovery_proof() {
    let mut c = config();
    c.allowed_group_ids.push("synthetic-second".into());
    let mut m = message();
    m.group_id = "synthetic-second".into();
    let (mut a, _, _) = setup(
        vec![
            Ok(Some(delivery("c1"))),
            Ok(Some(Delivery {
                message: m,
                cursor: "s1".into(),
            })),
            Err(AppError::Disconnected),
        ],
        vec![
            Ok(BackfillBatch {
                group_id: "synthetic-group".into(),
                deliveries: vec![],
                complete: true,
            }),
            Ok(BackfillBatch {
                group_id: "synthetic-second".into(),
                deliveries: vec![],
                complete: true,
            }),
        ],
        config().capability_set,
        false,
    );
    a.connect(c.clone(), token()).unwrap();
    for _ in 0..2 {
        a.next_message().unwrap();
        a.persist_pending().unwrap();
    }
    a.next_message().unwrap_err();
    a.connect(c.clone(), token()).unwrap();
    a.backfill("c1").unwrap();
    assert!(!a.health().gaps.is_empty());
    a.connect(c, token()).unwrap();
    a.backfill("s1").unwrap();
    assert!(!a.health().gaps.is_empty());
}
#[test]
fn pending_ack_prevents_reconfiguration_and_backfill() {
    let (mut a, _, _) = connected(vec![Ok(Some(delivery("c1")))], vec![]);
    a.next_message().unwrap();
    assert_eq!(a.connect(config(), token()), Err(AppError::Conflict));
    assert_eq!(a.backfill("c1"), Err(AppError::Conflict));
    assert_eq!(a.persist_pending(), Ok(AppendOutcome::Stored));
}
#[test]
fn receive_auth_and_incompatibility_errors_pause_transport() {
    for error in [AppError::AuthFailed, AppError::Unsupported] {
        let (mut a, _, calls) = connected(vec![Err(error), Ok(Some(delivery("c1")))], vec![]);
        assert_eq!(a.next_message(), Err(error));
        assert_eq!(a.next_message(), Err(error));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(a.health().last_received_at, None);
        assert!(!a.health().gaps.is_empty());
        assert_eq!(a.health().retry_delay(0, 0), None);
    }
}
#[test]
fn incomplete_group_backfill_keeps_other_groups_gap() {
    let mut c = config();
    c.allowed_group_ids.push("synthetic-second".into());
    let (mut a, _, _) = setup(
        vec![Ok(Some(delivery("c1"))), Err(AppError::Disconnected)],
        vec![Ok(BackfillBatch {
            group_id: "synthetic-group".into(),
            deliveries: vec![],
            complete: true,
        })],
        config().capability_set,
        false,
    );
    a.connect(c.clone(), token()).unwrap();
    a.next_message().unwrap();
    a.persist_pending().unwrap();
    a.next_message().unwrap_err();
    a.connect(c, token()).unwrap();
    a.backfill("c1").unwrap();
    assert!(!a.health().gaps.is_empty());
}
#[test]
fn backfill_persistence_failure_keeps_gap() {
    let (mut a, fail, _) = connected(
        vec![Ok(Some(delivery("c1"))), Err(AppError::Disconnected)],
        vec![Ok(BackfillBatch {
            group_id: "synthetic-group".into(),
            deliveries: vec![{
                let mut d = delivery("c2");
                d.message.native_message_id = "synthetic-n2".into();
                d
            }],
            complete: true,
        })],
    );
    a.next_message().unwrap();
    a.persist_pending().unwrap();
    a.next_message().unwrap_err();
    a.connect(config(), token()).unwrap();
    fail.store(true, Ordering::SeqCst);
    assert_eq!(a.backfill("c1"), Err(AppError::StorageFull));
    assert!(!a.health().gaps.is_empty());
    assert_eq!(a.health().last_persisted_at, Some(100));
}
#[test]
fn live_cursor_does_not_skip_original_missing_interval() {
    let (mut a, _, _) = connected(
        vec![
            Ok(Some(delivery("c1"))),
            Err(AppError::Disconnected),
            Ok(Some({
                let mut d = delivery("c3");
                d.message.native_message_id = "synthetic-n3".into();
                d
            })),
        ],
        vec![Ok(BackfillBatch {
            group_id: "synthetic-group".into(),
            deliveries: vec![{
                let mut d = delivery("c2");
                d.message.native_message_id = "synthetic-n2".into();
                d
            }],
            complete: true,
        })],
    );
    a.next_message().unwrap();
    a.persist_pending().unwrap();
    a.next_message().unwrap_err();
    a.connect(config(), token()).unwrap();
    a.next_message().unwrap();
    a.persist_pending().unwrap();
    assert_eq!(a.backfill("c3"), Err(AppError::InvalidInput));
    assert!(!a.health().gaps.is_empty());
    assert_eq!(a.backfill("c1").unwrap().len(), 1);
    assert!(a.health().gaps.is_empty());
}
#[test]
fn partial_recovery_and_live_receive_keep_original_recovery_start() {
    let (mut a, _, _) = connected(
        vec![
            Ok(Some(delivery("c1"))),
            Err(AppError::Disconnected),
            Ok(Some({
                let mut d = delivery("c4");
                d.message.native_message_id = "n4".into();
                d
            })),
        ],
        vec![
            Ok(BackfillBatch {
                group_id: "synthetic-group".into(),
                deliveries: vec![delivery("c2")],
                complete: false,
            }),
            Ok(BackfillBatch {
                group_id: "synthetic-group".into(),
                deliveries: vec![delivery("c3")],
                complete: true,
            }),
        ],
    );
    a.next_message().unwrap();
    a.persist_pending().unwrap();
    a.next_message().unwrap_err();
    a.connect(config(), token()).unwrap();
    a.backfill("c1").unwrap();
    a.next_message().unwrap();
    a.persist_pending().unwrap();
    assert_eq!(a.backfill("c4"), Err(AppError::InvalidInput));
    assert!(!a.health().gaps.is_empty());
    assert_eq!(a.backfill("c2"), Err(AppError::InvalidInput));
    a.backfill("c1").unwrap();
    assert!(a.health().gaps.is_empty());
}
#[test]
fn source_switch_preserves_unresolved_gap_without_backfill() {
    let (mut a, _, _) = setup(
        vec![Ok(Some(delivery("c1"))), Err(AppError::Disconnected)],
        vec![],
        vec![SourceCapability::LiveMessages],
        false,
    );
    a.connect(config(), token()).unwrap();
    a.next_message().unwrap();
    a.persist_pending().unwrap();
    a.next_message().unwrap_err();
    let mut b = config();
    b.source_id = SourceId::from_uuid(Uuid::from_u128(17));
    b.account_id = "synthetic-B".into();
    a.connect(b, token()).unwrap();
    a.connect(config(), token()).unwrap();
    assert!(!a.health().gaps.is_empty());
    assert_eq!(a.backfill("c1"), Err(AppError::Unsupported));
    assert!(!a.health().gaps.is_empty());
}
fn reopened_adapter(
    path: &std::path::Path,
    events: Vec<AppResult<Option<Delivery>>>,
) -> NativeQQAdapter<SyntheticTransport> {
    let db = Database::open(
        path,
        Arc::new(SyntheticProtection(Arc::new(AtomicBool::new(false)))),
    )
    .unwrap();
    let t = SyntheticTransport {
        caps: vec![SourceCapability::LiveMessages],
        events: events.into(),
        batches: VecDeque::new(),
        calls: Arc::new(AtomicUsize::new(0)),
        auth_fail: false,
    };
    NativeQQAdapter::new(
        t,
        LoopbackEndpoint::parse("127.0.0.1:3001").unwrap(),
        MessageStore::new(Arc::new(db)),
        || 100,
    )
}
#[test]
fn restart_preserves_gap_and_durable_immutable_source_binding() {
    let path = std::env::temp_dir().join(format!("shixu-n2-native-{}.db", Uuid::new_v4()));
    {
        let mut a = reopened_adapter(
            &path,
            vec![Ok(Some(delivery("c1"))), Err(AppError::Disconnected)],
        );
        a.connect(config(), token()).unwrap();
        a.next_message().unwrap();
        a.persist_pending().unwrap();
        a.next_message().unwrap_err();
    }
    for i in 0..4 {
        let mut a = reopened_adapter(&path, vec![]);
        let mut c = config();
        match i {
            0 => c.account_id = "synthetic-B".into(),
            1 => c.adapter_type = "other".into(),
            2 => c.allowed_group_ids.push("second".into()),
            _ => c.timezone = "Asia/Shanghai".into(),
        };
        assert_eq!(a.connect(c, token()), Err(AppError::InvalidInput));
    }
    {
        let mut a = reopened_adapter(&path, vec![]);
        a.connect(config(), token()).unwrap();
        assert!(!a.health().gaps.is_empty());
        assert_eq!(a.backfill("c1"), Err(AppError::Unsupported));
        assert!(!a.health().gaps.is_empty());
    }
    std::fs::remove_file(path).unwrap();
}
#[test]
fn equal_group_cursors_have_independent_scoped_recovery_handles() {
    let mut c = config();
    c.allowed_group_ids.push("synthetic-second".into());
    let mut second = delivery("1");
    second.message.group_id = "synthetic-second".into();
    let (mut a, _, _) = setup(
        vec![
            Ok(Some(delivery("1"))),
            Ok(Some(second)),
            Err(AppError::Disconnected),
        ],
        vec![
            Ok(BackfillBatch {
                group_id: "synthetic-group".into(),
                deliveries: vec![],
                complete: true,
            }),
            Ok(BackfillBatch {
                group_id: "synthetic-second".into(),
                deliveries: vec![],
                complete: true,
            }),
        ],
        config().capability_set,
        false,
    );
    a.connect(c.clone(), token()).unwrap();
    for _ in 0..2 {
        a.next_message().unwrap();
        a.persist_pending().unwrap();
    }
    a.next_message().unwrap_err();
    a.connect(c, token()).unwrap();
    let first = a.recovery_handle("synthetic-group").unwrap();
    let second = a.recovery_handle("synthetic-second").unwrap();
    assert_ne!(first, second);
    a.backfill(&first).unwrap();
    assert!(!a.health().gaps.is_empty());
    a.backfill(&second).unwrap();
    assert!(a.health().gaps.is_empty());
}
#[test]
fn failed_recovery_write_never_hides_disconnection_gap() {
    let path = std::env::temp_dir().join(format!("shixu-n2-gap-fault-{}.db", Uuid::new_v4()));
    {
        let mut a = reopened_adapter(&path, vec![Ok(Some(delivery("c1")))]);
        a.connect(config(), token()).unwrap();
        a.next_message().unwrap();
        a.persist_pending().unwrap();
        let sql = rusqlite::Connection::open(&path).unwrap();
        sql.execute_batch("CREATE TRIGGER synthetic_reject_recovery BEFORE INSERT ON source_recovery BEGIN SELECT RAISE(ABORT,'synthetic'); END;").unwrap();
        a.disconnect().unwrap();
        assert!(!a.health().gaps.is_empty());
        assert_eq!(a.health().connection_state, ConnectionState::Disconnected);
        sql.execute_batch("DROP TRIGGER synthetic_reject_recovery;")
            .unwrap();
    }
    {
        let mut a = reopened_adapter(&path, vec![]);
        a.connect(config(), token()).unwrap();
        assert!(!a.health().gaps.is_empty());
    }
    std::fs::remove_file(path).unwrap();
}
#[test]
fn stale_scoped_handle_is_invalidated_on_reconnect() {
    let (mut a, _, _) = connected(
        vec![Ok(Some(delivery("c1"))), Err(AppError::Disconnected)],
        vec![],
    );
    a.next_message().unwrap();
    a.persist_pending().unwrap();
    a.next_message().unwrap_err();
    a.connect(config(), token()).unwrap();
    let old = a.recovery_handle("synthetic-group").unwrap();
    a.connect(config(), token()).unwrap();
    let current = a.recovery_handle("synthetic-group").unwrap();
    assert_ne!(old, current);
    assert_eq!(a.backfill(&old), Err(AppError::InvalidInput));
    assert!(!a.health().gaps.is_empty());
}
#[test]
fn restart_without_clean_disconnect_requires_conservative_recovery() {
    let path = std::env::temp_dir().join(format!("shixu-n2-crash-{}.db", Uuid::new_v4()));
    {
        let mut a = reopened_adapter(&path, vec![Ok(Some(delivery("c1")))]);
        a.connect(config(), token()).unwrap();
        a.next_message().unwrap();
        a.persist_pending().unwrap();
        assert!(a.health().gaps.is_empty());
    }
    {
        let mut a = reopened_adapter(&path, vec![]);
        a.connect(config(), token()).unwrap();
        assert!(!a.health().gaps.is_empty());
        assert_eq!(a.backfill("c1"), Err(AppError::Unsupported));
    }
    std::fs::remove_file(path).unwrap();
}
#[test]
fn pending_retry_cannot_advance_live_cursor_before_failed_gap_write_recovers() {
    let path = std::env::temp_dir().join(format!("shixu-n2-gap-retry-{}.db", Uuid::new_v4()));
    {
        let mut live = delivery("c3");
        live.message.native_message_id = "n3".into();
        let mut a = reopened_adapter(&path, vec![Ok(Some(delivery("c1"))), Ok(Some(live))]);
        a.connect(config(), token()).unwrap();
        a.next_message().unwrap();
        a.persist_pending().unwrap();
        a.next_message().unwrap();
        let sql = rusqlite::Connection::open(&path).unwrap();
        sql.execute_batch("CREATE TRIGGER synthetic_reject_cursor BEFORE UPDATE OF cursor ON sources BEGIN SELECT RAISE(ABORT,'synthetic'); END; CREATE TRIGGER synthetic_reject_recovery BEFORE INSERT ON source_recovery BEGIN SELECT RAISE(ABORT,'synthetic'); END;").unwrap();
        assert_eq!(a.persist_pending(), Err(AppError::Conflict));
        sql.execute_batch("DROP TRIGGER synthetic_reject_cursor;")
            .unwrap();
        assert_eq!(a.persist_pending(), Err(AppError::Conflict));
        let cursor: String = sql
            .query_row("SELECT cursor FROM sources", [], |r| r.get(0))
            .unwrap();
        assert_eq!(cursor, "c1");
        sql.execute_batch("DROP TRIGGER synthetic_reject_recovery;")
            .unwrap();
        assert_eq!(a.persist_pending(), Ok(AppendOutcome::Stored));
        let anchor: String = sql
            .query_row("SELECT anchor FROM source_recovery", [], |r| r.get(0))
            .unwrap();
        assert_eq!(anchor, "c1");
    }
    std::fs::remove_file(path).unwrap();
}
fn recovery_adapter_at(
    path: &std::path::Path,
    events: Vec<AppResult<Option<Delivery>>>,
    batches: Vec<AppResult<BackfillBatch>>,
) -> NativeQQAdapter<SyntheticTransport> {
    let db = Database::open(
        path,
        Arc::new(SyntheticProtection(Arc::new(AtomicBool::new(false)))),
    )
    .unwrap();
    let t = SyntheticTransport {
        caps: config().capability_set,
        events: events.into(),
        batches: batches.into(),
        calls: Arc::new(AtomicUsize::new(0)),
        auth_fail: false,
    };
    NativeQQAdapter::new(
        t,
        LoopbackEndpoint::parse("127.0.0.1:3001").unwrap(),
        MessageStore::new(Arc::new(db)),
        || 100,
    )
}
#[test]
fn noncontiguous_partial_replays_original_interval_after_reopen_and_live_arrival() {
    let path = std::env::temp_dir().join(format!("shixu-n2-noncontiguous-{}.db", Uuid::new_v4()));
    {
        let mut c3 = delivery("c3");
        c3.message.native_message_id = "n3".into();
        let mut a = recovery_adapter_at(
            &path,
            vec![Ok(Some(delivery("c1"))), Err(AppError::Disconnected)],
            vec![Ok(BackfillBatch {
                group_id: "synthetic-group".into(),
                deliveries: vec![c3],
                complete: false,
            })],
        );
        a.connect(config(), token()).unwrap();
        a.next_message().unwrap();
        a.persist_pending().unwrap();
        a.next_message().unwrap_err();
        a.connect(config(), token()).unwrap();
        assert_eq!(a.backfill("c1").unwrap().len(), 1);
        assert!(!a.health().gaps.is_empty());
        let sql = rusqlite::Connection::open(&path).unwrap();
        let count: i64 = sql
            .query_row("SELECT count(*) FROM messages", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2);
        let start: String = sql
            .query_row("SELECT recovery_cursor FROM source_recovery", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(start, "c1");
        // Simulate already persisted round1 partial metadata. Reopen must use
        // anchor c1 even if this historical diagnostic cursor was advanced.
        sql.execute("UPDATE source_recovery SET recovery_cursor='c3'", [])
            .unwrap();
        // Persisting n3 proves no coverage for the omitted n2. Close/reopen must not
        // permit a truthfully complete suffix after c3 to stand in for the old interval.
    }
    {
        let mut c4 = delivery("c4");
        c4.message.native_message_id = "n4".into();
        let mut c2 = delivery("c2");
        c2.message.native_message_id = "n2".into();
        let mut c3 = delivery("c3");
        c3.message.native_message_id = "n3".into();
        let mut a = recovery_adapter_at(
            &path,
            vec![Ok(Some(c4))],
            vec![Ok(BackfillBatch {
                group_id: "synthetic-group".into(),
                deliveries: vec![c2, c3],
                complete: true,
            })],
        );
        a.connect(config(), token()).unwrap();
        a.next_message().unwrap();
        a.persist_pending().unwrap();
        assert_eq!(a.backfill("c3"), Err(AppError::InvalidInput));
        assert_eq!(a.backfill("c4"), Err(AppError::InvalidInput));
        assert!(!a.health().gaps.is_empty());
        assert_eq!(a.backfill("c1").unwrap().len(), 2);
        assert!(a.health().gaps.is_empty());
        let sql = rusqlite::Connection::open(&path).unwrap();
        let count: i64 = sql
            .query_row("SELECT count(*) FROM messages", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 4);
    }
    std::fs::remove_file(path).unwrap();
}
#[test]
fn d5_native_receive_port_retains_delivery_until_database_commit() {
    use shixu_core::{
        notifications::consent::{ConsentStore, ModelConsent},
        runtime::{Supervisor, workers::ReceivePort},
    };
    let c = config();
    let fail = Arc::new(AtomicBool::new(false));
    let db = Arc::new(
        Database::open(
            std::path::Path::new(":memory:"),
            Arc::new(SyntheticProtection(fail.clone())),
        )
        .unwrap(),
    );
    let calls = Arc::new(AtomicUsize::new(0));
    let transport = SyntheticTransport {
        caps: c.capability_set.clone(),
        events: vec![Ok(Some(delivery("cursor-one")))].into(),
        batches: VecDeque::new(),
        calls: calls.clone(),
        auth_fail: false,
    };
    let mut adapter = NativeQQAdapter::new(
        transport,
        LoopbackEndpoint::parse("127.0.0.1:3001").unwrap(),
        MessageStore::new(db.clone()),
        || 100,
    );
    QQAdapter::connect(&mut adapter, c.clone(), token()).unwrap();
    let supervisor = Supervisor::new(
        db.clone(),
        Arc::new(ConsentStore::new(ModelConsent::default())),
    );
    supervisor.start(vec![c.clone()]).unwrap();
    let first = ReceivePort::poll(&mut adapter)
        .unwrap()
        .expect("normalized native delivery");
    assert_eq!(
        MessageStore::new(db.clone())
            .cursor(&c, &first.message.group_id)
            .unwrap(),
        None
    );
    fail.store(true, Ordering::SeqCst);
    assert_eq!(
        supervisor.receive(first.message.clone(), &first.cursor),
        Err(AppError::StorageFull)
    );
    let retry = ReceivePort::poll(&mut adapter).unwrap().unwrap();
    assert_eq!(retry.cursor, first.cursor);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    fail.store(false, Ordering::SeqCst);
    supervisor
        .receive(retry.message.clone(), &retry.cursor)
        .unwrap();
    ReceivePort::acknowledge(&mut adapter, &retry.cursor).unwrap();
    assert!(ReceivePort::poll(&mut adapter).unwrap().is_none());
    assert_eq!(
        MessageStore::new(db)
            .cursor(&c, &retry.message.group_id)
            .unwrap(),
        Some("cursor-one".into())
    );
}

#[test]
fn d5_native_ack_uses_authoritative_commit_after_source_edit() {
    use shixu_core::{
        notifications::{
            consent::{ConsentStore, ModelConsent},
            settings::SettingsStore,
        },
        runtime::{Supervisor, workers::ReceivePort},
    };
    let mut c = config();
    let fail = Arc::new(AtomicBool::new(false));
    let db = Arc::new(
        Database::open(
            std::path::Path::new(":memory:"),
            Arc::new(SyntheticProtection(fail)),
        )
        .unwrap(),
    );
    SettingsStore::new(db.clone())
        .save_source(c.clone())
        .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let transport = SyntheticTransport {
        caps: c.capability_set.clone(),
        events: vec![
            Ok(Some(delivery("cursor-one"))),
            Ok(Some(delivery("cursor-two"))),
        ]
        .into(),
        batches: VecDeque::new(),
        calls: calls.clone(),
        auth_fail: false,
    };
    let mut adapter = NativeQQAdapter::new(
        transport,
        LoopbackEndpoint::parse("127.0.0.1:3001").unwrap(),
        MessageStore::new(db.clone()),
        || 100,
    );
    QQAdapter::connect(&mut adapter, c.clone(), token()).unwrap();
    let supervisor = Supervisor::new(
        db.clone(),
        Arc::new(ConsentStore::new(ModelConsent::default())),
    );
    supervisor.start(vec![c.clone()]).unwrap();
    let first = ReceivePort::poll(&mut adapter).unwrap().unwrap();
    c.timezone = "Asia/Shanghai".into();
    SettingsStore::new(db.clone())
        .save_source(c.clone())
        .unwrap();
    supervisor.refresh_sources().unwrap();
    assert_eq!(
        supervisor
            .receive(first.message.clone(), &first.cursor)
            .unwrap(),
        AppendOutcome::Stored
    );
    assert_eq!(
        MessageStore::new(db.clone())
            .cursor(&c, &first.message.group_id)
            .unwrap(),
        Some("cursor-one".into())
    );
    assert_eq!(
        ReceivePort::acknowledge(&mut adapter, "wrong-cursor"),
        Err(AppError::Conflict)
    );
    c.allowed_group_ids = vec!["other-group".into()];
    SettingsStore::new(db.clone())
        .save_source(c.clone())
        .unwrap();
    assert_eq!(
        ReceivePort::acknowledge(&mut adapter, &first.cursor),
        Err(AppError::Conflict)
    );
    assert_eq!(
        ReceivePort::poll(&mut adapter).unwrap().unwrap().cursor,
        first.cursor
    );
    c.allowed_group_ids = vec![first.message.group_id.clone()];
    c.enabled = false;
    SettingsStore::new(db.clone())
        .save_source(c.clone())
        .unwrap();
    assert_eq!(
        ReceivePort::acknowledge(&mut adapter, &first.cursor),
        Err(AppError::Conflict)
    );
    c.enabled = true;
    SettingsStore::new(db.clone())
        .save_source(c.clone())
        .unwrap();
    let ack = ReceivePort::acknowledge(&mut adapter, &first.cursor);
    let next = ReceivePort::poll(&mut adapter).unwrap();
    println!(
        "ack={ack:?}, still_same_pending={}, transport_polls={}",
        next.as_ref().is_some_and(|d| d.cursor == first.cursor),
        calls.load(Ordering::SeqCst)
    );
    assert_eq!(
        ack,
        Ok(()),
        "successful authoritative commit must not become permanently unacknowledgeable after source settings edit"
    );
    let next = next.expect("advance to next delivery after authoritative acknowledgment");
    assert_eq!(next.cursor, "cursor-two");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        ReceivePort::acknowledge(&mut adapter, &next.cursor),
        Err(AppError::Conflict)
    );
    assert_eq!(
        supervisor
            .receive(next.message.clone(), &next.cursor)
            .unwrap(),
        AppendOutcome::Duplicate
    );
    ReceivePort::acknowledge(&mut adapter, &next.cursor).unwrap();
    assert!(ReceivePort::poll(&mut adapter).unwrap().is_none());
    assert_eq!(
        MessageStore::new(db)
            .cursor(&c, &next.message.group_id)
            .unwrap(),
        Some("cursor-two".into())
    );
}
