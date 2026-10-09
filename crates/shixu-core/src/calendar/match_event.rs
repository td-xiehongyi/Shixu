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
    let binding: Option<(String, String, String,String)> = tx
        .query_row(
            "SELECT account_id,groups_json,timezone,adapter_type FROM source_bindings WHERE source_id=?1",
            [m.source_id.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?,r.get(3)?)),
        )
        .optional()
        .map_err(storage_error)?;
    let (account, groups, zone, adapter) = binding.ok_or(AppError::InvalidInput)?;
    let groups: Vec<String> = serde_json::from_str(&groups).map_err(|_| AppError::ParseFailed)?;
    if account != m.account_id
        || !groups.contains(&m.group_id)
        || batch.extractor_version != format!("n6.rules.1;timezone={zone}")
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
        let expected = canonical
            .candidates
            .iter()
            .find(|candidate| candidate.candidate_key == c.candidate_key)
            .ok_or(AppError::InvalidInput)?;
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
