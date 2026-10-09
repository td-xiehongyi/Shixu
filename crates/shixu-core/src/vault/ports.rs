use crate::contracts::{
    AppResult,
    vault::{SecretBytes, VaultMutation, VaultSummary},
};

/// Vault storage/cryptography boundary. All secret arguments transfer ownership.
/// Engines start closed, enforce revisions atomically, and never log secrets.
/// `close` must release decrypted state; an error never restores authorization.
pub trait VaultEngine {
    fn create(&mut self, master: SecretBytes) -> AppResult<()>;
    fn open(&mut self, master: SecretBytes) -> AppResult<()>;
    fn close(&mut self) -> AppResult<()>;
    fn list(&mut self) -> AppResult<Vec<VaultSummary>>;
    /// Delete returns the summary immediately before deletion.
    fn apply(&mut self, mutation: VaultMutation) -> AppResult<VaultSummary>;
    fn read_secret(&mut self, id: &str) -> AppResult<SecretBytes>;
    fn change_master(&mut self, current: SecretBytes, next: SecretBytes) -> AppResult<()>;
}
