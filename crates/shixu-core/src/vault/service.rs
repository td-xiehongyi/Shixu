use super::{ports::VaultEngine, session::SessionPolicy};
use crate::contracts::{
    AppResult, UtcMillis,
    error::AppError,
    vault::{
        EntryId, LockReason, SecretBytes, SessionId, VaultMutation, VaultStatus, VaultSummary,
    },
};

/// Single owner of engine access and vault authorization. Only explicit,
/// successful vault interactions refresh activity; other domains have no access
/// to the engine or the refresh operation.
pub struct VaultService<E: VaultEngine> {
    engine: E,
    session: SessionPolicy,
}

impl<E: VaultEngine> VaultService<E> {
    /// The injected engine must start closed. Initial existence is unknown until
    /// create/open; adapters discover existing storage during open.
    pub fn new(engine: E) -> Self {
        Self {
            engine,
            session: SessionPolicy::new(),
        }
    }

    pub fn create(&mut self, master: SecretBytes, now: UtcMillis) -> AppResult<SessionId> {
        self.prepare_unlock(&master)?;
        let result = self.engine.create(master);
        self.finish_unlock(result, now)
    }

    pub fn unlock(&mut self, master: SecretBytes, now: UtcMillis) -> AppResult<SessionId> {
        self.prepare_unlock(&master)?;
        let result = self.engine.open(master);
        self.finish_unlock(result, now)
    }

    pub fn lock(&mut self, _reason: LockReason) -> AppResult<()> {
        self.session.revoke();
        self.engine.close()
    }

    pub fn tick(&mut self, now: UtcMillis) -> AppResult<()> {
        if self.session.observe_time(now)? {
            self.lock(LockReason::Timeout)?;
        }
        Ok(())
    }

    pub fn status(&self) -> VaultStatus {
        self.session.status()
    }

    pub fn list(&mut self, session: &SessionId, now: UtcMillis) -> AppResult<Vec<VaultSummary>> {
        self.authorize_at(session, now)?;
        let result = self.engine.list();
        let summaries = self.finish_operation(session, result)?;
        self.session.refresh(now);
        Ok(summaries)
    }

    pub fn apply(
        &mut self,
        session: &SessionId,
        mutation: VaultMutation,
        now: UtcMillis,
    ) -> AppResult<VaultSummary> {
        self.authorize_at(session, now)?;
        match &mutation {
            VaultMutation::Create {
                channel,
                account,
                password,
            }
            | VaultMutation::Update {
                channel,
                account,
                password,
                ..
            } => {
                validate_fields(channel, account, password)?;
            }
            VaultMutation::Delete { .. } => {}
        }
        if let VaultMutation::Update {
            id,
            expected_revision,
            ..
        }
        | VaultMutation::Delete {
            id,
            expected_revision,
        } = &mutation
        {
            let result = self.engine.list();
            let summaries = self.finish_operation(session, result)?;
            let current = summaries
                .iter()
                .find(|summary| summary.entry_id == *id)
                .ok_or(AppError::InvalidInput)?;
            if current.revision != *expected_revision {
                return Err(AppError::Conflict);
            }
        }
        // Engine must repeat the revision check atomically with its write.
        let result = self.engine.apply(mutation);
        let summary = self.finish_operation(session, result)?;
        self.session.refresh(now);
        Ok(summary)
    }

    pub fn reveal(
        &mut self,
        session: &SessionId,
        id: &str,
        now: UtcMillis,
    ) -> AppResult<SecretBytes> {
        self.authorize_at(session, now)?;
        let _: EntryId = id.parse()?;
        let result = self.engine.read_secret(id);
        let secret = self.finish_operation(session, result)?;
        self.session.refresh(now);
        Ok(secret)
    }

    /// The fixed V1 interface has no timestamp: this checks current authority
    /// but does not refresh activity. Dispatchers must run tick(now) before
    /// timestamp-free commands, and continue ticking while operations are idle.
    pub fn change_master(
        &mut self,
        session: &SessionId,
        current: SecretBytes,
        next: SecretBytes,
    ) -> AppResult<()> {
        self.session.authorize(session)?;
        validate_secret(&current)?;
        validate_secret(&next)?;
        let result = self.engine.change_master(current, next);
        self.finish_operation(session, result)
    }

    /// Delivery guard for a delayed completion, bound to its ORIGINAL session.
    /// Lock/reopen permanently revokes old authority. The owned reply is dropped
    /// on rejection, including zeroizing SecretBytes. A future async dispatcher
    /// must tick its clock then call this guard immediately before delivery.
    /// No background delivery refreshes vault activity.
    pub fn accept_reply<T>(&self, session: &SessionId, reply: AppResult<T>) -> AppResult<T> {
        self.session.authorize(session)?;
        reply
    }

    fn authorize_at(&mut self, session: &SessionId, now: UtcMillis) -> AppResult<()> {
        // A stale request cannot change the newer session's clock or state.
        self.session.authorize(session)?;
        self.tick(now)?;
        self.session.authorize(session)
    }

    fn prepare_unlock(&mut self, master: &SecretBytes) -> AppResult<()> {
        // Retry cleanup even after a failed close, before opening any new engine
        // state. Failed attempts revoke any previously active identity.
        self.lock(LockReason::Manual)?;
        validate_secret(master)?;
        self.session.begin_unlock();
        Ok(())
    }

    fn finish_unlock(&mut self, result: AppResult<()>, now: UtcMillis) -> AppResult<SessionId> {
        match result {
            Ok(()) => Ok(self.session.activate(now)),
            Err(error) => {
                let _ = self.lock(LockReason::Manual);
                Err(error)
            }
        }
    }

    fn finish_operation<T>(&mut self, session: &SessionId, result: AppResult<T>) -> AppResult<T> {
        match result {
            Ok(reply) => self.accept_reply(session, Ok(reply)),
            Err(error) => {
                // Every engine failure is fail-closed. Preserve its payload-free
                // error; the next unlock must successfully retry engine cleanup.
                let _ = self.lock(LockReason::Manual);
                Err(error)
            }
        }
    }
}

fn validate_secret(secret: &SecretBytes) -> AppResult<()> {
    if secret.expose().is_empty() {
        Err(AppError::InvalidInput)
    } else {
        Ok(())
    }
}

fn validate_fields(channel: &str, account: &str, password: &SecretBytes) -> AppResult<()> {
    if channel.trim().is_empty() || account.trim().is_empty() {
        return Err(AppError::InvalidInput);
    }
    // Validate nonempty bytes without normalization, UTF-8 assumptions or copies.
    validate_secret(password)
}
