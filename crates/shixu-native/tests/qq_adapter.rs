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
    assert_eq!(a.backfill("c2").unwrap().len(), 1);
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
