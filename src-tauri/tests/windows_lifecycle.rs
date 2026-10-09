//! Portable lifecycle behavior plus one deliberately blocked OS acceptance gate.
use shixu_core::{
    calendar::EventService,
    contracts::{AppResult, calendar::EventQuery, error::AppError, notification::*, vault::*},
    notifications::consent::{ConsentStore, ModelConsent},
    runtime::Supervisor,
    storage::{DataProtector, Database},
    vault::{VaultService, ports::VaultEngine},
};
use shixu_desktop::lifecycle::*;
use std::sync::{Arc, Mutex};
struct Synthetic;
impl DataProtector for Synthetic {
    fn protect(&self, p: &[u8]) -> AppResult<Vec<u8>> {
        Ok(p.iter().map(|b| b ^ 0xa5).collect())
    }
    fn unprotect(&self, p: &[u8]) -> AppResult<Vec<u8>> {
        self.protect(p)
    }
}
struct Engine;
impl VaultEngine for Engine {
    fn create(&mut self, _: SecretBytes) -> AppResult<()> {
        Ok(())
    }
    fn open(&mut self, _: SecretBytes) -> AppResult<()> {
        Ok(())
    }
    fn close(&mut self) -> AppResult<()> {
        Ok(())
    }
    fn list(&mut self) -> AppResult<Vec<VaultSummary>> {
        Ok(vec![])
    }
    fn apply(&mut self, _: VaultMutation) -> AppResult<VaultSummary> {
        Err(AppError::Unsupported)
    }
    fn read_secret(&mut self, _: &str) -> AppResult<SecretBytes> {
        Err(AppError::Unsupported)
    }
    fn change_master(&mut self, _: SecretBytes, _: SecretBytes) -> AppResult<()> {
        Ok(())
    }
}
struct Vault(Mutex<VaultService<Engine>>);
impl VaultLifecycle for Vault {
    fn lock(&self, r: LockReason) -> AppResult<()> {
        self.0.lock().unwrap().lock(r)
    }
    fn tick(&self, n: i64) -> AppResult<()> {
        self.0.lock().unwrap().tick(n)
    }
}
#[derive(Default)]
struct Desktop(Mutex<Vec<&'static str>>);
impl DesktopLifecycle for Desktop {
    fn hide_main(&self) -> AppResult<()> {
        self.0.lock().unwrap().push("hide");
        Ok(())
    }
    fn focus_main(&self) -> AppResult<()> {
        self.0.lock().unwrap().push("focus");
        Ok(())
    }
    fn exit(&self) -> AppResult<()> {
        self.0.lock().unwrap().push("exit");
        Ok(())
    }
}
fn fixture() -> (
    Arc<Database>,
    SourceConfig,
    Arc<Vault>,
    Arc<Desktop>,
    Lifecycle,
) {
    let db =
        Arc::new(Database::open(std::path::Path::new(":memory:"), Arc::new(Synthetic)).unwrap());
    let c:SourceConfig=serde_json::from_value(serde_json::json!({"source_id":"11111111-1111-4111-8111-111111111111","adapter_type":"synthetic","account_id":"synthetic","allowed_group_ids":["g"],"timezone":"Asia/Shanghai","enabled":true,"capability_set":["live_messages"]})).unwrap();
    let supervisor = Arc::new(Supervisor::new(
        db.clone(),
        Arc::new(ConsentStore::new(ModelConsent::default())),
    ));
    supervisor.start(vec![c.clone()]).unwrap();
    let vault = Arc::new(Vault(Mutex::new(VaultService::new(Engine))));
    vault
        .0
        .lock()
        .unwrap()
        .create(SecretBytes::new(b"synthetic-only".to_vec()), 0)
        .unwrap();
    let desktop = Arc::new(Desktop::default());
    let lifecycle = Lifecycle {
        supervisor,
        vault: vault.clone(),
        desktop: desktop.clone(),
    };
    (db, c, vault, desktop, lifecycle)
}
fn receive(l: &Lifecycle, c: &SourceConfig) {
    let m=serde_json::from_value(serde_json::json!({"message_key":"22222222-2222-4222-8222-222222222222","source_id":c.source_id,"account_id":c.account_id,"group_id":"g","native_message_id":"one","sent_at":1791504000000i64,"received_at":1791504000000i64,"sender_id":"synthetic","text":"2026年10月12日9:00高数考试","reply_to":null,"revision":1,"revoked":false,"processing_state":"persisted","parts":[]})).unwrap();
    l.supervisor.receive(m, "c1").unwrap();
    l.supervisor.process_pending(299999).unwrap();
}
#[test]
fn close_to_tray_vs_exit() {
    let (db, c, v, d, l) = fixture();
    let session = {
        let mut vault = v.0.lock().unwrap();
        vault.lock(LockReason::Manual).unwrap();
        vault
            .unlock(SecretBytes::new(b"synthetic-only".to_vec()), 0)
            .unwrap()
    };
    assert_eq!(v.0.lock().unwrap().status(), VaultStatus::Unlocked);
    l.handle_lifecycle(LifecycleEvent::WindowClose, 10).unwrap();
    assert_eq!(v.0.lock().unwrap().status(), VaultStatus::Locked);
    assert!(v.0.lock().unwrap().list(&session, 11).is_err());
    receive(&l, &c);
    assert_eq!(
        EventService::new(db)
            .query(EventQuery {
                from_date: None,
                through_date: None,
                statuses: vec![],
                include_pending: true
            })
            .unwrap()
            .len(),
        1
    );
    assert_eq!(*d.0.lock().unwrap(), vec!["hide"]);
    assert!(l.supervisor.status().running);
    l.handle_lifecycle(LifecycleEvent::Exit, 20).unwrap();
    assert_eq!(v.0.lock().unwrap().status(), VaultStatus::Locked);
    assert!(!l.supervisor.status().running);
    assert_eq!(*d.0.lock().unwrap(), vec!["hide", "exit"]);
}
#[test]
fn locked_vault_keeps_collection_running() {
    let (db, c, v, _, l) = fixture();
    l.handle_lifecycle(LifecycleEvent::SessionLock, 10).unwrap();
    receive(&l, &c);
    assert_eq!(v.0.lock().unwrap().status(), VaultStatus::Locked);
    assert_eq!(
        EventService::new(db)
            .query(EventQuery {
                from_date: None,
                through_date: None,
                statuses: vec![],
                include_pending: true
            })
            .unwrap()
            .len(),
        1
    );
}
#[test]
fn calendar_activity_does_not_refresh_vault_timer() {
    let (_, c, v, _, l) = fixture();
    receive(&l, &c);
    l.vault.tick(300000).unwrap();
    assert_eq!(v.0.lock().unwrap().status(), VaultStatus::Locked);
}
#[test]
fn resume_requires_vault_unlock_and_records_gap() {
    let (_, _, v, _, l) = fixture();
    l.handle_lifecycle(LifecycleEvent::Suspend, 20).unwrap();
    assert!(!l.supervisor.status().running);
    l.handle_lifecycle(LifecycleEvent::Resume, 30).unwrap();
    assert_eq!(v.0.lock().unwrap().status(), VaultStatus::Locked);
    assert!(l.supervisor.status().running);
    assert!(!l.supervisor.status().sources[0].1.gaps.is_empty());
}
#[test]
fn second_instance_only_focuses_existing() {
    let (_, _, _, d, l) = fixture();
    l.handle_lifecycle(LifecycleEvent::SecondInstance, 10)
        .unwrap();
    assert_eq!(*d.0.lock().unwrap(), vec!["focus"]);
}
#[test]
#[ignore = "requires actual Windows session/power/tray/autostart hooks and engine"]
fn actual_windows_lifecycle_acceptance() {
    panic!(
        "BLOCKED: real Windows lifecycle hooks, vault engine, QQ/parser/provider and OS acceptance not available; portable ports do not satisfy this gate"
    );
}

#[test]
fn close_to_tray_reports_lock_failure_and_keeps_background_running() {
    let (_, _, _, desktop, mut lifecycle) = fixture();
    lifecycle.vault = Arc::new(UnavailableVault);
    assert_eq!(
        lifecycle.handle_lifecycle(LifecycleEvent::WindowClose, 10),
        Err(AppError::Unsupported)
    );
    assert_eq!(*desktop.0.lock().unwrap(), vec!["hide"]);
    assert!(lifecycle.supervisor.status().running);
}
