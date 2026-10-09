//! Independent bounded worker lanes. The receive port retains its delivery until
//! acknowledge succeeds; it must not prefetch an unbounded queue. Every port call
//! must finish within its documented finite deadline and must not re-enter stop.
use super::{Supervisor, supervisor::AttachmentParser};
use crate::{
    contracts::{AppResult, error::AppError, notification::MessageEnvelope},
    notifications::{AppendOutcome, model::ModelTransport},
};
use std::sync::{Arc, Condvar, Mutex, atomic::Ordering};
use std::thread::JoinHandle;
use std::time::Duration;
pub struct Delivery {
    pub message: MessageEnvelope,
    pub cursor: String,
}
pub trait ReceivePort: Send {
    fn poll(&mut self) -> AppResult<Option<Delivery>>;
    fn acknowledge(&mut self, cursor: &str) -> AppResult<()>;
    fn disconnect(&mut self) -> AppResult<()>;
}
#[derive(Default)]
pub struct WorkerPorts {
    pub receiver: Option<Box<dyn ReceivePort>>,
    pub parser: Option<Arc<dyn AttachmentParser>>,
    pub model: Option<Arc<dyn ModelTransport + Send + Sync>>,
}
struct Wake {
    cancelled: Mutex<bool>,
    wake: Condvar,
}
pub struct BackgroundWorkers {
    supervisor: Arc<Supervisor>,
    wake: Arc<Wake>,
    threads: Vec<JoinHandle<()>>,
}
fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}
fn spawn(
    wake: Arc<Wake>,
    supervisor: Arc<Supervisor>,
    mut step: impl FnMut() -> AppResult<()> + Send + 'static,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        loop {
            if *wake.cancelled.lock().unwrap_or_else(|p| p.into_inner()) {
                break;
            }
            if supervisor.is_running()
                && let Err(error) = step()
            {
                supervisor.worker_failed(error);
            }
            let cancelled = wake.cancelled.lock().unwrap_or_else(|p| p.into_inner());
            if *cancelled {
                break;
            }
            let _ = wake.wake.wait_timeout(cancelled, Duration::from_millis(50));
        }
    })
}
impl Supervisor {
    pub fn spawn_workers(self: &Arc<Self>, ports: WorkerPorts) -> AppResult<BackgroundWorkers> {
        if !self.is_running() {
            return Err(AppError::Disconnected);
        }
        if self
            .workers_owned
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(AppError::Conflict);
        }
        let wake = Arc::new(Wake {
            cancelled: Mutex::new(false),
            wake: Condvar::new(),
        });
        let mut threads = vec![];
        let s = self.clone();
        threads.push(spawn(wake.clone(), self.clone(), move || {
            s.process_pending(now()).map(|_| ())
        }));
        if let Some(parser) = ports.parser {
            let s = self.clone();
            threads.push(spawn(wake.clone(), self.clone(), move || {
                s.process_attachment(now(), parser.as_ref()).map(|_| ())
            }));
        }
        if let Some(model) = ports.model {
            let s = self.clone();
            threads.push(spawn(wake.clone(), self.clone(), move || {
                s.process_model(now(), model.as_ref()).map(|_| ())
            }));
        }
        if let Some(mut receiver) = ports.receiver {
            let s = self.clone();
            let signal = wake.clone();
            threads.push(std::thread::spawn(move || {
                while !*signal.cancelled.lock().unwrap_or_else(|p| p.into_inner()) {
                    let result = (|| {
                        let _transport =
                            s.transport_io.lock().map_err(|_| AppError::Disconnected)?;
                        if !s.is_running() {
                            return Ok(());
                        }
                        if let Some(delivery) = receiver.poll()? {
                            let id = delivery.message.source_id;
                            let at = delivery.message.received_at;
                            match s.receive(delivery.message, &delivery.cursor)? {
                                AppendOutcome::Stored | AppendOutcome::Duplicate => {
                                    receiver.acknowledge(&delivery.cursor)?;
                                    s.received_online(id, at);
                                }
                                _ => return Err(AppError::Conflict),
                            }
                        }
                        Ok(())
                    })();
                    if let Err(error) = result {
                        s.worker_failed(error);
                    }
                    let cancelled = signal.cancelled.lock().unwrap_or_else(|p| p.into_inner());
                    if *cancelled {
                        break;
                    }
                    let _ = signal
                        .wake
                        .wait_timeout(cancelled, Duration::from_millis(50));
                }
                if let Err(e) = receiver.disconnect() {
                    s.worker_failed(e);
                }
            }));
        }
        Ok(BackgroundWorkers {
            supervisor: self.clone(),
            wake,
            threads,
        })
    }
}
impl BackgroundWorkers {
    pub fn stop(mut self) -> AppResult<()> {
        self.shutdown()
    }
    fn shutdown(&mut self) -> AppResult<()> {
        *self
            .wake
            .cancelled
            .lock()
            .map_err(|_| AppError::Disconnected)? = true;
        self.wake.wake.notify_all();
        let mut result = Ok(());
        for thread in self.threads.drain(..) {
            if thread.join().is_err() {
                result = Err(AppError::Disconnected);
            }
        }
        let stopped = self.supervisor.stop();
        self.supervisor.workers_owned.store(false, Ordering::SeqCst);
        result.and(stopped)
    }
}
impl Drop for BackgroundWorkers {
    fn drop(&mut self) {
        if !self.threads.is_empty() {
            let _ = self.shutdown();
        }
    }
}
