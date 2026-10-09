use super::{Revision, UtcMillis};
use serde::{Deserialize, Serialize};

uuid_id!(EventId);
uuid_id!(ChangeId);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Precision {
    UnknownDate,
    DateOnly,
    Exact,
    ExplicitAllDay,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimeValue {
    pub precision: Precision,
    pub local_date: Option<String>,
    pub start_at: Option<UtcMillis>,
    pub end_at: Option<UtcMillis>,
    pub timezone: String,
    pub raw_time_text: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventStatus {
    Active,
    Cancelled,
    Removed,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventField {
    Title,
    Time,
    Location,
    Status,
}

/// Event fields use the design's flat storage/DTO names; TimeValue is the
/// reusable time object used in candidate and patch payloads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalendarEvent {
    pub event_id: EventId,
    pub title: String,
    pub kind: String,
    pub time_precision: Precision,
    pub local_date: Option<String>,
    pub start_at: Option<UtcMillis>,
    pub end_at: Option<UtcMillis>,
    pub timezone: String,
    pub raw_time_text: String,
    pub location: Option<String>,
    pub status: EventStatus,
    pub revision: Revision,
    pub user_overrides: Vec<EventField>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplySummary {
    pub created: u64,
    pub updated: u64,
    pub cancelled: u64,
    pub pending: u64,
    pub conflicts: u64,
    pub change_ids: Vec<ChangeId>,
}

/// Explicit Set/Clear avoids conflating a missing patch field with JSON null.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "operation",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum LocationPatch {
    Set(String),
    Clear,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventPatch {
    pub event_id: EventId,
    pub expected_revision: Revision,
    pub title: Option<String>,
    pub time: Option<TimeValue>,
    pub location: Option<LocationPatch>,
    pub status: Option<EventStatus>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventQuery {
    pub from_date: Option<String>,
    pub through_date: Option<String>,
    pub statuses: Vec<EventStatus>,
    pub include_pending: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UndoRequest {
    pub change_id: ChangeId,
    pub expected_revision: Revision,
}
