pub mod coordinator;
pub mod database;
use crate::contracts::AppResult;
pub use database::Database;
/// Native local protection, independent of vault passwords. No plaintext fallback.
pub trait DataProtector: Send + Sync {
    fn protect(&self, plain: &[u8]) -> AppResult<Vec<u8>>;
    fn unprotect(&self, sealed: &[u8]) -> AppResult<Vec<u8>>;
}
