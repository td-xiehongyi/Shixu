//! Current authorization is separate from immutable historical bindings and per-message proof.
use crate::{
    contracts::{AppResult, error::AppError, notification::*},
    storage::{Database, database::storage_error},
};
use rusqlite::{OptionalExtension, Transaction, params};
use std::sync::Arc;
#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct SourceSetting {
    pub config: SourceConfig,
    pub epoch: u64,
}
pub struct SettingsStore {
    db: Arc<Database>,
}
impl SettingsStore {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }
    pub fn sources(&self) -> AppResult<Vec<SourceSetting>> {
        self.db.transaction(|tx| {
            let mut stmt = tx
                .prepare("SELECT epoch,payload FROM source_settings ORDER BY source_id LIMIT 100")
                .map_err(storage_error)?;
            stmt.query_map([], |r| {
                Ok((r.get::<_, i64>(0)? as u64, r.get::<_, Vec<u8>>(1)?))
            })
            .map_err(storage_error)?
            .map(|r| {
                let (epoch, payload) = r.map_err(storage_error)?;
                Ok(SourceSetting {
                    config: canonical(self.db.unprotect(&payload)?),
                    epoch,
                })
            })
            .collect()
        })
    }
    pub fn save_source(&self, config: SourceConfig) -> AppResult<()> {
        validate(&config)?;
        let config = canonical(config);
        self.db.transaction(|tx|{
 let prior:Option<(String,String)>=tx.query_row("SELECT adapter_type,account_id FROM source_bindings WHERE source_id=?1",[config.source_id.to_string()],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(storage_error)?;
 if let Some((adapter,account))=prior {if adapter!=config.adapter_type||account!=config.account_id{return Err(AppError::Conflict);}}
 else { let legacy:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM sources WHERE source_id=?1)",[config.source_id.to_string()],|r|r.get(0)).map_err(storage_error)?;if legacy{return Err(AppError::Conflict);}
 let count:u32=tx.query_row("SELECT count(*) FROM source_settings",[],|r|r.get(0)).map_err(storage_error)?;if count>=100{return Err(AppError::InvalidInput);}
 tx.execute("INSERT INTO source_bindings(source_id,adapter_type,account_id,groups_json,timezone) VALUES (?1,?2,?3,?4,?5)",params![config.source_id.to_string(),config.adapter_type,config.account_id,serde_json::to_string(&config.allowed_group_ids).map_err(|_|AppError::InvalidInput)?,config.timezone]).map_err(storage_error)?;}
 // Re-enable/reconfigure deferred durable notices; revoked source rows never block other sources.
 tx.execute("UPDATE runtime_work SET pending=1 WHERE message_key IN (SELECT m.message_key FROM messages m JOIN sources s USING(namespace) WHERE s.source_id=?1 AND m.payload IS NOT NULL)",[config.source_id.to_string()]).map_err(storage_error)?;
 // Epoch advances explicitly in durable settings; identity/cursors/history are never reset.
 tx.execute("INSERT INTO source_settings VALUES (?1,1,?2) ON CONFLICT(source_id) DO UPDATE SET epoch=epoch+1,payload=excluded.payload",params![config.source_id.to_string(),self.db.protect(&config)?]).map_err(storage_error)?;
 Ok(())})
    }
    pub fn autostart(&self) -> AppResult<bool> {
        self.db.transaction(|tx| {
            Ok(tx
                .query_row(
                    "SELECT autostart FROM desktop_settings WHERE id=1",
                    [],
                    |r| r.get(0),
                )
                .optional()
                .map_err(storage_error)?
                .unwrap_or(false))
        })
    }
    pub fn set_autostart(&self, value: bool) -> AppResult<()> {
        self.db.transaction(|tx|{tx.execute("INSERT INTO desktop_settings VALUES (1,?1) ON CONFLICT(id) DO UPDATE SET autostart=excluded.autostart",[value]).map_err(storage_error)?;Ok(())})
    }
}
/// Canonical group-set representation for new writes and legacy current reads.
/// Historical message proofs/bindings and configuration epochs are not rewritten.
pub(crate) fn canonical(mut config: SourceConfig) -> SourceConfig {
    config.allowed_group_ids.sort();
    config.allowed_group_ids.dedup();
    config
}
pub fn validate(c: &SourceConfig) -> AppResult<()> {
    let valid = |s: &str, max| !s.is_empty() && s.len() <= max && !s.chars().any(char::is_control);
    if !valid(&c.adapter_type, 128)
        || !valid(&c.account_id, 128)
        || c.allowed_group_ids.len() > 100
        || c.allowed_group_ids.iter().any(|g| !valid(g, 128))
        || c.capability_set.len() > 6
        || c.timezone.parse::<chrono_tz::Tz>().is_err()
        || c.enabled && c.allowed_group_ids.is_empty()
    {
        return Err(AppError::InvalidInput);
    }
    Ok(())
}
pub(crate) fn current(
    db: &Database,
    tx: &Transaction<'_>,
    id: SourceId,
) -> AppResult<Option<(u64, SourceConfig)>> {
    let row: Option<(u64, Vec<u8>)> = tx
        .query_row(
            "SELECT epoch,payload FROM source_settings WHERE source_id=?1",
            [id.to_string()],
            |r| Ok((r.get::<_, i64>(0)? as u64, r.get(1)?)),
        )
        .optional()
        .map_err(storage_error)?;
    row.map(|(epoch, payload)| Ok((epoch, canonical(db.unprotect(&payload)?))))
        .transpose()
}
pub(crate) fn proof(
    db: &Database,
    tx: &Transaction<'_>,
    m: &MessageEnvelope,
) -> AppResult<SourceConfig> {
    let row: Option<Vec<u8>> = tx
        .query_row(
            "SELECT payload FROM message_source_proof WHERE message_key=?1",
            [m.message_key.to_string()],
            |r| r.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    if let Some(payload) = row {
        return db.unprotect(&payload);
    }
    let (adapter,account,groups,zone):(String,String,String,String)=tx.query_row("SELECT adapter_type,account_id,groups_json,timezone FROM source_bindings WHERE source_id=?1",[m.source_id.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(storage_error)?.ok_or(AppError::InvalidInput)?;
    Ok(SourceConfig {
        source_id: m.source_id,
        adapter_type: adapter,
        account_id: account,
        allowed_group_ids: serde_json::from_str(&groups).map_err(|_| AppError::ParseFailed)?,
        timezone: zone,
        enabled: true,
        capability_set: vec![],
    })
}
