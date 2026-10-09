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
        title: value.title,
        kind: value.kind,
        time_precision: value.time_precision,
        local_date: value.local_date,
        start_at: value.start_at,
        end_at: value.end_at,
        timezone: value.timezone,
        raw_time_text: value.raw_time_text,
        location: value.location,
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
