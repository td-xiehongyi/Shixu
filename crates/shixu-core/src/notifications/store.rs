use super::identity::{content_digest, message_identity, namespace};
use crate::{
    contracts::{AppResult, UtcMillis, error::AppError, notification::*},
    storage::{Database, database::storage_error},
};
use rusqlite::{OptionalExtension, params};
use std::sync::Arc;
#[derive(Debug, PartialEq, Eq)]
pub enum AppendOutcome {
    Stored,
    Duplicate,
    RevisionConflict,
    Filtered,
}
struct StoredMessage {
    revision: i64,
    sealed_digest: Vec<u8>,
    existing_payload: Option<Vec<u8>>,
    revoked: bool,
}
pub struct MessageStore {
    db: Arc<Database>,
}
impl MessageStore {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }
    pub fn append(
        &self,
        config: &SourceConfig,
        envelope: MessageEnvelope,
    ) -> AppResult<AppendOutcome> {
        self.append_inner(config, envelope, None)
    }
    fn append_inner(
        &self,
        config: &SourceConfig,
        mut envelope: MessageEnvelope,
        cursor: Option<&str>,
    ) -> AppResult<AppendOutcome> {
        // Authorization MUST precede identity derivation, protection and all writes.
        if !config.enabled
            || envelope.source_id != config.source_id
            || envelope.account_id != config.account_id
            || !config.allowed_group_ids.contains(&envelope.group_id)
            || !config.capability_set.iter().any(|c| {
                matches!(
                    c,
                    SourceCapability::LiveMessages | SourceCapability::Backfill
                )
            })
        {
            return Ok(AppendOutcome::Filtered);
        }
        if envelope.revision == 0
            || envelope.revision > i64::MAX as u64
            || config.adapter_type.is_empty()
            || config.account_id.is_empty()
            || envelope
                .parts
                .iter()
                .any(|p| p.message_key != envelope.message_key)
            || (!envelope.parts.is_empty()
                && !config
                    .capability_set
                    .contains(&SourceCapability::Attachments))
        {
            return Err(AppError::InvalidInput);
        }
        if envelope.revoked
            && !config
                .capability_set
                .contains(&SourceCapability::Revocations)
        {
            return Ok(AppendOutcome::RevisionConflict);
        }
        let identity = message_identity(config, &envelope)?;
        let ns = namespace(config, &envelope.group_id);
        envelope.message_key = identity.key;
        for part in &mut envelope.parts {
            part.message_key = identity.key;
        }
        let digest = content_digest(&envelope)?;
        self.db.transaction(|tx| {
        let existing: Option<StoredMessage> = tx.query_row("SELECT revision,content_digest,payload,revoked FROM messages WHERE message_key=?1", [identity.key.to_string()], |r| Ok(StoredMessage {revision:r.get(0)?,sealed_digest:r.get(1)?,existing_payload:r.get(2)?,revoked:r.get(3)?})).optional().map_err(storage_error)?;
        if let Some(StoredMessage { revision, sealed_digest, existing_payload, revoked }) = existing {
            let old_digest: Vec<u8> = self.db.unprotect(&sealed_digest)?;
            if old_digest == digest {
                let trusted_revision = !identity.degraded &&
                    (config.capability_set.contains(&SourceCapability::Edits) ||
                     (revoked && config.capability_set.contains(&SourceCapability::Revocations)));
                if trusted_revision && envelope.revision > revision as u64 {
                    // Same logical content: advance the durable revision without
                    // requeueing work, replacing local part state, or reviving a
                    // cleaned payload. Result evidence follows identical content.
                    let payload = existing_payload.as_ref().map(|sealed| {
                        let mut stored: MessageEnvelope = self.db.unprotect(sealed)?;
                        stored.revision = envelope.revision;
                        self.db.protect(&stored)
                    }).transpose()?;
                    tx.execute("UPDATE messages SET revision=?2,payload=?3 WHERE message_key=?1", params![identity.key.to_string(),envelope.revision as i64,payload]).map_err(storage_error)?;
                    tx.execute("UPDATE part_results SET revision=?2 WHERE message_key=?1 AND revision=?3", params![identity.key.to_string(),envelope.revision as i64,revision]).map_err(storage_error)?;
                }
                if let Some(cursor) = cursor {
                    tx.execute("UPDATE sources SET cursor=?2 WHERE namespace=?1", params![ns,cursor]).map_err(storage_error)?;
                }
                return Ok(AppendOutcome::Duplicate);
            }
            let allowed_change = if envelope.revoked && !revoked { config.capability_set.contains(&SourceCapability::Revocations) } else { config.capability_set.contains(&SourceCapability::Edits) && !revoked };
            if existing_payload.is_none() || identity.degraded || envelope.revision <= revision as u64 || !allowed_change { return Ok(AppendOutcome::RevisionConflict); }
        }
        envelope.processing_state = if envelope.revoked { ProcessingState::SourceRevoked } else { ProcessingState::Persisted };
        let payload = self.db.protect(&envelope)?; let protected_digest = self.db.protect(&digest)?;
        // Missing IDs use the derived key as the degraded native identity slot.
        let native_id = if identity.degraded { identity.key.to_string() } else { envelope.native_message_id.clone() };
        tx.execute("INSERT INTO sources VALUES (?1,?2,?3,?4,?5,?6) ON CONFLICT(namespace) DO NOTHING", params![ns,config.source_id.to_string(),config.adapter_type,config.account_id,envelope.group_id,""]).map_err(storage_error)?;
        tx.execute("INSERT INTO messages VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10) ON CONFLICT(message_key) DO UPDATE SET revision=excluded.revision,received_at=excluded.received_at,processing_state=excluded.processing_state,revoked=excluded.revoked,payload=excluded.payload,content_digest=excluded.content_digest", params![identity.key.to_string(),ns,native_id,identity.degraded,envelope.revision as i64,envelope.received_at,state_name(envelope.processing_state),envelope.revoked,payload,protected_digest]).map_err(storage_error)?;
        tx.execute("DELETE FROM part_results WHERE message_key=?1", [identity.key.to_string()]).map_err(storage_error)?;
        if envelope.revoked { tx.execute("INSERT OR IGNORE INTO suppressions VALUES (?1,'source_revoked')", [identity.key.to_string()]).map_err(storage_error)?; }
        if let Some(cursor) = cursor {
            tx.execute("UPDATE sources SET cursor=?2 WHERE namespace=?1", params![ns,cursor]).map_err(storage_error)?;
        }
        Ok(AppendOutcome::Stored)
    })
    }
    /// Append and acknowledge an opaque, non-secret delivery cursor in the same
    /// SQLite transaction. Only Stored/Duplicate advance it. The receive worker
    /// must consume deliveries serially in transport order; cursors are opaque
    /// and cannot be compared to detect out-of-order acknowledgment.
    pub fn append_with_cursor(
        &self,
        config: &SourceConfig,
        envelope: MessageEnvelope,
        cursor: &str,
    ) -> AppResult<AppendOutcome> {
        if cursor.is_empty()
            || cursor.len() > 4096
            || cursor.contains(['/', '\\', ':'])
            || cursor.chars().any(char::is_control)
        {
            return Err(AppError::InvalidInput);
        }
        self.append_inner(config, envelope, Some(cursor))
    }
    pub fn cursor(&self, config: &SourceConfig, group: &str) -> AppResult<Option<String>> {
        if !config.enabled || !config.allowed_group_ids.iter().any(|g| g == group) {
            return Err(AppError::InvalidInput);
        }
        let ns = namespace(config, group);
        self.db.transaction(|tx| {
            tx.query_row(
                "SELECT NULLIF(cursor,'') FROM sources WHERE namespace=?1",
                [ns],
                |r| r.get(0),
            )
            .optional()
            .map(|v| v.flatten())
            .map_err(storage_error)
        })
    }
    /// Bind immutable technical identity once. Group membership is a canonical
    /// set, independent of ordering; capabilities/enabled are runtime settings.
    /// Returns true if this source already existed (resume requires recovery).
    pub fn bind_source(&self, config: &SourceConfig) -> AppResult<bool> {
        let mut groups = config.allowed_group_ids.clone();
        groups.sort();
        groups.dedup();
        if config.adapter_type.is_empty()
            || config.account_id.is_empty()
            || config.timezone.is_empty()
            || groups.is_empty()
            || groups.iter().any(|g| g.is_empty())
        {
            return Err(AppError::InvalidInput);
        }
        let groups_json = serde_json::to_string(&groups).map_err(|_| AppError::InvalidInput)?;
        self.db.transaction(|tx|{
            let old:Option<(String,String,String,String)>=tx.query_row("SELECT adapter_type,account_id,groups_json,timezone FROM source_bindings WHERE source_id=?1",[config.source_id.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(storage_error)?;
            if let Some(old)=old {
                if old!=(config.adapter_type.clone(),config.account_id.clone(),groups_json,config.timezone.clone()){return Err(AppError::InvalidInput);}
                return Ok(true);
            }
            // Legacy rows have no authoritative group-set/timezone binding.
            // Fail closed: assigning their ID a new config would invent identity.
            let legacy:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM sources WHERE source_id=?1)",[config.source_id.to_string()],|r|r.get(0)).map_err(storage_error)?;
            if legacy{return Err(AppError::Conflict);}
            tx.execute("INSERT INTO source_bindings(source_id,adapter_type,account_id,groups_json,timezone) VALUES (?1,?2,?3,?4,?5)",params![config.source_id.to_string(),config.adapter_type,config.account_id,groups_json,config.timezone]).map_err(storage_error)?;
            Ok(false)
        })
    }
    /// Start/invalidate an interval proof. Keep incomplete original anchors;
    /// completed groups begin again at the last live cursor. Historical partial
    /// recovery_cursor metadata never replaces an unresolved anchor.
    /// Epoch change invalidates every previously issued handle/completion.
    pub fn begin_recovery(&self, config: &SourceConfig, since: UtcMillis) -> AppResult<()> {
        self.bind_source(config)?;
        self.db.transaction(|tx|{
            let epoch:i64=tx.query_row("SELECT recovery_epoch FROM source_bindings WHERE source_id=?1",[config.source_id.to_string()],|r|r.get(0)).map_err(storage_error)?;
            let epoch=epoch.checked_add(1).ok_or(AppError::Conflict)?;
            tx.execute("UPDATE source_bindings SET recovery_epoch=?2 WHERE source_id=?1",params![config.source_id.to_string(),epoch]).map_err(storage_error)?;
            for group in &config.allowed_group_ids {
                let cursor:Option<String>=tx.query_row("SELECT NULLIF(cursor,'') FROM sources WHERE namespace=?1",[namespace(config,group)],|r|r.get(0)).optional().map_err(storage_error)?.flatten();
                tx.execute("INSERT INTO source_recovery VALUES (?1,?2,?3,?4,?5,?5,0) ON CONFLICT(source_id,group_id) DO UPDATE SET epoch=excluded.epoch,since=CASE WHEN source_recovery.complete=0 THEN source_recovery.since ELSE excluded.since END,anchor=CASE WHEN source_recovery.complete=0 THEN source_recovery.anchor ELSE excluded.anchor END,recovery_cursor=CASE WHEN source_recovery.complete=0 THEN source_recovery.recovery_cursor ELSE excluded.recovery_cursor END,complete=0",params![config.source_id.to_string(),group,epoch,since,cursor]).map_err(storage_error)?;
            }
            Ok(())
        })
    }
    pub fn recoveries(
        &self,
        config: &SourceConfig,
    ) -> AppResult<Vec<super::source::GroupRecovery>> {
        self.bind_source(config)?;
        self.db.transaction(|tx|{
            let mut stmt=tx.prepare("SELECT group_id,epoch,since,anchor,recovery_cursor,complete FROM source_recovery WHERE source_id=?1 ORDER BY group_id").map_err(storage_error)?;
            let rows=stmt.query_map([config.source_id.to_string()],|r|Ok(super::source::GroupRecovery{group_id:r.get(0)?,epoch:r.get::<_,i64>(1)? as u64,since:r.get(2)?,anchor:r.get(3)?,recovery_cursor:r.get(4)?,complete:r.get(5)?})).map_err(storage_error)?;
            rows.map(|r|r.map_err(storage_error)).collect()
        })
    }
    /// Acknowledge exhaustive recovery only from the immutable original anchor,
    /// after every returned delivery persisted. Incomplete batches establish no
    /// coverage: retain/reset recovery_cursor to anchor, ignoring their last ID.
    /// CAS protects against stale epoch/completion from another receive owner.
    pub fn advance_recovery(
        &self,
        config: &SourceConfig,
        group: &str,
        epoch: u64,
        from: &str,
        next: &str,
        complete: bool,
    ) -> AppResult<()> {
        if epoch > i64::MAX as u64
            || !config.allowed_group_ids.iter().any(|g| g == group)
            || next.is_empty()
            || next.len() > 4096
            || next.contains(['/', '\\', ':'])
            || next.chars().any(char::is_control)
        {
            return Err(AppError::InvalidInput);
        }
        self.bind_source(config)?;
        self.db.transaction(|tx|{
            let updated=tx.execute("UPDATE source_recovery SET recovery_cursor=CASE WHEN ?6 THEN ?5 ELSE anchor END,complete=?6 WHERE source_id=?1 AND group_id=?2 AND epoch=?3 AND anchor=?4 AND complete=0",params![config.source_id.to_string(),group,epoch as i64,from,next,complete]).map_err(storage_error)?;
            if updated!=1{return Err(AppError::Conflict);}Ok(())
        })
    }
    pub fn pending(&self, limit: u32) -> AppResult<Vec<MessageEnvelope>> {
        self.db.transaction(|tx| {
        let mut stmt = tx.prepare("SELECT payload,processing_state FROM messages WHERE payload IS NOT NULL AND processing_state IN ('persisted','parsing','retryable_failure','pending') ORDER BY received_at,message_key LIMIT ?1").map_err(storage_error)?;
        let rows = stmt.query_map([limit], |r| Ok((r.get::<_,Vec<u8>>(0)?,r.get::<_,String>(1)?))).map_err(storage_error)?;
        rows.map(|row| { let (payload,state) = row.map_err(storage_error)?; let mut message: MessageEnvelope = self.db.unprotect(&payload)?; message.processing_state = serde_json::from_value(serde_json::Value::String(state)).map_err(|_| AppError::ParseFailed)?; Ok(message) }).collect()
    })
    }
    /// Preconditions for asynchronous callers (N3): hold message-generation
    /// coordination spanning job dispatch through completion, including edits,
    /// revocations and retries; reject stale completions before calling this API.
    /// This API has no expected_revision argument. The SQL operation mutex alone
    /// does NOT serialize a whole parse job or prevent stale evidence writes.
    pub fn record_parts(&self, key: &MessageKey, parts: Vec<PartResult>) -> AppResult<()> {
        self.db.transaction(|tx| {
        let data: Option<Vec<u8>> = tx.query_row("SELECT payload FROM messages WHERE message_key=?1 AND payload IS NOT NULL AND revoked=0", [key.to_string()], |r| r.get(0)).optional().map_err(storage_error)?;
        let mut envelope: MessageEnvelope = self.db.unprotect(&data.ok_or(AppError::Conflict)?)?;
        if parts.is_empty() { return Ok(()); }
        let previous: Option<(i64,Vec<u8>)> = tx.query_row("SELECT revision,payload FROM part_results WHERE message_key=?1", [key.to_string()], |r| Ok((r.get(0)?,r.get(1)?))).optional().map_err(storage_error)?;
        let mut merged = std::collections::HashMap::new();
        if let Some((revision, sealed)) = previous
            && revision == envelope.revision as i64 {
            let previous: Vec<PartResult> = self.db.unprotect(&sealed)?;
            for result in previous { merged.insert(result.part_id, result); }
        }
        let mut seen = std::collections::HashSet::new();
        for result in &parts { if !seen.insert(result.part_id) || result.blocks.iter().any(|b| b.part_id != result.part_id) { return Err(AppError::InvalidInput); }
            let part = envelope.parts.iter_mut().find(|p| p.part_id == result.part_id).ok_or(AppError::InvalidInput)?; part.parse_state = result.status; part.failure_code = result.reason_code;
        }
        for result in parts { merged.insert(result.part_id, result); }
        // Preserve omitted current-revision evidence; retries replace only their
        // own part result. Envelope order makes persistence deterministic.
        let results: Vec<PartResult> = envelope.parts.iter().filter_map(|part| merged.remove(&part.part_id)).collect();
        if !merged.is_empty() { return Err(AppError::ParseFailed); }
        let payload = self.db.protect(&results)?; let message_payload = self.db.protect(&envelope)?;
        tx.execute("INSERT INTO part_results VALUES (?1,?2,?3) ON CONFLICT(message_key) DO UPDATE SET revision=excluded.revision,payload=excluded.payload", params![key.to_string(),envelope.revision as i64,payload]).map_err(storage_error)?;
        tx.execute("UPDATE messages SET payload=?2 WHERE message_key=?1", params![key.to_string(),message_payload]).map_err(storage_error)?;
        Ok(())
    })
    }
    pub fn cleanup(&self, now: UtcMillis) -> AppResult<u64> {
        let cutoff = now
            .checked_sub(super::retention::NON_EVENT_RETENTION_MILLIS)
            .ok_or(AppError::InvalidInput)?;
        self.db.transaction(|tx| {
        tx.execute("DELETE FROM part_results WHERE message_key IN (SELECT message_key FROM messages WHERE payload IS NOT NULL AND processing_state='non_event' AND received_at<=?1)", [cutoff]).map_err(storage_error)?;
        let removed = tx.execute("UPDATE messages SET payload=NULL WHERE payload IS NOT NULL AND processing_state='non_event' AND received_at<=?1", [cutoff]).map_err(storage_error)?;
        Ok(removed as u64)
    })
    }
}
pub(crate) fn state_name(state: ProcessingState) -> &'static str {
    match state {
        ProcessingState::Persisted => "persisted",
        ProcessingState::Parsing => "parsing",
        ProcessingState::Committed => "committed",
        ProcessingState::RetryableFailure => "retryable_failure",
        ProcessingState::Unparseable => "unparseable",
        ProcessingState::NonEvent => "non_event",
        ProcessingState::Pending => "pending",
        ProcessingState::SourceRevoked => "source_revoked",
    }
}
