//! Receive adapter for a normalized transport. No wire-protocol implementation.
use super::transport::*;
use shixu_core::{
    contracts::{AppResult, UtcMillis, error::AppError, notification::*, vault::SecretBytes},
    notifications::{AppendOutcome, MessageStore, source::*},
};
use std::collections::{HashMap, HashSet};
/// Own on a dedicated receive worker: persistence acknowledgments and deliveries
/// must be serialized. At most one live delivery awaits durable acknowledgment.
/// Construction/configuration is internal trusted application configuration;
/// this type is not an IPC command and never discovers endpoints from messages.
pub struct NativeQQAdapter<T: ReceiveTransport> {
    transport: T,
    endpoint: LoopbackEndpoint,
    store: MessageStore,
    now: fn() -> UtcMillis,
    config: Option<SourceConfig>,
    health: SourceHealth,
    pending: Option<Delivery>,
    identities: HashMap<SourceId, SourceConfig>,
    recovered_groups: HashSet<String>,
}
impl<T: ReceiveTransport> NativeQQAdapter<T> {
    pub fn new(
        transport: T,
        endpoint: LoopbackEndpoint,
        store: MessageStore,
        now: fn() -> UtcMillis,
    ) -> Self {
        Self {
            transport,
            endpoint,
            store,
            now,
            config: None,
            health: SourceHealth::default(),
            pending: None,
            identities: HashMap::new(),
            recovered_groups: HashSet::new(),
        }
    }
    /// Call only after a successful calendar apply acknowledgment for this source.
    pub fn report_applied(&mut self, source: SourceId, at: UtcMillis) -> AppResult<()> {
        if self.config.as_ref().map(|c| c.source_id) != Some(source) {
            return Err(AppError::InvalidInput);
        }
        self.health.applied(at);
        Ok(())
    }
    fn normalize(&self, c: &SourceConfig, mut d: Delivery) -> AppResult<Delivery> {
        let original = d.message.message_key;
        if d.message.parts.iter().any(|p| p.message_key != original) {
            return Err(AppError::InvalidInput);
        }
        d.message.message_key =
            shixu_core::notifications::identity::message_identity(c, &d.message)?.key;
        for part in &mut d.message.parts {
            part.message_key = d.message.message_key;
        }
        d.message.received_at = (self.now)();
        Ok(d)
    }
    fn failure(&mut self, error: AppError) {
        self.health.failed(error, (self.now)());
        self.recovered_groups.clear();
    }
    fn ready(&self) -> AppResult<&SourceConfig> {
        match self.health.connection_state {
            ConnectionState::Connected => self.config.as_ref().ok_or(AppError::Disconnected),
            ConnectionState::WaitingForLogin => Err(AppError::AuthFailed),
            ConnectionState::Incompatible => Err(AppError::Unsupported),
            ConnectionState::Disconnected => Err(AppError::Disconnected),
        }
    }
    fn authorized(c: &SourceConfig, m: &MessageEnvelope) -> bool {
        m.source_id == c.source_id
            && m.account_id == c.account_id
            && c.allowed_group_ids.contains(&m.group_id)
    }
    /// Durable acknowledgment. Failure keeps the pending delivery and gap. Never
    /// refreshes last_applied_at; calendar application must report that separately.
    pub fn persist_pending(&mut self) -> AppResult<AppendOutcome> {
        let d = self.pending.as_ref().ok_or(AppError::Conflict)?;
        let c = self.config.as_ref().ok_or(AppError::Disconnected)?;
        match self
            .store
            .append_with_cursor(c, d.message.clone(), &d.cursor)
        {
            Ok(outcome) => {
                self.pending = None;
                if matches!(outcome, AppendOutcome::Stored | AppendOutcome::Duplicate) {
                    self.health.persisted((self.now)());
                } else {
                    self.failure(AppError::Conflict);
                }
                Ok(outcome)
            }
            Err(e) => {
                self.failure(e);
                Err(e)
            }
        }
    }
}
impl<T: ReceiveTransport> QQAdapter for NativeQQAdapter<T> {
    fn connect(&mut self, mut config: SourceConfig, token: SecretBytes) -> AppResult<SourceHealth> {
        if self.pending.is_some() {
            return Err(AppError::Conflict);
        }
        if !config.enabled
            || config.adapter_type.is_empty()
            || config.account_id.is_empty()
            || config.allowed_group_ids.is_empty()
            || config.allowed_group_ids.iter().any(|g| g.is_empty())
            || token.expose().is_empty()
        {
            return Err(AppError::InvalidInput);
        }
        if let Some(old) = self.identities.get(&config.source_id)
            && (old.account_id != config.account_id
                || old.adapter_type != config.adapter_type
                || old.allowed_group_ids != config.allowed_group_ids
                || old.timezone != config.timezone)
        {
            return Err(AppError::InvalidInput);
        }
        if self.config.is_some() {
            self.transport
                .disconnect()
                .inspect_err(|e| self.failure(*e))?;
            self.failure(AppError::Disconnected);
        }
        let changed = self
            .config
            .as_ref()
            .is_some_and(|old| old.source_id != config.source_id);
        if changed {
            self.health = SourceHealth::default();
            self.recovered_groups.clear();
        }
        self.identities.insert(config.source_id, config.clone());
        self.config = Some(config.clone());
        let caps = match self.transport.connect(&self.endpoint, &config, token) {
            Ok(c) => c,
            Err(e) => {
                self.failure(e);
                return Err(e);
            }
        };
        if !caps.contains(&SourceCapability::LiveMessages)
            || caps.iter().any(|c| !config.capability_set.contains(c))
        {
            let _ = self.transport.disconnect();
            self.failure(AppError::Unsupported);
            return Err(AppError::Unsupported);
        }
        config.capability_set = caps.clone();
        self.config = Some(config);
        self.health.connected((self.now)(), caps);
        Ok(self.health.clone())
    }
    fn disconnect(&mut self) -> AppResult<()> {
        let result = self.transport.disconnect();
        self.failure(
            result
                .as_ref()
                .err()
                .copied()
                .unwrap_or(AppError::Disconnected),
        );
        result
    }
    fn health(&self) -> SourceHealth {
        self.health.clone()
    }
    fn next_message(&mut self) -> AppResult<Option<MessageEnvelope>> {
        if self.pending.is_some() {
            return Err(AppError::Conflict);
        }
        let c = self.ready()?.clone();
        let d = match self.transport.next() {
            Ok(Some(d)) => d,
            Ok(None) => return Ok(None),
            Err(e) => {
                self.failure(e);
                return Err(e);
            }
        };
        if !Self::authorized(&c, &d.message) {
            return Ok(None);
        }
        let d = self.normalize(&c, d).inspect_err(|e| self.failure(*e))?;
        self.health.received((self.now)());
        let message = d.message.clone();
        self.pending = Some(d);
        Ok(Some(message))
    }
    fn backfill(&mut self, cursor: &str) -> AppResult<Vec<MessageEnvelope>> {
        if self.pending.is_some() {
            return Err(AppError::Conflict);
        }
        let c = self.ready()?.clone();
        if !self
            .health
            .capabilities
            .contains(&SourceCapability::Backfill)
        {
            return Err(AppError::Unsupported);
        }
        // Never accept a caller's unscoped cursor: resolve exactly one current
        // source/account/group durable cursor before invoking the receive transport.
        let mut owners = vec![];
        for group in &c.allowed_group_ids {
            if self.store.cursor(&c, group)?.as_deref() == Some(cursor) {
                owners.push(group.clone());
            }
        }
        if owners.len() != 1 {
            return Err(AppError::InvalidInput);
        }
        let group = &owners[0];
        let batch = match self.transport.backfill(group, cursor) {
            Ok(b) => b,
            Err(e) => {
                self.failure(e);
                return Err(e);
            }
        };
        if batch.group_id != *group
            || batch
                .deliveries
                .iter()
                .any(|d| !Self::authorized(&c, &d.message) || d.message.group_id != *group)
        {
            self.failure(AppError::InvalidInput);
            return Err(AppError::InvalidInput);
        }
        let mut messages = vec![];
        for d in batch.deliveries {
            let d = self.normalize(&c, d).inspect_err(|e| self.failure(*e))?;
            self.health.received((self.now)());
            match self
                .store
                .append_with_cursor(&c, d.message.clone(), &d.cursor)
            {
                Ok(AppendOutcome::Stored | AppendOutcome::Duplicate) => {
                    self.health.persisted((self.now)());
                    messages.push(d.message);
                }
                Ok(_) => {
                    self.failure(AppError::Conflict);
                    return Err(AppError::Conflict);
                }
                Err(e) => {
                    self.failure(e);
                    return Err(e);
                }
            }
        }
        if batch.complete {
            self.recovered_groups.insert(group.clone());
        } else {
            self.recovered_groups.remove(group);
        }
        if c.allowed_group_ids
            .iter()
            .all(|g| self.recovered_groups.contains(g))
        {
            self.health.gaps.clear();
        }
        Ok(messages)
    }
}
/// Deterministic reversible mapping of a native opaque ID to the F0 restricted
/// reference alphabet. Includes source/adapter/account/group; temporary URLs and
/// control characters are rejected. The real transport must resolve this mapping
/// transiently in N3 without persisting download URLs or credentials.
pub fn stable_attachment_ref(
    config: &SourceConfig,
    group: &str,
    native_id: &str,
) -> AppResult<SourceFileRef> {
    if native_id.is_empty()
        || native_id.len() > 1024
        || native_id.contains("://")
        || native_id.chars().any(char::is_control)
        || !config.allowed_group_ids.iter().any(|g| g == group)
    {
        return Err(AppError::InvalidInput);
    }
    let mut reference = String::from("native-v1");
    for field in [
        config.source_id.to_string(),
        config.adapter_type.clone(),
        config.account_id.clone(),
        group.into(),
        native_id.into(),
    ] {
        reference.push('-');
        for b in field.bytes() {
            use std::fmt::Write;
            write!(&mut reference, "{b:02x}").map_err(|_| AppError::InvalidInput)?;
        }
    }
    SourceFileRef::try_from(reference)
}
