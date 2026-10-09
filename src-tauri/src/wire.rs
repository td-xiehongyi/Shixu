//! Explicit native boundary types; core numeric u64 values never reach JS JSON.
use serde::{Deserialize, Serialize};
use shixu_core::contracts::{
    AppResult,
    calendar::{self, CalendarEvent, EventPatch, TimeValue, UndoRequest},
    error::AppError,
    vault::VaultSummary,
};

pub fn revision(value: &str) -> AppResult<u64> {
    if value.is_empty()
        || value.len() > 20
        || !value.bytes().all(|b| b.is_ascii_digit())
        || (value.len() > 1 && value.starts_with('0'))
    {
        return Err(AppError::InvalidInput);
    }
    value.parse().map_err(|_| AppError::InvalidInput)
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Decimal(u64);
impl TryFrom<String> for Decimal {
    type Error = AppError;
    fn try_from(value: String) -> AppResult<Self> {
        revision(&value).map(Self)
    }
}
impl From<Decimal> for String {
    fn from(value: Decimal) -> Self {
        value.0.to_string()
    }
}
impl From<u64> for Decimal {
    fn from(value: u64) -> Self {
        Self(value)
    }
}
const SAFE_MILLIS: i64 = 9_007_199_254_740_991;
fn millis(value: i64) -> AppResult<()> {
    if (-SAFE_MILLIS..=SAFE_MILLIS).contains(&value) {
        Ok(())
    } else {
        Err(AppError::InvalidInput)
    }
}
fn times(start: Option<i64>, end: Option<i64>) -> AppResult<()> {
    if let Some(v) = start {
        millis(v)?;
    }
    if let Some(v) = end {
        millis(v)?;
    }
    Ok(())
}
pub fn text(value: &str, limit: usize) -> AppResult<()> {
    if value.len() > limit || value.contains('\0') {
        Err(AppError::InvalidInput)
    } else {
        Ok(())
    }
}
fn validate_time(value: &TimeValue) -> AppResult<()> {
    times(value.start_at, value.end_at)?;
    text(&value.timezone, 128)?;
    text(&value.raw_time_text, 4096)?;
    if let Some(date) = &value.local_date {
        text(date, 10)?;
    }
    Ok(())
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireEvent {
    pub event_id: calendar::EventId,
    pub title: String,
    pub kind: String,
    pub time_precision: calendar::Precision,
    pub local_date: Option<String>,
    pub start_at: Option<i64>,
    pub end_at: Option<i64>,
    pub timezone: String,
    pub raw_time_text: String,
    pub location: Option<String>,
    pub status: calendar::EventStatus,
    pub revision: Decimal,
    pub user_overrides: Vec<calendar::EventField>,
}
pub fn event(value: CalendarEvent) -> AppResult<WireEvent> {
    times(value.start_at, value.end_at)?;
    Ok(WireEvent {
        event_id: value.event_id,
        title: excerpt(&value.title, 4096),
        kind: excerpt(&value.kind, 128),
        time_precision: value.time_precision,
        local_date: value.local_date,
        start_at: value.start_at,
        end_at: value.end_at,
        timezone: value.timezone,
        raw_time_text: excerpt(&value.raw_time_text, 4096),
        location: value.location.map(|s| excerpt(&s, 4096)),
        status: value.status,
        revision: value.revision.into(),
        user_overrides: value.user_overrides,
    })
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireSummary {
    pub entry_id: shixu_core::contracts::vault::EntryId,
    pub channel: String,
    pub account: String,
    pub revision: Decimal,
    pub created_at: i64,
    pub updated_at: i64,
}
pub fn summary(value: VaultSummary) -> AppResult<WireSummary> {
    millis(value.created_at)?;
    millis(value.updated_at)?;
    Ok(WireSummary {
        entry_id: value.entry_id,
        channel: value.channel,
        account: value.account,
        revision: value.revision.into(),
        created_at: value.created_at,
        updated_at: value.updated_at,
    })
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WirePatch {
    pub event_id: calendar::EventId,
    pub expected_revision: Decimal,
    pub title: Option<String>,
    pub time: Option<TimeValue>,
    pub location: Option<calendar::LocationPatch>,
    pub status: Option<calendar::EventStatus>,
}
impl WirePatch {
    pub fn into_core(self) -> AppResult<EventPatch> {
        if let Some(v) = &self.title {
            text(v, 4096)?;
        }
        if let Some(v) = &self.time {
            validate_time(v)?;
        }
        if let Some(calendar::LocationPatch::Set(v)) = &self.location {
            text(v, 4096)?;
        }
        Ok(EventPatch {
            event_id: self.event_id,
            expected_revision: self.expected_revision.0,
            title: self.title,
            time: self.time,
            location: self.location,
            status: self.status,
        })
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireUndo {
    pub change_id: calendar::ChangeId,
    pub expected_revision: Decimal,
}
impl From<WireUndo> for UndoRequest {
    fn from(v: WireUndo) -> Self {
        Self {
            change_id: v.change_id,
            expected_revision: v.expected_revision.0,
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireApplySummary {
    pub created: Decimal,
    pub updated: Decimal,
    pub cancelled: Decimal,
    pub pending: Decimal,
    pub conflicts: Decimal,
    pub change_ids: Vec<calendar::ChangeId>,
}
impl From<calendar::ApplySummary> for WireApplySummary {
    fn from(v: calendar::ApplySummary) -> Self {
        Self {
            created: v.created.into(),
            updated: v.updated.into(),
            cancelled: v.cancelled.into(),
            pending: v.pending.into(),
            conflicts: v.conflicts.into(),
            change_ids: v.change_ids,
        }
    }
}

/// Bounded UTF-8 read excerpt. Complete text remains protected in durable storage.
pub fn excerpt(value: &str, max: usize) -> String {
    let sanitized = value.replace('\0', "�");
    let value = sanitized.as_str();
    if value.len() <= max {
        return value.to_owned();
    }
    let mut end = max - 3;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", value[..end].replace('\0', "�"))
}
use shixu_core::{
    calendar::changes::EventOrigin, contracts::notification::*,
    notifications::consent::ModelConsent,
};
#[derive(Serialize)]
pub struct WireChange {
    pub change_id: calendar::ChangeId,
    pub before: Option<WireEvent>,
    pub after: WireEvent,
    pub undone: bool,
}
#[derive(Serialize)]
pub struct WireSource {
    pub message_key: MessageKey,
    pub message_revision: Decimal,
    pub group_id: String,
    pub outcome: shixu_core::calendar::changes::SourceOutcome,
    pub evidence: Vec<EvidenceBlock>,
}
#[derive(Serialize)]
pub struct WireDetails {
    pub origin: EventOrigin,
    pub history: Vec<WireChange>,
    pub sources: Vec<WireSource>,
}
pub fn blocks(value: Vec<EvidenceBlock>) -> Vec<EvidenceBlock> {
    value
        .into_iter()
        .take(32)
        .map(|mut b| {
            b.text = excerpt(&b.text, 4096);
            b.engine_version = excerpt(&b.engine_version, 128);
            b
        })
        .collect()
}
#[derive(Serialize)]
pub struct WireMessage {
    pub calendar_applied: bool,
    pub message_key: MessageKey,
    pub source_id: SourceId,
    pub account_id: String,
    pub group_id: String,
    pub native_message_id: String,
    pub sent_at: i64,
    pub received_at: i64,
    pub sender_id: String,
    pub text: String,
    pub reply_to: Option<MessageKey>,
    pub revision: Decimal,
    pub revoked: bool,
    pub processing_state: ProcessingState,
    pub parts: Vec<WirePart>,
}
#[derive(Serialize)]
pub struct WirePart {
    pub part_id: PartId,
    pub message_key: MessageKey,
    pub kind: PartKind,
    pub source_file_ref: Option<String>,
    pub original_name: Option<String>,
    pub declared_type: Option<String>,
    pub detected_type: Option<String>,
    pub byte_size: Option<Decimal>,
    pub content_hash: Option<String>,
    pub fetch_state: FetchState,
    pub parse_state: PartStatus,
    pub failure_code: Option<PartReason>,
    pub encrypted_blob_ref: Option<String>,
    pub retained_until: Option<i64>,
}
pub fn message(m: MessageEnvelope, calendar_applied: bool) -> AppResult<WireMessage> {
    millis(m.sent_at)?;
    millis(m.received_at)?;
    Ok(WireMessage {
        calendar_applied,
        message_key: m.message_key,
        source_id: m.source_id,
        account_id: excerpt(&m.account_id, 128),
        group_id: excerpt(&m.group_id, 128),
        native_message_id: excerpt(&m.native_message_id, 128),
        sent_at: m.sent_at,
        received_at: m.received_at,
        sender_id: excerpt(&m.sender_id, 128),
        text: excerpt(&m.text, 4096),
        reply_to: m.reply_to,
        revision: m.revision.into(),
        revoked: m.revoked,
        processing_state: m.processing_state,
        parts: m
            .parts
            .into_iter()
            .take(5)
            .map(|p| WirePart {
                part_id: p.part_id,
                message_key: p.message_key,
                kind: p.kind,
                source_file_ref: None,
                original_name: p.original_name.map(|v| excerpt(&v, 256)),
                declared_type: p.declared_type.map(|v| excerpt(&v, 128)),
                detected_type: p.detected_type.map(|v| excerpt(&v, 128)),
                byte_size: p.byte_size.map(Into::into),
                content_hash: None,
                fetch_state: p.fetch_state,
                parse_state: p.parse_state,
                failure_code: p.failure_code,
                encrypted_blob_ref: None,
                retained_until: p.retained_until,
            })
            .collect(),
    })
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireConsent {
    pub enabled: bool,
    pub provider_id: Option<String>,
    pub allowed_group_ids: Vec<String>,
    pub allow_attachment_text: bool,
    pub revision: Decimal,
}
impl From<ModelConsent> for WireConsent {
    fn from(v: ModelConsent) -> Self {
        Self {
            enabled: v.enabled,
            provider_id: v.provider_id,
            allowed_group_ids: v.allowed_group_ids,
            allow_attachment_text: v.allow_attachment_text,
            revision: v.revision.into(),
        }
    }
}
impl WireConsent {
    pub fn into_core(self) -> AppResult<ModelConsent> {
        if self.allowed_group_ids.len() > 100 {
            return Err(AppError::InvalidInput);
        }
        for g in &self.allowed_group_ids {
            text(g, 128)?;
            if g.is_empty() {
                return Err(AppError::InvalidInput);
            }
        }
        if let Some(p) = &self.provider_id {
            text(p, 128)?;
            if p.is_empty() || p.chars().any(char::is_control) {
                return Err(AppError::InvalidInput);
            }
        }
        if self.enabled && (self.provider_id.is_none() || self.allowed_group_ids.is_empty()) {
            return Err(AppError::InvalidInput);
        }
        Ok(ModelConsent {
            enabled: self.enabled,
            provider_id: self.provider_id,
            allowed_group_ids: self.allowed_group_ids,
            allow_attachment_text: self.allow_attachment_text,
            revision: self.revision.0,
        })
    }
}
#[derive(Serialize)]
pub struct WireSourceSetting {
    pub config: SourceConfig,
    pub epoch: Decimal,
}
#[derive(Serialize)]
pub struct WireSettings {
    pub runtime: WireRuntime,
    pub sources: Vec<WireSourceSetting>,
    pub model: WireConsent,
    pub autostart: bool,
    pub transport_supported: bool,
}

#[derive(Serialize)]
pub struct WireRuntime {
    pub running: bool,
    pub pending_rules: u32,
    pub attachment_queue: u32,
    pub model_queue: u32,
    pub last_calendar_commit: Option<i64>,
    pub last_error: Option<shixu_core::contracts::error::AppError>,
    pub sources: Vec<WireRuntimeSource>,
}
#[derive(Serialize)]
pub struct WireRuntimeSource {
    pub source_id: SourceId,
    pub connection_state: &'static str,
    pub last_received_at: Option<i64>,
    pub last_persisted_at: Option<i64>,
    pub last_applied_at: Option<i64>,
    pub gap: bool,
}
impl From<shixu_core::runtime::supervisor::RuntimeStatus> for WireRuntime {
    fn from(s: shixu_core::runtime::supervisor::RuntimeStatus) -> Self {
        use shixu_core::notifications::source::ConnectionState;
        Self {
            running: s.running,
            pending_rules: s.pending_rules,
            attachment_queue: s.attachment_queue,
            model_queue: s.model_queue,
            last_calendar_commit: s.last_calendar_commit,
            last_error: s.last_error,
            sources: s
                .sources
                .into_iter()
                .map(|(source_id, h)| WireRuntimeSource {
                    source_id,
                    connection_state: match h.connection_state {
                        ConnectionState::Connected => "connected",
                        ConnectionState::Disconnected => "disconnected",
                        ConnectionState::WaitingForLogin => "waiting_for_login",
                        ConnectionState::Incompatible => "incompatible",
                    },
                    last_received_at: h.last_received_at,
                    last_persisted_at: h.last_persisted_at,
                    last_applied_at: h.last_applied_at,
                    gap: !h.gaps.is_empty(),
                })
                .collect(),
        }
    }
}
