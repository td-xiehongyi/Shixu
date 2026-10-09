//! Process-local writer barrier shared by the authoritative database and its blob cache.
//! Lock order: coordinator permit, then database/cache/consent mutex. Never pause while
//! holding a permit. Paused operations fail Conflict (including SQL reads), so no caller
//! can deadlock by waiting on its own pause. A guard is a live, non-cloneable capability.
use crate::contracts::{AppResult, error::AppError};
use std::sync::{Arc, Condvar, Mutex};
#[derive(Default)]
struct State {
    paused: bool,
    active: usize,
}
#[derive(Default)]
pub struct WriteCoordinator {
    state: Mutex<State>,
    drained: Condvar,
}
pub struct WritePermit {
    coordinator: Arc<WriteCoordinator>,
}
pub struct PauseGuard {
    coordinator: Arc<WriteCoordinator>,
}
impl WriteCoordinator {
    pub fn enter(self: &Arc<Self>) -> AppResult<WritePermit> {
        let mut state = self.state.lock().map_err(|_| AppError::Disconnected)?;
        if state.paused {
            return Err(AppError::Conflict);
        }
        state.active += 1;
        Ok(WritePermit {
            coordinator: self.clone(),
        })
    }
    pub fn pause(self: &Arc<Self>) -> AppResult<PauseGuard> {
        let mut state = self.state.lock().map_err(|_| AppError::Disconnected)?;
        if state.paused {
            return Err(AppError::Conflict);
        }
        state.paused = true;
        while state.active != 0 {
            state = self
                .drained
                .wait(state)
                .map_err(|_| AppError::Disconnected)?;
        }
        Ok(PauseGuard {
            coordinator: self.clone(),
        })
    }
}
impl Drop for WritePermit {
    fn drop(&mut self) {
        if let Ok(mut state) = self.coordinator.state.lock() {
            state.active -= 1;
            self.coordinator.drained.notify_all();
        }
    }
}
impl Drop for PauseGuard {
    fn drop(&mut self) {
        if let Ok(mut state) = self.coordinator.state.lock() {
            state.paused = false;
            self.coordinator.drained.notify_all();
        }
    }
}
