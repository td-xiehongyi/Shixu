//! Atomic admission barrier. Notification never takes a controller/engine lock.
use shixu_core::contracts::{AppResult, error::AppError};
pub use shixu_native::vault::notifications::NativeVaultEvent;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    mpsc::{SyncSender, TrySendError},
};
const REVOKED: usize = 1;
pub(crate) const WINDOW_CLOSED: usize = 2;
pub struct VaultEventSignal {
    generation: AtomicU64,
    pending: AtomicUsize,
    wake: SyncSender<()>,
    alive: AtomicBool,
}
impl VaultEventSignal {
    pub(crate) fn new(wake: SyncSender<()>) -> Self {
        Self {
            generation: AtomicU64::new(0),
            pending: AtomicUsize::new(0),
            wake,
            alive: AtomicBool::new(true),
        }
    }
    pub fn notify(&self, event: NativeVaultEvent) {
        let bits = match event {
            NativeVaultEvent::WindowClosed => REVOKED | WINDOW_CLOSED,
            NativeVaultEvent::Revoked => REVOKED,
        };
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.pending.fetch_or(bits, Ordering::AcqRel);
        if let Err(TrySendError::Disconnected(())) = self.wake.try_send(()) {
            self.fail();
        }
    }
    pub(crate) fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }
    pub fn check(&self, original: u64) -> AppResult<()> {
        if self.generation() != original || !self.alive.load(Ordering::Acquire) {
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
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.pending.fetch_or(REVOKED, Ordering::AcqRel);
        let _ = self.wake.try_send(());
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    #[test]
    fn full_wake_queue_retains_close_and_revokes_old_admission() {
        let (tx, rx) = mpsc::sync_channel(1);
        let signal = VaultEventSignal::new(tx);
        let original = signal.generation();
        signal.notify(NativeVaultEvent::WindowClosed);
        signal.notify(NativeVaultEvent::Revoked);
        assert_eq!(signal.check(original), Err(AppError::Locked));
        assert_eq!(signal.take_pending(), REVOKED | WINDOW_CLOSED);
        assert!(rx.try_recv().is_ok());
        assert!(rx.try_recv().is_err());
        assert_eq!(signal.check(signal.generation()), Ok(()));
    }
    #[test]
    fn disconnected_consumer_stays_unavailable_after_further_events() {
        let (tx, rx) = mpsc::sync_channel(1);
        drop(rx);
        let signal = VaultEventSignal::new(tx);
        signal.notify(NativeVaultEvent::WindowClosed);
        signal.notify(NativeVaultEvent::Revoked);
        assert_eq!(signal.check(signal.generation()), Err(AppError::Locked));
    }
}
