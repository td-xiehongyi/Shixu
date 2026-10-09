//! Rule-only candidate extraction. Each block is an independent layout unit.
use super::time::{
    clear_time, date_expression, inherit_year, negated_change, parse_time, temporal_stripped,
};
use crate::contracts::{
    AppResult, UtcMillis, calendar::Precision, error::AppError, notification::*,
};
use regex::Regex;
use sha2::{Digest, Sha256};
use std::sync::OnceLock;
pub(crate) const VERSION: &str = "n6.rules.1;timezone=";
pub(crate) fn candidate_key(message: MessageKey, subject: &str) -> CandidateKey {
    let mut digest = Sha256::new();
    digest.update(b"shixu-n6-subject-v1\0");
    digest.update(message.as_uuid().as_bytes());
    digest.update(subject.as_bytes());
    let digest = digest.finalize();
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    CandidateKey::from_uuid(uuid::Uuid::from_bytes(bytes))
}
pub(crate) fn subject(title: &str) -> String {
    let mut value = temporal_stripped(title);
    for token in [
        "通知",
        "原定",
        "改至",
        "改到",
        "调整至",
        "调整到",
        "延期至",
        "取消",
        "已取消",
        "安排",
        "见附件",
        "定于",
        "举行",
        "将于",
        "将",
        "今天",
        "明天",
        "后天",
        "昨天",
        "下周",
        "近期",
        "上午",
        "下午",
        "晚上",
        "全天",
        "整天",
        "截止时间",
        "时间",
        "地点",
    ] {
        value = value.replace(token, "");
    }
    value.chars().filter(|c| c.is_alphanumeric()).collect()
}
pub(crate) fn rejection(text: &str) -> bool {
    negated_change(text)
        || [
            "?",
            "？",
            "怎么",
            "如何",
            "哪天",
            "几号",
            "吗",
            "是否",
            "请问",
            "听说",
            "据说",
            "可能",
            "也许",
            "好像",
            "不确定",
            "如果",
            "假如",
            "没有",
            "不考试",
            "不取消",
            "不会取消",
            "未取消",
            "考试资料",
            "考试成绩",
            "会议纪要",
            "活动照片",
            "不补考",
            "不召开",
            "不开会",
            "不举行",
            "无需",
            "不用",
            "不需要",
            "未确定",
            "未安排",
            "尚未",
            "已经结束",
            "考完",
            "真难",
            "真累",
            "喜欢",
            "讨厌",
            "想参加",
            "希望",
            "别忘了考试多难",
            "并非",
        ]
        .iter()
        .any(|s| text.contains(s))
}
fn event_token(text: &str) -> Option<(&'static str, usize, usize)> {
    for (token, kind) in [
        ("补考", "exam"),
        ("考试", "exam"),
        ("报名截止", "deadline"),
        ("提交截止", "deadline"),
        ("截止", "deadline"),
        ("会议", "meeting"),
        ("开会", "meeting"),
        ("班会", "meeting"),
        ("活动", "activity"),
        ("讲座", "activity"),
    ] {
        if let Some(start) = text.find(token) {
            return Some((kind, start, start + token.len()));
        }
    }
    None
}
pub(crate) fn title_range(text: &str, token_start: usize, token_end: usize) -> (usize, usize) {
    // Select a contiguous source substring; normalize only for identity, never title.
    static PREFIX: OnceLock<Regex> = OnceLock::new();
    let prefix=PREFIX.get_or_init(||Regex::new(r"^(?:\s|[：:，,（()）]|通知|原定|取消|已取消|定于|将于|将|今天|明天|后天|昨天|下周|近期|上午|下午|晚上|全天|整天|\d{4}年\d{1,2}月\d{1,2}[日号]|\d{1,2}月\d{1,2}[日号]|\d{4}[-/]\d{1,2}[-/]\d{1,2}|\d{1,2}[:：]\d{2}|\d{1,2}[点时](?:\d{1,2}分?|半)?|[—–~～-])+" ).expect("static title prefix"));
    let start = prefix.find(&text[..token_start]).map_or(0, |v| v.end());
    (start, token_end)
}
fn coordinated_remainder(text: &str) -> Option<usize> {
    text.char_indices()
        .filter(|(_, c)| ['，', ','].contains(c))
        .map(|(i, c)| (i, i + c.len_utf8()))
        .chain(
            ["以及", "同时", "并且", "和", "及"]
                .iter()
                .flat_map(|m| text.match_indices(m).map(move |(i, _)| (i, i + m.len()))),
        )
        .filter(|(i, end)| {
            event_token(&text[..*i]).is_some()
                && !["地点：", "地点:"].iter().any(|m| {
                    text[..*i]
                        .rsplit(['，', ','])
                        .next()
                        .unwrap_or("")
                        .trim_start()
                        .starts_with(m)
                })
                && !["地点：", "地点:"]
                    .iter()
                    .any(|m| text[*end..].trim_start().starts_with(m))
                && (event_token(&text[*end..]).is_some()
                    || ["答辩", "测验"].iter().any(|m| text[*end..].contains(m)))
        })
        .map(|(i, _)| i)
        .min()
}
pub(crate) fn explicit_location(text: &str) -> Option<String> {
    ["地点：", "地点:"]
        .iter()
        .find_map(|token| text.find(token).map(|i| &text[i + token.len()..]))
        .map(|v| v.split(['，', ',']).next().unwrap_or(v).trim().to_owned())
        .filter(|v| !v.is_empty())
}
pub(crate) fn from_block(
    message: MessageKey,
    block: &EvidenceBlock,
    sent_at: UtcMillis,
    timezone: &str,
) -> AppResult<Vec<Candidate>> {
    if context_evidence_target(block)?.is_some() {
        return Err(AppError::Conflict);
    }
    let mut result: Vec<Candidate> = vec![];
    let mut offset = 0;
    for raw in block.text.split_inclusive(['；', ';', '\n', '。']) {
        let text = raw.trim_matches(|c: char| c.is_whitespace() || ['；', ';', '。'].contains(&c));
        let start = offset + raw.find(text).unwrap_or(0);
        offset += raw.len();
        let text = &text[..coordinated_remainder(text).unwrap_or(text.len())];
        if text.is_empty() || rejection(text) {
            continue;
        }
        let Some((kind, token_start, token_end)) = event_token(text) else {
            continue;
        };
        let (title_start, title_end) = title_range(text, token_start, token_end);
        let title = text[title_start..title_end].to_owned();
        let subject = subject(&title);
        if subject.is_empty() {
            continue;
        }
        let mut evidence = block.clone();
        evidence.text = text.into();
        if let Some(EvidenceLocation::TextSpan { start: base, .. }) = block.cell_range_or_bbox {
            let start = u32::try_from(start)
                .map_err(|_| AppError::InvalidInput)?
                .checked_add(base)
                .ok_or(AppError::InvalidInput)?;
            let end = start
                .checked_add(u32::try_from(text.len()).map_err(|_| AppError::InvalidInput)?)
                .ok_or(AppError::InvalidInput)?;
            evidence.cell_range_or_bbox = Some(EvidenceLocation::TextSpan { start, end });
        }
        let mut time = parse_time(text, sent_at, timezone)?;
        if !evidence.quality_flags.is_empty() {
            clear_time(&mut time)
        }
        if time.precision == Precision::UnknownDate
            && !evidence.quality_flags.contains(&QualityFlag::UncertainDate)
        {
            evidence.quality_flags.push(QualityFlag::UncertainDate);
        }
        let action = if text.contains("取消") {
            CandidateAction::Cancel
        } else if ["改至", "改到", "调整至", "调整到", "延期至"]
            .iter()
            .any(|s| text.contains(s))
        {
            CandidateAction::Reschedule
        } else {
            CandidateAction::Create
        };
        let location = explicit_location(text);
        let key = candidate_key(message, &subject);
        if result.iter().any(|c| c.candidate_key == key) {
            return Err(AppError::Conflict);
        }
        result.push(Candidate {
            candidate_key: key,
            action,
            title,
            kind: kind.into(),
            time,
            location,
            evidence: vec![evidence],
            target_message_key: None,
        });
    }
    Ok(result)
}
pub(crate) fn reconcile(
    candidates: &mut Vec<Candidate>,
    incoming: Candidate,
    sent_at: UtcMillis,
) -> AppResult<()> {
    validate_context_roles(&incoming)?;
    let Some(existing) = candidates
        .iter_mut()
        .find(|c| c.candidate_key == incoming.candidate_key)
    else {
        candidates.push(incoming);
        return Ok(());
    };
    validate_context_roles(existing)?;
    if existing.action != incoming.action || existing.kind != incoming.kind {
        return Err(AppError::Conflict);
    }
    if existing.evidence.iter().any(|old| {
        incoming.evidence.iter().any(|new| {
            old.part_id == new.part_id
                && (old.page_or_sheet != new.page_or_sheet
                    || old.cell_range_or_bbox != new.cell_range_or_bbox
                    || old.text != new.text)
        })
    }) {
        return Err(AppError::Conflict);
    }
    let mut combined = existing.evidence.clone();
    for evidence in &incoming.evidence {
        if !combined.contains(evidence) {
            combined.push(evidence.clone());
        }
    }
    let current: Vec<_> = combined
        .iter()
        .filter(|e| context_evidence_target(e).is_ok_and(|role| role.is_none()))
        .collect();
    let parsed: Vec<_> = current
        .iter()
        .map(|e| parse_time(&e.text, sent_at, &existing.time.timezone))
        .collect::<AppResult<_>>()?;
    let interpreted: Vec<_> = current
        .iter()
        .zip(parsed)
        .map(|(e, t)| {
            if existing.evidence.contains(*e) && e.text == existing.time.raw_time_text {
                existing.time.clone()
            } else if incoming.evidence.contains(*e)
                && e.text == incoming.time.raw_time_text
                && incoming.time.precision != Precision::UnknownDate
            {
                incoming.time.clone()
            } else {
                t
            }
        })
        .collect();
    let mut locations: Vec<_> = current
        .iter()
        .filter_map(|e| explicit_location(&e.text))
        .collect();
    locations.sort();
    locations.dedup();
    let compatible = |a: &crate::contracts::calendar::TimeValue,
                      b: &crate::contracts::calendar::TimeValue| {
        if a.local_date != b.local_date {
            return false;
        }
        if a.precision == Precision::ExplicitAllDay && b.precision == Precision::Exact
            || b.precision == Precision::ExplicitAllDay && a.precision == Precision::Exact
        {
            return false;
        }
        a.start_at.is_none()
            || b.start_at.is_none()
            || a.start_at == b.start_at
                && (a.end_at.is_none() || b.end_at.is_none() || a.end_at == b.end_at)
    };
    let rank = |t: &crate::contracts::calendar::TimeValue| match t.precision {
        Precision::Exact => 3 + u8::from(t.end_at.is_some()),
        Precision::ExplicitAllDay => 2,
        Precision::DateOnly => 1,
        Precision::UnknownDate => 0,
    };
    let best = current
        .iter()
        .zip(&interpreted)
        .filter(|(e, t)| e.quality_flags.is_empty() && t.precision != Precision::UnknownDate)
        .max_by_key(|(_, t)| rank(t))
        .map(|(_, t)| t.clone());
    if let Some(best) = best {
        let conflict = current.iter().zip(&interpreted).any(|(e, t)| {
            date_expression(&e.text)
                && (t.precision == Precision::UnknownDate || !compatible(&best, t))
        });
        if conflict {
            clear_time(&mut existing.time);
            for evidence in &mut combined {
                if !evidence.quality_flags.contains(&QualityFlag::UncertainDate) {
                    evidence.quality_flags.push(QualityFlag::UncertainDate);
                }
            }
        } else if existing.time.precision != best.precision
            || existing.time.local_date != best.local_date
            || existing.time.start_at != best.start_at
            || existing.time.end_at != best.end_at
        {
            existing.time = best;
        }
    } else {
        clear_time(&mut existing.time);
    }
    existing.location = if locations.len() == 1 {
        locations.pop()
    } else {
        None
    };
    existing.evidence = combined;
    Ok(())
}
/// Context is usable only through one explicit reply in the same namespace, with
/// at most five envelopes and twenty thousand Unicode scalar values in total.
/// `source_order` is original nonnegative UTC milliseconds, NOT a global sequence.
pub fn extract(
    envelope: &MessageEnvelope,
    blocks: &[EvidenceBlock],
    context: &[MessageEnvelope],
    timezone: &str,
) -> AppResult<ExtractBatch> {
    let _: chrono_tz::Tz = timezone.parse().map_err(|_| AppError::InvalidInput)?;
    let source_order = u64::try_from(envelope.sent_at).map_err(|_| AppError::InvalidInput)?;
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(envelope.sent_at)
        .ok_or(AppError::InvalidInput)?;
    if context.len() > 5
        || context
            .iter()
            .map(|m| m.text.chars().count())
            .sum::<usize>()
            > 20000
        || context.iter().any(|m| {
            m.source_id != envelope.source_id
                || m.account_id != envelope.account_id
                || m.group_id != envelope.group_id
        })
    {
        return Err(AppError::InvalidInput);
    }
    let referenced: Vec<_> = context
        .iter()
        .filter(|m| Some(m.message_key) == envelope.reply_to && !m.revoked)
        .collect();
    let mut candidates = vec![];
    let text_part = body_part_id(envelope);
    let body = EvidenceBlock {
        part_id: text_part,
        page_or_sheet: None,
        cell_range_or_bbox: Some(EvidenceLocation::TextSpan {
            start: 0,
            end: u32::try_from(envelope.text.len()).map_err(|_| AppError::InvalidInput)?,
        }),
        text: envelope.text.clone(),
        method: Method::NativeText,
        engine_version: "n6.body.utf8-bytes.1".into(),
        quality_flags: vec![],
    };
    let mut normalized_blocks = blocks.to_vec();
    for block in &mut normalized_blocks {
        if envelope
            .parts
            .iter()
            .any(|p| p.part_id == block.part_id && p.parse_state == PartStatus::PartialParse)
            && !block.quality_flags.contains(&QualityFlag::PartialSource)
        {
            block.quality_flags.push(QualityFlag::PartialSource);
        }
    }
    for block in std::iter::once(&body).chain(&normalized_blocks) {
        for candidate in from_block(envelope.message_key, block, envelope.sent_at, timezone)? {
            reconcile(&mut candidates, candidate, envelope.sent_at)?;
        }
    }
    let mut deferred_context_year = false;
    for c in &mut candidates {
        if referenced.len() == 1 {
            let original = referenced[0];
            let original_part = body_part_id(original);
            let original_block = EvidenceBlock {
                part_id: original_part,
                text: original.text.clone(),
                cell_range_or_bbox: Some(EvidenceLocation::TextSpan {
                    start: 0,
                    end: u32::try_from(original.text.len()).map_err(|_| AppError::InvalidInput)?,
                }),
                ..body.clone()
            };
            let original_candidates = match from_block(
                original.message_key,
                &original_block,
                original.sent_at,
                timezone,
            ) {
                Ok(candidates) => candidates,
                // Ambiguous repeated subjects in referenced context cannot
                // establish a target/year; retain the current notice unbound.
                Err(AppError::Conflict) => continue,
                Err(error) => return Err(error),
            };
            let matching: Vec<_> = original_candidates
                .iter()
                .filter(|original| {
                    original.kind == c.kind && subject(&original.title) == subject(&c.title)
                })
                .collect();
            if matching.len() == 1 {
                let original_candidate = matching[0];
                if c.action != CandidateAction::Create {
                    c.target_message_key = Some(original.message_key);
                }
                if c.action == CandidateAction::Create
                    && c.time.precision == Precision::UnknownDate
                    && let Some(date) = &original_candidate.time.local_date
                    && inherit_year(&c.time.raw_time_text, &date[..4]).is_some()
                {
                    deferred_context_year = true;
                }
                if c.target_message_key.is_some()
                    && c.time.precision == Precision::UnknownDate
                    && c.evidence.len() == 1
                    && c.evidence[0].part_id == text_part
                    && c.evidence[0].method == Method::NativeText
                    && c.evidence[0].engine_version == "n6.body.utf8-bytes.1"
                    && c.evidence.iter().all(|e| {
                        e.quality_flags
                            .iter()
                            .all(|f| *f == QualityFlag::UncertainDate)
                    })
                    && let Some(date) = &original_candidate.time.local_date
                    && let Some(inherited) = inherit_year(&c.time.raw_time_text, &date[..4])
                {
                    let mut inherited_time = parse_time(&inherited, envelope.sent_at, timezone)?;
                    if inherited_time.precision != Precision::UnknownDate {
                        inherited_time.raw_time_text = c.time.raw_time_text.clone();
                        c.time = inherited_time;
                        for evidence in &mut c.evidence {
                            evidence
                                .quality_flags
                                .retain(|flag| *flag != QualityFlag::UncertainDate);
                        }
                        for mut evidence in original_candidate.evidence.clone() {
                            evidence.engine_version =
                                format!("{CONTEXT_ROLE_PREFIX}{}", original.message_key);
                            c.evidence.push(evidence);
                        }
                    }
                }
            }
        }
    }
    let mut part_results: Vec<_> = envelope
        .parts
        .iter()
        .map(|p| PartResult {
            part_id: p.part_id,
            status: p.parse_state,
            blocks: vec![],
            reason_code: p.failure_code,
        })
        .collect();
    for block in &normalized_blocks {
        if let Some(part) = part_results.iter_mut().find(|p| p.part_id == block.part_id) {
            if !part.blocks.contains(block) {
                part.blocks.push(block.clone());
            }
        } else {
            part_results.push(PartResult {
                part_id: block.part_id,
                status: if block.quality_flags.is_empty() {
                    PartStatus::Success
                } else {
                    PartStatus::PartialParse
                },
                blocks: vec![block.clone()],
                reason_code: if block.quality_flags.is_empty() {
                    None
                } else {
                    Some(PartReason::PartialSource)
                },
            });
        }
    }
    if !envelope.text.is_empty() {
        // Semantic rule incompleteness is explicit even when no candidate exists.
        // Unknown ordinary statements are conservatively partial; empty does not
        // license downstream NonEvent without independent classification.
        let incomplete = deferred_context_year
            || envelope.text.split(['；', ';', '\n', '。']).any(|text| {
                !text.trim().is_empty()
                    && (coordinated_remainder(text).is_some()
                        || !rejection(text) && event_token(text).is_none())
                    && !["谢谢老师", "谢谢", "收到", "好的", "今天天气很好"].contains(&text.trim())
            });
        let mut semantic = body;
        if incomplete {
            semantic.quality_flags.push(QualityFlag::PartialSource);
        }
        let result = PartResult {
            part_id: text_part,
            status: if incomplete {
                PartStatus::PartialParse
            } else {
                PartStatus::Success
            },
            blocks: vec![semantic],
            reason_code: incomplete.then_some(PartReason::PartialSource),
        };
        if let Some(old) = part_results.iter_mut().find(|p| p.part_id == text_part) {
            *old = result;
        } else {
            part_results.push(result);
        }
    }
    Ok(ExtractBatch {
        message_key: envelope.message_key,
        message_revision: envelope.revision,
        source_order,
        candidates,
        part_results,
        extractor_version: format!("{VERSION}{timezone}"),
    })
}

