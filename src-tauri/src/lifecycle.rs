use shixu_core::{
    contracts::{AppResult, vault::LockReason},
    runtime::Supervisor,
};
use std::sync::Arc;
#[derive(Clone, Copy)]
pub enum LifecycleEvent {
    WindowClose,
    Exit,
    SessionLock,
    Suspend,
    Resume,
    LoginStart,
    SecondInstance,
}
pub trait VaultLifecycle: Send + Sync {
    fn lock(&self, reason: LockReason) -> AppResult<()>;
    fn tick(&self, now: i64) -> AppResult<()>;
}
pub trait DesktopLifecycle: Send + Sync {
    fn hide_main(&self) -> AppResult<()>;
    fn focus_main(&self) -> AppResult<()>;
    fn exit(&self) -> AppResult<()>;
}
pub struct Lifecycle {
    pub supervisor: Arc<Supervisor>,
    pub vault: Arc<dyn VaultLifecycle>,
    pub desktop: Arc<dyn DesktopLifecycle>,
}
impl Lifecycle {
    pub fn handle_lifecycle(&self, event: LifecycleEvent, now: i64) -> AppResult<()> {
        match event {
            LifecycleEvent::WindowClose => self.desktop.hide_main(),
            LifecycleEvent::SecondInstance => self.desktop.focus_main(),
            LifecycleEvent::SessionLock => self.vault.lock(LockReason::SessionLock),
            LifecycleEvent::Suspend => {
                let locked = self.vault.lock(LockReason::Suspend);
                let stopped = self.supervisor.suspend(now);
                locked.and(stopped)
            }
            LifecycleEvent::Resume => {
                let locked = self.vault.lock(LockReason::Suspend);
                let resumed = self.supervisor.resume();
                locked.and(resumed)
            }
            LifecycleEvent::LoginStart => {
                let locked = self.vault.lock(LockReason::Manual);
                let started = self.supervisor.login_start();
                locked.and(started)
            }
            LifecycleEvent::Exit => {
                let locked = self.vault.lock(LockReason::Exit);
                let stopped = self.supervisor.stop();
                let exited = self.desktop.exit();
                locked.and(stopped).and(exited)
            }
        }
    }
}
/// No real engine/session dispatcher is configured. Never invent an unlocked state.
pub struct UnavailableVault;
impl VaultLifecycle for UnavailableVault {
    fn lock(&self, _: LockReason) -> AppResult<()> {
        Err(shixu_core::contracts::error::AppError::Unsupported)
    }
    fn tick(&self, _: i64) -> AppResult<()> {
        Err(shixu_core::contracts::error::AppError::Unsupported)
    }
}
