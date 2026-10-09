//! Late evidence preserves the original revision and source chronology.
use super::extract::{VERSION, from_block, reconcile, validate_context_roles};
use crate::contracts::{AppResult, error::AppError, notification::*};
/// No envelope/reply context is available here. New evidence can contribute
/// standalone candidates, but cannot invent reference linkage or inherited dates.
pub fn merge_parts(existing: &ExtractBatch, new_parts: &[PartResult]) -> AppResult<ExtractBatch> {
    let timezone = existing
        .extractor_version
        .strip_prefix(VERSION)
        .ok_or(AppError::InvalidInput)?;
    let _: chrono_tz::Tz = timezone.parse().map_err(|_| AppError::InvalidInput)?;
    let sent_at = i64::try_from(existing.source_order).map_err(|_| AppError::InvalidInput)?;
    for candidate in &existing.candidates {
        validate_context_roles(candidate)?;
    }
    let mut result = existing.clone();
    for input in new_parts {
        let mut normalized = input.clone();
        if !normalized.blocks.is_empty()
            && !matches!(
                normalized.status,
                PartStatus::Success | PartStatus::PartialParse
            )
        {
            return Err(AppError::InvalidInput);
        }
        if normalized.status == PartStatus::PartialParse {
            for block in &mut normalized.blocks {
                if !block.quality_flags.contains(&QualityFlag::PartialSource) {
                    block.quality_flags.push(QualityFlag::PartialSource);
                }
            }
        }
        let part = &normalized;
        if part.blocks.iter().any(|b| b.part_id != part.part_id) {
            return Err(AppError::InvalidInput);
        }
        if let Some(old) = result
            .part_results
            .iter_mut()
            .find(|old| old.part_id == part.part_id)
        {
            if old == part {
                continue;
            }
            // Evidence replacements could change already-grounded candidates. The
            // exact API supplies no source revision for such replacements; fail closed.
            if !old.blocks.is_empty() && old.blocks != part.blocks {
                return Err(AppError::Conflict);
            }
            *old = part.clone();
        } else {
            result.part_results.push(part.clone());
        }
        for block in &part.blocks {
            for candidate in from_block(result.message_key, block, sent_at, timezone)? {
                reconcile(&mut result.candidates, candidate, sent_at)?;
            }
        }
    }
    Ok(result)
}
