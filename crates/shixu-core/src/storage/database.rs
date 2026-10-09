use super::DataProtector;
use crate::contracts::{AppResult, error::AppError};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior};
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
use zeroize::Zeroizing;

const SENTINEL: &[u8] = b"shixu-local-protection-v1";
/// Shared core-only SQLite owner. No connection or SQL access is exposed publicly.
/// D1 must use `transaction` so message/calendar writes commit atomically.
pub struct Database {
    connection: Mutex<Connection>,
    protector: Arc<dyn DataProtector>,
}
impl Database {
    pub fn open(path: &Path, protector: Arc<dyn DataProtector>) -> AppResult<Self> {
        let mut connection = Connection::open(path).map_err(storage_error)?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(storage_error)?;
        connection.execute_batch("PRAGMA foreign_keys=ON; PRAGMA secure_delete=ON; PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;").map_err(storage_error)?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        let version: u32 = tx
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(storage_error)?;
        match version {
            0 => {
                let sealed = protector.protect(SENTINEL)?;
                if protector.unprotect(&sealed)? != SENTINEL {
                    return Err(AppError::AuthFailed);
                }
                tx.execute_batch(include_str!("migrations/0001_messages.sql"))
                    .map_err(storage_error)?;
                tx.execute("INSERT INTO protection_metadata VALUES (1,?1)", [&sealed])
                    .map_err(storage_error)?;
            }
            1 => {
                let sealed: Vec<u8> = tx
                    .query_row(
                        "SELECT sentinel FROM protection_metadata WHERE id=1",
                        [],
                        |r| r.get(0),
                    )
                    .map_err(storage_error)?;
                if protector.unprotect(&sealed)? != SENTINEL {
                    return Err(AppError::AuthFailed);
                }
            }
            _ => return Err(AppError::Unsupported),
        }
        tx.commit().map_err(storage_error)?;
        Ok(Self {
            connection: Mutex::new(connection),
            protector,
        })
    }
    pub(crate) fn transaction<T>(
        &self,
        action: impl FnOnce(&Transaction<'_>) -> AppResult<T>,
    ) -> AppResult<T> {
        let mut connection = self.connection.lock().map_err(|_| AppError::Disconnected)?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        let result = action(&tx)?;
        tx.commit().map_err(storage_error)?;
        Ok(result)
    }
    pub(crate) fn protect<T: serde::Serialize>(&self, value: &T) -> AppResult<Vec<u8>> {
        let plain = Zeroizing::new(serde_json::to_vec(value).map_err(|_| AppError::InvalidInput)?);
        self.protector.protect(&plain)
    }
    pub(crate) fn unprotect<T: serde::de::DeserializeOwned>(&self, sealed: &[u8]) -> AppResult<T> {
        let plain = Zeroizing::new(self.protector.unprotect(sealed)?);
        serde_json::from_slice(&plain).map_err(|_| AppError::ParseFailed)
    }
    /// Credentials use local protection, without any vault API. Callers must use
    /// a source configuration explicitly enabled by the user; no credential DTO.
    pub fn store_source_secret(
        &self,
        config: &crate::contracts::notification::SourceConfig,
        secret: &crate::contracts::vault::SecretBytes,
    ) -> AppResult<()> {
        if !config.enabled || config.allowed_group_ids.is_empty() {
            return Err(AppError::InvalidInput);
        }
        let sealed = self.protector.protect(secret.expose())?;
        self.transaction(|tx| { tx.execute("INSERT INTO source_secrets VALUES (?1,?2,?3,?4) ON CONFLICT(source_id,adapter_type,account_id) DO UPDATE SET payload=excluded.payload", rusqlite::params![config.source_id.to_string(), config.adapter_type, config.account_id, sealed]).map_err(storage_error)?; Ok(()) })
    }
    pub fn source_secret(
        &self,
        config: &crate::contracts::notification::SourceConfig,
    ) -> AppResult<Option<crate::contracts::vault::SecretBytes>> {
        if !config.enabled || config.allowed_group_ids.is_empty() {
            return Err(AppError::InvalidInput);
        }
        self.transaction(|tx| { let sealed: Option<Vec<u8>> = tx.query_row("SELECT payload FROM source_secrets WHERE source_id=?1 AND adapter_type=?2 AND account_id=?3", rusqlite::params![config.source_id.to_string(), config.adapter_type, config.account_id], |r| r.get(0)).optional().map_err(storage_error)?;
            sealed.map(|v| self.protector.unprotect(&v).map(crate::contracts::vault::SecretBytes::new)).transpose() })
    }
}
pub(crate) fn storage_error(error: rusqlite::Error) -> AppError {
    match error.sqlite_error_code() {
        Some(rusqlite::ErrorCode::DiskFull) => AppError::StorageFull,
        Some(rusqlite::ErrorCode::ConstraintViolation) => AppError::Conflict,
        _ => AppError::Disconnected,
    }
}
