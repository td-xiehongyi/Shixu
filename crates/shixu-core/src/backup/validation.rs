//! Cross-row invariants shared by protected snapshots and plaintext imports.
//! Historical revisions may precede current messages; current settings do not
//! replace immutable message provenance, and omitted bodies are not re-extracted.
use crate::{
    calendar::{changes::*, match_event::validate_time, service::BatchRecord},
    contracts::{AppResult, calendar::*, error::AppError, notification::*},
    storage::{Database, database::storage_error},
};
use rusqlite::Connection;
use std::collections::HashMap;

fn require(valid: bool) -> AppResult<()> {
    if valid {
        Ok(())
    } else {
        Err(AppError::InvalidInput)
    }
}
fn event(event: &CalendarEvent) -> AppResult<()> {
    require(event.revision > 0 && event.revision <= i64::MAX as u64)?;
    validate_time(&TimeValue {
        precision: event.time_precision,
        local_date: event.local_date.clone(),
        start_at: event.start_at,
        end_at: event.end_at,
        timezone: event.timezone.clone(),
        raw_time_text: event.raw_time_text.clone(),
    })
}
fn payloads<T: serde::de::DeserializeOwned>(
    db: &Database,
    c: &Connection,
    table: &str,
) -> AppResult<Vec<(String, T)>> {
    let mut stmt = c
        .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
        .map_err(storage_error)?;
    let index = stmt.column_index("payload").map_err(storage_error)?;
    stmt.query_map([], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(index)?))
    })
    .map_err(storage_error)?
    .map(|row| {
        let (key, bytes) = row.map_err(storage_error)?;
        Ok((key, db.unprotect(&bytes)?))
    })
    .collect()
}
struct MessageIdentity {
    source: SourceId,
    adapter: String,
    account: String,
    group: String,
    revision: u64,
    sent_at: Option<i64>,
}
fn source(
    source: &EventSource,
    messages: &HashMap<MessageKey, MessageIdentity>,
    events: &HashMap<EventId, StoredEvent>,
) -> AppResult<()> {
    let message = messages
        .get(&source.message_key)
        .ok_or(AppError::InvalidInput)?;
    require(
        source.message_revision > 0
            && source.message_revision <= message.revision
            && source.source_id == message.source
            && source.account_id == message.account
            && source.group_id == message.group
            && source.event_id.is_none_or(|id| events.contains_key(&id)),
    )?;
    // sent_at belongs to the retained current message. Historical edits may have
    // changed it, so only a current-revision source must match that timestamp.
    if source.message_revision == message.revision
        && let Some(sent) = message.sent_at
    {
        require(u64::try_from(sent).ok() == Some(source.source_order))?;
    }
    validate_time(&source.candidate.time)
}
pub(super) fn validate(db: &Database, c: &Connection) -> AppResult<()> {
    let mut messages = HashMap::new();
    let mut stmt = c.prepare("SELECT m.message_key,m.revision,m.native_message_id,m.identity_degraded,m.received_at,m.revoked,m.payload,s.source_id,s.adapter_type,s.account_id,s.group_id FROM messages m JOIN sources s USING(namespace)").map_err(storage_error)?;
    let mut rows = stmt.query([]).map_err(storage_error)?;
    while let Some(row) = rows.next().map_err(storage_error)? {
        let key: MessageKey = row.get::<_, String>(0).map_err(storage_error)?.parse()?;
        let revision = row.get::<_, i64>(1).map_err(storage_error)? as u64;
        let mut identity = MessageIdentity {
            source: row.get::<_, String>(7).map_err(storage_error)?.parse()?,
            adapter: row.get(8).map_err(storage_error)?,
            account: row.get(9).map_err(storage_error)?,
            group: row.get(10).map_err(storage_error)?,
            revision,
            sent_at: None,
        };
        if let Some(bytes) = row.get::<_, Option<Vec<u8>>>(6).map_err(storage_error)? {
            let m: MessageEnvelope = db.unprotect(&bytes)?;
            let degraded: bool = row.get(3).map_err(storage_error)?;
            require(
                m.message_key == key
                    && m.revision == revision
                    && m.source_id == identity.source
                    && m.account_id == identity.account
                    && m.group_id == identity.group
                    && m.received_at == row.get::<_, i64>(4).map_err(storage_error)?
                    && m.revoked == row.get::<_, bool>(5).map_err(storage_error)?
                    && (if degraded {
                        m.native_message_id.is_empty()
                    } else {
                        m.native_message_id == row.get::<_, String>(2).map_err(storage_error)?
                    }),
            )?;
            identity.sent_at = Some(m.sent_at);
        }
        messages.insert(key, identity);
    }
    for (key, config) in payloads::<SourceConfig>(db, c, "source_settings")? {
        require(key == config.source_id.to_string())?;
        let mut binding = c
            .prepare("SELECT adapter_type,account_id FROM source_bindings WHERE source_id=?1")
            .map_err(storage_error)?;
        let mut rows = binding.query([&key]).map_err(storage_error)?;
        if let Some(row) = rows.next().map_err(storage_error)? {
            require(
                config.adapter_type == row.get::<_, String>(0).map_err(storage_error)?
                    && config.account_id == row.get::<_, String>(1).map_err(storage_error)?,
            )?;
        }
    }
    for (key, config) in payloads::<SourceConfig>(db, c, "message_source_proof")? {
        let m = messages.get(&key.parse()?).ok_or(AppError::InvalidInput)?;
        require(
            config.source_id == m.source
                && config.adapter_type == m.adapter
                && config.account_id == m.account
                && config.allowed_group_ids.contains(&m.group),
        )?;
    }
    let mut events = HashMap::new();
    for (key, stored) in payloads::<StoredEvent>(db, c, "calendar_events")? {
        event(&stored.event)?;
        require(key == stored.event.event_id.to_string())?;
        match stored.last_message {
            Some(key) => {
                let m = messages.get(&key).ok_or(AppError::InvalidInput)?;
                require(stored.last_revision > 0 && stored.last_revision <= m.revision)?;
            }
            None => require(
                stored.origin == EventOrigin::Manual
                    && stored.last_revision == 0
                    && stored.last_order == 0,
            )?,
        }
        events.insert(stored.event.event_id, stored);
    }
    let mut sources = HashMap::new();
    for (key, s) in payloads::<EventSource>(db, c, "calendar_sources")? {
        let (message_key, event_id): (String, Option<String>) = c
            .query_row(
                "SELECT message_key,event_id FROM calendar_sources WHERE candidate_key=?1",
                [&key],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(storage_error)?;
        require(
            key == s.candidate.candidate_key.to_string()
                && message_key == s.message_key.to_string()
                && event_id == s.event_id.map(|id| id.to_string()),
        )?;
        source(&s, &messages, &events)?;
        sources.insert(s.candidate.candidate_key, s);
    }
    for stored in events.values() {
        if let Some(key) = stored.last_message {
            require(sources.values().any(|s| {
                s.message_key == key
                    && s.event_id == Some(stored.event.event_id)
                    && s.source_order == stored.last_order
                    && s.message_revision >= stored.last_revision
            }))?;
        }
    }
    let mut latest: HashMap<EventId, CalendarEvent> = HashMap::new();
    for (key, change) in payloads::<EventChange>(db, c, "calendar_changes")? {
        let id: String = c
            .query_row(
                "SELECT event_id FROM calendar_changes WHERE change_id=?1",
                [&key],
                |r| r.get(0),
            )
            .map_err(storage_error)?;
        require(
            key == change.change_id.to_string()
                && id == change.event_id.to_string()
                && change.after.event_id == change.event_id,
        )?;
        event(&change.after)?;
        let current = events.get(&change.event_id).ok_or(AppError::InvalidInput)?;
        require(change.after.revision <= current.event.revision)?;
        if let Some(before) = &change.before {
            event(before)?;
            require(
                before.event_id == change.event_id
                    && before.revision.checked_add(1) == Some(change.after.revision),
            )?;
        } else {
            require(change.after.revision == 1)?;
        }
        if let Some(previous) = latest.get(&change.event_id) {
            require(change.before.as_ref() == Some(previous))?;
        } else {
            require(change.before.is_none())?;
        }
        if let Some(s) = &change.source {
            source(s, &messages, &events)?;
            require(
                change.candidate_key == Some(s.candidate.candidate_key)
                    && s.event_id == Some(change.event_id)
                    && sources
                        .get(&s.candidate.candidate_key)
                        .is_some_and(|current| current.message_key == s.message_key),
            )?;
        } else {
            require(change.candidate_key.is_none())?;
        }
        latest.insert(change.event_id, change.after);
    }
    for (id, stored) in &events {
        require(latest.get(id) == Some(&stored.event))?;
    }
    for (key, record) in payloads::<BatchRecord>(db, c, "calendar_batches")? {
        let b = record.batch;
        let m = messages.get(&b.message_key).ok_or(AppError::InvalidInput)?;
        require(
            key == b.message_key.to_string()
                && b.message_revision > 0
                && b.message_revision <= m.revision,
        )?;
        if b.message_revision == m.revision
            && let Some(sent) = m.sent_at
        {
            require(u64::try_from(sent).ok() == Some(b.source_order))?;
        }
        for candidate in b.candidates {
            validate_time(&candidate.time)?;
        }
    }
    Ok(())
}
