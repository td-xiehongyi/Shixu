//! Versioned, strict table-shaped transfer. Table/column names come exclusively
//! from our fixed schema; user input is always bound values, never SQL or paths.
use super::{
    CalendarBackup,
    calendar::copy_database,
    manifest::{TABLES, columns, validate_payload},
};
use crate::{
    contracts::{
        AppResult, SchemaVersion, backup::RestorePreview, error::AppError,
        notification::MessageEnvelope,
    },
    storage::database::storage_error,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
pub const MAX_IMPORT: usize = 20 * 1024 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Transfer {
    transfer_schema_version: SchemaVersion,
    physical_schema_version: u32,
    include_raw_messages: bool,
    tables: BTreeMap<String, Vec<Vec<Value>>>,
}
impl CalendarBackup {
    pub fn export_json(&self, include_raw_messages: bool, confirmed: bool) -> AppResult<Vec<u8>> {
        if !confirmed {
            return Err(AppError::InvalidInput);
        }
        let pause = self.db.pause_writes()?;
        let mut copy = rusqlite::Connection::open_in_memory().map_err(storage_error)?;
        self.db
            .paused(&pause, |live| copy_database(live, &mut copy))?;
        drop(pause);
        let mut tables = BTreeMap::new();
        for table in TABLES {
            let cols = columns(&copy, table)?;
            // History consumers use insertion order; UUID order is unrelated to chronology.
            let order = if *table == "calendar_changes" {
                "rowid"
            } else {
                "1"
            };
            let mut stmt = copy
                .prepare(&format!("SELECT * FROM {table} ORDER BY {order}"))
                .map_err(storage_error)?;
            let mut rows = stmt.query([]).map_err(storage_error)?;
            let mut values = vec![];
            while let Some(row) = rows.next().map_err(storage_error)? {
                let mut cells = vec![];
                for (i, (_, ty)) in cols.iter().enumerate() {
                    let cell = match row.get_ref(i).map_err(storage_error)? {
                        rusqlite::types::ValueRef::Null => Value::Null,
                        rusqlite::types::ValueRef::Integer(n) => Value::String(n.to_string()),
                        rusqlite::types::ValueRef::Text(s) => Value::String(
                            std::str::from_utf8(s)
                                .map_err(|_| AppError::InvalidInput)?
                                .into(),
                        ),
                        rusqlite::types::ValueRef::Blob(b) if ty == "BLOB" => {
                            self.db.unprotect::<Value>(b)?
                        }
                        _ => return Err(AppError::InvalidInput),
                    };
                    cells.push(cell)
                }
                if *table == "messages" {
                    let index = cols
                        .iter()
                        .position(|c| c.0 == "payload")
                        .ok_or(AppError::InvalidInput)?;
                    if !cells[index].is_null() {
                        let mut m: MessageEnvelope = serde_json::from_value(cells[index].clone())
                            .map_err(|_| AppError::InvalidInput)?;
                        if !include_raw_messages {
                            m.text.clear();
                        }
                        for p in &mut m.parts {
                            p.encrypted_blob_ref = None;
                        }
                        cells[index] =
                            serde_json::to_value(m).map_err(|_| AppError::InvalidInput)?;
                    }
                }
                // Without full messages, retain provenance excerpts/history and tombstones,
                // but no parser payload corpus or resumable body work that no longer exists.
                if !include_raw_messages
                    && matches!(
                        *table,
                        "part_results"
                            | "attachment_tasks"
                            | "calendar_batches"
                            | "runtime_work"
                            | "model_attempts"
                    )
                {
                    continue;
                }
                cells.shrink_to_fit();
                values.push(cells);
            }
            tables.insert((*table).to_string(), values);
        }
        let bytes = serde_json::to_vec(&Transfer {
            transfer_schema_version: SchemaVersion::V1,
            physical_schema_version: 7,
            include_raw_messages,
            tables,
        })
        .map_err(|_| AppError::InvalidInput)?;
        if bytes.len() > MAX_IMPORT {
            return Err(AppError::InvalidInput);
        }
        Ok(bytes)
    }
    pub fn import_json(&self, data: &[u8], now: i64) -> AppResult<RestorePreview> {
        if data.len() > MAX_IMPORT {
            return Err(AppError::InvalidInput);
        }
        let _op = self.operation.lock().map_err(|_| AppError::Conflict)?;
        let transfer: Transfer =
            serde_json::from_slice(data).map_err(|_| AppError::InvalidInput)?;
        if transfer.physical_schema_version != 7
            || transfer.tables.len() != TABLES.len()
            || TABLES.iter().any(|t| !transfer.tables.contains_key(*t))
        {
            return Err(AppError::InvalidInput);
        }
        let empty = self.db.empty_copy()?;
        let guard = empty.pause_writes()?;
        let path = self.root.join(format!("import-{}", uuid::Uuid::new_v4()));
        empty.paused(&guard, |c| {
            c.execute_batch("PRAGMA foreign_keys=OFF; DROP TRIGGER runtime_message_insert; DROP TRIGGER runtime_message_update;").map_err(storage_error)?;
            let tx = c.transaction().map_err(storage_error)?;
            for table in TABLES {
                let cols = columns(&tx, table)?;
                let marks = vec!["?"; cols.len()].join(",");
                let sql = format!("INSERT INTO {table} VALUES({marks})");
                for row in &transfer.tables[*table] {
                    if row.len() != cols.len() { return Err(AppError::InvalidInput); }
                    let mut cells = vec![];
                    for ((name, ty), v) in cols.iter().zip(row) {
                        let value = if v.is_null() {
                            rusqlite::types::Value::Null
                        } else if ty == "BLOB" {
                            validate_payload(table, name, v)?;
                            rusqlite::types::Value::Blob(self.db.protect(v)?)
                        } else if ty == "INTEGER" {
                            let s = v.as_str().ok_or(AppError::InvalidInput)?;
                            let n = s.parse::<i64>().map_err(|_| AppError::InvalidInput)?;
                            if n.to_string() != s { return Err(AppError::InvalidInput); }
                            rusqlite::types::Value::Integer(n)
                        } else {
                            let s = v.as_str().ok_or(AppError::InvalidInput)?;
                            if s.len() > 65536 || s.contains('\0') { return Err(AppError::InvalidInput); }
                            rusqlite::types::Value::Text(s.into())
                        };
                        cells.push(value);
                    }
                    tx.execute(&sql, rusqlite::params_from_iter(cells)).map_err(storage_error)?;
                }
            }
            tx.commit().map_err(storage_error)?;
            let schema = include_str!("../storage/migrations/0007_runtime.sql");
            for trigger in ["runtime_message_insert", "runtime_message_update"] {
                let start = schema.find(&format!("CREATE TRIGGER {trigger}")).ok_or(AppError::InvalidInput)?;
                let end = schema[start..].find("END;").ok_or(AppError::InvalidInput)? + start + 4;
                c.execute_batch(&schema[start..end]).map_err(storage_error)?;
            }
            c.execute_batch("PRAGMA foreign_keys=ON").map_err(storage_error)?;
            super::manifest::validate_database(&self.db, c)?;
            super::manifest::deactivate(&self.db, c)?;
            // Never let imported references acquire access to an unrelated local blob.
            let rows = {
                let mut stmt = c.prepare("SELECT message_key,payload FROM messages WHERE payload IS NOT NULL").map_err(storage_error)?;
                stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?)))
                    .map_err(storage_error)?.collect::<Result<Vec<_>, _>>().map_err(storage_error)?
            };
            for (key, bytes) in rows {
                let mut m: MessageEnvelope = self.db.unprotect(&bytes)?;
                for p in &mut m.parts { p.encrypted_blob_ref = None; }
                c.execute("UPDATE messages SET payload=?2 WHERE message_key=?1", rusqlite::params![key, self.db.protect(&m)?]).map_err(storage_error)?;
            }
            c.execute_batch("UPDATE runtime_work SET model_pending=0").map_err(storage_error)?;
            if !transfer.include_raw_messages {
                c.execute_batch("DELETE FROM runtime_work; DELETE FROM attachment_tasks; DELETE FROM model_attempts;").map_err(storage_error)?;
            }
            self.publish_copy(c, &path, now, None)?;
            Ok(())
        })?;
        self.prepare(path, now)
    }
}
