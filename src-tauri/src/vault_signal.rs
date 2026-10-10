//! Atomic admission barrier. Notification never takes a controller/engine lock.
use shixu_core::contracts::{AppResult, error::AppError};
pub use shixu_native::vault::notifications::NativeVaultEvent;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    mpsc::{SyncSender, TrySendError},
};
const SESSION: usize = 1;
const POWER: usize = 2;
const FAILED: usize = 4;
const REGISTRATION: usize = 8;
pub struct VaultEventSignal {
    generation: AtomicU64,
    blocked: AtomicUsize,
    pending: AtomicUsize,
    wake: SyncSender<()>,
    alive: AtomicBool,
}
impl VaultEventSignal {
    pub(crate) fn new(wake: SyncSender<()>) -> Self {
        Self {
            generation: AtomicU64::new(0),
            blocked: AtomicUsize::new(0),
            pending: AtomicUsize::new(0),
            wake,
            alive: AtomicBool::new(true),
        }
    }
    pub fn notify(&self, event: NativeVaultEvent) {
        let bits = match event {
            NativeVaultEvent::SessionLocked => {
                self.blocked.fetch_or(SESSION, Ordering::AcqRel);
                9
            }
            NativeVaultEvent::SessionUnlocked => {
                self.blocked.fetch_and(!SESSION, Ordering::AcqRel);
                17
            }
            NativeVaultEvent::Suspend => {
                self.blocked.fetch_or(POWER, Ordering::AcqRel);
                3
            }
            NativeVaultEvent::Resume => {
                if self.blocked.fetch_and(!POWER, Ordering::AcqRel) & POWER == 0 {
                    return;
                }
                5
            }
            NativeVaultEvent::WindowClosed => 33,
            NativeVaultEvent::Revoked => 1,
        };
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.pending.fetch_or(bits, Ordering::AcqRel);
        if let Err(TrySendError::Disconnected(())) = self.wake.try_send(()) {
            self.fail();
        }
    }
    pub fn require_registration(&self) {
        self.blocked.fetch_or(REGISTRATION, Ordering::AcqRel);
        self.generation.fetch_add(1, Ordering::AcqRel);
    }
    pub fn registration_ready(&self) {
        self.blocked.fetch_and(!REGISTRATION, Ordering::AcqRel);
    }
    pub(crate) fn suspended(&self) -> bool {
        self.blocked.load(Ordering::Acquire) & POWER != 0
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }
    pub fn check(&self, original: u64) -> AppResult<()> {
        if self.generation() != original
            || self.blocked.load(Ordering::Acquire) != 0
            || !self.alive.load(Ordering::Acquire)
        {
            Err(AppError::Locked)
        } else {
            Ok(())
        }
    }
    pub(crate) fn take_pending(&self) -> usize {
        self.pending.swap(0, Ordering::AcqRel)
    }
    pub fn fail(&self) {
        if !self.alive.swap(false, Ordering::AcqRel) {
            return;
        }
        self.blocked.fetch_or(FAILED, Ordering::AcqRel);
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.alive.store(false, Ordering::Release);
        self.pending.fetch_or(1, Ordering::AcqRel);
        let _ = self.wake.try_send(());
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    #[test]
    fn full_wake_queue_retains_lock_suspend_and_resume() {
        let (tx, rx) = mpsc::sync_channel(1);
        let signal = VaultEventSignal::new(tx);
        signal.notify(NativeVaultEvent::SessionLocked);
        signal.notify(NativeVaultEvent::Suspend);
        signal.notify(NativeVaultEvent::Resume);
        assert_eq!(signal.generation(), 3);
        assert_eq!(signal.take_pending(), 15);
        assert!(!signal.suspended());
        assert_eq!(signal.check(3), Err(AppError::Locked));
        assert!(rx.try_recv().is_ok());
        assert!(rx.try_recv().is_err());
        signal.notify(NativeVaultEvent::SessionUnlocked);
        assert_eq!(signal.check(4), Ok(()));
    }
    #[test]
    fn disconnected_consumer_stays_unavailable_even_after_unlock() {
        let (tx, rx) = mpsc::sync_channel(1);
        drop(rx);
        let signal = VaultEventSignal::new(tx);
        signal.notify(NativeVaultEvent::SessionLocked);
        signal.notify(NativeVaultEvent::SessionUnlocked);
        assert_eq!(signal.check(signal.generation()), Err(AppError::Locked));
    }
    #[test]
    fn duplicate_automatic_resume_does_not_revoke_fresh_manual_admission() {
        let (tx, _rx) = mpsc::sync_channel(1);
        let signal = VaultEventSignal::new(tx);
        signal.notify(NativeVaultEvent::Suspend);
        signal.notify(NativeVaultEvent::Resume);
        let generation = signal.generation();
        signal.notify(NativeVaultEvent::Resume);
        assert_eq!(signal.generation(), generation);
        assert_eq!(signal.check(generation), Ok(()));
    }
}
