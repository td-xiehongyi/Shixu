//! SYNTHETIC POLICY TESTS ONLY: plaintext memory, no encryption or disk backend.
use shixu_core::{
    contracts::{
        AppResult,
        error::AppError,
        vault::{EntryId, SecretBytes, VaultMutation, VaultRecord, VaultSummary},
    },
    vault::ports::VaultEngine,
};
use std::{cell::RefCell, rc::Rc};

#[derive(Default)]
pub struct Faults {
    pub close_error: Option<AppError>,
    pub read_error: Option<AppError>,
    pub apply_error: Option<AppError>,
}

/// Intentionally permissive about fields/revisions: policy must reject bad input
/// before it reaches the engine. Real engines must ALSO enforce atomic revisions.
pub struct FakeVaultEngine {
    master: Option<SecretBytes>,
    opened: bool,
    records: Vec<VaultRecord>,
    pub faults: Rc<RefCell<Faults>>,
    next_id: u128,
}
impl Default for FakeVaultEngine {
    fn default() -> Self {
        Self {
            master: None,
            opened: false,
            records: Vec::new(),
            faults: Rc::new(RefCell::new(Faults::default())),
            next_id: 1,
        }
    }
}
impl FakeVaultEngine {
    fn require_open(&self) -> AppResult<()> {
        if self.opened {
            Ok(())
        } else {
            Err(AppError::Locked)
        }
    }
}
impl VaultEngine for FakeVaultEngine {
    fn create(&mut self, master: SecretBytes) -> AppResult<()> {
        if self.master.is_some() {
            return Err(AppError::Conflict);
        }
        self.master = Some(master);
        self.opened = true;
        Ok(())
    }
    fn open(&mut self, master: SecretBytes) -> AppResult<()> {
        if !self
            .master
            .as_ref()
            .is_some_and(|stored| stored.expose() == master.expose())
        {
            return Err(AppError::AuthFailed);
        }
        self.opened = true;
        Ok(())
    }
    fn close(&mut self) -> AppResult<()> {
        if let Some(error) = self.faults.borrow().close_error {
            return Err(error);
        }
        self.opened = false;
        Ok(())
    }
    fn list(&mut self) -> AppResult<Vec<VaultSummary>> {
        self.require_open()?;
        Ok(self.records.iter().map(VaultSummary::from).collect())
    }
    fn apply(&mut self, mutation: VaultMutation) -> AppResult<VaultSummary> {
        self.require_open()?;
        if let Some(error) = self.faults.borrow().apply_error {
            return Err(error);
        }
        match mutation {
            VaultMutation::Create {
                channel,
                account,
                password,
            } => {
                let record = VaultRecord {
                    entry_id: EntryId::from_uuid(uuid::Uuid::from_u128(self.next_id)),
                    channel,
                    account,
                    password,
                    revision: 1,
                    created_at: 0,
                    updated_at: 0,
                };
                self.next_id += 1;
                let summary = VaultSummary::from(&record);
                self.records.push(record);
                Ok(summary)
            }
            VaultMutation::Update {
                id,
                channel,
                account,
                password,
                ..
            } => {
                let record = self
                    .records
                    .iter_mut()
                    .find(|record| record.entry_id == id)
                    .ok_or(AppError::InvalidInput)?;
                record.channel = channel;
                record.account = account;
                record.password = password;
                record.revision += 1;
                record.updated_at += 1;
                Ok(VaultSummary::from(&*record))
            }
            VaultMutation::Delete { id, .. } => {
                let index = self
                    .records
                    .iter()
                    .position(|record| record.entry_id == id)
                    .ok_or(AppError::InvalidInput)?;
                Ok(VaultSummary::from(&self.records.remove(index)))
            }
        }
    }
    fn read_secret(&mut self, id: &str) -> AppResult<SecretBytes> {
        self.require_open()?;
        if let Some(error) = self.faults.borrow().read_error {
            return Err(error);
        }
        let id: EntryId = id.parse()?;
        let record = self
            .records
            .iter()
            .find(|record| record.entry_id == id)
            .ok_or(AppError::InvalidInput)?;
        Ok(SecretBytes::new(record.password.expose().to_vec()))
    }
    fn change_master(&mut self, current: SecretBytes, next: SecretBytes) -> AppResult<()> {
        self.require_open()?;
        if !self
            .master
            .as_ref()
            .is_some_and(|stored| stored.expose() == current.expose())
        {
            return Err(AppError::AuthFailed);
        }
        self.master = Some(next);
        Ok(())
    }
}
