use crate::{
    contracts::{AppResult, calendar::*, error::AppError, notification::*},
    notifications::extract::{body_part_id, context_evidence_target, extract},
    storage::{Database, database::storage_error},
};
use rusqlite::{OptionalExtension, Transaction};
pub(crate) fn message(
    db: &Database,
    tx: &Transaction<'_>,
    key: MessageKey,
) -> AppResult<MessageEnvelope> {
    let sealed: Option<Vec<u8>> = tx
        .query_row(
            "SELECT payload FROM messages WHERE message_key=?1 AND payload IS NOT NULL",
            [key.to_string()],
            |r| r.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    db.unprotect(&sealed.ok_or(AppError::Conflict)?)
}
pub(crate) fn validate_time(t: &TimeValue) -> AppResult<()> {
    let zone: chrono_tz::Tz = t.timezone.parse().map_err(|_| AppError::InvalidInput)?;
    let date = t
        .local_date
        .as_ref()
        .map(|d| {
            chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").map_err(|_| AppError::InvalidInput)
        })
        .transpose()?;
    if date
        .as_ref()
        .zip(t.local_date.as_ref())
        .is_some_and(|(d, s)| d.format("%Y-%m-%d").to_string() != *s)
    {
        return Err(AppError::InvalidInput);
    }
    let valid = match t.precision {
        Precision::UnknownDate => date.is_none() && t.start_at.is_none() && t.end_at.is_none(),
        Precision::DateOnly | Precision::ExplicitAllDay => {
            date.is_some() && t.start_at.is_none() && t.end_at.is_none()
        }
        Precision::Exact => date.is_some() && t.start_at.is_some(),
    };
    if !valid
        || t.end_at
            .is_some_and(|end| t.start_at.is_none_or(|start| end < start))
    {
        return Err(AppError::InvalidInput);
    }
    if let Some(start) = t.start_at {
        let start = chrono::DateTime::from_timestamp_millis(start).ok_or(AppError::InvalidInput)?;
        if Some(start.with_timezone(&zone).date_naive()) != date {
            return Err(AppError::InvalidInput);
        }
    }
    if t.end_at
        .is_some_and(|v| chrono::DateTime::from_timestamp_millis(v).is_none())
    {
        return Err(AppError::InvalidInput);
    }
    Ok(())
}
fn body(e: &EvidenceBlock, m: &MessageEnvelope) -> bool {
    e.part_id == body_part_id(m)
        && e.method == Method::NativeText
        && e.page_or_sheet.is_none()
        && match e.cell_range_or_bbox {
            Some(EvidenceLocation::TextSpan { start, end }) => {
                m.text.get(start as usize..end as usize) == Some(e.text.as_str())
            }
            _ => false,
        }
}
fn subblock(e: &EvidenceBlock, original: &EvidenceBlock) -> bool {
    if e.part_id != original.part_id
        || e.method != original.method
        || e.engine_version != original.engine_version
        || e.page_or_sheet != original.page_or_sheet
        || !original
            .quality_flags
            .iter()
            .all(|f| e.quality_flags.contains(f))
    {
        return false;
    }
    match (&e.cell_range_or_bbox, &original.cell_range_or_bbox) {
        (
            Some(EvidenceLocation::TextSpan { start, end }),
            Some(EvidenceLocation::TextSpan {
                start: base,
                end: limit,
            }),
        ) => {
            start >= base
                && end <= limit
                && original
                    .text
                    .get((start - base) as usize..(end - base) as usize)
                    == Some(e.text.as_str())
        }
        (a, b) => a == b && original.text.contains(&e.text),
    }
}
pub(crate) fn validate(
    db: &Database,
    tx: &Transaction<'_>,
    batch: &ExtractBatch,
) -> AppResult<(MessageEnvelope, ExtractBatch)> {
    validate_with_model(db, tx, batch, false)
}
pub(crate) fn validate_model(
    db: &Database,
    tx: &Transaction<'_>,
    batch: &ExtractBatch,
) -> AppResult<(MessageEnvelope, ExtractBatch)> {
    validate_with_model(db, tx, batch, true)
}
fn validate_with_model(
    db: &Database,
    tx: &Transaction<'_>,
    batch: &ExtractBatch,
    allow_model: bool,
) -> AppResult<(MessageEnvelope, ExtractBatch)> {
    let m = message(db, tx, batch.message_key)?;
    let revision: i64 = tx
        .query_row(
            "SELECT revision FROM messages WHERE message_key=?1",
            [m.message_key.to_string()],
            |r| r.get(0),
        )
        .map_err(storage_error)?;
    if m.revision != batch.message_revision
        || revision as u64 != batch.message_revision
        || u64::try_from(m.sent_at).ok() != Some(batch.source_order)
    {
        return Err(AppError::Conflict);
    }
    let mut seen = tx
        .prepare("SELECT payload FROM calendar_sources WHERE message_key=?1")
        .map_err(storage_error)?;
    for row in seen
        .query_map([m.message_key.to_string()], |r| r.get::<_, Vec<u8>>(0))
        .map_err(storage_error)?
    {
        let source: super::changes::EventSource = db.unprotect(&row.map_err(storage_error)?)?;
        if source.source_order != batch.source_order {
            return Err(AppError::Conflict);
        }
    }
    let proof = crate::notifications::settings::proof(db, tx, &m)?;
    let (account, groups, zone, adapter) = (
        proof.account_id,
        proof.allowed_group_ids,
        proof.timezone,
        proof.adapter_type,
    );
    if account != m.account_id
        || !groups.contains(&m.group_id)
        || (batch.extractor_version != format!("n6.rules.1;timezone={zone}")
            && !(allow_model
                && batch.extractor_version
                    == crate::notifications::model::composite_version(&zone)))
    {
        return Err(AppError::InvalidInput);
    }
    let namespace:String=tx.query_row("SELECT messages.namespace FROM messages JOIN sources USING(namespace) WHERE message_key=?1 AND sources.source_id=?2 AND sources.adapter_type=?3 AND sources.account_id=?4 AND sources.group_id=?5",rusqlite::params![m.message_key.to_string(),m.source_id.to_string(),adapter,m.account_id,m.group_id],|r|r.get(0)).optional().map_err(storage_error)?.ok_or(AppError::InvalidInput)?;
    let context = if let Some(key) = m.reply_to {
        match message(db, tx, key) {
            Ok(target) => {
                let target_namespace: String = tx
                    .query_row(
                        "SELECT namespace FROM messages WHERE message_key=?1",
                        [target.message_key.to_string()],
                        |r| r.get(0),
                    )
                    .map_err(storage_error)?;
                if target_namespace != namespace {
                    return Err(AppError::InvalidInput);
                }
                if target.source_id != m.source_id
                    || target.account_id != m.account_id
                    || target.group_id != m.group_id
                {
                    return Err(AppError::InvalidInput);
                }
                vec![target]
            }
            Err(AppError::Conflict) => vec![],
            Err(e) => return Err(e),
        }
    } else {
        vec![]
    };
    let stored: Option<Vec<u8>> = tx
        .query_row(
            "SELECT payload FROM part_results WHERE message_key=?1 AND revision=?2",
            rusqlite::params![m.message_key.to_string(), m.revision as i64],
            |r| r.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    let parts: Vec<PartResult> = stored
        .map(|s| db.unprotect(&s))
        .transpose()?
        .unwrap_or_default();
    let mut blocks = vec![];
    for p in &parts {
        if !m.parts.iter().any(|mp| mp.part_id == p.part_id) {
            return Err(AppError::InvalidInput);
        }
        if matches!(p.status, PartStatus::Success | PartStatus::PartialParse) {
            blocks.extend(p.blocks.clone());
        }
    }
    let canonical = extract(&m, &blocks, &context, &zone)?;
    if allow_model {
        let fallback_evidence: Vec<_> = batch
            .candidates
            .iter()
            .filter(|c| {
                !canonical
                    .candidates
                    .iter()
                    .any(|rule| rule.candidate_key == c.candidate_key)
            })
            .flat_map(|c| c.evidence.clone())
            .collect();
        crate::notifications::model::bounded(&fallback_evidence)?;
        if !fallback_evidence.is_empty()
            && batch.extractor_version != crate::notifications::model::composite_version(&zone)
        {
            return Err(AppError::InvalidInput);
        }
    }
    let check = |e: &EvidenceBlock| -> AppResult<()> {
        if let Some(target) = context_evidence_target(e)? {
            let target = context
                .iter()
                .find(|m| m.message_key == target && !m.revoked)
                .ok_or(AppError::InvalidInput)?;
            if !body(e, target) {
                return Err(AppError::InvalidInput);
            }
        } else if e.part_id == body_part_id(&m) {
            if e.engine_version != "n6.body.utf8-bytes.1" || !body(e, &m) {
                return Err(AppError::InvalidInput);
            }
        } else if !blocks.iter().any(|p| subblock(e, p)) {
            return Err(AppError::InvalidInput);
        }
        Ok(())
    };
    let mut keys = std::collections::HashSet::new();
    for c in &batch.candidates {
        if !keys.insert(c.candidate_key) || c.evidence.is_empty() || c.time.timezone != zone {
            return Err(AppError::InvalidInput);
        }
        validate_time(&c.time)?;
        for e in &c.evidence {
            check(e)?;
            if context_evidence_target(e)?.is_some_and(|key| Some(key) != c.target_message_key) {
                return Err(AppError::InvalidInput);
            }
        }
        let fallback;
        let expected = match canonical
            .candidates
            .iter()
            .find(|candidate| candidate.candidate_key == c.candidate_key)
        {
            Some(expected) => expected,
            None if allow_model => {
                if c.evidence.len() != 1 {
                    return Err(AppError::InvalidInput);
                }
                fallback =
                    crate::notifications::model::grounded_candidate(&m, &c.evidence[0], &zone)?;
                let mut current_evidence = blocks.clone();
                current_evidence.push(EvidenceBlock {
                    part_id: body_part_id(&m),
                    page_or_sheet: None,
                    cell_range_or_bbox: Some(EvidenceLocation::TextSpan {
                        start: 0,
                        end: u32::try_from(m.text.len()).map_err(|_| AppError::InvalidInput)?,
                    }),
                    text: m.text.clone(),
                    method: Method::NativeText,
                    engine_version: "n6.body.utf8-bytes.1".into(),
                    quality_flags: vec![],
                });
                for block in &mut current_evidence {
                    if parts
                        .iter()
                        .any(|p| p.part_id == block.part_id && p.status == PartStatus::PartialParse)
                        && !block.quality_flags.contains(&QualityFlag::PartialSource)
                    {
                        block.quality_flags.push(QualityFlag::PartialSource);
                    }
                }
                // Full original layout units are required for fallback Creates:
                // a true subspan can still strip negation or competing dates.
                if !current_evidence.contains(&c.evidence[0]) {
                    return Err(AppError::InvalidInput);
                }
                crate::notifications::model::validate_corroboration(
                    &m,
                    &fallback,
                    &current_evidence,
                    &zone,
                )?;
                if c.candidate_key != fallback.candidate_key {
                    return Err(AppError::InvalidInput);
                }
                &fallback
            }
            None => return Err(AppError::InvalidInput),
        };
        if c.title != expected.title
            || c.kind != expected.kind
            || c.action != expected.action
            || c.target_message_key != expected.target_message_key
            || c.location != expected.location
            || c.time != expected.time
            || c.evidence != expected.evidence
        {
            return Err(AppError::InvalidInput);
        }
    }
    for p in &batch.part_results {
        if p.part_id == body_part_id(&m) {
            if canonical
                .part_results
                .iter()
                .find(|b| b.part_id == p.part_id)
                != Some(p)
            {
                return Err(AppError::InvalidInput);
            }
        } else if parts.iter().find(|b| b.part_id == p.part_id) != Some(p)
            && !(p.blocks.is_empty()
                && m.parts.iter().any(|mp| {
                    mp.part_id == p.part_id
                        && mp.parse_state == p.status
                        && mp.failure_code == p.reason_code
                }))
        {
            return Err(AppError::InvalidInput);
        }
        for e in &p.blocks {
            check(e)?
        }
    }
    Ok((m, canonical))
}

/// Recompute from current durable inputs under the caller's transaction. The
/// empty header is internal; no supplied candidate or saved source is trusted.
pub(crate) fn extract_current(
    db: &Database,
    tx: &Transaction<'_>,
    key: MessageKey,
) -> AppResult<(MessageEnvelope, ExtractBatch)> {
    let m = message(db, tx, key)?;
    let zone = crate::notifications::settings::proof(db, tx, &m)?.timezone;
    let header = ExtractBatch {
        message_key: key,
        message_revision: m.revision,
        source_order: u64::try_from(m.sent_at).map_err(|_| AppError::InvalidInput)?,
        candidates: vec![],
        part_results: vec![],
        extractor_version: format!("n6.rules.1;timezone={zone}"),
    };
    validate(db, tx, &header)
}
/// Proof of what an earlier targetless reply could establish independently.
pub(crate) fn without_reply_context(
    m: &MessageEnvelope,
    current: &ExtractBatch,
) -> AppResult<ExtractBatch> {
    let blocks: Vec<_> = current
        .part_results
        .iter()
        .filter(|p| p.part_id != body_part_id(m))
        .flat_map(|p| p.blocks.clone())
        .collect();
    extract(
        m,
        &blocks,
        &[],
        current
            .extractor_version
            .strip_prefix("n6.rules.1;timezone=")
            .ok_or(AppError::InvalidInput)?,
    )
}

/// Explicit identifier grammar is bounded and exact; the surrounding Chinese
/// subject is never a similarity key. A partial/oversized token fails closed.
pub(crate) fn identifier(text: &str) -> Option<String> {
    static GRAMMAR: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let grammar = GRAMMAR.get_or_init(|| {
        regex::Regex::new(r"事件编号\s*[:：]?\s*([A-Za-z0-9][A-Za-z0-9_-]{0,63})")
            .expect("static identifier grammar")
    });
    let captures: Vec<_> = grammar.captures_iter(text).collect();
    if captures.len() != 1 {
        return None;
    }
    let value = captures[0].get(1)?;
    // A supported prefix is insufficient: reject all unrecognized token
    // continuations. Chinese subject adjacency is an approved field boundary;
    // whitespace and explicit comma/colon separators also terminate the ID.
    let boundary = text[value.end()..].chars().next().is_none_or(|c| {
        c.is_whitespace()
            || matches!(c, ','|'，'|':'|'：'|'\u{3400}'..='\u{4dbf}'|'\u{4e00}'..='\u{9fff}')
    });
    if !boundary {
        return None;
    }
    Some(value.as_str().into())
}
pub(crate) struct ExplicitReference {
    pub identifier: String,
    pub time: TimeValue,
}
/// Prove the original date in the same grounded evidence block as an explicit
/// identifier. The reschedule date is excluded before parsing the old date.
pub(crate) fn explicit_reference(c: &Candidate, sent: i64) -> AppResult<Option<ExplicitReference>> {
    static DATE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let date = DATE.get_or_init(|| {
        regex::Regex::new(
            r"[0-9]{4}年[0-9]{1,2}月[0-9]{1,2}[日号]|[0-9]{4}[-/][0-9]{1,2}[-/][0-9]{1,2}",
        )
        .expect("static explicit date grammar")
    });
    let mut result: Option<ExplicitReference> = None;
    for evidence in &c.evidence {
        if context_evidence_target(evidence)?.is_some()
            || evidence
                .quality_flags
                .iter()
                .any(|f| *f != QualityFlag::UncertainDate)
        {
            continue;
        }
        let text = &evidence.text;
        let original: Vec<_> = text.match_indices("原定").collect();
        if original.len() != 1 {
            continue;
        }
        let start = original[0].0 + "原定".len();
        let changes: Vec<_> = ["改至", "改到", "调整至", "调整到", "延期至"]
            .iter()
            .flat_map(|marker| text.match_indices(marker).map(|(index, _)| index))
            .collect();
        let end = match c.action {
            CandidateAction::Reschedule if changes.len() == 1 && changes[0] >= start => changes[0],
            CandidateAction::Cancel if changes.is_empty() => text[start..]
                .find("取消")
                .map(|index| start + index)
                .unwrap_or(text.len()),
            _ => continue,
        };
        let old = &text[start..end];
        let Some(id) = identifier(old) else { continue };
        if !date.is_match(old) {
            continue;
        }
        let time = crate::notifications::time::parse_time(old, sent, &c.time.timezone)?;
        if time.local_date.is_none() {
            continue;
        }
        if let Some(previous) = &mut result {
            if previous.identifier != id
                || previous.time.local_date != time.local_date
                || previous
                    .time
                    .start_at
                    .zip(time.start_at)
                    .is_some_and(|(a, b)| a != b)
                || previous
                    .time
                    .end_at
                    .zip(time.end_at)
                    .is_some_and(|(a, b)| a != b)
            {
                return Ok(None);
            }
            // Missing clock fields make no claim. Keep every supplied endpoint
            // so an earlier date/start-only block cannot hide a later old end.
            previous.time.start_at = previous.time.start_at.or(time.start_at);
            previous.time.end_at = previous.time.end_at.or(time.end_at);
            if previous.time.start_at.is_some() {
                previous.time.precision = Precision::Exact;
            }
        } else {
            result = Some(ExplicitReference {
                identifier: id,
                time,
            });
        }
    }
    Ok(result)
}
