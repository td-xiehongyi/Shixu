//! Bounded, data-only optional extraction. No provider or tool execution lives here.
use super::{
    consent::{ConsentStore, authorize_request},
    extract::{body_part_id, candidate_key, context_evidence_target, extract, subject},
    time::parse_time,
};
use crate::contracts::{AppResult, calendar::Precision, error::AppError, notification::*};
use serde::Serialize;
const MAX_BYTES: usize = 65536;
const MAX_BLOCKS: usize = 32;
#[derive(Clone, Serialize)]
pub struct ModelRequest {
    pub request_id: String,
    pub provider_id: String,
    pub sent_at: i64,
    pub timezone: String,
    pub evidence: Vec<EvidenceBlock>,
    pub reply_context: Vec<EvidenceBlock>,
}
/// Implementations must enforce a finite timeout and return only candidate JSON.
/// Sending is synchronous under the consent lock; no deferred/queued send may
/// outlive this call and implementations must not re-enter the consent store.
/// No credentials, tools, callbacks or file handles are present in the request.
pub trait ModelTransport {
    fn send(&self, request: &ModelRequest) -> AppResult<String>;
}
pub struct PreparedRequest {
    consent_revision: u64,
    message: MessageEnvelope,
    rules: ExtractBatch,
    request: ModelRequest,
    attachment: bool,
}
pub struct ModelResult {
    pub batch: ExtractBatch,
    pub request_id: String,
    pub retryable: bool,
}
pub struct ModelService<'a> {
    pub consent: &'a ConsentStore,
    pub transport: &'a dyn ModelTransport,
}
pub(crate) fn bounded(evidence: &[EvidenceBlock]) -> AppResult<()> {
    if evidence.len() > MAX_BLOCKS
        || evidence.iter().map(|e| e.text.len()).sum::<usize>() > MAX_BYTES
        || evidence.iter().any(|e| {
            e.text.len() > 16384
                || e.engine_version.len() > 128
                || e.cell_range_or_bbox.as_ref().is_some_and(
                    |p| matches!(p,EvidenceLocation::CellRange{range} if range.len()>128),
                )
                || e.page_or_sheet
                    .as_ref()
                    .is_some_and(|p| matches!(p,PageOrSheet::Sheet{name} if name.len()>128))
        })
    {
        return Err(AppError::InvalidInput);
    }
    Ok(())
}
impl ModelService<'_> {
    pub fn prepare(
        &self,
        m: &MessageEnvelope,
        blocks: &[EvidenceBlock],
        context: &[MessageEnvelope],
        source: &SourceConfig,
    ) -> AppResult<PreparedRequest> {
        if !source.enabled
            || source.source_id != m.source_id
            || source.account_id != m.account_id
            || !source.allowed_group_ids.contains(&m.group_id)
            || m.text.len() > 16384
        {
            return Err(AppError::InvalidInput);
        }
        let consent = self.consent.snapshot()?;
        let attachment = !blocks.is_empty();
        authorize_request(&consent, m, attachment)?;
        bounded(blocks)?;
        for b in blocks {
            if context_evidence_target(b)?.is_some()
                || !m
                    .parts
                    .iter()
                    .any(|p| p.part_id == b.part_id && p.kind != PartKind::Text)
            {
                return Err(AppError::InvalidInput);
            }
        }
        let rules = extract(m, blocks, context, &source.timezone)?;
        let mut evidence = vec![EvidenceBlock {
            part_id: body_part_id(m),
            page_or_sheet: None,
            cell_range_or_bbox: Some(EvidenceLocation::TextSpan {
                start: 0,
                end: m.text.len() as u32,
            }),
            text: m.text.clone(),
            method: Method::NativeText,
            engine_version: "n6.body.utf8-bytes.1".into(),
            quality_flags: vec![],
        }];
        evidence.extend(
            rules
                .part_results
                .iter()
                .filter(|p| p.part_id != body_part_id(m))
                .flat_map(|p| p.blocks.clone()),
        );
        bounded(&evidence)?;
        // Only evidence actually required by rule reply matching/year inheritance.
        let mut reply_context = vec![];
        for e in rules.candidates.iter().flat_map(|c| &c.evidence) {
            if context_evidence_target(e)?.is_some() && !reply_context.contains(e) {
                reply_context.push(e.clone());
            }
        }
        if reply_context.iter().map(|e| e.text.len()).sum::<usize>() > 4096 {
            return Err(AppError::InvalidInput);
        }
        bounded(&reply_context)?;
        Ok(PreparedRequest {
            consent_revision: consent.revision,
            message: m.clone(),
            rules,
            request: ModelRequest {
                request_id: format!("n7:{}:{}", m.message_key, m.revision),
                provider_id: consent.provider_id.ok_or(AppError::InvalidInput)?,
                sent_at: m.sent_at,
                timezone: source.timezone.clone(),
                evidence,
                reply_context,
            },
            attachment,
        })
    }
    pub fn dispatch(&self, prepared: PreparedRequest) -> AppResult<ModelResult> {
        let current = self
            .consent
            .current
            .lock()
            .map_err(|_| AppError::Conflict)?;
        authorize_request(&current, &prepared.message, prepared.attachment)?;
        if current.revision != prepared.consent_revision
            || current.provider_id.as_ref() != Some(&prepared.request.provider_id)
        {
            return Err(AppError::Conflict);
        }
        // This is the actual send admission, protected by the same lock as replace.
        let response = self.transport.send(&prepared.request);
        drop(current);
        let mut batch = prepared.rules;
        let retryable = matches!(response, Err(AppError::Disconnected));
        if let Ok(output) = response
            && let Ok(candidates) = validate_at(
                &output,
                &prepared.request.evidence,
                &prepared.message,
                &prepared.request.timezone,
            )
        {
            for candidate in candidates {
                if !batch
                    .candidates
                    .iter()
                    .any(|c| c.candidate_key == candidate.candidate_key)
                {
                    batch.extractor_version = composite_version(&prepared.request.timezone);
                    batch.candidates.push(candidate);
                }
            }
        }
        Ok(ModelResult {
            batch,
            request_id: prepared.request.request_id,
            retryable,
        })
    }
}
fn parse_output(output: &str) -> AppResult<Vec<Candidate>> {
    if output.len() > MAX_BYTES {
        return Err(AppError::InvalidInput);
    }
    let candidates: Vec<Candidate> =
        serde_json::from_str(output).map_err(|_| AppError::InvalidInput)?;
    if candidates.len() > MAX_BLOCKS {
        return Err(AppError::InvalidInput);
    }
    Ok(candidates)
}
fn equal_fields(a: &Candidate, b: &Candidate) -> bool {
    a.action == b.action
        && a.title == b.title
        && a.kind == b.kind
        && a.time == b.time
        && a.location == b.location
        && a.evidence == b.evidence
        && a.target_message_key == b.target_message_key
}
fn validate_at(
    output: &str,
    evidence: &[EvidenceBlock],
    m: &MessageEnvelope,
    zone: &str,
) -> AppResult<Vec<Candidate>> {
    bounded(evidence)?;
    let mut validated = vec![];
    for candidate in parse_output(output)? {
        if candidate.evidence.len() != 1 || !evidence.contains(&candidate.evidence[0]) {
            return Err(AppError::InvalidInput);
        }
        let grounded = grounded_candidate(m, &candidate.evidence[0], zone)?;
        validate_corroboration(m, &grounded, evidence, zone)?;
        if !equal_fields(&candidate, &grounded)
            || validated
                .iter()
                .any(|c: &Candidate| c.candidate_key == grounded.candidate_key)
        {
            return Err(AppError::InvalidInput);
        }
        validated.push(grounded);
    }
    Ok(validated)
}
/// Standalone validation is deliberately limited to absolute source dates (or
/// unknown dates). It cannot authenticate source identity or relative dates.
/// Dispatch and calendar use the original durable sent_at and source timezone.
pub fn validate_model_output(
    output: &str,
    evidence: &[EvidenceBlock],
) -> AppResult<Vec<Candidate>> {
    bounded(evidence)?;
    let mut validated = vec![];
    for c in parse_output(output)? {
        if c.evidence.len() != 1
            || !evidence.contains(&c.evidence[0])
            || c.time.timezone.len() > 128
        {
            return Err(AppError::InvalidInput);
        }
        let m = MessageEnvelope {
            message_key: MessageKey::from_uuid(uuid::Uuid::nil()),
            source_id: SourceId::from_uuid(uuid::Uuid::nil()),
            account_id: String::new(),
            group_id: String::new(),
            native_message_id: String::new(),
            sent_at: 0,
            received_at: 0,
            sender_id: String::new(),
            text: String::new(),
            reply_to: None,
            revision: 0,
            revoked: false,
            processing_state: ProcessingState::Persisted,
            parts: vec![],
        };
        if ["今天", "明天", "后天", "昨天"]
            .iter()
            .any(|s| c.evidence[0].text.contains(s))
            && c.time.precision != Precision::UnknownDate
        {
            return Err(AppError::InvalidInput);
        }
        let grounded = grounded_candidate(&m, &c.evidence[0], &c.time.timezone)?;
        if !equal_fields(&c, &grounded) {
            return Err(AppError::InvalidInput);
        }
        validated.push(c);
    }
    Ok(validated)
}
/// The only supported predicates are literal 测验/答辩; occurrence identity
/// is resolved separately from whether the source asserts a valid Create.
fn single_predicate(text: &str) -> Option<(&'static str, &'static str, usize)> {
    let mut matches = [("测验", "exam"), ("答辩", "activity")]
        .into_iter()
        .flat_map(|(token, kind)| {
            text.match_indices(token)
                .map(move |(start, _)| (token, kind, start))
        });
    let found = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    Some(found)
}
fn label_char(c: char) -> bool {
    c.is_alphanumeric()
        || c.is_whitespace()
        || ['-', '_', '·', '(', ')', '（', '）', ',', '，', '.'].contains(&c)
}
fn identity_prefix(text: &str) -> &str {
    // One anchored marker before time, and optionally one after it. Never
    // delete words from the middle or normalize a whole paragraph into a title.
    [
        "不会举行",
        "不举行",
        "未举行",
        "不安排",
        "未安排",
        "不进行",
        "未进行",
        "已取消",
        "不取消",
        "未取消",
        "取消",
        "可能",
        "也许",
        "听说",
        "据说",
        "好像",
        "似乎",
        "估计",
        "传闻",
        "已经完成",
        "已完成",
        "已经结束",
        "已结束",
        "不",
        "未",
    ]
    .iter()
    .find_map(|prefix| text.strip_prefix(prefix))
    .unwrap_or(text)
    .trim_start()
}
fn current_subject_identity(text: &str) -> Option<String> {
    let text = identity_prefix(text.trim());
    let (token, _, token_start) = single_predicate(text)?;
    let (start, end) = super::extract::title_range(text, token_start, token_start + token.len());
    let title = identity_prefix(text[start..end].trim());
    let token_start = title.find(token)?;
    let (start, end) = super::extract::title_range(title, token_start, token_start + token.len());
    let title = &title[start..end];
    if title.len() > 256
        || !title.chars().all(label_char)
        || super::extract::rejection(title)
        || title.contains(['不', '未'])
    {
        return None;
    }
    Some(subject(title))
}
/// A closed tail grammar: end, an optional exact affirmative occurrence verb,
/// and optionally a single explicit location label. Unknown suffixes are data,
/// not proof that an exam/defense takes place. No second predicate/date/action
/// can be hidden in a location label.
fn statement_tail(tail: &str) -> AppResult<Option<String>> {
    let tail = tail
        .trim()
        .trim_end_matches(['。', '.', '!', '！'])
        .trim_end();
    let tail = ["举行", "进行"]
        .iter()
        .find_map(|verb| tail.strip_prefix(verb))
        .unwrap_or(tail)
        .trim_start();
    let tail = tail.strip_prefix(['，', ',']).unwrap_or(tail).trim_start();
    if tail.is_empty() {
        return Ok(None);
    }
    let location = ["地点：", "地点:"]
        .iter()
        .find_map(|prefix| tail.strip_prefix(prefix))
        .ok_or(AppError::InvalidInput)?
        .trim();
    if location.is_empty()
        || location.len() > 128
        || !location.chars().all(|c| {
            c.is_alphanumeric()
                || c.is_whitespace()
                || ['-', '_', '·', '(', ')', '（', '）'].contains(&c)
        })
        || super::time::date_expression(location)
        || super::extract::rejection(location)
        || [
            "不",
            "未",
            "取消",
            "调整",
            "改至",
            "改到",
            "延期",
            "测验",
            "答辩",
            "举行",
            "进行",
            "安排",
            "定于",
            "将于",
            "原定",
            "已完成",
            "已经完成",
            "已结束",
            "已经结束",
        ]
        .iter()
        .any(|word| location.contains(word))
    {
        return Err(AppError::InvalidInput);
    }
    Ok(Some(location.into()))
}
/// Conservative fallback: one complete literal 测验/答辩 statement per block.
/// No confidence, role metadata, inferred dates or arbitrary suffixes grant it.
pub(crate) fn grounded_candidate(
    m: &MessageEnvelope,
    e: &EvidenceBlock,
    zone: &str,
) -> AppResult<Candidate> {
    let text = &e.text;
    if text.is_empty() || text.len() > 16384 || context_evidence_target(e)?.is_some() {
        return Err(AppError::InvalidInput);
    }
    let (token, kind, token_start) = single_predicate(text).ok_or(AppError::InvalidInput)?;
    let token_end = token_start + token.len();
    let assertion = &text[..token_end];
    if super::extract::rejection(assertion)
        || [
            "不",
            "未",
            "取消",
            "改至",
            "改到",
            "调整",
            "延期",
            "和",
            "及",
            "同时",
            "并且",
            "似乎",
            "大概",
            "估计",
            "传闻",
            "示例",
            "举例",
            "例如",
            "已完成",
            "已经完成",
            "已结束",
            "已经结束",
            "考试",
            "补考",
            "会议",
            "开会",
            "班会",
            "活动",
            "讲座",
            "截止",
        ]
        .iter()
        .any(|s| assertion.contains(s))
        || assertion
            .chars()
            .any(|c| ['；', ';', '\n', '。', '?', '？'].contains(&c))
    {
        return Err(AppError::InvalidInput);
    }
    let location = statement_tail(&text[token_end..])?;
    let (start, end) = super::extract::title_range(text, token_start, token_end);
    let title = &text[start..end];
    if title.len() > 256 || !title.chars().all(label_char) {
        return Err(AppError::InvalidInput);
    }
    let mut time = parse_time(text, m.sent_at, zone)?;
    if !e.quality_flags.is_empty() {
        super::time::clear_time(&mut time);
    }
    Ok(Candidate {
        candidate_key: candidate_key(m.message_key, &format!("n7:{}", subject(title))),
        action: CandidateAction::Create,
        title: title.into(),
        kind: kind.into(),
        time,
        location,
        evidence: vec![e.clone()],
        target_message_key: None,
    })
}

