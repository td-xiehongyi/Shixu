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
pub(super) enum ChildOwner {
    Std(Child),
    #[cfg(windows)]
    Windows(super::windows::WindowsProcess),
}
impl ChildOwner {
    fn kill(&mut self) -> std::io::Result<()> {
        match self {
            Self::Std(child) => child.kill(),
            #[cfg(windows)]
            Self::Windows(child) => child.kill(),
        }
    }
    fn wait(&mut self) -> std::io::Result<()> {
        match self {
            Self::Std(child) => child.wait().map(|_| ()),
            #[cfg(windows)]
            Self::Windows(child) => child.wait(),
        }
    }
    #[cfg(all(test, target_os = "linux"))]
    fn id(&self) -> u32 {
        match self {
            Self::Std(child) => child.id(),
        }
    }
}
/// Native-only cancellation capability. No process ID or arbitrary command API.
#[derive(Clone, Default)]
pub struct VaultCancellation(Arc<Mutex<CancellationState>>);
#[derive(Default)]
struct CancellationState {
    cancelled: bool,
    child: Option<Arc<Mutex<ChildOwner>>>,
}
impl VaultCancellation {
    pub fn cancel(&self) {
        if let Ok(mut state) = self.0.lock() {
            state.cancelled = true;
            if let Some(child) = &state.child
                && let Ok(mut child) = child.lock()
            {
                let _ = child.kill();
            }
        }
    }
    pub fn check(&self) -> AppResult<()> {
        if self.0.lock().map_err(|_| AppError::Locked)?.cancelled {
            Err(AppError::Locked)
        } else {
            Ok(())
        }
    }
    fn register(&self, child: Arc<Mutex<ChildOwner>>) -> AppResult<()> {
        let mut state = self.0.lock().map_err(|_| AppError::Locked)?;
        if state.cancelled {
            if let Ok(mut child) = child.lock() {
                let _ = child.kill();
                let _ = child.wait();
            }
            return Err(AppError::Locked);
        }
        state.child = Some(child);
        Ok(())
    }
}
pub(super) struct PrivatePipe {
    child: Arc<Mutex<ChildOwner>>,
    requests: Option<SyncSender<Zeroizing<Vec<u8>>>>,
    replies: Receiver<AppResult<Zeroizing<Vec<u8>>>>,
    worker: Option<JoinHandle<()>>,
}
impl PrivatePipe {
    pub fn spawn(
        resources: &Path,
        work: &Path,
        cancellation: VaultCancellation,
    ) -> AppResult<Self> {
        cancellation.check()?;
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
        let input = child.stdin.take().ok_or(AppError::Unsupported)?;
        let output = child.stdout.take().ok_or(AppError::Unsupported)?;
        let child = Arc::new(Mutex::new(ChildOwner::Std(child)));
        cancellation.register(child.clone())?;
        Self::from_io(child, Box::new(input), Box::new(output))
    }
    #[cfg(windows)]
    pub(super) fn spawn_windows(
        store: &super::windows::Store,
        cancellation: VaultCancellation,
    ) -> AppResult<Self> {
        cancellation.check()?;
        let suspended =
            super::windows::launch(&store.resources, &store.inbox, store.profile.clone())?;
        Self::register_windows(suspended, cancellation)
    }
    #[cfg(windows)]
    pub(super) fn register_windows(
        suspended: super::windows::Suspended,
        cancellation: VaultCancellation,
    ) -> AppResult<Self> {
        let super::windows::Suspended {
            process,
            thread,
            input,
            output,
        } = suspended;
        let child = Arc::new(Mutex::new(ChildOwner::Windows(process)));
        // Cancellation and resume are serialized by the same authority mutex.
        let mut state = cancellation.0.lock().map_err(|_| AppError::Locked)?;
        if state.cancelled {
            child
                .lock()
                .map_err(|_| AppError::Locked)?
                .kill()
                .map_err(|_| AppError::Unsupported)?;
            return Err(AppError::Locked);
        }
        state.child = Some(child.clone());
        if let Err(error) = super::windows::Suspended::resume_thread(&thread) {
            if let Ok(mut owner) = child.lock() {
                let _ = owner.kill();
                let _ = owner.wait();
            }
            state.child = None;
            return Err(error);
        }
        drop(state);
        drop(thread);
        Self::from_io(child, Box::new(input), Box::new(output))
    }
    pub(super) fn from_io(
        child: Arc<Mutex<ChildOwner>>,
        mut input: Box<dyn Write + Send>,
        mut output: Box<dyn Read + Send>,
    ) -> AppResult<Self> {
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
            #[cfg(not(windows))]
            let _ = worker.join();
            #[cfg(windows)]
            super::windows::finish_worker(worker);
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    #[test]
    fn cancelled_generation_kills_child_registered_after_cancel() {
        let cancel = VaultCancellation::default();
        cancel.cancel();
        let child = Command::new("/bin/cat")
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        let pid = child.id();
        assert!(matches!(
            cancel.register(Arc::new(Mutex::new(ChildOwner::Std(child)))),
            Err(AppError::Locked)
        ));
        assert!(!Path::new(&format!("/proc/{pid}")).exists());
    }
    #[test]
    fn registered_real_helper_is_cancelled_during_operation() {
        let repo = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap();
        let work = repo
            .join(".superpowers/sdd/shixu-v0.1")
            .join(format!("task-kdbxweb-ui-pipe-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&work).unwrap();
        let cancel = VaultCancellation::default();
        let mut pipe = PrivatePipe::spawn(
            &repo.join("resources/vault-linux-x64"),
            &work,
            cancel.clone(),
        )
        .unwrap();
        let pid = pipe.child.lock().unwrap().id();
        let stop = cancel.clone();
        let killer = thread::spawn(move || {
            thread::sleep(Duration::from_millis(100));
            stop.cancel();
        });
        assert!(
            pipe.send(Zeroizing::new(
                b"{\"v\":1,\"id\":1,\"op\":\"create\",\"master\":\"synthetic master\"}".to_vec()
            ))
            .is_err()
        );
        killer.join().unwrap();
        drop(pipe);
        assert!(!Path::new(&format!("/proc/{pid}")).exists());
        std::fs::remove_dir_all(work).unwrap();
    }
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
        let mut pipe = PrivatePipe::spawn(
            &repo.join("resources/vault-linux-x64"),
            &work,
            VaultCancellation::default(),
        )
        .unwrap();
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
