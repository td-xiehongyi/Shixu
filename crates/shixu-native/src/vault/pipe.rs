//! Private bounded inherited pipes, one in-flight request, fixed timeout.
use shixu_core::contracts::{AppResult, error::AppError};
use std::{
    io::{Read, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, SyncSender},
    },
    thread::{self, JoinHandle},
    time::Duration,
};
use zeroize::Zeroizing;
const MAX_FRAME: usize = 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(30);
/// Native-only cancellation capability. No process ID or arbitrary command API.
#[derive(Clone)]
pub struct VaultCancellation(Arc<Mutex<Child>>);
impl VaultCancellation {
    pub fn cancel(&self) {
        if let Ok(mut child) = self.0.lock() {
            let _ = child.kill();
        }
    }
}
pub(super) struct PrivatePipe {
    child: Arc<Mutex<Child>>,
    requests: Option<SyncSender<Zeroizing<Vec<u8>>>>,
    replies: Receiver<AppResult<Zeroizing<Vec<u8>>>>,
    worker: Option<JoinHandle<()>>,
}
impl PrivatePipe {
    pub fn spawn(resources: &Path, work: &Path) -> AppResult<Self> {
        let mut command = Command::new(resources.join("runtime/node"));
        command
            .env_clear()
            .current_dir(work)
            .args([
                "--permission",
                "--no-addons",
                "--no-warnings",
                "--max-old-space-size=128",
            ])
            .arg(format!(
                "--allow-fs-read={}",
                resources.join("helper").display()
            ))
            .arg(format!("--allow-fs-read={}", work.display()))
            .arg(format!("--allow-fs-write={}", work.display()))
            .arg(resources.join("helper/helper.mjs"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = command.spawn().map_err(|_| AppError::Unsupported)?;
        let mut input = child.stdin.take().ok_or(AppError::Unsupported)?;
        let mut output = child.stdout.take().ok_or(AppError::Unsupported)?;
        let child = Arc::new(Mutex::new(child));
        let (tx, rx) = mpsc::sync_channel::<Zeroizing<Vec<u8>>>(1);
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        let worker = thread::spawn(move || {
            while let Ok(frame) = rx.recv() {
                let result = (|| {
                    input
                        .write_all(&(frame.len() as u32).to_be_bytes())
                        .map_err(|_| AppError::AuthFailed)?;
                    input.write_all(&frame).map_err(|_| AppError::AuthFailed)?;
                    input.flush().map_err(|_| AppError::AuthFailed)?;
                    let mut size = [0; 4];
                    output
                        .read_exact(&mut size)
                        .map_err(|_| AppError::AuthFailed)?;
                    let size = u32::from_be_bytes(size) as usize;
                    if size == 0 || size > MAX_FRAME {
                        return Err(AppError::Unsupported);
                    }
                    let mut bytes = Zeroizing::new(vec![0; size]);
                    output
                        .read_exact(&mut bytes)
                        .map_err(|_| AppError::AuthFailed)?;
                    Ok(bytes)
                })();
                let failed = result.is_err();
                if reply_tx.send(result).is_err() || failed {
                    break;
                }
            }
        });
        Ok(Self {
            child,
            requests: Some(tx),
            replies: reply_rx,
            worker: Some(worker),
        })
    }
    pub fn cancellation(&self) -> VaultCancellation {
        VaultCancellation(self.child.clone())
    }
    pub fn send(&mut self, frame: Zeroizing<Vec<u8>>) -> AppResult<Zeroizing<Vec<u8>>> {
        if frame.is_empty() || frame.len() > MAX_FRAME {
            return Err(AppError::InvalidInput);
        }
        self.requests
            .as_ref()
            .ok_or(AppError::Locked)?
            .try_send(frame)
            .map_err(|_| AppError::AuthFailed)?;
        self.replies
            .recv_timeout(TIMEOUT)
            .map_err(|_| AppError::AuthFailed)?
    }
}
impl Drop for PrivatePipe {
    fn drop(&mut self) {
        // Terminating helper closes both pipe ends, unblocks IO, then joins.
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.requests.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    #[test]
    fn actual_stopped_helper_timeout_kills_and_joins_without_unbounded_queue() {
        let repo = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap();
        let work = repo.join(".superpowers/sdd/shixu-v0.1").join(format!(
            "task-kdbxweb-engine-timeout-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir(&work).unwrap();
        let mut pipe = PrivatePipe::spawn(&repo.join("resources/vault-linux-x64"), &work).unwrap();
        assert!(matches!(
            pipe.send(Zeroizing::new(vec![0; MAX_FRAME + 1])),
            Err(AppError::InvalidInput)
        ));
        let pid = pipe.child.lock().unwrap().id();
        assert!(
            Command::new("/bin/kill")
                .args(["-STOP", &pid.to_string()])
                .status()
                .unwrap()
                .success()
        );
        let now = std::time::Instant::now();
        assert!(matches!(
            pipe.send(Zeroizing::new(
                b"{\"v\":1,\"id\":1,\"op\":\"list\"}".to_vec()
            )),
            Err(AppError::AuthFailed)
        ));
        drop(pipe);
        assert!(now.elapsed() >= TIMEOUT);
        assert!(now.elapsed() < Duration::from_secs(35));
        assert!(!std::path::Path::new(&format!("/proc/{pid}")).exists());
        std::fs::remove_dir_all(work).unwrap();
    }
}