/// Stable routing identity for the message body, including reply-context bodies.
/// Uses the first explicit Text part when present; otherwise a virtual UUIDv8
/// derived only from message_key and the versioned `body-evidence` domain.
/// D1 must compare against this helper and validate UTF-8 byte spans in durable
/// message text; virtual IDs do not enter attachment download/membership checks.
pub fn body_part_id(envelope: &MessageEnvelope) -> PartId {
    envelope
        .parts
        .iter()
        .find(|p| p.kind == PartKind::Text)
        .map(|p| p.part_id)
        .unwrap_or_else(|| {
            PartId::from_uuid(*candidate_key(envelope.message_key, "body-evidence").as_uuid())
        })
}

const CONTEXT_ROLE_PREFIX: &str = "n6.body.utf8-bytes.1;n6_context_year_target=";
/// Versioned N6-only historical year/target role. D1 must additionally prove it
/// against durable reply/source/body/spans; this metadata is not authorization.
pub fn context_evidence_target(evidence: &EvidenceBlock) -> AppResult<Option<MessageKey>> {
    if !evidence.engine_version.contains("n6_context_year_target") {
        return Ok(None);
    }
    let target = evidence
        .engine_version
        .strip_prefix(CONTEXT_ROLE_PREFIX)
        .ok_or(AppError::InvalidInput)?;
    let key: MessageKey = target.parse()?;
    if key.to_string() != target
        || evidence.method != Method::NativeText
        || evidence.page_or_sheet.is_some()
        || !matches!(
            evidence.cell_range_or_bbox,
            Some(EvidenceLocation::TextSpan { .. })
        )
    {
        return Err(AppError::InvalidInput);
    }
    Ok(Some(key))
}
pub(crate) fn validate_context_roles(candidate: &Candidate) -> AppResult<()> {
    for evidence in &candidate.evidence {
        if let Some(target) = context_evidence_target(evidence)?
            && candidate.target_message_key != Some(target)
        {
            return Err(AppError::Conflict);
        }
    }
    Ok(())
}
