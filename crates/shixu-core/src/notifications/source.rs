//! Normalized receive-only port. Blocking implementations belong on a background
//! worker. Real protocol and ordinary-group acceptance require the separate G2 gate.
use crate::contracts::{
    AppResult, UtcMillis, error::AppError, notification::*, vault::SecretBytes,
};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Disconnected,
    Connected,
    WaitingForLogin,
    Incompatible,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gap {
    pub since: UtcMillis,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceHealth {
    pub connection_state: ConnectionState,
    pub capabilities: Vec<SourceCapability>,
    pub last_connected_at: Option<UtcMillis>,
    pub last_received_at: Option<UtcMillis>,
    pub last_persisted_at: Option<UtcMillis>,
    pub last_applied_at: Option<UtcMillis>,
    pub gaps: Vec<Gap>,
    pub ordinary_group_verified: bool,
}
impl Default for SourceHealth {
    fn default() -> Self {
        Self {
            connection_state: ConnectionState::Disconnected,
            capabilities: vec![],
            last_connected_at: None,
            last_received_at: None,
            last_persisted_at: None,
            last_applied_at: None,
            gaps: vec![],
            ordinary_group_verified: false,
        }
    }
}
impl SourceHealth {
    pub fn connected(&mut self, at: UtcMillis, capabilities: Vec<SourceCapability>) {
        self.connection_state = ConnectionState::Connected;
        self.last_connected_at = Some(at);
        self.capabilities = capabilities;
    }
    pub fn received(&mut self, at: UtcMillis) {
        self.last_received_at = Some(at);
    }
    pub fn persisted(&mut self, at: UtcMillis) {
        self.last_persisted_at = Some(at);
    }
    pub fn applied(&mut self, at: UtcMillis) {
        self.last_applied_at = Some(at);
    }
    pub fn failed(&mut self, error: AppError, at: UtcMillis) {
        if self.gaps.is_empty() {
            self.gaps.push(Gap { since: at });
        }
        self.connection_state = match error {
            AppError::AuthFailed => ConnectionState::WaitingForLogin,
            AppError::Unsupported => ConnectionState::Incompatible,
            _ if self.connection_state == ConnectionState::WaitingForLogin => {
                ConnectionState::WaitingForLogin
            }
            _ => ConnectionState::Disconnected,
        };
    }
    pub fn retry_delay(&self, attempt: u32, jitter: u32) -> Option<u64> {
        (self.connection_state == ConnectionState::Disconnected)
            .then(|| super::reconnect::next_retry(attempt, jitter))
    }
}
pub trait QQAdapter {
    fn connect(&mut self, config: SourceConfig, token: SecretBytes) -> AppResult<SourceHealth>;
    fn disconnect(&mut self) -> AppResult<()>;
    fn health(&self) -> SourceHealth;
    fn next_message(&mut self) -> AppResult<Option<MessageEnvelope>>;
    /// Adapter-defined source/group/epoch recovery handle, or an unambiguous
    /// original recovery anchor. Live/partial result cursors cannot prove coverage.
    fn backfill(&mut self, cursor: &str) -> AppResult<Vec<MessageEnvelope>>;
}

/// Durable per-group interval proof. The anchor is authoritative while unresolved;
/// incomplete batches and live acknowledgments cannot establish prefix coverage.
/// recovery_cursor is diagnostic after complete proof, never a new recovery start.
/// An absent anchor means no verified recovery starting point.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupRecovery {
    pub group_id: String,
    pub epoch: u64,
    pub since: UtcMillis,
    pub anchor: Option<String>,
    pub recovery_cursor: Option<String>,
    pub complete: bool,
}
impl GroupRecovery {
    /// Public QQAdapter::backfill accepts this source/group/epoch-scoped handle.
    pub fn handle(&self, source: SourceId) -> String {
        use std::fmt::Write;
        let mut result = format!("recovery-v1-{source}-{}-", self.epoch);
        for b in self.group_id.bytes() {
            write!(&mut result, "{b:02x}").expect("String write is infallible");
        }
        result
    }
}
