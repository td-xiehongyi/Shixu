//! Trusted internal backend only: no Tauri command/path/program input.
use super::{
    commit,
    pipe::{PrivatePipe, VaultCancellation},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use shixu_core::{
    contracts::{
        AppResult,
        error::AppError,
        vault::{SecretBytes, VaultMutation, VaultSummary},
    },
    vault::ports::VaultEngine,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
};
use zeroize::Zeroizing;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResourceManifest {
    version: u32,
    node: String,
    kdbxweb: String,
    hash_wasm: String,
    archive: String,
    archive_sha256: String,
    files: BTreeMap<String, String>,
}
const MANIFEST: &str = include_str!("../../../../vault-helper/resources-linux-x64.json");
fn verify_resources(root: &Path) -> AppResult<()> {
    let manifest: ResourceManifest =
        serde_json::from_str(MANIFEST).map_err(|_| AppError::Unsupported)?;
    if manifest.version != 1
        || manifest.node != "26.11.1"
        || manifest.kdbxweb != "2.1.1"
        || manifest.hash_wasm != "4.12.0"
        || manifest.archive != "node-v26.11.1-linux-x64.tar.xz"
        || manifest.archive_sha256
            != "3883bfc73f9a680ca4eab04b196068aaaab1373ffa77d8fc1a4408222495b651"
    {
        return Err(AppError::Unsupported);
    }
    fn visit(dir: &Path, root: &Path, actual: &mut BTreeSet<String>) -> AppResult<()> {
        for item in fs::read_dir(dir).map_err(|_| AppError::Unsupported)? {
            let file = item.map_err(|_| AppError::Unsupported)?.path();
            let metadata = fs::symlink_metadata(&file).map_err(|_| AppError::Unsupported)?;
            if metadata.file_type().is_symlink() {
                return Err(AppError::Unsupported);
            }
            if metadata.is_dir() {
                visit(&file, root, actual)?;
            } else if metadata.is_file() {
                actual.insert(
                    file.strip_prefix(root)
                        .map_err(|_| AppError::Unsupported)?
                        .to_string_lossy()
                        .into_owned(),
                );
            } else {
                return Err(AppError::Unsupported);
            }
        }
        Ok(())
    }
    let mut actual = BTreeSet::new();
    visit(root, root, &mut actual)?;
    if actual != manifest.files.keys().cloned().collect() {
        return Err(AppError::Unsupported);
    }
    for (relative, expected) in manifest.files {
        let path = root.join(relative);
        let mut file = fs::File::open(path).map_err(|_| AppError::Unsupported)?;
        let mut hash = Sha256::new();
        let mut chunk = [0; 65536];
        let mut size = 0_u64;
        loop {
            let count = file.read(&mut chunk).map_err(|_| AppError::Unsupported)?;
            if count == 0 {
                break;
            }
            size += count as u64;
            if size > 256 * 1024 * 1024 {
                return Err(AppError::Unsupported);
            }
            hash.update(&chunk[..count]);
        }
        if hash
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
            != expected
        {
            return Err(AppError::Unsupported);
        }
    }
    Ok(())
}
#[derive(Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum RequestOp<'a> {
    Create {
        master: &'a str,
    },
    Open {
        master: &'a str,
    },
    List,
    CreateEntry {
        channel: &'a str,
        account: &'a str,
        password: &'a str,
    },
    Update {
        entry_id: &'a str,
        expected_revision: u64,
        channel: &'a str,
        account: &'a str,
        password: &'a str,
    },
    Delete {
        entry_id: &'a str,
        expected_revision: u64,
    },
    Reveal {
        entry_id: &'a str,
    },
    ChangeMaster {
        current: &'a str,
        next: &'a str,
    },
}
#[derive(Serialize)]
struct Request<'a> {
    v: u32,
    id: u64,
    #[serde(flatten)]
    op: RequestOp<'a>,
}
#[derive(Deserialize)]
#[serde(transparent)]
struct SecretWire(Vec<u8>);
impl SecretWire {
    fn into_secret(mut self) -> SecretBytes {
        SecretBytes::new(std::mem::take(&mut self.0))
    }
}
impl Drop for SecretWire {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.0.zeroize();
    }
}
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Response {
    Unit {
        v: u32,
        id: u64,
        staged_digest: Option<String>,
    },
    List {
        v: u32,
        id: u64,
        value: Vec<VaultSummary>,
    },
    Summary {
        v: u32,
        id: u64,
        value: VaultSummary,
        staged_digest: String,
    },
    Secret {
        v: u32,
        id: u64,
        value: SecretWire,
    },
    Error {
        v: u32,
        id: u64,
        value: AppError,
    },
}
impl Response {
    fn identity(&self) -> (u32, u64) {
        match self {
            Self::Unit { v, id, .. }
            | Self::List { v, id, .. }
            | Self::Summary { v, id, .. }
            | Self::Secret { v, id, .. }
            | Self::Error { v, id, .. } => (*v, *id),
        }
    }
}
pub struct KdbxWebEngine {
    resources: PathBuf,
    work: PathBuf,
    pipe: Option<PrivatePipe>,
    cancellation: VaultCancellation,
    expected: Option<String>,
    next_id: u64,
}
impl KdbxWebEngine {
    /// Inputs come from trusted native configuration, never WebView arguments.
    /// Windows refuses bootstrap until native isolation/file gates are verified.
    pub fn prepared(resources: PathBuf, work: PathBuf) -> AppResult<Self> {
        if !cfg!(target_os = "linux") {
            return Err(AppError::Unsupported);
        }
        let resources = commit::ensure_plain_directory(&resources)?;
        let work = commit::ensure_plain_directory(&work)?;
        if work.starts_with(&resources) || resources.starts_with(&work) {
            return Err(AppError::Unsupported);
        }
        verify_resources(&resources)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&work, fs::Permissions::from_mode(0o700))
                .map_err(|_| AppError::Unsupported)?;
        }
        Ok(Self {
            resources,
            work,
            pipe: None,
            cancellation: VaultCancellation::default(),
            expected: None,
            next_id: 0,
        })
    }
    /// Obtain before dispatching blocking operations; another native owner can
    /// immediately terminate the helper when locking/revoking a session.
    pub fn cancellation(&self) -> Option<VaultCancellation> {
        Some(self.cancellation.clone())
    }
    /// Arm only with a fresh handle admitted by the authoritative native epoch.
    /// Startup never resets a cancelled handle.
    pub fn arm(&mut self, cancellation: VaultCancellation) {
        self.cancellation = cancellation;
    }
    fn start(&mut self) -> AppResult<()> {
        self.close()?;
        self.cancellation.check()?;
        verify_resources(&self.resources)?;
        for name in ["vault.pending.kdbx", "vault.checkpoint.kdbx"] {
            let path = self.work.join(name);
            if path.try_exists().map_err(|_| AppError::AuthFailed)? {
                let meta = fs::symlink_metadata(&path).map_err(|_| AppError::AuthFailed)?;
                if !meta.is_file() || meta.file_type().is_symlink() {
                    return Err(AppError::Unsupported);
                }
                fs::remove_file(path).map_err(|_| AppError::AuthFailed)?;
            }
        }
        self.pipe = Some(PrivatePipe::spawn(
            &self.resources,
            &self.work,
            self.cancellation.clone(),
        )?);
        self.next_id = 0;
        Ok(())
    }
    fn request(&mut self, op: RequestOp<'_>) -> AppResult<Response> {
        if self.next_id >= 9_007_199_254_740_991 {
            return Err(AppError::Unsupported);
        }
        self.next_id += 1;
        let frame = Zeroizing::new(
            serde_json::to_vec(&Request {
                v: 1,
                id: self.next_id,
                op,
            })
            .map_err(|_| AppError::InvalidInput)?,
        );
        let bytes = self.pipe.as_mut().ok_or(AppError::Locked)?.send(frame)?;
        let response: Response =
            serde_json::from_slice(&bytes).map_err(|_| AppError::Unsupported)?;
        if response.identity() != (1, self.next_id) {
            return Err(AppError::Unsupported);
        }
        if let Response::Error { value, .. } = response {
            return Err(value);
        }
        Ok(response)
    }
    fn check(&self) -> AppResult<()> {
        if self.pipe.is_none() {
            return Err(AppError::Locked);
        }
        if Some(commit::digest(&self.work.join("vault.kdbx"))?) != self.expected {
            return Err(AppError::Conflict);
        }
        Ok(())
    }
    fn persist(&mut self, verified_digest: &str) -> AppResult<()> {
        self.expected = Some(commit::commit(
            &self.work,
            self.expected.as_deref(),
            verified_digest,
        )?);
        Ok(())
    }
    fn finish<T>(&mut self, result: AppResult<T>) -> AppResult<T> {
        if result.is_err() {
            let _ = self.close();
        }
        result
    }
}
fn text(secret: &SecretBytes) -> AppResult<&str> {
    let value = std::str::from_utf8(secret.expose()).map_err(|_| AppError::InvalidInput)?;
    validate(value, true)?;
    Ok(value)
}
fn validate(value: &str, no_newline: bool) -> AppResult<()> {
    if value.is_empty()
        || value.len() > 65536
        || (no_newline && value.contains(['\r', '\n', '\u{85}', '\u{2028}', '\u{2029}']))
    {
        Err(AppError::InvalidInput)
    } else {
        Ok(())
    }
}
fn unit(response: Response) -> AppResult<Option<String>> {
    match response {
        Response::Unit { staged_digest, .. } => Ok(staged_digest),
        _ => Err(AppError::Unsupported),
    }
}
impl VaultEngine for KdbxWebEngine {
    fn create(&mut self, master: SecretBytes) -> AppResult<()> {
        let result = (|| {
            let master = text(&master)?;
            self.start()?;
            if self
                .work
                .join("vault.kdbx")
                .try_exists()
                .map_err(|_| AppError::AuthFailed)?
            {
                return Err(AppError::Conflict);
            }
            let digest =
                unit(self.request(RequestOp::Create { master })?)?.ok_or(AppError::Unsupported)?;
            self.persist(&digest)
        })();
        self.finish(result)
    }
    fn open(&mut self, master: SecretBytes) -> AppResult<()> {
        let result = (|| {
            let master = text(&master)?;
            self.start()?;
            self.expected = Some(commit::digest(&self.work.join("vault.kdbx"))?);
            unit(self.request(RequestOp::Open { master })?)?;
            self.check()
        })();
        self.finish(result)
    }
    fn close(&mut self) -> AppResult<()> {
        self.pipe.take();
        self.expected = None;
        Ok(())
    }
    fn list(&mut self) -> AppResult<Vec<VaultSummary>> {
        let result = (|| {
            self.check()?;
            match self.request(RequestOp::List)? {
                Response::List { value, .. } => Ok(value),
                _ => Err(AppError::Unsupported),
            }
        })();
        self.finish(result)
    }
    fn apply(&mut self, mutation: VaultMutation) -> AppResult<VaultSummary> {
        let result = (|| {
            self.check()?;
            let response = match mutation {
                VaultMutation::Create {
                    channel,
                    account,
                    password,
                } => {
                    validate(&channel, false)?;
                    validate(&account, false)?;
                    self.request(RequestOp::CreateEntry {
                        channel: &channel,
                        account: &account,
                        password: text(&password)?,
                    })?
                }
                VaultMutation::Update {
                    id,
                    expected_revision,
                    channel,
                    account,
                    password,
                } => {
                    validate(&channel, false)?;
                    validate(&account, false)?;
                    self.request(RequestOp::Update {
                        entry_id: &id.to_string(),
                        expected_revision,
                        channel: &channel,
                        account: &account,
                        password: text(&password)?,
                    })?
                }
                VaultMutation::Delete {
                    id,
                    expected_revision,
                } => self.request(RequestOp::Delete {
                    entry_id: &id.to_string(),
                    expected_revision,
                })?,
            };
            let (value, digest) = match response {
                Response::Summary {
                    value,
                    staged_digest,
                    ..
                } => (value, staged_digest),
                _ => return Err(AppError::Unsupported),
            };
            self.persist(&digest)?;
            Ok(value)
        })();
        self.finish(result)
    }
    fn read_secret(&mut self, id: &str) -> AppResult<SecretBytes> {
        let result = (|| {
            self.check()?;
            let _: shixu_core::contracts::vault::EntryId = id.parse()?;
            match self.request(RequestOp::Reveal { entry_id: id })? {
                Response::Secret { value, .. } => {
                    if value.0.len() > 65536 {
                        return Err(AppError::Unsupported);
                    }
                    Ok(value.into_secret())
                }
                _ => Err(AppError::Unsupported),
            }
        })();
        self.finish(result)
    }
    fn change_master(&mut self, current: SecretBytes, next: SecretBytes) -> AppResult<()> {
        let result = (|| {
            self.check()?;
            let digest = unit(self.request(RequestOp::ChangeMaster {
                current: text(&current)?,
                next: text(&next)?,
            })?)?
            .ok_or(AppError::Unsupported)?;
            self.persist(&digest)
        })();
        self.finish(result)
    }
}
impl Drop for KdbxWebEngine {
    fn drop(&mut self) {
        let _ = self.close();
    }
}
