//! Portable scheduler. No vault interface, credentials, provider or parser fallback.
use crate::{
    calendar::{EventService, match_event},
    contracts::{AppResult, calendar::ApplySummary, error::AppError, notification::*},
    notifications::{
        AppendOutcome, MessageStore,
        consent::ConsentStore,
        parts::{TaskLease, TaskQueue},
        settings::{SettingsStore, current},
        source::SourceHealth,
    },
    storage::{Database, coordinator::PauseGuard, database::storage_error},
};
use rusqlite::params;
use std::sync::{Arc, Mutex, atomic::Ordering};
#[derive(Clone, Default)]
pub struct RuntimeStatus {
    pub running: bool,
    pub sources: Vec<(SourceId, SourceHealth)>,
    pub attachment_queue: u32,
    pub model_queue: u32,
    pub pending_rules: u32,
    pub last_calendar_commit: Option<i64>,
    pub last_error: Option<AppError>,
}
pub struct Supervisor {
    pub(super) workers_owned: std::sync::atomic::AtomicBool,
    db: Arc<Database>,
    consent: Arc<ConsentStore>,
    status: Mutex<RuntimeStatus>,
    // Transport order and all rule applies are serialized; optional workers never own this lock.
    pub(super) transport_io: Mutex<()>,
    receive: Mutex<()>,
    apply: Mutex<()>,
    model: Mutex<()>,
    attachment: Mutex<Option<TaskLease>>,
}
impl Supervisor {
    pub fn new(db: Arc<Database>, consent: Arc<ConsentStore>) -> Self {
        Self {
            workers_owned: std::sync::atomic::AtomicBool::new(false),
            db,
            consent,
            status: Mutex::default(),
            transport_io: Mutex::new(()),
            receive: Mutex::new(()),
            apply: Mutex::new(()),
            model: Mutex::new(()),
            attachment: Mutex::new(None),
        }
    }
    pub fn start(&self, configs: Vec<SourceConfig>) -> AppResult<()> {
        let _receive = self.receive.lock().map_err(|_| AppError::Disconnected)?;
        if self
            .status
            .lock()
            .map_err(|_| AppError::Disconnected)?
            .running
        {
            return Ok(());
        }
        if self
            .db
            .runtime_owned
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(AppError::Conflict);
        }
        let result = (|| {
            self.consent.bind(self.db.coordinator())?;
            let settings = SettingsStore::new(self.db.clone());
            let existing = settings.sources()?;
            if configs.len() > 100 {
                return Err(AppError::InvalidInput);
            }
            let mut seen = std::collections::HashSet::new();
            for c in &configs {
                crate::notifications::settings::validate(c)?;
                if !seen.insert(c.source_id) {
                    return Err(AppError::InvalidInput);
                }
                if let Some(saved) = existing.iter().find(|s| s.config.source_id == c.source_id) {
                    if saved.config != *c {
                        return Err(AppError::Conflict);
                    }
                } else {
                    settings.save_source(c.clone())?;
                }
            }
            TaskQueue::new(self.db.clone()).recover_after_children_stopped()?;
            self.db.transaction(|tx|{tx.execute("UPDATE model_attempts SET state='done' WHERE consent_session!=?1",[&self.consent.instance_id]).map_err(storage_error)?;tx.execute("UPDATE model_attempts SET state=CASE WHEN attempts>=3 THEN 'done' ELSE 'queued' END WHERE state='running'",[]).map_err(storage_error)?;Ok(())})?;
            let mut status = self.status.lock().map_err(|_| AppError::Disconnected)?;
            status.running = true;
            let prior = status.sources.clone();
            status.sources = configs
                .into_iter()
                .map(|c| {
                    let health = prior
                        .iter()
                        .find(|(id, _)| *id == c.source_id)
                        .map(|(_, h)| h.clone())
                        .unwrap_or_default();
                    (c.source_id, health)
                })
                .collect();
            Ok(())
        })();
        if result.is_err() {
            self.db.runtime_owned.store(false, Ordering::SeqCst);
        }
        result
    }
    pub fn stop(&self) -> AppResult<()> {
        let _transport = self
            .transport_io
            .lock()
            .map_err(|_| AppError::Disconnected)?;
        let _receive = self.receive.lock().map_err(|_| AppError::Disconnected)?;
        let _apply = self.apply.lock().map_err(|_| AppError::Disconnected)?;
        let _model = self.model.lock().map_err(|_| AppError::Disconnected)?;
        let _attachment = self.attachment.lock().map_err(|_| AppError::Disconnected)?;
        let mut status = self.status.lock().map_err(|_| AppError::Disconnected)?;
        if status.running {
            status.running = false;
            self.db.runtime_owned.store(false, Ordering::SeqCst);
        }
        Ok(())
    }
    pub fn refresh_sources(&self) -> AppResult<()> {
        let configs = SettingsStore::new(self.db.clone()).sources()?;
        let mut status = self.status.lock().map_err(|_| AppError::Disconnected)?;
        let prior = std::mem::take(&mut status.sources);
        status.sources = configs
            .into_iter()
            .map(|s| {
                let mut health = prior
                    .iter()
                    .find(|(id, _)| *id == s.config.source_id)
                    .map(|(_, h)| h.clone())
                    .unwrap_or_default();
                if !s.config.enabled {
                    health.failed(AppError::Disconnected, health.last_received_at.unwrap_or(0));
                }
                (s.config.source_id, health)
            })
            .collect();
        Ok(())
    }
    pub fn suspend(&self, now: i64) -> AppResult<()> {
        self.stop()?;
        for setting in SettingsStore::new(self.db.clone()).sources()? {
            if setting.config.enabled {
                MessageStore::new(self.db.clone()).begin_recovery(&setting.config, now)?;
                self.failure(setting.config.source_id, AppError::Disconnected, now);
            }
        }
        Ok(())
    }
    pub fn resume(&self) -> AppResult<()> {
        self.start(
            SettingsStore::new(self.db.clone())
                .sources()?
                .into_iter()
                .map(|s| s.config)
                .collect(),
        )
    }
    pub fn login_start(&self) -> AppResult<()> {
        if !SettingsStore::new(self.db.clone()).autostart()? {
            return Err(AppError::Conflict);
        }
        self.resume()
    }
    pub fn pause_writes(&self) -> AppResult<PauseGuard> {
        self.db.pause_writes()
    }
    pub(super) fn is_running(&self) -> bool {
        self.status.lock().is_ok_and(|s| s.running)
    }
    pub(super) fn worker_failed(&self, e: AppError) {
        if let Ok(mut status) = self.status.lock() {
            status.last_error = Some(e);
            for (_, h) in &mut status.sources {
                h.failed(e, h.last_received_at.unwrap_or(0));
            }
        }
    }
    pub(super) fn received_online(&self, id: SourceId, at: i64) {
        if let Ok(mut s) = self.status.lock()
            && let Some((_, h)) = s.sources.iter_mut().find(|(source, _)| *source == id)
        {
            h.connected(at, h.capabilities.clone());
        }
    }
    pub fn status(&self) -> RuntimeStatus {
        let mut status = self
            .status
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        let counts = self.db.transaction(|tx| {
            Ok((
                tx.query_row(
                    "SELECT count(*) FROM runtime_work WHERE pending=1",
                    [],
                    |r| r.get(0),
                )
                .map_err(storage_error)?,
                tx.query_row(
                    "SELECT count(*) FROM attachment_tasks WHERE state!='done'",
                    [],
                    |r| r.get(0),
                )
                .map_err(storage_error)?,
                tx.query_row(
                    "SELECT count(*) FROM model_attempts WHERE state!='done'",
                    [],
                    |r| r.get(0),
                )
                .map_err(storage_error)?,
            ))
        });
        match counts {
            Ok((rules, attachments, models)) => {
                status.pending_rules = rules;
                status.attachment_queue = attachments;
                status.model_queue = models;
            }
            Err(e) => status.last_error = Some(e),
        }
        status
    }
    fn active(&self) -> AppResult<()> {
        if self
            .status
            .lock()
            .map_err(|_| AppError::Disconnected)?
            .running
        {
            Ok(())
        } else {
            Err(AppError::Disconnected)
        }
    }
    fn failure(&self, id: SourceId, error: AppError, now: i64) {
        if let Ok(mut s) = self.status.lock() {
            s.last_error = Some(error);
            if let Some((_, h)) = s.sources.iter_mut().find(|(source, _)| *source == id) {
                h.failed(error, now);
            }
        }
    }
    pub fn receive(&self, message: MessageEnvelope, cursor: &str) -> AppResult<AppendOutcome> {
        let _receive = self.receive.lock().map_err(|_| AppError::Disconnected)?;
        self.active()?;
        let id = message.source_id;
        let now = message.received_at;
        if let Some((_, h)) = self
            .status
            .lock()
            .map_err(|_| AppError::Disconnected)?
            .sources
            .iter_mut()
            .find(|(source, _)| *source == id)
        {
            h.received(now);
        } else {
            return Err(AppError::InvalidInput);
        }
        let result = (|| {
            let config = SettingsStore::new(self.db.clone())
                .sources()?
                .into_iter()
                .find(|s| s.config.source_id == id)
                .ok_or(AppError::Conflict)?
                .config;
            MessageStore::new(self.db.clone()).append_with_cursor(&config, message, cursor)
        })();
        match &result {
            Ok(AppendOutcome::Stored | AppendOutcome::Duplicate) => {
                if let Some((_, h)) = self
                    .status
                    .lock()
                    .map_err(|_| AppError::Disconnected)?
                    .sources
                    .iter_mut()
                    .find(|(source, _)| *source == id)
                {
                    h.persisted(now);
                }
            }
            Err(e) => self.failure(id, *e, now),
            _ => {}
        }
        result
    }
    /// Run only on the model lane: consent may be held by a finite provider call.
    pub fn schedule_models(&self, now: i64) -> AppResult<()> {
        self.active()?;
        let messages:Vec<(MessageEnvelope,i64)>=self.db.transaction(|tx|{
            let mut q=tx.prepare("SELECT m.payload,w.generation FROM runtime_work w JOIN messages m USING(message_key) WHERE w.pending=0 AND w.model_pending=1 AND m.payload IS NOT NULL ORDER BY m.received_at,m.message_key LIMIT 100").map_err(storage_error)?;
            q.query_map([],|r|Ok((r.get::<_,Vec<u8>>(0)?,r.get::<_,i64>(1)?))).map_err(storage_error)?.map(|r|{let(p,g)=r.map_err(storage_error)?;Ok((self.db.unprotect(&p)?,g))}).collect()
        })?;
        for (m, generation) in messages {
            match super::model_queue::enqueue(&self.db, &self.consent, &m, now) {
                Ok(()) => {}
                Err(AppError::StorageFull) => self.failure(m.source_id, AppError::StorageFull, now),
                Err(e) => return Err(e),
            }
            self.db.transaction(|tx|{tx.execute("UPDATE runtime_work SET model_pending=0 WHERE message_key=?1 AND generation=?2",params![m.message_key.to_string(),generation]).map_err(storage_error)?;Ok(())})?;
        }
        Ok(())
    }
    pub fn process_model(
        &self,
        now: i64,
        transport: &dyn crate::notifications::model::ModelTransport,
    ) -> AppResult<bool> {
        let _model = self.model.lock().map_err(|_| AppError::Disconnected)?;
        self.active()?;
        self.schedule_models(now)?;
        super::model_queue::run(self.db.clone(), &self.consent, transport, now)
    }
    pub fn process_attachment(&self, now: i64, parser: &dyn AttachmentParser) -> AppResult<bool> {
        let mut stopped = self.attachment.lock().map_err(|_| AppError::Disconnected)?;
        self.active()?;
        let queue = TaskQueue::new(self.db.clone());
        if let Some(job) = stopped.as_ref() {
            queue.release_stopped(job)?;
            *stopped = None;
        }
        let Some(job) = queue.claim(now, &ParserLimits::v01())? else {
            return Ok(false);
        };
        *stopped = Some(job);
        let job = stopped.as_ref().ok_or(AppError::Conflict)?;
        let allowed = self.db.transaction(|tx| {
            let m = match_event::message(&self.db, tx, job.message_key)?;
            Ok(m.revision == job.revision
                && !m.revoked
                && current(&self.db, tx, m.source_id)?
                    .is_some_and(|(_, c)| c.enabled && c.allowed_group_ids.contains(&m.group_id)))
        })?;
        let result = if allowed {
            parser.parse(&job.part, &ParserLimits::v01())
        } else {
            Err(AppError::AuthFailed)
        };
        let result = result.unwrap_or_else(|e| PartResult {
            part_id: job.part.part_id,
            status: if e == AppError::Unsupported {
                PartStatus::Unsupported
            } else {
                PartStatus::RecognitionFailed
            },
            blocks: vec![],
            reason_code: Some(match e {
                AppError::Unsupported => PartReason::FormatUnsupported,
                AppError::StorageFull => PartReason::StorageFull,
                AppError::AuthFailed => PartReason::PermissionDenied,
                _ => PartReason::RecognitionFailed,
            }),
        });
        let retry = result.status == PartStatus::DownloadFailed
            && result.reason_code == Some(PartReason::DownloadUnavailable);
        match queue.finish(job, result, retry, now) {
            Ok(()) => {
                *stopped = None;
                Ok(true)
            }
            Err(AppError::Conflict) => {
                queue.release_stopped(job)?;
                *stopped = None;
                Ok(true)
            }
            Err(e) => Err(e),
        }
    }
    pub fn dispatch(&self, batch: ExtractBatch) -> AppResult<ApplySummary> {
        let _apply = self.apply.lock().map_err(|_| AppError::Disconnected)?;
        self.active()?;
        EventService::new(self.db.clone()).apply(batch)
    }
    pub fn process_pending(&self, now: i64) -> AppResult<u32> {
        let _apply = self.apply.lock().map_err(|_| AppError::Disconnected)?;
        self.active()?;
        let work:Vec<(MessageKey,i64)>=self.db.transaction(|tx|{let mut stmt=tx.prepare("SELECT w.message_key,w.generation FROM runtime_work w JOIN messages m USING(message_key) WHERE w.pending=1 AND m.payload IS NOT NULL ORDER BY m.received_at,m.message_key LIMIT 100").map_err(storage_error)?;stmt.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?))).map_err(storage_error)?.map(|r|{let(k,g)=r.map_err(storage_error)?;Ok((k.parse()?,g))}).collect()})?;
        let mut applied = 0;
        for (key, generation) in work {
            let (message, batch) = self
                .db
                .transaction(|tx| match_event::extract_current(&self.db, tx, key))?;
            let result = (|| {
                // Current authorization affects new work, while immutable proof supplies interpretation.
                let allowed = self.db.transaction(|tx| {
                    Ok(
                        current(&self.db, tx, message.source_id)?.is_some_and(|(_, c)| {
                            c.enabled
                                && c.account_id == message.account_id
                                && c.allowed_group_ids.contains(&message.group_id)
                        }),
                    )
                })?;
                if !allowed {
                    self.db.transaction(|tx|{tx.execute("UPDATE runtime_work SET pending=0 WHERE message_key=?1 AND generation=?2",params![key.to_string(),generation]).map_err(storage_error)?;Ok(())})?;
                    return Ok(false);
                }
                EventService::new(self.db.clone()).apply(batch)?;
                if !message.revoked
                    && message
                        .parts
                        .iter()
                        .any(|p| p.parse_state == PartStatus::PendingDownload)
                {
                    match TaskQueue::new(self.db.clone()).enqueue(
                        &key,
                        message.revision,
                        now,
                        &ParserLimits::v01(),
                    ) {
                        Ok(_) => {}
                        Err(AppError::StorageFull | AppError::InvalidInput) => {
                            let parts = message
                                .parts
                                .iter()
                                .filter(|p| {
                                    p.kind != PartKind::Text
                                        && p.parse_state == PartStatus::PendingDownload
                                })
                                .map(|p| PartResult {
                                    part_id: p.part_id,
                                    status: PartStatus::LimitExceeded,
                                    blocks: vec![],
                                    reason_code: Some(PartReason::LimitExceeded),
                                })
                                .collect();
                            MessageStore::new(self.db.clone()).record_parts(
                                &key,
                                message.revision,
                                parts,
                            )?;
                            self.failure(message.source_id, AppError::StorageFull, now);
                        }
                        Err(e) => return Err(e),
                    }
                }
                self.db.transaction(|tx| {
                    tx.execute(
                        "UPDATE runtime_work SET pending=0 WHERE message_key=?1 AND generation=?2",
                        params![key.to_string(), generation],
                    )
                    .map_err(storage_error)?;
                    Ok(())
                })?;
                Ok(true)
            })();
            match result {
                Ok(true) => {
                    applied += 1;
                    let mut s = self.status.lock().map_err(|_| AppError::Disconnected)?;
                    s.last_calendar_commit = Some(now);
                    if let Some((_, h)) = s
                        .sources
                        .iter_mut()
                        .find(|(id, _)| *id == message.source_id)
                    {
                        h.applied(now);
                    }
                }
                Ok(false) => {}
                Err(e) => {
                    self.failure(message.source_id, e, now);
                    return Err(e);
                }
            }
        }
        Ok(applied)
    }
}
impl Drop for Supervisor {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

/// Must return only after its isolated child has exited; finite resource limits apply.
pub trait AttachmentParser: Send + Sync {
    fn parse(&self, part: &MessagePart, limits: &ParserLimits) -> AppResult<PartResult>;
}
