//! Protected OneBot configuration and a bounded control capability. Only the
//! receive worker owns sockets; UI calls never hold a database/runtime lock while waiting.
use super::{onebot::connected_onebot_receiver, transport::LoopbackEndpoint};
use shixu_core::{
    contracts::{AppResult, error::AppError, notification::*, vault::SecretBytes},
    notifications::{
        MessageStore,
        settings::{SettingsStore, SourceSetting},
    },
    runtime::workers::{Delivery, ReceivePort},
    storage::Database,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, SyncSender},
};
use std::time::{Duration, Instant};
use zeroize::Zeroizing;
const RECORD: &[u8] = b"shixu-onebot11-text-v1\0";
fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}
#[derive(serde::Serialize)]
pub struct ConnectionMetadata {
    pub endpoint: Option<String>,
    pub has_credential: bool,
    pub active: bool,
    pub last_error: Option<AppError>,
}
#[derive(Default)]
struct Status {
    active: Option<(SourceId, u64)>,
    last_error: Option<AppError>,
}
enum Action {
    Connect(SourceId),
    Disconnect(SourceId),
}
struct Request {
    action: Action,
    deadline: Instant,
    reply: SyncSender<AppResult<()>>,
}
pub struct ConnectionController {
    gate: Arc<Mutex<()>>,
    db: Arc<Database>,
    send: SyncSender<Request>,
    status: Arc<Mutex<Status>>,
    alive: Arc<AtomicBool>,
}
pub struct ManagedReceiver {
    gate: Arc<Mutex<()>>,
    db: Arc<Database>,
    requests: Receiver<Request>,
    status: Arc<Mutex<Status>>,
    alive: Arc<AtomicBool>,
    active: Option<(SourceSetting, Box<dyn ReceivePort>)>,
    pending: bool,
}
fn setting(db: &Arc<Database>, id: SourceId) -> AppResult<SourceSetting> {
    let s = SettingsStore::new(db.clone())
        .sources()?
        .into_iter()
        .find(|s| s.config.source_id == id)
        .ok_or(AppError::InvalidInput)?;
    if s.config.adapter_type != "onebot11-text"
        || !s.config.enabled
        || s.config.allowed_group_ids.is_empty()
    {
        return Err(AppError::InvalidInput);
    }
    Ok(s)
}
fn record(db: &Database, c: &SourceConfig) -> AppResult<Option<(String, SecretBytes)>> {
    let Some(bytes) = db.source_secret(c)? else {
        return Ok(None);
    };
    let b = bytes.expose();
    if !b.starts_with(RECORD) || b.len() < RECORD.len() + 2 {
        return Err(AppError::ParseFailed);
    }
    let n = u16::from_le_bytes([b[RECORD.len()], b[RECORD.len() + 1]]) as usize;
    let start = RECORD.len() + 2;
    let end = start.checked_add(n).ok_or(AppError::ParseFailed)?;
    let endpoint = std::str::from_utf8(b.get(start..end).ok_or(AppError::ParseFailed)?)
        .map_err(|_| AppError::ParseFailed)?
        .to_owned();
    LoopbackEndpoint::parse(&endpoint)?;
    let token = b.get(end..).ok_or(AppError::ParseFailed)?;
    validate_token(token)?;
    Ok(Some((endpoint, SecretBytes::new(token.to_vec()))))
}
pub fn validate_token(token: &[u8]) -> AppResult<()> {
    if token.is_empty() || token.len() > 4096 || !token.iter().all(|b| b.is_ascii_graphic()) {
        Err(AppError::InvalidInput)
    } else {
        Ok(())
    }
}
impl ConnectionController {
    pub fn new(db: Arc<Database>) -> (Arc<Self>, Box<dyn ReceivePort>) {
        let gate = Arc::new(Mutex::new(()));
        let (send, requests) = mpsc::sync_channel(4);
        let status = Arc::new(Mutex::new(Status::default()));
        let alive = Arc::new(AtomicBool::new(true));
        let receiver = ManagedReceiver {
            gate: gate.clone(),
            db: db.clone(),
            requests,
            status: status.clone(),
            alive: alive.clone(),
            active: None,
            pending: false,
        };
        (
            Arc::new(Self {
                gate,
                db,
                send,
                status,
                alive,
            }),
            Box::new(receiver),
        )
    }
    pub fn save(&self, id: SourceId, endpoint: &str, token: SecretBytes) -> AppResult<()> {
        let _gate = self.gate.try_lock().map_err(|_| AppError::Conflict)?;
        let s = setting(&self.db, id)?;
        let endpoint = LoopbackEndpoint::parse(endpoint)?.address().to_string();
        validate_token(token.expose())?;
        if self
            .status
            .lock()
            .map_err(|_| AppError::Disconnected)?
            .active
            .is_some()
        {
            return Err(AppError::Conflict);
        }
        let mut bytes = Zeroizing::new(RECORD.to_vec());
        bytes.extend_from_slice(&(endpoint.len() as u16).to_le_bytes());
        bytes.extend_from_slice(endpoint.as_bytes());
        bytes.extend_from_slice(token.expose());
        self.db
            .store_source_secret(&s.config, &SecretBytes::new(std::mem::take(&mut *bytes)))
    }
    pub fn read(&self, id: SourceId) -> AppResult<ConnectionMetadata> {
        let s = SettingsStore::new(self.db.clone())
            .sources()?
            .into_iter()
            .find(|s| s.config.source_id == id)
            .ok_or(AppError::InvalidInput)?;
        if !s.config.enabled || s.config.allowed_group_ids.is_empty() {
            return Ok(ConnectionMetadata {
                endpoint: None,
                has_credential: self.db.has_source_secret(&s.config)?,
                active: false,
                last_error: self
                    .status
                    .lock()
                    .map_err(|_| AppError::Disconnected)?
                    .last_error,
            });
        }
        let saved = record(&self.db, &s.config)?;
        let status = self.status.lock().map_err(|_| AppError::Disconnected)?;
        Ok(ConnectionMetadata {
            endpoint: saved.as_ref().map(|r| r.0.clone()),
            has_credential: saved.is_some(),
            active: s.config.enabled && status.active == Some((id, s.epoch)),
            last_error: status.last_error,
        })
    }
    fn request(&self, action: Action) -> AppResult<()> {
        if !self.alive.load(Ordering::SeqCst) {
            return Err(AppError::Disconnected);
        }
        let (reply, result) = mpsc::sync_channel(1);
        self.send
            .try_send(Request {
                action,
                deadline: Instant::now() + Duration::from_secs(4),
                reply,
            })
            .map_err(|e| match e {
                mpsc::TrySendError::Full(_) => AppError::Conflict,
                mpsc::TrySendError::Disconnected(_) => AppError::Disconnected,
            })?;
        result
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| AppError::Disconnected)?
    }
    pub fn connect(&self, id: SourceId) -> AppResult<()> {
        self.request(Action::Connect(id))
    }
    pub fn disconnect(&self, id: SourceId) -> AppResult<()> {
        self.request(Action::Disconnect(id))
    }
}
impl ManagedReceiver {
    fn retire(&mut self, mut error: Option<AppError>) -> AppResult<()> {
        if let Some((s, mut receiver)) = self.active.take() {
            // Current authority governs any ack. No false ack of revoked deliveries.
            if self.pending
                && receiver
                    .poll()
                    .and_then(|d| {
                        d.map(|d| receiver.acknowledge(&d.cursor))
                            .unwrap_or(Err(AppError::Conflict))
                    })
                    .is_err()
            {
                error = Some(AppError::Conflict);
            }
            let result = receiver.disconnect();
            self.pending = false;
            if let Err(e) = MessageStore::new(self.db.clone()).begin_recovery(&s.config, now()) {
                error = Some(e);
            }
            let mut status = self.status.lock().map_err(|_| AppError::Disconnected)?;
            status.active = None;
            status.last_error = error;
            result
        } else {
            Ok(())
        }
    }
    fn guard_epoch(&mut self) -> AppResult<()> {
        if let Some((old, _)) = &self.active {
            let current = SettingsStore::new(self.db.clone())
                .sources()?
                .into_iter()
                .find(|s| s.config.source_id == old.config.source_id);
            if current.is_none_or(|s| s.epoch != old.epoch || !s.config.enabled) {
                self.retire(Some(AppError::Conflict))?;
            }
        }
        Ok(())
    }
    fn action(&mut self, action: Action) -> AppResult<()> {
        let gate = self.gate.clone();
        let _gate = gate.try_lock().map_err(|_| AppError::Conflict)?;
        match action {
            Action::Disconnect(id) => {
                if self
                    .active
                    .as_ref()
                    .is_some_and(|(s, _)| s.config.source_id != id)
                {
                    return Err(AppError::Conflict);
                }
                self.retire(None)
            }
            Action::Connect(id) => {
                if self.active.is_some() {
                    return Err(AppError::Conflict);
                }
                let s = setting(&self.db, id)?;
                let (endpoint, token) =
                    record(&self.db, &s.config)?.ok_or(AppError::InvalidInput)?;
                let mut receiver = connected_onebot_receiver(
                    LoopbackEndpoint::parse(&endpoint)?,
                    s.config.clone(),
                    token,
                    MessageStore::new(self.db.clone()),
                    now,
                )?;
                if setting(&self.db, id)?.epoch != s.epoch {
                    let _ = receiver.disconnect();
                    return Err(AppError::Conflict);
                }
                self.status
                    .lock()
                    .map_err(|_| AppError::Disconnected)?
                    .active = Some((id, s.epoch));
                self.active = Some((s, receiver));
                Ok(())
            }
        }
    }
}
impl ReceivePort for ManagedReceiver {
    fn service_controls(&mut self) -> AppResult<()> {
        self.guard_epoch()?;
        if let Ok(request) = self.requests.try_recv() {
            let creates_connection =
                matches!(&request.action, Action::Connect(_)) && self.active.is_none();
            let result = if Instant::now() > request.deadline {
                Err(AppError::Disconnected)
            } else {
                self.action(request.action)
            };
            if let Err(e) = result {
                self.status
                    .lock()
                    .map_err(|_| AppError::Disconnected)?
                    .last_error = Some(e)
            }
            if request.reply.send(result).is_err() && creates_connection && result.is_ok() {
                self.retire(Some(AppError::Disconnected))?;
            }
        }
        Ok(())
    }
    fn poll(&mut self) -> AppResult<Option<Delivery>> {
        self.guard_epoch()?;
        let Some((_, receiver)) = self.active.as_mut() else {
            return Ok(None);
        };
        match receiver.poll() {
            Ok(d) => {
                self.pending = d.is_some();
                Ok(d)
            }
            Err(e) => {
                if e != AppError::Unsupported {
                    self.retire(Some(e))?
                }
                Err(e)
            }
        }
    }
    fn acknowledge(&mut self, cursor: &str) -> AppResult<()> {
        let result = self
            .active
            .as_mut()
            .ok_or(AppError::Disconnected)?
            .1
            .acknowledge(cursor);
        if result.is_ok() {
            self.pending = false
        }
        result
    }
    fn disconnect(&mut self) -> AppResult<()> {
        self.alive.store(false, Ordering::SeqCst);
        while let Ok(r) = self.requests.try_recv() {
            let _ = r.reply.send(Err(AppError::Disconnected));
        }
        self.retire(None)
    }
}
impl Drop for ManagedReceiver {
    fn drop(&mut self) {
        let _ = self.disconnect();
    }
}
