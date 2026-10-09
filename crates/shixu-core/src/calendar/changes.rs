use crate::contracts::{calendar::*, notification::*};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventOrigin {
    Manual,
    Source,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventChange {
    pub change_id: ChangeId,
    pub event_id: EventId,
    pub before: Option<CalendarEvent>,
    pub after: CalendarEvent,
    pub candidate_key: Option<CandidateKey>,
    pub source: Option<EventSource>,
    pub undone: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceOutcome {
    Applied,
    Pending,
    Conflict,
    Suppressed,
    Revoked,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventSource {
    pub message_key: MessageKey,
    pub message_revision: u64,
    pub source_order: u64,
    /// Optional only for protected pre-N7 payloads. This is audit metadata;
    /// durable semantic validation, never this string, grants acceptance.
    #[serde(default)]
    pub extractor_version: Option<String>,
    pub source_id: SourceId,
    pub account_id: String,
    pub group_id: String,
    pub candidate: Candidate,
    pub event_id: Option<EventId>,
    pub outcome: SourceOutcome,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct StoredEvent {
    pub event: CalendarEvent,
    pub origin: EventOrigin,
    pub last_message: Option<MessageKey>,
    pub last_order: u64,
    pub last_revision: u64,
}