/// A selected block cannot hide contradictory or incomplete same-subject current
/// evidence. Historical context never supplies the date for fallback Creates.
pub(crate) fn validate_corroboration(
    m: &MessageEnvelope,
    c: &Candidate,
    evidence: &[EvidenceBlock],
    zone: &str,
) -> AppResult<()> {
    for e in evidence {
        match grounded_candidate(m, e, zone) {
            Ok(other)
                if other.candidate_key == c.candidate_key
                    && (other.time.precision != c.time.precision
                        || other.time.local_date != c.time.local_date
                        || other.time.start_at != c.time.start_at
                        || other.time.end_at != c.time.end_at
                        || other.location != c.location) =>
            {
                return Err(AppError::InvalidInput);
            }
            Err(_) => match current_subject_identity(&e.text) {
                Some(identity) if identity == subject(&c.title) => {
                    return Err(AppError::InvalidInput);
                }
                // A supported predicate with unresolvable subject cannot prove
                // independence from this event. Keep the source unresolved.
                None if ["测验", "答辩"].iter().any(|token| e.text.contains(token)) => {
                    return Err(AppError::InvalidInput);
                }
                _ => {}
            },
            _ => {}
        }
    }
    Ok(())
}

pub(crate) fn composite_version(zone: &str) -> String {
    format!("n6.rules.1+n7.guarded-create.1;timezone={zone}")
}
