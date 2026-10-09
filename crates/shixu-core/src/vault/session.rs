use crate::contracts::{
    AppResult, UtcMillis,
    error::AppError,
    vault::{SessionId, VaultStatus},
};

const IDLE_TIMEOUT_MS: UtcMillis = 300_000;

struct ActiveSession {
    id: SessionId,
    last_activity: UtcMillis,
    last_observed: UtcMillis,
}

/// Authority is derived solely from the active engine session, never from a
/// separate cached "master password verified" flag.
pub(super) struct SessionPolicy {
    status: VaultStatus,
    active: Option<ActiveSession>,
}

impl SessionPolicy {
    pub fn new() -> Self {
        Self {
            status: VaultStatus::NotCreated,
            active: None,
        }
    }

    pub fn status(&self) -> VaultStatus {
        self.status
    }

    pub fn begin_unlock(&mut self) {
        self.active = None;
        self.status = VaultStatus::Unlocking;
    }

    pub fn activate(&mut self, now: UtcMillis) -> SessionId {
        let id = SessionId::from_uuid(uuid::Uuid::new_v4());
        self.active = Some(ActiveSession {
            id,
            last_activity: now,
            last_observed: now,
        });
        self.status = VaultStatus::Unlocked;
        id
    }

    /// Invalidate BEFORE calling any fallible engine cleanup. Removing the
    /// identity also cancels authority to deliver every pending old reply.
    pub fn revoke(&mut self) {
        self.active = None;
        self.status = VaultStatus::Locked;
    }

    pub fn authorize(&self, id: &SessionId) -> AppResult<()> {
        if self.status == VaultStatus::Unlocked
            && self.active.as_ref().is_some_and(|active| active.id == *id)
        {
            Ok(())
        } else {
            Err(AppError::Locked)
        }
    }

    /// Timer observations never refresh vault activity. Caller-supplied clocks
    /// must be nondecreasing, including observations by background ticks.
    pub fn observe_time(&mut self, now: UtcMillis) -> AppResult<bool> {
        let Some(active) = &mut self.active else {
            return Ok(false);
        };
        if now < active.last_observed {
            return Err(AppError::InvalidInput);
        }
        active.last_observed = now;
        Ok(now.saturating_sub(active.last_activity) >= IDLE_TIMEOUT_MS)
    }

    pub fn refresh(&mut self, now: UtcMillis) {
        if let Some(active) = &mut self.active {
            active.last_activity = now;
        }
    }
}
