//! Receive adapter for normalized transports, including OneBot text wire events.
use super::transport::*;
use shixu_core::{
    contracts::{AppResult, UtcMillis, error::AppError, notification::*, vault::SecretBytes},
    notifications::{AppendOutcome, MessageStore, source::*},
};
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
    recovery_write_failed: bool,
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
            recovery_write_failed: false,
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
    /// Scope equal opaque cursor values by source, group and recovery epoch.
    pub fn recovery_handle(&self, group: &str) -> AppResult<String> {
        let c = self.ready()?;
        self.store
            .recoveries(c)?
            .into_iter()
            .find(|r| r.group_id == group && !r.complete)
            .map(|r| r.handle(c.source_id))
            .ok_or(AppError::InvalidInput)
    }
    fn refresh_gaps(&mut self) -> AppResult<()> {
        if let Some(c) = &self.config {
            let states = self.store.recoveries(c)?;
            self.health.gaps = states
                .iter()
                .filter(|r| !r.complete)
                .map(|r| Gap { since: r.since })
                .collect();
        }
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
        if let Some(c) = &self.config {
            self.recovery_write_failed = self.store.begin_recovery(c, (self.now)()).is_err();
            if !self.recovery_write_failed {
                let _ = self.refresh_gaps();
            }
        }
        // If recovery persistence failed, retain the conservative in-memory gap.
        // Resuming this durable binding creates a gap before reads after restart.
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
        if self.recovery_write_failed {
            let c = self.config.as_ref().ok_or(AppError::Disconnected)?;
            self.store.begin_recovery(c, (self.now)())?;
            self.recovery_write_failed = false;
            self.refresh_gaps()?;
        }
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
        config.allowed_group_ids.sort();
        config.allowed_group_ids.dedup();
        let resumed = self.store.bind_source(&config)?;
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
        }
        self.config = Some(config.clone());
        if resumed {
            self.store.begin_recovery(&config, (self.now)())?;
        }
        self.recovery_write_failed = false;
        self.refresh_gaps()?;
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
            Err(AppError::Unsupported) if self.transport.consumed_unsupported_delivery() => {
                return Err(AppError::Unsupported);
            }
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
        // Only the original anchor defines missing-interval coverage. Partial
        // results may omit earlier messages; their last ID proves no prefix.
        // A handle selects a group/epoch even when opaque cursor strings collide.
        let recoveries = self.store.recoveries(&c)?;
        let owners: Vec<_> = recoveries
            .into_iter()
            .filter(|r| {
                !r.complete
                    && if cursor.starts_with("recovery-v1-") {
                        r.handle(c.source_id) == cursor
                    } else {
                        r.anchor.as_deref() == Some(cursor)
                    }
            })
            .collect();
        if owners.len() != 1 {
            return Err(AppError::InvalidInput);
        }
        let recovery = &owners[0];
        let group = &recovery.group_id;
        let from = recovery.anchor.as_deref().ok_or(AppError::Unsupported)?;
        let batch = match self.transport.backfill(group, from) {
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
        let mut next = from.to_string();
        for d in batch.deliveries {
            let d = self.normalize(&c, d).inspect_err(|e| self.failure(*e))?;
            self.health.received((self.now)());
            if d.cursor.is_empty()
                || d.cursor.len() > 4096
                || d.cursor.contains(['/', '\\', ':'])
                || d.cursor.chars().any(char::is_control)
            {
                self.failure(AppError::InvalidInput);
                return Err(AppError::InvalidInput);
            }
            match self.store.append(&c, d.message.clone()) {
                Ok(AppendOutcome::Stored | AppendOutcome::Duplicate) => {
                    self.health.persisted((self.now)());
                    if batch.complete {
                        next = d.cursor;
                    }
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
        self.store
            .advance_recovery(&c, group, recovery.epoch, from, &next, batch.complete)
            .inspect_err(|e| self.failure(*e))?;
        self.refresh_gaps()?;
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
impl<T: ReceiveTransport> shixu_core::runtime::workers::ReceivePort for NativeQQAdapter<T> {
    fn poll(&mut self) -> AppResult<Option<shixu_core::runtime::workers::Delivery>> {
        if self.pending.is_none() {
            let _ = QQAdapter::next_message(self)?;
        }
        Ok(self
            .pending
            .as_ref()
            .map(|d| shixu_core::runtime::workers::Delivery {
                message: d.message.clone(),
                cursor: d.cursor.clone(),
            }))
    }
    fn acknowledge(&mut self, cursor: &str) -> AppResult<()> {
        let delivery = self.pending.as_ref().ok_or(AppError::Conflict)?;
        if delivery.cursor != cursor {
            return Err(AppError::Conflict);
        }
        let current = self.store.verify_committed_delivery(
            self.config.as_ref().ok_or(AppError::Disconnected)?,
            &delivery.message,
            cursor,
        )?;
        if self.recovery_write_failed {
            self.store.begin_recovery(&current, (self.now)())?;
            self.recovery_write_failed = false;
        }
        self.config = Some(current);
        self.pending = None;
        self.health.persisted((self.now)());
        Ok(())
    }
    fn disconnect(&mut self) -> AppResult<()> {
        QQAdapter::disconnect(self)
    }
}
