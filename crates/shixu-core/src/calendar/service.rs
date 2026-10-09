use super::{
    changes::*,
    match_event::{
        explicit_reference, extract_current, identifier, message, validate, validate_time,
        without_reply_context,
    },
};
use crate::{
    contracts::{AppResult, calendar::*, error::AppError, notification::*},
    storage::{Database, database::storage_error},
};
use rusqlite::{OptionalExtension, Transaction, params};
use std::sync::Arc;
use uuid::Uuid;
#[derive(serde::Serialize, serde::Deserialize)]
struct BatchRecord {
    batch: ExtractBatch,
    incomplete: bool,
}
pub struct EventService {
    db: Arc<Database>,
}
impl EventService {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }
    fn event(&self, tx: &Transaction<'_>, id: EventId) -> AppResult<StoredEvent> {
        let sealed: Option<Vec<u8>> = tx
            .query_row(
                "SELECT payload FROM calendar_events WHERE event_id=?1",
                [id.to_string()],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage_error)?;
        self.db.unprotect(&sealed.ok_or(AppError::InvalidInput)?)
    }
    fn save_event(&self, tx: &Transaction<'_>, event: &StoredEvent) -> AppResult<()> {
        tx.execute("INSERT INTO calendar_events VALUES (?1,?2) ON CONFLICT(event_id) DO UPDATE SET payload=excluded.payload",params![event.event.event_id.to_string(),self.db.protect(event)?]).map_err(storage_error)?;
        Ok(())
    }
    fn source(&self, tx: &Transaction<'_>, source: &EventSource) -> AppResult<()> {
        tx.execute("INSERT INTO calendar_sources VALUES (?1,?2,?3,?4) ON CONFLICT(candidate_key) DO UPDATE SET event_id=excluded.event_id,payload=excluded.payload",params![source.candidate.candidate_key.to_string(),source.message_key.to_string(),source.event_id.map(|id|id.to_string()),self.db.protect(source)?]).map_err(storage_error)?;
        Ok(())
    }
    fn change(
        &self,
        tx: &Transaction<'_>,
        before: Option<CalendarEvent>,
        after: CalendarEvent,
        source: Option<EventSource>,
    ) -> AppResult<ChangeId> {
        let id = ChangeId::from_uuid(Uuid::new_v4());
        let c = EventChange {
            change_id: id,
            event_id: after.event_id,
            before,
            after,
            candidate_key: source.as_ref().map(|s| s.candidate.candidate_key),
            source,
            undone: false,
        };
        tx.execute(
            "INSERT INTO calendar_changes VALUES (?1,?2,?3)",
            params![id.to_string(), c.event_id.to_string(), self.db.protect(&c)?],
        )
        .map_err(storage_error)?;
        Ok(id)
    }
    fn sources_tx(&self, tx: &Transaction<'_>) -> AppResult<Vec<EventSource>> {
        let mut stmt = tx
            .prepare("SELECT payload FROM calendar_sources ORDER BY candidate_key")
            .map_err(storage_error)?;
        stmt.query_map([], |r| r.get::<_, Vec<u8>>(0))
            .map_err(storage_error)?
            .map(|r| self.db.unprotect(&r.map_err(storage_error)?))
            .collect()
    }
    /// Durable provenance, pending/conflict notices and revocation are available to later UI.
    pub fn sources(&self, id: &str) -> AppResult<Vec<EventSource>> {
        let id: EventId = id.parse()?;
        self.db.transaction(|tx| {
            self.event(tx, id)?;
            Ok(self
                .sources_tx(tx)?
                .into_iter()
                .filter(|s| s.event_id == Some(id))
                .collect())
        })
    }
    pub fn notices(&self) -> AppResult<Vec<EventSource>> {
        self.db.transaction(|tx| self.sources_tx(tx))
    }
    /// Current-revision body application outcome, independent of pending/failed attachments.
    pub fn message_applied(&self, key: MessageKey, revision: u64) -> AppResult<bool> {
        self.db.transaction(|tx| {
            let m = message(&self.db, tx, key)?;
            if m.revision != revision || m.revoked {
                return Ok(false);
            }
            let body = crate::notifications::extract::body_part_id(&m);
            let mut stmt = tx
                .prepare("SELECT payload FROM calendar_sources WHERE message_key=?1")
                .map_err(storage_error)?;
            for row in stmt
                .query_map([key.to_string()], |r| r.get::<_, Vec<u8>>(0))
                .map_err(storage_error)?
            {
                let source: EventSource = self.db.unprotect(&row.map_err(storage_error)?)?;
                if source.message_revision == revision
                    && source.event_id.is_some()
                    && source.candidate.evidence.iter().any(|e| e.part_id == body)
                    && source.outcome == SourceOutcome::Applied
                {
                    return Ok(true);
                }
            }
            Ok(false)
        })
    }
    pub fn history(&self, id: &str) -> AppResult<Vec<EventChange>> {
        let id: EventId = id.parse()?;
        self.db.transaction(|tx| {
            self.event(tx, id)?;
            let mut stmt = tx
                .prepare("SELECT payload FROM calendar_changes WHERE event_id=?1 ORDER BY rowid")
                .map_err(storage_error)?;
            stmt.query_map([id.to_string()], |r| r.get::<_, Vec<u8>>(0))
                .map_err(storage_error)?
                .map(|r| self.db.unprotect(&r.map_err(storage_error)?))
                .collect()
        })
    }
    pub fn origin(&self, id: &str) -> AppResult<EventOrigin> {
        let id = id.parse()?;
        self.db.transaction(|tx| Ok(self.event(tx, id)?.origin))
    }
    /// Explicit optional-model boundary. Every candidate is re-grounded inside
    /// the same durable transaction; this does not trust provider metadata or
    /// widen plain apply's canonical N6-only contract.
    pub fn apply_model(&self, batch: ExtractBatch) -> AppResult<ApplySummary> {
        self.apply_guarded(batch, true)
    }
    pub fn apply(&self, batch: ExtractBatch) -> AppResult<ApplySummary> {
        self.apply_guarded(batch, false)
    }
    fn apply_guarded(&self, batch: ExtractBatch, allow_model: bool) -> AppResult<ApplySummary> {
        self.db.transaction(|tx| {
            let (m, canonical) = if allow_model {
                super::match_event::validate_model(&self.db, tx, &batch)?
            } else {
                validate(&self.db, tx, &batch)?
            };
            let mut summary = ApplySummary {
                created: 0,
                updated: 0,
                cancelled: 0,
                pending: 0,
                conflicts: 0,
                change_ids: vec![],
            };
            if m.revoked {
                for mut source in self
                    .sources_tx(tx)?
                    .into_iter()
                    .filter(|s| s.message_key == m.message_key)
                {
                    source.outcome = SourceOutcome::Revoked;
                    source.message_revision = m.revision;
                    self.source(tx, &source)?;
                }
                tx.execute(
                    "UPDATE messages SET processing_state='source_revoked' WHERE message_key=?1",
                    [m.message_key.to_string()],
                )
                .map_err(storage_error)?;
                return Ok(summary);
            }
            let previous = self.sources_tx(tx)?;
            let ambiguous_edit = previous.iter().any(|p| {
                p.message_key == m.message_key
                    && p.message_revision < m.revision
                    && p.candidate.action == CandidateAction::Create
                    && !batch
                        .candidates
                        .iter()
                        .any(|c| c.candidate_key == p.candidate.candidate_key)
            });
            for candidate in &batch.candidates {
                let provenance = if canonical.candidates.contains(candidate) {
                    canonical.extractor_version.clone()
                } else {
                    format!("n7.guarded-create.1;timezone={}", candidate.time.timezone)
                };
                if ambiguous_edit
                    && candidate.action == CandidateAction::Create
                    && !previous
                        .iter()
                        .any(|p| p.candidate.candidate_key == candidate.candidate_key)
                {
                    self.source(
                        tx,
                        &EventSource {
                            message_key: m.message_key,
                            message_revision: m.revision,
                            source_order: batch.source_order,
                            extractor_version: Some(provenance.clone()),
                            source_id: m.source_id,
                            account_id: m.account_id.clone(),
                            group_id: m.group_id.clone(),
                            candidate: candidate.clone(),
                            event_id: None,
                            outcome: SourceOutcome::Conflict,
                        },
                    )?;
                    summary.conflicts += 1;
                } else {
                    self.apply_candidate(
                        tx,
                        &m,
                        batch.source_order,
                        candidate,
                        &provenance,
                        &mut summary,
                    )?;
                }
            }
            // An omitted pending candidate is not authority to replay the old
            // source interpretation when another message arrives later.
            for mut source in previous.into_iter().filter(|s| {
                s.message_key == m.message_key
                    && s.outcome == SourceOutcome::Pending
                    && !batch
                        .candidates
                        .iter()
                        .any(|c| c.candidate_key == s.candidate.candidate_key)
            }) {
                source.outcome = SourceOutcome::Conflict;
                self.source(tx, &source)?;
                summary.conflicts += 1;
            }
            let incomplete = canonical
                .part_results
                .iter()
                .any(|p| p.status != PartStatus::Success)
                || canonical.candidates.len() != batch.candidates.len();
            self.save_batch(
                tx,
                &BatchRecord {
                    batch: batch.clone(),
                    incomplete,
                },
            )?;
            self.refresh_state(tx, m.message_key)?;
            self.retry_pending(tx, &m, &mut summary)?;
            Ok(summary)
        })
    }
    fn save_batch(&self, tx: &Transaction<'_>, record: &BatchRecord) -> AppResult<()> {
        tx.execute("INSERT INTO calendar_batches VALUES (?1,?2) ON CONFLICT(message_key) DO UPDATE SET payload=excluded.payload",
            params![record.batch.message_key.to_string(),self.db.protect(record)?]).map_err(storage_error)?;
        Ok(())
    }
    fn retry_pending(
        &self,
        tx: &Transaction<'_>,
        arrived: &MessageEnvelope,
        summary: &mut ApplySummary,
    ) -> AppResult<()> {
        let pending: Vec<_> = self
            .sources_tx(tx)?
            .into_iter()
            .filter(|s| s.outcome == SourceOutcome::Pending && s.message_key != arrived.message_key)
            .collect();
        for mut source in pending {
            let current = message(&self.db, tx, source.message_key)?;
            // The persisted reply relationship survives extraction before the
            // original message itself was available. Targetless snapshots do not.
            if current.reply_to != Some(arrived.message_key)
                && source.candidate.target_message_key != Some(arrived.message_key)
            {
                continue;
            }
            if current.revoked {
                continue;
            }
            let saved: Option<Vec<u8>> = tx
                .query_row(
                    "SELECT payload FROM calendar_batches WHERE message_key=?1",
                    [source.message_key.to_string()],
                    |r| r.get(0),
                )
                .optional()
                .map_err(storage_error)?;
            let saved: Option<BatchRecord> = saved.map(|v| self.db.unprotect(&v)).transpose()?;
            let Some(mut record) = saved else { continue };
            if current.revision != record.batch.message_revision
                || current.revision != source.message_revision
            {
                continue;
            }
            let recomputed = extract_current(&self.db, tx, source.message_key);
            let fresh = match recomputed {
                Ok((_, fresh)) => fresh,
                Err(AppError::InvalidInput | AppError::Conflict) => {
                    source.outcome = SourceOutcome::Conflict;
                    self.source(tx, &source)?;
                    summary.conflicts += 1;
                    self.refresh_state(tx, current.message_key)?;
                    continue;
                }
                Err(error) => return Err(error),
            };
            let supplied = record
                .batch
                .candidates
                .iter()
                .position(|c| c.candidate_key == source.candidate.candidate_key);
            let grounded = fresh
                .candidates
                .iter()
                .find(|c| c.candidate_key == source.candidate.candidate_key);
            let accepted = match (supplied, grounded) {
                (Some(index), Some(candidate)) => {
                    let saved = &record.batch.candidates[index];
                    if saved == candidate {
                        Some((index, candidate.clone()))
                    } else if saved.target_message_key.is_none()
                        && candidate.target_message_key.is_some()
                        && candidate.target_message_key == current.reply_to
                    {
                        let independent = without_reply_context(&current, &fresh)?;
                        if independent.candidates.iter().any(|c| c == saved) {
                            Some((index, candidate.clone()))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                }
                _ => None,
            };
            if let Some((index, candidate)) = accepted {
                // Apply this exact freshly grounded value, not source.candidate.
                self.apply_candidate(
                    tx,
                    &current,
                    fresh.source_order,
                    &candidate,
                    &fresh.extractor_version,
                    summary,
                )?;
                record.batch.candidates[index] = candidate;
                record.batch.part_results = fresh.part_results.clone();
                record.incomplete = fresh
                    .part_results
                    .iter()
                    .any(|p| p.status != PartStatus::Success)
                    || fresh.candidates.len() != record.batch.candidates.len();
                self.save_batch(tx, &record)?;
            } else {
                source.outcome = SourceOutcome::Conflict;
                self.source(tx, &source)?;
                summary.conflicts += 1;
            }
            self.refresh_state(tx, current.message_key)?;
        }
        Ok(())
    }
    fn refresh_state(&self, tx: &Transaction<'_>, key: MessageKey) -> AppResult<()> {
        let payload: Option<Vec<u8>> = tx
            .query_row(
                "SELECT payload FROM calendar_batches WHERE message_key=?1",
                [key.to_string()],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage_error)?;
        let Some(payload) = payload else {
            return Ok(());
        };
        let record: BatchRecord = self.db.unprotect(&payload)?;
        let unresolved = self.sources_tx(tx)?.iter().any(|s| {
            s.message_key == key
                && matches!(s.outcome, SourceOutcome::Pending | SourceOutcome::Conflict)
        });
        let state = if record.incomplete
            || unresolved
            || record.batch.candidates.iter().any(|c| {
                c.time.precision == Precision::UnknownDate && c.action != CandidateAction::Cancel
            }) {
            "pending"
        } else if record.batch.candidates.is_empty() {
            if self
                .sources_tx(tx)?
                .iter()
                .any(|source| source.message_key == key)
            {
                "pending"
            } else {
                "non_event"
            }
        } else {
            "committed"
        };
        tx.execute(
            "UPDATE messages SET processing_state=?2 WHERE message_key=?1 AND revoked=0",
            params![key.to_string(), state],
        )
        .map_err(storage_error)?;
        Ok(())
    }
    fn apply_candidate(
        &self,
        tx: &Transaction<'_>,
        m: &MessageEnvelope,
        order: u64,
        c: &Candidate,
        provenance: &str,
        summary: &mut ApplySummary,
    ) -> AppResult<()> {
        let prior: Option<Vec<u8>> = tx
            .query_row(
                "SELECT payload FROM calendar_sources WHERE candidate_key=?1",
                [c.candidate_key.to_string()],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage_error)?;
        let prior: Option<EventSource> = prior.map(|v| self.db.unprotect(&v)).transpose()?;
        let suppressed: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM calendar_suppressions WHERE candidate_key=?1)",
                [c.candidate_key.to_string()],
                |r| r.get(0),
            )
            .map_err(storage_error)?;
        if let Some(p) = &prior {
            if p.message_key != m.message_key
                || p.message_revision > m.revision
                || p.source_order != order
            {
                return Err(AppError::Conflict);
            }
            if p.message_revision == m.revision
                && p.candidate == *c
                && p.outcome != SourceOutcome::Pending
                && !suppressed
            {
                return Ok(());
            }
        }
        let mut source = EventSource {
            message_key: m.message_key,
            message_revision: m.revision,
            source_order: order,
            extractor_version: Some(provenance.into()),
            source_id: m.source_id,
            account_id: m.account_id.clone(),
            group_id: m.group_id.clone(),
            candidate: c.clone(),
            event_id: prior.as_ref().and_then(|p| p.event_id),
            outcome: SourceOutcome::Pending,
        };
        if suppressed {
            source.outcome = SourceOutcome::Suppressed;
            self.source(tx, &source)?;
            return Ok(());
        }
        let event_id = if c.action == CandidateAction::Create {
            source.event_id
        } else if c.target_message_key.is_none() && m.reply_to.is_none() {
            self.explicit_target(tx, m, c)?
        } else {
            let matching: Vec<_> = self
                .sources_tx(tx)?
                .into_iter()
                .filter(|s| {
                    Some(s.message_key) == c.target_message_key
                        && s.source_id == m.source_id
                        && s.account_id == m.account_id
                        && s.group_id == m.group_id
                        && s.candidate.action == CandidateAction::Create
                        && s.candidate.kind == c.kind
                        && crate::notifications::extract::subject(&s.candidate.title)
                            == crate::notifications::extract::subject(&c.title)
                })
                .filter_map(|s| s.event_id)
                .collect();
            let unique: std::collections::HashSet<_> = matching.into_iter().collect();
            if unique.len() == 1 {
                unique.into_iter().next()
            } else {
                None
            }
        };
        if event_id.is_none() && c.action != CandidateAction::Create {
            summary.pending += 1;
            self.source(tx, &source)?;
            return Ok(());
        }
        if let Some(id) = event_id {
            source.event_id = Some(id);
            let mut stored = self.event(tx, id)?;
            let before = stored.event.clone();
            let newer = if stored.last_message == Some(m.message_key) {
                m.revision >= stored.last_revision && order == stored.last_order
            } else {
                order > stored.last_order
            };
            if !newer || before.status == EventStatus::Removed {
                summary.conflicts += 1;
                source.outcome = SourceOutcome::Conflict;
                self.source(tx, &source)?;
                return Ok(());
            }
            let mut next = before.clone();
            let mut conflict = false;
            if c.action == CandidateAction::Cancel {
                assign(
                    &mut next.status,
                    EventStatus::Cancelled,
                    EventField::Status,
                    &before.user_overrides,
                    &mut conflict,
                )
            } else {
                if c.action == CandidateAction::Create {
                    assign(
                        &mut next.title,
                        c.title.clone(),
                        EventField::Title,
                        &before.user_overrides,
                        &mut conflict,
                    )
                }
                let mut time = event_time(&next);
                assign(
                    &mut time,
                    c.time.clone(),
                    EventField::Time,
                    &before.user_overrides,
                    &mut conflict,
                );
                set_time(&mut next, &time);
                assign(
                    &mut next.location,
                    c.location.clone(),
                    EventField::Location,
                    &before.user_overrides,
                    &mut conflict,
                );
            }
            if conflict {
                summary.conflicts += 1;
                source.outcome = SourceOutcome::Conflict
            } else {
                source.outcome = SourceOutcome::Applied
            }
            if next != before {
                next.revision = advance(before.revision)?;
                stored.event = next.clone();
                self.save_event(tx, &stored)?;
                summary.change_ids.push(self.change(
                    tx,
                    Some(before),
                    next,
                    Some(source.clone()),
                )?);
                if c.action == CandidateAction::Cancel {
                    summary.cancelled += 1
                } else {
                    summary.updated += 1
                }
            }
            stored.last_message = Some(m.message_key);
            stored.last_order = order;
            stored.last_revision = m.revision;
            self.save_event(tx, &stored)?;
        } else {
            let id = EventId::from_uuid(*c.candidate_key.as_uuid());
            let mut event = CalendarEvent {
                event_id: id,
                title: c.title.clone(),
                kind: c.kind.clone(),
                time_precision: Precision::UnknownDate,
                local_date: None,
                start_at: None,
                end_at: None,
                timezone: c.time.timezone.clone(),
                raw_time_text: String::new(),
                location: c.location.clone(),
                status: EventStatus::Active,
                revision: 1,
                user_overrides: vec![],
            };
            set_time(&mut event, &c.time);
            let stored = StoredEvent {
                event: event.clone(),
                origin: EventOrigin::Source,
                last_message: Some(m.message_key),
                last_order: order,
                last_revision: m.revision,
            };
            if tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM calendar_events WHERE event_id=?1)",
                    [id.to_string()],
                    |r| r.get::<_, bool>(0),
                )
                .map_err(storage_error)?
            {
                return Err(AppError::Conflict);
            }
            self.save_event(tx, &stored)?;
            source.event_id = Some(id);
            source.outcome = SourceOutcome::Applied;
            summary.created += 1;
            summary
                .change_ids
                .push(self.change(tx, None, event, Some(source.clone()))?);
        }
        self.source(tx, &source)?;
        Ok(())
    }
    fn explicit_target(
        &self,
        tx: &Transaction<'_>,
        m: &MessageEnvelope,
        c: &Candidate,
    ) -> AppResult<Option<EventId>> {
        let Some(reference) = explicit_reference(c, m.sent_at)? else {
            return Ok(None);
        };
        let mut originals = self.sources_tx(tx)?;
        // Original-date proof survives later source edits. These protected
        // immutable snapshots were grounded when the automatic change committed.
        let mut stmt = tx
            .prepare("SELECT payload FROM calendar_changes")
            .map_err(storage_error)?;
        for row in stmt
            .query_map([], |r| r.get::<_, Vec<u8>>(0))
            .map_err(storage_error)?
        {
            let change: EventChange = self.db.unprotect(&row.map_err(storage_error)?)?;
            if let Some(source) = change.source {
                originals.push(source)
            }
        }
        let mut matching = std::collections::HashSet::new();
        for original in originals {
            if original.source_id != m.source_id
                || original.account_id != m.account_id
                || original.group_id != m.group_id
                || original.candidate.action != CandidateAction::Create
                || original.candidate.kind != c.kind
                || original.candidate.time.local_date != reference.time.local_date
                || reference
                    .time
                    .start_at
                    .is_some_and(|start| original.candidate.time.start_at != Some(start))
                || reference
                    .time
                    .end_at
                    .is_some_and(|end| original.candidate.time.end_at != Some(end))
            {
                continue;
            }
            let Some(id) = original.event_id else {
                continue;
            };
            let same_namespace:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM messages AS original JOIN messages AS notice ON original.namespace=notice.namespace WHERE original.message_key=?1 AND notice.message_key=?2 AND original.revoked=0)",
                params![original.message_key.to_string(),m.message_key.to_string()],|r|r.get(0)).map_err(storage_error)?;
            if !same_namespace {
                continue;
            }
            let identifiers: std::collections::HashSet<_> = original
                .candidate
                .evidence
                .iter()
                .filter_map(|e| identifier(&e.text))
                .collect();
            if identifiers.len() == 1 && identifiers.contains(&reference.identifier) {
                matching.insert(id);
            }
        }
        Ok(if matching.len() == 1 {
            matching.into_iter().next()
        } else {
            None
        })
    }
    pub fn query(&self, q: EventQuery) -> AppResult<Vec<CalendarEvent>> {
        for d in [&q.from_date, &q.through_date].into_iter().flatten() {
            if chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d")
                .map_err(|_| AppError::InvalidInput)?
                .format("%Y-%m-%d")
                .to_string()
                != *d
            {
                return Err(AppError::InvalidInput);
            }
        }
        if q.from_date
            .as_ref()
            .zip(q.through_date.as_ref())
            .is_some_and(|(a, b)| a > b)
        {
            return Err(AppError::InvalidInput);
        }
        self.db.transaction(|tx| {
            let mut stmt = tx
                .prepare("SELECT payload FROM calendar_events ORDER BY event_id")
                .map_err(storage_error)?;
            let mut events = vec![];
            for row in stmt
                .query_map([], |r| r.get::<_, Vec<u8>>(0))
                .map_err(storage_error)?
            {
                let stored: StoredEvent = self.db.unprotect(&row.map_err(storage_error)?)?;
                let e = stored.event;
                if !q.statuses.is_empty() && !q.statuses.contains(&e.status) {
                    continue;
                }
                if let Some(date) = &e.local_date {
                    if q.from_date.as_ref().is_some_and(|d| date < d)
                        || q.through_date.as_ref().is_some_and(|d| date > d)
                    {
                        continue;
                    }
                } else if !q.include_pending {
                    continue;
                }
                events.push(e)
            }
            events.sort_by_key(|e| (e.local_date.clone(), e.start_at, e.event_id.to_string()));
            Ok(events)
        })
    }
    pub fn edit(
        &self,
        id: &str,
        expected_revision: u64,
        patch: EventPatch,
    ) -> AppResult<CalendarEvent> {
        let id: EventId = id.parse()?;
        if id != patch.event_id || expected_revision != patch.expected_revision {
            return Err(AppError::InvalidInput);
        }
        self.db.transaction(|tx| {
            let mut stored = self.event(tx, id)?;
            if stored.event.revision != expected_revision {
                return Err(AppError::Conflict);
            }
            let before = stored.event.clone();
            apply_patch(&mut stored.event, &patch)?;
            stored.event.revision = advance(expected_revision)?;
            self.save_event(tx, &stored)?;
            self.change(tx, Some(before), stored.event.clone(), None)?;
            if stored.event.status == EventStatus::Removed {
                for s in self
                    .sources_tx(tx)?
                    .into_iter()
                    .filter(|s| s.event_id == Some(id))
                {
                    self.suppress(tx, s.candidate.candidate_key)?;
                }
            }
            Ok(stored.event)
        })
    }
    pub fn create_manual(&self, patch: EventPatch) -> AppResult<CalendarEvent> {
        if patch.expected_revision != 0
            || patch.title.as_ref().is_none_or(|s| s.trim().is_empty())
            || patch.time.is_none()
        {
            return Err(AppError::InvalidInput);
        }
        validate_time(patch.time.as_ref().ok_or(AppError::InvalidInput)?)?;
        self.db.transaction(|tx| {
            if tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM calendar_events WHERE event_id=?1)",
                    [patch.event_id.to_string()],
                    |r| r.get::<_, bool>(0),
                )
                .map_err(storage_error)?
            {
                return Err(AppError::Conflict);
            }
            let mut event = CalendarEvent {
                event_id: patch.event_id,
                title: patch.title.clone().ok_or(AppError::InvalidInput)?,
                kind: "manual".into(),
                time_precision: Precision::UnknownDate,
                local_date: None,
                start_at: None,
                end_at: None,
                timezone: String::new(),
                raw_time_text: String::new(),
                location: None,
                status: EventStatus::Active,
                revision: 1,
                user_overrides: vec![],
            };
            apply_patch(&mut event, &patch)?;
            self.save_event(
                tx,
                &StoredEvent {
                    event: event.clone(),
                    origin: EventOrigin::Manual,
                    last_message: None,
                    last_order: 0,
                    last_revision: 0,
                },
            )?;
            self.change(tx, None, event.clone(), None)?;
            Ok(event)
        })
    }
    fn suppress(&self, tx: &Transaction<'_>, key: CandidateKey) -> AppResult<()> {
        tx.execute("INSERT INTO calendar_suppressions VALUES (?1,?2) ON CONFLICT(candidate_key) DO NOTHING",params![key.to_string(),self.db.protect(&"user_removed")?]).map_err(storage_error)?;
        Ok(())
    }
    pub fn undo(&self, request: UndoRequest) -> AppResult<CalendarEvent> {
        self.db.transaction(|tx| {
            let sealed: Option<Vec<u8>> = tx
                .query_row(
                    "SELECT payload FROM calendar_changes WHERE change_id=?1",
                    [request.change_id.to_string()],
                    |r| r.get(0),
                )
                .optional()
                .map_err(storage_error)?;
            let mut change: EventChange =
                self.db.unprotect(&sealed.ok_or(AppError::InvalidInput)?)?;
            let mut stored = self.event(tx, change.event_id)?;
            if change.undone
                || stored.event.revision != request.expected_revision
                || change.after.revision != request.expected_revision
            {
                return Err(AppError::Conflict);
            }
            let before = stored.event.clone();
            if let Some(old) = &change.before {
                stored.event = old.clone()
            } else {
                stored.event.status = EventStatus::Removed;
                for s in self
                    .sources_tx(tx)?
                    .into_iter()
                    .filter(|s| s.event_id == Some(change.event_id))
                {
                    self.suppress(tx, s.candidate.candidate_key)?;
                }
            }
            stored.event.revision = advance(before.revision)?;
            self.save_event(tx, &stored)?;
            change.undone = true;
            tx.execute(
                "UPDATE calendar_changes SET payload=?2 WHERE change_id=?1",
                params![change.change_id.to_string(), self.db.protect(&change)?],
            )
            .map_err(storage_error)?;
            self.change(tx, Some(before), stored.event.clone(), None)?;
            Ok(stored.event)
        })
    }
    pub fn allow_suppressed(&self, key: &str) -> AppResult<()> {
        let key: CandidateKey = key.parse()?;
        self.db.transaction(|tx| {
            let sealed: Option<Vec<u8>> = tx
                .query_row(
                    "SELECT payload FROM calendar_sources WHERE candidate_key=?1",
                    [key.to_string()],
                    |r| r.get(0),
                )
                .optional()
                .map_err(storage_error)?;
            let mut s: EventSource = self.db.unprotect(&sealed.ok_or(AppError::InvalidInput)?)?;
            if tx
                .execute(
                    "DELETE FROM calendar_suppressions WHERE candidate_key=?1",
                    [key.to_string()],
                )
                .map_err(storage_error)?
                == 0
            {
                return Ok(());
            }
            if let Some(id) = s.event_id {
                let mut stored = self.event(tx, id)?;
                if stored.event.status == EventStatus::Removed {
                    let before = stored.event.clone();
                    stored.event.status = EventStatus::Active;
                    stored
                        .event
                        .user_overrides
                        .retain(|f| *f != EventField::Status);
                    stored.event.revision = advance(stored.event.revision)?;
                    self.save_event(tx, &stored)?;
                    self.change(tx, Some(before), stored.event, None)?;
                }
            }
            s.outcome = SourceOutcome::Pending;
            self.source(tx, &s)?;
            Ok(())
        })
    }
}
fn advance(r: u64) -> AppResult<u64> {
    r.checked_add(1)
        .filter(|v| *v <= i64::MAX as u64)
        .ok_or(AppError::Conflict)
}
fn assign<T: PartialEq>(
    value: &mut T,
    new: T,
    field: EventField,
    overrides: &[EventField],
    conflict: &mut bool,
) {
    if *value != new {
        if overrides.contains(&field) {
            *conflict = true
        } else {
            *value = new
        }
    }
}
fn event_time(e: &CalendarEvent) -> TimeValue {
    TimeValue {
        precision: e.time_precision,
        local_date: e.local_date.clone(),
        start_at: e.start_at,
        end_at: e.end_at,
        timezone: e.timezone.clone(),
        raw_time_text: e.raw_time_text.clone(),
    }
}
fn set_time(e: &mut CalendarEvent, t: &TimeValue) {
    e.time_precision = t.precision;
    e.local_date = t.local_date.clone();
    e.start_at = t.start_at;
    e.end_at = t.end_at;
    e.timezone = t.timezone.clone();
    e.raw_time_text = t.raw_time_text.clone();
}
fn apply_patch(e: &mut CalendarEvent, p: &EventPatch) -> AppResult<()> {
    let mut fields = vec![];
    if let Some(title) = &p.title {
        if title.trim().is_empty() {
            return Err(AppError::InvalidInput);
        }
        e.title = title.clone();
        fields.push(EventField::Title)
    }
    if let Some(time) = &p.time {
        validate_time(time)?;
        set_time(e, time);
        fields.push(EventField::Time)
    }
    if let Some(location) = &p.location {
        e.location = match location {
            LocationPatch::Set(v) => {
                if v.trim().is_empty() {
                    return Err(AppError::InvalidInput);
                }
                Some(v.clone())
            }
            LocationPatch::Clear => None,
        };
        fields.push(EventField::Location)
    }
    if let Some(status) = p.status {
        e.status = status;
        fields.push(EventField::Status)
    }
    for field in fields {
        if !e.user_overrides.contains(&field) {
            e.user_overrides.push(field)
        }
    }
    Ok(())
}
