use crate::contracts::{SchemaVersion, backup::BackupManifest};
use serde::{Deserialize, Serialize};
#[derive(Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub(crate) struct DiskManifest {
    pub transfer_schema_version: SchemaVersion,
    pub physical_schema_version: u32,
    pub manifest: BackupManifest,
    pub database_hash: String,
}
use crate::{
    contracts::{AppResult, backup::*, error::AppError, notification::*},
    storage::{Database, database::storage_error},
};
use rusqlite::Connection;
pub(crate) const TABLES: &[&str] = &[
    "sources",
    "source_bindings",
    "source_recovery",
    "messages",
    "part_results",
    "suppressions",
    "calendar_events",
    "calendar_sources",
    "calendar_changes",
    "calendar_suppressions",
    "calendar_batches",
    "source_settings",
    "message_source_proof",
    "desktop_settings",
    "attachment_tasks",
    "runtime_work",
    "model_attempts",
];
pub(crate) fn schema(c: &Connection) -> AppResult<Vec<(String, String)>> {
    let mut s = c
        .prepare("SELECT name,sql FROM sqlite_master WHERE sql IS NOT NULL ORDER BY name")
        .map_err(storage_error)?;
    s.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(storage_error)?
        .map(|r| r.map_err(storage_error))
        .collect()
}
pub(crate) fn validate_database(db: &Database, c: &Connection) -> AppResult<()> {
    let version: u32 = c
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(storage_error)?;
    if version != 7 {
        return Err(AppError::Unsupported);
    }
    let empty = db.empty_copy()?;
    let p = empty.pause_writes()?;
    let baseline = empty.paused(&p, |c| schema(c))?;
    if schema(c)? != baseline {
        return Err(AppError::InvalidInput);
    }
    let integrity: String = c
        .query_row("PRAGMA quick_check", [], |r| r.get(0))
        .map_err(storage_error)?;
    if integrity != "ok"
        || c.prepare("PRAGMA foreign_key_check")
            .map_err(storage_error)?
            .exists([])
            .map_err(storage_error)?
    {
        return Err(AppError::InvalidInput);
    }
    let secrets: i64 = c
        .query_row("SELECT count(*) FROM source_secrets", [], |r| r.get(0))
        .map_err(storage_error)?;
    if secrets != 0 {
        return Err(AppError::InvalidInput);
    }
    db.verify_identity(c)?;
    // Every protected cell must authenticate and decode, even when not projected by UI.
    for table in TABLES {
        let columns = columns(c, table)?;
        for col in columns.iter().filter(|(_, ty)| ty == "BLOB") {
            let mut stmt = c
                .prepare(&format!(
                    "SELECT {} FROM {table} WHERE {} IS NOT NULL",
                    col.0, col.0
                ))
                .map_err(storage_error)?;
            let rows = stmt
                .query_map([], |r| r.get::<_, Vec<u8>>(0))
                .map_err(storage_error)?;
            for row in rows {
                let v: serde_json::Value = db.unprotect(&row.map_err(storage_error)?)?;
                validate_payload(table, &col.0, &v)?;
            }
        }
    }
    super::validation::validate(db, c)
}
pub(crate) fn columns(c: &Connection, table: &str) -> AppResult<Vec<(String, String)>> {
    let mut stmt = c
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(storage_error)?;
    stmt.query_map([], |r| Ok((r.get(1)?, r.get(2)?)))
        .map_err(storage_error)?
        .map(|r| r.map_err(storage_error))
        .collect()
}
pub(crate) fn validate_payload(table: &str, column: &str, v: &serde_json::Value) -> AppResult<()> {
    fn typed<T: serde::de::DeserializeOwned + serde::Serialize>(
        v: &serde_json::Value,
    ) -> AppResult<()> {
        let parsed: T = serde_json::from_value(v.clone()).map_err(|_| AppError::InvalidInput)?;
        fn matches_schema(raw: &serde_json::Value, typed: &serde_json::Value) -> bool {
            match (raw, typed) {
                (serde_json::Value::Object(raw), serde_json::Value::Object(typed)) => raw
                    .iter()
                    .all(|(k, v)| typed.get(k).is_some_and(|t| matches_schema(v, t))),
                (serde_json::Value::Array(raw), serde_json::Value::Array(typed)) => {
                    raw.len() == typed.len()
                        && raw.iter().zip(typed).all(|(r, t)| matches_schema(r, t))
                }
                _ => raw == typed,
            }
        }
        if !matches_schema(
            v,
            &serde_json::to_value(parsed).map_err(|_| AppError::InvalidInput)?,
        ) {
            return Err(AppError::InvalidInput);
        }
        Ok(())
    }
    if column == "content_digest" {
        typed::<Vec<u8>>(v)?;
        return if v.as_array().is_some_and(|a| a.len() == 32) {
            Ok(())
        } else {
            Err(AppError::InvalidInput)
        };
    }
    match table {
        "messages" => {
            typed::<MessageEnvelope>(v)?;
            let m: MessageEnvelope =
                serde_json::from_value(v.clone()).map_err(|_| AppError::InvalidInput)?;
            if m.parts.len() > 5
                || m.revision == 0
                || m.revision > i64::MAX as u64
                || m.parts.iter().any(|p| p.message_key != m.message_key)
            {
                return Err(AppError::InvalidInput);
            }
            Ok(())
        }
        "part_results" => typed::<Vec<PartResult>>(v),
        "source_settings" | "message_source_proof" => {
            typed::<SourceConfig>(v)?;
            crate::notifications::settings::validate(
                &serde_json::from_value(v.clone()).map_err(|_| AppError::InvalidInput)?,
            )
        }
        "calendar_events" => typed::<crate::calendar::changes::StoredEvent>(v),
        "calendar_sources" => typed::<crate::calendar::changes::EventSource>(v),
        "calendar_changes" => typed::<crate::calendar::changes::EventChange>(v),
        "calendar_suppressions" => typed::<String>(v),
        "calendar_batches" => typed::<crate::calendar::service::BatchRecord>(v),
        _ => Err(AppError::InvalidInput),
    }
}
pub(crate) fn parts(db: &Database, c: &Connection) -> AppResult<Vec<MessagePart>> {
    let mut stmt = c
        .prepare("SELECT payload FROM messages WHERE payload IS NOT NULL ORDER BY message_key")
        .map_err(storage_error)?;
    let mut parts = vec![];
    for row in stmt
        .query_map([], |r| r.get::<_, Vec<u8>>(0))
        .map_err(storage_error)?
    {
        let m: MessageEnvelope = db.unprotect(&row.map_err(storage_error)?)?;
        parts.extend(m.parts)
    }
    parts.sort_by_key(|p| p.part_id.to_string());
    let mut ids = std::collections::HashSet::new();
    for p in &parts {
        if !ids.insert(
            p.encrypted_blob_ref
                .unwrap_or(BlobId::from_uuid(*p.part_id.as_uuid())),
        ) {
            return Err(AppError::InvalidInput);
        }
    }
    Ok(parts)
}
pub(crate) fn describe(db: &Database, c: &Connection, now: i64) -> AppResult<BackupManifest> {
    let count = |table: &str| -> AppResult<u64> {
        c.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| {
            r.get::<_, i64>(0).map(|v| v as u64)
        })
        .map_err(storage_error)
    };
    Ok(BackupManifest {
        schema_version: SchemaVersion::V1,
        created_at: now,
        entity_counts: EntityCounts {
            events: count("calendar_events")?,
            messages: count("messages")?,
            sources: count("source_bindings")?,
            changes: count("calendar_changes")?,
            suppressions: count("suppressions")? + count("calendar_suppressions")?,
            vault_records: 0,
        },
        blobs: parts(db, c)?
            .iter()
            .map(|p| BlobManifest {
                blob_id: p
                    .encrypted_blob_ref
                    .unwrap_or(BlobId::from_uuid(*p.part_id.as_uuid())),
                content_hash: None,
                byte_size: None,
                state: if p.encrypted_blob_ref.is_some() {
                    BlobState::Present
                } else if p.content_hash.is_some() {
                    if p.fetch_state == FetchState::Unavailable {
                        BlobState::Cleaned
                    } else {
                        BlobState::NotMigrated
                    }
                } else {
                    BlobState::NeverFetched
                },
            })
            .collect(),
    })
}
pub(crate) fn deactivate(db: &Database, c: &mut Connection) -> AppResult<()> {
    let tx = c.transaction().map_err(storage_error)?;
    let rows = {
        let mut stmt = tx
            .prepare("SELECT source_id,payload FROM source_settings")
            .map_err(storage_error)?;
        stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))
        })
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?
    };
    for (id, bytes) in rows {
        let mut config: SourceConfig = db.unprotect(&bytes)?;
        config.enabled = false;
        tx.execute(
            "UPDATE source_settings SET epoch=epoch+1,payload=?2 WHERE source_id=?1",
            rusqlite::params![id, db.protect(&config)?],
        )
        .map_err(storage_error)?;
    }
    // Older sources without current settings also require explicit reactivation.
    let legacy = {
        let mut stmt=tx.prepare("SELECT source_id,adapter_type,account_id,groups_json,timezone FROM source_bindings WHERE source_id NOT IN (SELECT source_id FROM source_settings)").map_err(storage_error)?;
        stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        })
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?
    };
    for (id, adapter, account, groups, timezone) in legacy {
        let config = SourceConfig {
            source_id: id.parse()?,
            adapter_type: adapter,
            account_id: account,
            allowed_group_ids: serde_json::from_str(&groups).map_err(|_| AppError::InvalidInput)?,
            timezone,
            enabled: false,
            capability_set: vec![],
        };
        tx.execute(
            "INSERT INTO source_settings VALUES (?1,1,?2)",
            rusqlite::params![id, db.protect(&config)?],
        )
        .map_err(storage_error)?;
    }
    tx.execute_batch("DELETE FROM source_secrets; UPDATE desktop_settings SET autostart=0; UPDATE attachment_tasks SET state='queued',lease=NULL WHERE state='running'; UPDATE model_attempts SET state='done'; UPDATE runtime_work SET model_pending=0;").map_err(storage_error)?;
    tx.commit().map_err(storage_error)
}

pub(crate) fn mark_missing(db: &Database, c: &Connection, blobs: &[BlobManifest]) -> AppResult<()> {
    let states = blobs
        .iter()
        .map(|b| (b.blob_id, b.state))
        .collect::<std::collections::HashMap<_, _>>();
    let rows = {
        let mut s = c
            .prepare("SELECT message_key,payload FROM messages WHERE payload IS NOT NULL")
            .map_err(storage_error)?;
        s.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))
        })
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?
    };
    for (key, bytes) in rows {
        let mut m: MessageEnvelope = db.unprotect(&bytes)?;
        let mut changed = false;
        for p in &mut m.parts {
            let id = p
                .encrypted_blob_ref
                .unwrap_or(BlobId::from_uuid(*p.part_id.as_uuid()));
            if states.get(&id) == Some(&BlobState::Cleaned) {
                p.fetch_state = FetchState::Unavailable;
                changed = true;
            }
        }
        if changed {
            c.execute(
                "UPDATE messages SET payload=?2 WHERE message_key=?1",
                rusqlite::params![key, db.protect(&m)?],
            )
            .map_err(storage_error)?;
        }
    }
    Ok(())
}
