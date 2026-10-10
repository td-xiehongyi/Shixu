//! Native-only single service owner. Epoch revocation and cancellation never wait
//! for the worker's blocking engine operation. No session capability crosses IPC.
pub use crate::vault_signal::{NativeVaultEvent, VaultEventSignal};
use shixu_core::{
    contracts::{
        AppResult,
        error::AppError,
        vault::{LockReason, SecretBytes, SessionId, VaultMutation, VaultSummary},
    },
    vault::VaultService,
};
use shixu_native::vault::{
    VaultCancellation,
    clipboard::{ClipboardPort, SystemClipboard},
    engine::KdbxWebEngine,
};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
type LifecycleHandler = Arc<dyn Fn(NativeVaultEvent) + Send + Sync>;
const DEADLINE: Duration = Duration::from_secs(35);
const IDLE: Duration = Duration::from_secs(300);
#[derive(serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CopyField {
    Account,
    Password,
}
pub enum Operation {
    Create(SecretBytes),
    Unlock(SecretBytes),
    Activity,
    List,
    Apply(VaultMutation),
    Reveal(String),
    Copy(String, CopyField),
    Change(SecretBytes, SecretBytes),
    Lock(LockReason),
}
pub enum Reply {
    Unit,
    List(Vec<VaultSummary>),
    Summary(VaultSummary),
    Secret(SecretBytes),
    Copy(SecretBytes),
}
struct Request {
    generation: u64,
    epoch: u64,
    op: Operation,
    cancel: VaultCancellation,
    reply: SyncSender<(AppResult<Reply>, Option<SessionId>)>,
}
struct Authority {
    generation: u64,
    epoch: u64,
    unlocked: bool,
    session: Option<SessionId>,
    activity: Option<Instant>,
    cancel: VaultCancellation,
}
impl Authority {
    fn synchronize(&mut self, signal: &VaultEventSignal) {
        let generation = signal.generation();
        if self.generation != generation {
            self.revoke();
            self.generation = generation;
        }
    }
    fn revoke(&mut self) {
        self.epoch += 1;
        self.unlocked = false;
        self.session = None;
        self.activity = None;
        self.cancel.cancel();
    }
    fn expire(&mut self) {
        if self.activity.is_some_and(|t| t.elapsed() >= IDLE) {
            self.revoke();
        }
    }
}
/// Owned reply bound to its original native admission and service session.
/// No serialization or publication occurs until publish holds the authority gate.
pub struct PendingDelivery<'a> {
    controller: &'a VaultController,
    generation: u64,
    epoch: u64,
    session: Option<SessionId>,
    reply: Reply,
}
impl PendingDelivery<'_> {
    pub fn publish<T>(self, publish: impl FnOnce(Reply) -> AppResult<T>) -> AppResult<T> {
        self.publish_checked(|reply, _check| publish(reply))
    }
    pub fn publish_checked<T>(
        self,
        publish: impl FnOnce(Reply, &dyn Fn() -> AppResult<()>) -> AppResult<T>,
    ) -> AppResult<T> {
        let mut a = self
            .controller
            .authority
            .lock()
            .map_err(|_| AppError::Locked)?;
        a.expire();
        let check = || self.controller.signal.check(self.generation);
        check()?;
        if a.epoch != self.epoch || a.session != self.session {
            return Err(AppError::Locked);
        }
        let result = (|| {
            let reply = match self.reply {
                Reply::Copy(secret) => {
                    self.controller
                        .clipboard
                        .lock()
                        .map_err(|_| AppError::Unsupported)?
                        .write_checked(secret, &check)?;
                    Reply::Unit
                }
                reply => reply,
            };
            check()?;
            publish(reply, &check)
        })();
        if result.is_err() && self.controller.signal.generation() == self.generation {
            a.revoke();
        }
        result
    }
}
pub struct VaultController {
    queue: SyncSender<Request>,
    authority: Arc<Mutex<Authority>>,
    stop: Arc<AtomicBool>,
    threads: Mutex<Vec<JoinHandle<()>>>,
    clipboard: Mutex<Box<dyn ClipboardPort + Send>>,
    signal: Arc<VaultEventSignal>,
    lifecycle_handler: Arc<Mutex<Option<LifecycleHandler>>>,
}
impl VaultController {
    /// Missing or unsupported resources produce a locked unavailable controller;
    /// optional vault setup cannot stop calendar/QQ initialization.
    pub fn prepared(resources: std::path::PathBuf, work: std::path::PathBuf) -> Arc<Self> {
        Self::new(KdbxWebEngine::prepared(resources, work).ok())
    }
    pub fn unavailable() -> Arc<Self> {
        Self::new(None)
    }
    fn new(engine: Option<KdbxWebEngine>) -> Arc<Self> {
        let arm = Arc::new(Mutex::new(VaultCancellation::default()));
        Self::new_with(
            engine.map(|engine| ArmedEngine {
                engine,
                arm: arm.clone(),
            }),
            arm,
        )
    }
    fn new_with<E: shixu_core::vault::ports::VaultEngine + Send + 'static>(
        engine: Option<E>,
        arm: Arc<Mutex<VaultCancellation>>,
    ) -> Arc<Self> {
        Self::new_with_port(engine, arm, SystemClipboard)
    }
    fn new_with_port<E: shixu_core::vault::ports::VaultEngine + Send + 'static>(
        engine: Option<E>,
        arm: Arc<Mutex<VaultCancellation>>,
        clipboard: impl ClipboardPort + Send + 'static,
    ) -> Arc<Self> {
        let (wake, wake_rx) = mpsc::sync_channel(1);
        let signal = Arc::new(VaultEventSignal::new(wake));
        let (queue, rx) = mpsc::sync_channel::<Request>(1);
        let authority = Arc::new(Mutex::new(Authority {
            generation: 0,
            epoch: 0,
            unlocked: false,
            session: None,
            activity: None,
            cancel: VaultCancellation::default(),
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let gate = authority.clone();
        let stopping = stop.clone();
        let worker_signal = signal.clone();
        let worker = thread::spawn(move || {
            let origin = Instant::now();
            let now = || origin.elapsed().as_millis().min(i64::MAX as u128) as i64;
            // The engine is armed through this native adapter before each admitted startup.
            let mut service = engine.map(VaultService::new);
            let mut session: Option<SessionId> = None;
            let mut owned_epoch = 0;
            while !stopping.load(Ordering::Acquire) {
                let current = match gate.lock() {
                    Ok(mut a) => {
                        a.synchronize(&worker_signal);
                        a.expire();
                        a.epoch
                    }
                    Err(_) => break,
                };
                if owned_epoch != current {
                    if let Some(s) = &mut service {
                        let _ = s.lock(LockReason::Manual);
                    }
                    session = None;
                    owned_epoch = current;
                }
                let request = match rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(r) => r,
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(_) => break,
                };
                let admitted = worker_signal.check(request.generation).is_ok()
                    && gate.lock().is_ok_and(|a| a.epoch == request.epoch);
                if !admitted {
                    let _ = request.reply.send((Err(AppError::Locked), None));
                    continue;
                }
                if owned_epoch != request.epoch {
                    if let Some(s) = &mut service {
                        let _ = s.lock(LockReason::Manual);
                    }
                    session = None;
                    owned_epoch = request.epoch;
                }
                let startup = matches!(request.op, Operation::Create(_) | Operation::Unlock(_));
                let interaction = !matches!(request.op, Operation::Lock(_));
                let result = if let Operation::Lock(reason) = request.op {
                    session = None;
                    service
                        .as_mut()
                        .map_or(Ok(()), |s| s.lock(reason))
                        .map(|_| Reply::Unit)
                } else if let Some(s) = &mut service {
                    // Arming occurs only for the handle already registered in the
                    // admission gate; cancellation before this point stays sticky.
                    if startup && let Ok(mut handle) = arm.lock() {
                        *handle = request.cancel;
                    }
                    let result = (|| {
                        s.tick(now())?;
                        match request.op {
                            Operation::Create(master) => {
                                session = Some(s.create(master, now())?);
                                Ok(Reply::Unit)
                            }
                            Operation::Unlock(master) => {
                                session = Some(s.unlock(master, now())?);
                                Ok(Reply::Unit)
                            }
                            op => {
                                let original = session.ok_or(AppError::Locked)?;
                                let result = match op {
                                    Operation::Activity => {
                                        s.activity(&original, now()).map(|_| Reply::Unit)
                                    }
                                    Operation::List => s.list(&original, now()).map(Reply::List),
                                    Operation::Apply(m) => {
                                        s.apply(&original, m, now()).map(Reply::Summary)
                                    }
                                    Operation::Reveal(id) => {
                                        s.reveal(&original, &id, now()).map(Reply::Secret)
                                    }
                                    Operation::Copy(id, CopyField::Password) => {
                                        s.reveal(&original, &id, now()).map(Reply::Copy)
                                    }
                                    Operation::Copy(id, CopyField::Account) => {
                                        let rows = s.list(&original, now())?;
                                        let row = rows
                                            .into_iter()
                                            .find(|row| {
                                                row.entry_id.to_string() == id.to_ascii_lowercase()
                                            })
                                            .ok_or(AppError::InvalidInput)?;
                                        Ok(Reply::Copy(SecretBytes::new(row.account.into_bytes())))
                                    }
                                    Operation::Change(current, next) => s
                                        .change_master(&original, current, next)
                                        .and_then(|_| s.activity(&original, now()))
                                        .map(|_| Reply::Unit),
                                    _ => unreachable!(),
                                };
                                s.tick(now())?;
                                match result {
                                    Ok(reply) => s.accept_reply(&original, Ok(reply)),
                                    Err(error) => Err(error),
                                }
                            }
                        }
                    })();
                    if startup && result.is_ok() {
                        s.tick(now())
                            .and_then(|_| s.accept_reply(&session.ok_or(AppError::Locked)?, result))
                    } else {
                        result
                    }
                } else {
                    Err(AppError::Unsupported)
                };
                let mut a = match gate.lock() {
                    Ok(a) => a,
                    Err(_) => break,
                };
                a.synchronize(&worker_signal);
                a.expire();
                let result = if a.epoch != request.epoch
                    || worker_signal.check(request.generation).is_err()
                {
                    Err(AppError::Locked)
                } else {
                    result
                };
                if result.is_ok() && interaction {
                    a.unlocked = true;
                    a.session = session;
                    a.activity = Some(Instant::now());
                }
                let failed = result.is_err();
                if failed && a.epoch == request.epoch {
                    a.revoke();
                }
                drop(a);
                if failed {
                    session = None;
                    if let Some(s) = &mut service {
                        let _ = s.lock(LockReason::Manual);
                    }
                }
                // Bounded one-shot sender: an abandoned caller cannot strand worker.
                let _ = request.reply.try_send((result, session));
            }
            if let Some(mut s) = service {
                let _ = s.lock(LockReason::Exit);
            }
        });
        let gate = authority.clone();
        let stopping = stop.clone();
        let ticker = thread::spawn(move || {
            while !stopping.load(Ordering::Acquire) {
                if let Ok(mut a) = gate.lock() {
                    a.expire();
                }
                thread::sleep(Duration::from_millis(100));
            }
        });
        let lifecycle_handler = Arc::new(Mutex::new(None::<LifecycleHandler>));
        let handler = lifecycle_handler.clone();
        let dispatch_signal = signal.clone();
        let gate = authority.clone();
        let stopping = stop.clone();
        let dispatcher = thread::spawn(move || {
            struct Consumer(Arc<VaultEventSignal>);
            impl Drop for Consumer {
                fn drop(&mut self) {
                    self.0.fail();
                }
            }
            let _consumer = Consumer(dispatch_signal.clone());
            while !stopping.load(Ordering::Acquire) {
                match wake_rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(()) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(_) => break,
                }
                let pending = dispatch_signal.take_pending();
                if pending == 0 {
                    continue;
                }
                // UI redaction does not wait for cancellation/engine ownership.
                let callback = handler.lock().ok().and_then(|h| h.clone());
                if let Some(callback) = &callback {
                    callback(NativeVaultEvent::Revoked);
                }
                if pending & crate::vault_signal::WINDOW_CLOSED != 0
                    && let Some(callback) = &callback
                {
                    callback(NativeVaultEvent::WindowClosed);
                }
                if let Ok(mut a) = gate.lock() {
                    a.synchronize(&dispatch_signal);
                } else {
                    break;
                }
            }
        });
        Arc::new(Self {
            queue,
            signal,
            lifecycle_handler,
            clipboard: Mutex::new(Box::new(clipboard)),
            authority,
            stop,
            threads: Mutex::new(vec![worker, ticker, dispatcher]),
        })
    }
    pub fn set_lifecycle_handler(
        &self,
        handler: Arc<dyn Fn(NativeVaultEvent) + Send + Sync>,
    ) -> AppResult<()> {
        *self
            .lifecycle_handler
            .lock()
            .map_err(|_| AppError::Locked)? = Some(handler);
        Ok(())
    }
    pub fn signal(&self) -> Arc<VaultEventSignal> {
        self.signal.clone()
    }
    pub fn execute(&self, op: Operation) -> AppResult<Reply> {
        self.deliver(op, Ok)
    }
    pub fn deliver<T>(
        &self,
        op: Operation,
        deliver: impl FnOnce(Reply) -> AppResult<T>,
    ) -> AppResult<T> {
        self.prepare(op)?.publish(deliver)
    }
    pub fn deliver_checked<T>(
        &self,
        op: Operation,
        deliver: impl FnOnce(Reply, &dyn Fn() -> AppResult<()>) -> AppResult<T>,
    ) -> AppResult<T> {
        self.prepare(op)?.publish_checked(deliver)
    }
    pub fn prepare(&self, op: Operation) -> AppResult<PendingDelivery<'_>> {
        let (reply, rx) = mpsc::sync_channel(1);
        let mut a = self.authority.lock().map_err(|_| AppError::Locked)?;
        a.synchronize(&self.signal);
        a.expire();
        let generation = self.signal.generation();
        self.signal.check(generation)?;
        let startup = matches!(op, Operation::Create(_) | Operation::Unlock(_));
        let locking = matches!(op, Operation::Lock(_));
        if startup || locking {
            a.revoke();
        }
        if startup {
            a.cancel = VaultCancellation::default();
            a.activity = Some(Instant::now());
        }
        let epoch = a.epoch;
        self.queue
            .try_send(Request {
                generation,
                epoch,
                op,
                cancel: a.cancel.clone(),
                reply,
            })
            .map_err(|_| AppError::Conflict)?;
        drop(a);
        let response = rx
            .recv_timeout(DEADLINE)
            .map_err(|_| AppError::Disconnected);
        if response.is_err() {
            let mut a = self.authority.lock().map_err(|_| AppError::Locked)?;
            if a.epoch == epoch {
                a.revoke();
            }
        }
        let (result, session) = response?;
        Ok(PendingDelivery {
            controller: self,
            generation,
            epoch,
            session,
            reply: result?,
        })
    }
    pub fn lock(&self, reason: LockReason) -> AppResult<()> {
        self.execute(Operation::Lock(reason)).map(|_| ())
    }
    /// Orderly shutdown is outside the notification callback. It releases
    /// handler ownership and waits for actual service/engine cleanup.
    pub fn shutdown(&self) {
        self.signal.fail();
        self.stop.store(true, Ordering::Release);
        if let Ok(mut handler) = self.lifecycle_handler.lock() {
            *handler = None;
        }
        if let Ok(mut a) = self.authority.lock() {
            a.revoke();
        }
        if let Ok(mut threads) = self.threads.lock() {
            for thread in threads.drain(..) {
                let _ = thread.join();
            }
        }
    }
    pub fn tick(&self) -> AppResult<()> {
        self.authority
            .lock()
            .map_err(|_| AppError::Locked)?
            .expire();
        Ok(())
    }
}
impl Drop for VaultController {
    fn drop(&mut self) {
        self.shutdown();
    }
}
impl crate::lifecycle::VaultLifecycle for VaultController {
    fn lock(&self, reason: LockReason) -> AppResult<()> {
        self.lock(reason)
    }
    fn tick(&self, _: i64) -> AppResult<()> {
        self.tick()
    }
}
// All business operations still go through VaultService. The wrapper only arms
// the engine at the instant it begins create/open, after service cleanup.
struct ArmedEngine {
    engine: KdbxWebEngine,
    arm: Arc<Mutex<VaultCancellation>>,
}
impl shixu_core::vault::ports::VaultEngine for ArmedEngine {
    fn create(&mut self, master: SecretBytes) -> AppResult<()> {
        self.engine
            .arm(self.arm.lock().map_err(|_| AppError::Locked)?.clone());
        self.engine.create(master)
    }
    fn open(&mut self, master: SecretBytes) -> AppResult<()> {
        self.engine
            .arm(self.arm.lock().map_err(|_| AppError::Locked)?.clone());
        self.engine.open(master)
    }
    fn close(&mut self) -> AppResult<()> {
        self.engine.close()
    }
    fn list(&mut self) -> AppResult<Vec<VaultSummary>> {
        self.engine.list()
    }
    fn apply(&mut self, m: VaultMutation) -> AppResult<VaultSummary> {
        self.engine.apply(m)
    }
    fn read_secret(&mut self, id: &str) -> AppResult<SecretBytes> {
        self.engine.read_secret(id)
    }
    fn change_master(&mut self, current: SecretBytes, next: SecretBytes) -> AppResult<()> {
        self.engine.change_master(current, next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shixu_core::vault::ports::VaultEngine;
    struct Controlled {
        fail_startup: bool,
        startup: Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>,
        reveal: Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>,
    }
    impl VaultEngine for Controlled {
        fn create(&mut self, master: SecretBytes) -> AppResult<()> {
            self.open(master)
        }
        fn open(&mut self, _: SecretBytes) -> AppResult<()> {
            if let Some((started, release)) = self.startup.take() {
                started.send(()).unwrap();
                release.recv().unwrap();
                if self.fail_startup {
                    return Err(AppError::AuthFailed);
                }
            }
            Ok(())
        }
        fn close(&mut self) -> AppResult<()> {
            Ok(())
        }
        fn list(&mut self) -> AppResult<Vec<VaultSummary>> {
            Ok(vec![])
        }
        fn apply(&mut self, _: VaultMutation) -> AppResult<VaultSummary> {
            Err(AppError::Unsupported)
        }
        fn read_secret(&mut self, _: &str) -> AppResult<SecretBytes> {
            if let Some((started, release)) = self.reveal.take() {
                started.send(()).unwrap();
                release.recv().unwrap();
            }
            Ok(SecretBytes::new(b"late synthetic secret".to_vec()))
        }
        fn change_master(&mut self, _: SecretBytes, _: SecretBytes) -> AppResult<()> {
            Ok(())
        }
    }
    fn master() -> SecretBytes {
        SecretBytes::new(b"synthetic master".to_vec())
    }
    fn controller(engine: Controlled) -> Arc<VaultController> {
        VaultController::new_with(
            Some(engine),
            Arc::new(Mutex::new(VaultCancellation::default())),
        )
    }
    fn wait_epoch(c: &VaultController, epoch: u64) {
        let start = Instant::now();
        while c.authority.lock().unwrap().epoch < epoch {
            assert!(start.elapsed() < Duration::from_secs(2));
            thread::yield_now();
        }
    }
    #[test]
    fn old_delayed_unlock_error_cannot_cancel_new_startup_generation() {
        let (started, ready) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let c = controller(Controlled {
            fail_startup: true,
            startup: Some((started, wait)),
            reveal: None,
        });
        let old = c.clone();
        let old = thread::spawn(move || old.execute(Operation::Unlock(master())));
        ready.recv_timeout(Duration::from_secs(2)).unwrap();
        c.authority.lock().unwrap().revoke(); // same authoritative revocation as idle/lifecycle
        let new = c.clone();
        let new = thread::spawn(move || new.execute(Operation::Unlock(master())));
        wait_epoch(&c, 3);
        release.send(()).unwrap();
        assert!(matches!(old.join().unwrap(), Err(AppError::Locked)));
        assert!(matches!(new.join().unwrap(), Ok(Reply::Unit)));
        assert!(matches!(c.execute(Operation::List), Ok(Reply::List(_))));
    }
    #[test]
    fn delayed_reveal_after_lock_drops_secret_and_reopen_uses_fresh_generation() {
        let (started, ready) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let c = controller(Controlled {
            fail_startup: false,
            startup: None,
            reveal: Some((started, wait)),
        });
        assert!(c.execute(Operation::Unlock(master())).is_ok());
        let old = c.clone();
        let old = thread::spawn(move || {
            old.execute(Operation::Reveal(
                "11111111-1111-4111-8111-111111111111".into(),
            ))
        });
        ready.recv_timeout(Duration::from_secs(2)).unwrap();
        let locked = c.clone();
        let locked = thread::spawn(move || locked.lock(LockReason::Manual));
        wait_epoch(&c, 2);
        release.send(()).unwrap();
        assert!(matches!(old.join().unwrap(), Err(AppError::Locked)));
        assert!(locked.join().unwrap().is_ok());
        assert!(matches!(c.execute(Operation::List), Err(AppError::Locked)));
        assert!(c.execute(Operation::Unlock(master())).is_ok());
        assert!(c.execute(Operation::List).is_ok());
    }
    #[test]
    fn native_idle_ticker_revokes_without_ui_and_only_explicit_activity_refreshes() {
        let c = controller(Controlled {
            fail_startup: false,
            startup: None,
            reveal: None,
        });
        assert!(c.execute(Operation::Unlock(master())).is_ok());
        let previous = c.authority.lock().unwrap().activity.unwrap();
        thread::sleep(Duration::from_millis(150));
        assert_eq!(c.authority.lock().unwrap().activity, Some(previous));
        c.authority.lock().unwrap().activity = Some(Instant::now() - Duration::from_secs(299));
        assert!(c.execute(Operation::Activity).is_ok());
        assert!(c.authority.lock().unwrap().activity.unwrap().elapsed() < Duration::from_secs(1));
        c.authority.lock().unwrap().activity = Some(Instant::now() - IDLE);
        thread::sleep(Duration::from_millis(200));
        assert!(!c.authority.lock().unwrap().unlocked);
        assert!(matches!(c.execute(Operation::List), Err(AppError::Locked)));
    }
    #[test]
    fn startup_completion_after_idle_expiry_cannot_activate_authority() {
        let (started, ready) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let c = controller(Controlled {
            fail_startup: false,
            startup: Some((started, wait)),
            reveal: None,
        });
        let old = c.clone();
        let old = thread::spawn(move || old.execute(Operation::Unlock(master())));
        ready.recv_timeout(Duration::from_secs(2)).unwrap();
        c.authority.lock().unwrap().activity = Some(Instant::now() - IDLE);
        wait_epoch(&c, 2);
        release.send(()).unwrap();
        assert!(matches!(old.join().unwrap(), Err(AppError::Locked)));
        assert!(matches!(c.execute(Operation::List), Err(AppError::Locked)));
        assert!(c.execute(Operation::Unlock(master())).is_ok());
    }
    #[test]
    fn native_output_conversion_failure_revokes_before_returning() {
        let c = controller(Controlled {
            fail_startup: false,
            startup: None,
            reveal: None,
        });
        assert!(c.execute(Operation::Unlock(master())).is_ok());
        assert_eq!(
            c.deliver(Operation::List, |_| Err::<(), _>(AppError::InvalidInput)),
            Err(AppError::InvalidInput)
        );
        assert!(matches!(c.execute(Operation::List), Err(AppError::Locked)));
    }
    #[test]
    fn completed_reply_lock_before_submission_is_dropped_and_fresh_reopen_submits() {
        let c = controller(Controlled {
            fail_startup: false,
            startup: None,
            reveal: None,
        });
        c.execute(Operation::Unlock(master())).unwrap();
        let pending = c
            .prepare(Operation::Reveal(
                "11111111-1111-4111-8111-111111111111".into(),
            ))
            .unwrap();
        c.lock(LockReason::Manual).unwrap();
        let mut submitted = 0;
        assert_eq!(
            pending.publish(|_| {
                submitted += 1;
                Ok(())
            }),
            Err(AppError::Locked)
        );
        assert_eq!(submitted, 0);
        c.execute(Operation::Unlock(master())).unwrap();
        c.prepare(Operation::List)
            .unwrap()
            .publish(|_| {
                submitted += 1;
                Ok(())
            })
            .unwrap();
        assert_eq!(submitted, 1);
    }

    #[test]
    fn lifecycle_barrier_returns_during_blocked_reveal_and_requires_fresh_unlock() {
        let (started, ready) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let c = controller(Controlled {
            fail_startup: false,
            startup: None,
            reveal: Some((started, wait)),
        });
        c.execute(Operation::Unlock(master())).unwrap();
        let old = c.clone();
        let old = thread::spawn(move || old.execute(Operation::Reveal(SELECTED.into())));
        ready.recv_timeout(Duration::from_secs(2)).unwrap();
        let original = c.signal().generation();
        let began = Instant::now();
        c.signal().notify(NativeVaultEvent::WindowClosed);
        assert!(began.elapsed() < Duration::from_millis(100));
        let blocked = c.signal().check(original);
        release.send(()).unwrap();
        let old_result = old.join().unwrap();
        assert_eq!(blocked, Err(AppError::Locked));
        assert!(matches!(old_result, Err(AppError::Locked)));
        assert!(matches!(c.execute(Operation::List), Err(AppError::Locked)));
        c.execute(Operation::Unlock(master())).unwrap();
        assert!(c.execute(Operation::List).is_ok());
    }
    #[test]
    fn lifecycle_after_serialization_prevents_actual_submission() {
        let c = controller(Controlled {
            fail_startup: false,
            startup: None,
            reveal: None,
        });
        c.execute(Operation::Unlock(master())).unwrap();
        let pending = c.prepare(Operation::Reveal(SELECTED.into())).unwrap();
        let mut submitted = false;
        assert_eq!(
            pending.publish_checked(|reply, check| {
                let Reply::Secret(secret) = reply else {
                    panic!("secret expected")
                };
                let _serialized = serde_json::to_vec(secret.expose()).unwrap();
                c.signal().notify(NativeVaultEvent::WindowClosed);
                check()?;
                submitted = true;
                Ok(())
            }),
            Err(AppError::Locked)
        );
        assert!(!submitted);
    }
    #[test]
    fn lifecycle_invalidates_blocked_startup_and_disabled_consumer_stays_closed() {
        let (started, ready) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let c = controller(Controlled {
            fail_startup: false,
            startup: Some((started, wait)),
            reveal: None,
        });
        let old = c.clone();
        let old = thread::spawn(move || old.execute(Operation::Unlock(master())));
        ready.recv_timeout(Duration::from_secs(2)).unwrap();
        c.signal().notify(NativeVaultEvent::WindowClosed);
        release.send(()).unwrap();
        assert!(matches!(old.join().unwrap(), Err(AppError::Locked)));
        assert!(matches!(c.execute(Operation::List), Err(AppError::Locked)));
        c.execute(Operation::Unlock(master())).unwrap();
        c.signal().fail();
        c.signal().notify(NativeVaultEvent::Revoked);
        assert!(matches!(
            c.execute(Operation::Unlock(master())),
            Err(AppError::Locked)
        ));
    }

    const SELECTED: &str = "11111111-1111-4111-8111-111111111111";
    struct CopyEngine;
    impl VaultEngine for CopyEngine {
        fn create(&mut self, _: SecretBytes) -> AppResult<()> {
            Ok(())
        }
        fn open(&mut self, _: SecretBytes) -> AppResult<()> {
            Ok(())
        }
        fn close(&mut self) -> AppResult<()> {
            Ok(())
        }
        fn list(&mut self) -> AppResult<Vec<VaultSummary>> {
            Ok([
                ("22222222-2222-4222-8222-222222222222", "other account"),
                (SELECTED, " selected\r\n账号 "),
            ]
            .into_iter()
            .map(|(id, account)| VaultSummary {
                entry_id: id.parse().unwrap(),
                channel: "c".into(),
                account: account.into(),
                revision: 1,
                created_at: 0,
                updated_at: 0,
            })
            .collect())
        }
        fn apply(&mut self, _: VaultMutation) -> AppResult<VaultSummary> {
            Err(AppError::Unsupported)
        }
        fn read_secret(&mut self, id: &str) -> AppResult<SecretBytes> {
            assert_eq!(id, SELECTED);
            Ok(SecretBytes::new(b"selected password".to_vec()))
        }
        fn change_master(&mut self, _: SecretBytes, _: SecretBytes) -> AppResult<()> {
            Ok(())
        }
    }
    struct CaptureClipboard {
        content: Arc<Mutex<Vec<u8>>>,
        fail: bool,
    }
    impl ClipboardPort for CaptureClipboard {
        fn write(&mut self, value: SecretBytes) -> AppResult<()> {
            if self.fail {
                return Err(AppError::Unsupported);
            }
            *self.content.lock().unwrap() = value.expose().to_vec();
            Ok(())
        }
    }
    fn copy_controller(fail: bool) -> (Arc<VaultController>, Arc<Mutex<Vec<u8>>>) {
        let content = Arc::new(Mutex::new(Vec::new()));
        (
            VaultController::new_with_port(
                Some(CopyEngine),
                Arc::new(Mutex::new(VaultCancellation::default())),
                CaptureClipboard {
                    content: content.clone(),
                    fail,
                },
            ),
            content,
        )
    }
    #[test]
    fn selected_copy_is_native_only_and_persists_across_lock_and_drop() {
        let (c, content) = copy_controller(false);
        c.execute(Operation::Unlock(master())).unwrap();
        assert!(matches!(
            c.execute(Operation::Copy(SELECTED.into(), CopyField::Account)),
            Ok(Reply::Unit)
        ));
        assert_eq!(*content.lock().unwrap(), " selected\r\n账号 ".as_bytes());
        assert!(matches!(
            c.execute(Operation::Copy(SELECTED.into(), CopyField::Password)),
            Ok(Reply::Unit)
        ));
        c.lock(LockReason::Manual).unwrap();
        drop(c);
        assert_eq!(*content.lock().unwrap(), b"selected password");
    }
    #[test]
    fn completed_copy_cannot_write_after_lock_or_into_reopened_session() {
        let (c, content) = copy_controller(false);
        c.execute(Operation::Unlock(master())).unwrap();
        let pending = c
            .prepare(Operation::Copy(SELECTED.into(), CopyField::Password))
            .unwrap();
        c.lock(LockReason::Manual).unwrap();
        c.execute(Operation::Unlock(master())).unwrap();
        assert!(matches!(pending.publish(Ok), Err(AppError::Locked)));
        assert!(content.lock().unwrap().is_empty());
        assert!(matches!(
            c.execute(Operation::Copy(SELECTED.into(), CopyField::Password)),
            Ok(Reply::Unit)
        ));
    }
    #[test]
    fn clipboard_error_never_submits_success_and_revokes_session() {
        let (c, content) = copy_controller(true);
        c.execute(Operation::Unlock(master())).unwrap();
        let mut submitted = false;
        assert_eq!(
            c.deliver(
                Operation::Copy(SELECTED.into(), CopyField::Password),
                |_| {
                    submitted = true;
                    Ok(())
                }
            ),
            Err(AppError::Unsupported)
        );
        assert!(!submitted);
        assert!(content.lock().unwrap().is_empty());
        assert!(matches!(c.execute(Operation::List), Err(AppError::Locked)));
    }
}
