use shixu_core::{
    contracts::{AppResult, error::AppError, notification::*},
    notifications::limits::Resource,
    storage::DataProtector,
};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::Arc,
};
use zeroize::Zeroizing;
/// Transient endpoint, deliberately no Debug/Serialize or URL persistence. HTTPS
/// on port 443 only. The path can carry ephemeral authorization and is never logged.
pub struct Endpoint {
    pub host: String,
    pub path: String,
}
pub struct Grant {
    pub endpoint: Endpoint,
    pub expires_at: i64,
    pub thumbnail_only: bool,
}
pub enum Response {
    Body(Box<dyn Read>),
    Redirect(Endpoint),
}
/// Trusted adapter boundary, currently synthetic implementations only. Resolve
/// must authenticate the message/part/source reference tuple. get must use HTTPS
/// with valid TLS, port 443, no automatic redirects, no ambient cookies/proxy,
/// bounded connect/read deadlines, and validate every resolved peer address
/// against the source's approved egress policy (including DNS rebinding).
/// No production HTTP implementation or QQ download capability is implied here.
pub trait DownloadPort {
    fn resolve(&mut self, reference: &AttachmentRef) -> Result<Grant, PartReason>;
    fn get(&mut self, endpoint: &Endpoint) -> Result<Response, PartReason>;
}
pub trait Clock {
    fn now(&self) -> i64;
}
/// Only ciphertext is written. Plaintext lives in zeroizing memory through
/// validation/protection. Product DPAPI is injected; there is no encryption default.
/// Unix owner-only cache supports synthetic tests. Windows ACL integration is
/// blocked, so construction is Unsupported there pending native verification.
pub struct ProtectedCache {
    root: PathBuf,
    protector: Arc<dyn DataProtector>,
    _lock: File,
    coordinator: Arc<shixu_core::storage::coordinator::WriteCoordinator>,
}
impl ProtectedCache {
    /// Runtime cache creation, staging cleanup and publication share the DB barrier.
    pub fn create_coordinated(
        root: &Path,
        protector: Arc<dyn DataProtector>,
        db: &shixu_core::storage::Database,
    ) -> AppResult<Self> {
        Self::create_with_coordinator(root, protector, db.coordinator())
    }
    /// Standalone cache for portable tests/tools; cannot participate in an app DB pause.
    /// The runtime must use create_coordinated. Windows remains Unsupported.
    pub fn create(root: &Path, protector: Arc<dyn DataProtector>) -> AppResult<Self> {
        Self::create_with_coordinator(root, protector, Arc::default())
    }
    fn create_with_coordinator(
        root: &Path,
        protector: Arc<dyn DataProtector>,
        coordinator: Arc<shixu_core::storage::coordinator::WriteCoordinator>,
    ) -> AppResult<Self> {
        let _permit = coordinator.enter()?;
        #[cfg(not(unix))]
        {
            let _ = (root, protector);
            Err(AppError::Unsupported)
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
            if !root.is_absolute()
                || root.parent().is_none()
                || root
                    .components()
                    .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
            {
                return Err(AppError::InvalidInput);
            }
            // Existing ancestors must be ordinary directories; never traverse a symlink.
            for ancestor in root.ancestors().skip(1) {
                let m = std::fs::symlink_metadata(ancestor).map_err(io_error)?;
                if !m.is_dir() || m.file_type().is_symlink() {
                    return Err(AppError::InvalidInput);
                }
            }
            match std::fs::DirBuilder::new().mode(0o700).create(root) {
                Ok(()) => (),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
                Err(e) => return Err(io_error(e)),
            }
            let m = std::fs::symlink_metadata(root).map_err(io_error)?;
            if !m.is_dir() || m.file_type().is_symlink() || m.permissions().mode() & 0o077 != 0 {
                return Err(AppError::InvalidInput);
            }
            let lock_path = root.join(".owner");
            let lock = match OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&lock_path)
            {
                Ok(file) => file,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    let m = std::fs::symlink_metadata(&lock_path).map_err(io_error)?;
                    if !m.is_file()
                        || m.file_type().is_symlink()
                        || m.permissions().mode() & 0o077 != 0
                    {
                        return Err(AppError::InvalidInput);
                    }
                    OpenOptions::new()
                        .read(true)
                        .write(true)
                        .open(&lock_path)
                        .map_err(io_error)?
                }
                Err(e) => return Err(io_error(e)),
            };
            lock.try_lock().map_err(|_| AppError::Conflict)?;
            // Only unpublished staging names are reclaimable. A final blob may
            // already be linked by the message store; never remove it here.
            for entry in std::fs::read_dir(root).map_err(io_error)? {
                let entry = entry.map_err(io_error)?;
                let pending = entry
                    .file_name()
                    .to_str()
                    .and_then(|n| n.strip_prefix(".pending-"))
                    .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok());
                if pending {
                    let m = std::fs::symlink_metadata(entry.path()).map_err(io_error)?;
                    if !m.is_file() || m.file_type().is_symlink() {
                        return Err(AppError::InvalidInput);
                    }
                    std::fs::remove_file(entry.path()).map_err(io_error)?;
                }
            }
            File::open(root)
                .and_then(|f| f.sync_all())
                .map_err(io_error)?;
            Ok(Self {
                root: root.to_owned(),
                protector,
                _lock: lock,
                coordinator,
            })
        }
    }
    fn store(
        &mut self,
        reference: &AttachmentRef,
        plain: &[u8],
        limits: &ParserLimits,
    ) -> Result<PathBuf, PartReason> {
        let _permit = self
            .coordinator
            .enter()
            .map_err(|_| PartReason::PermissionDenied)?;
        let sealed = Zeroizing::new(
            self.protector
                .protect(plain)
                .map_err(|_| PartReason::AuthRequired)?,
        );
        // Conservative accounting is safe only for non-compressing protection.
        if sealed.len() < plain.len() {
            return Err(PartReason::RecognitionFailed);
        }
        // Bind identity to the authenticated native reference tuple and actual
        // content, never an ephemeral endpoint or clear native reference name.
        use sha2::{Digest, Sha256};
        let binding = Zeroizing::new(
            serde_json::to_vec(&(
                reference.message_key,
                reference.part_id,
                &reference.source_file_ref,
            ))
            .map_err(|_| PartReason::RecognitionFailed)?,
        );
        let mut digest = Sha256::new();
        digest.update((binding.len() as u64).to_be_bytes());
        digest.update(&binding);
        digest.update(plain);
        let identity = digest
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let path = self.root.join(format!(
            "{}.{}.{identity}.blob",
            reference.message_key, reference.part_id
        ));
        match std::fs::symlink_metadata(&path) {
            Ok(m) => {
                if !m.is_file() || m.file_type().is_symlink() || m.len() != sealed.len() as u64 {
                    return Err(PartReason::RecognitionFailed);
                }
                let mut existing = Zeroizing::new(Vec::new());
                File::open(&path)
                    .and_then(|f| f.take(sealed.len() as u64 + 1).read_to_end(&mut existing))
                    .map_err(|_| PartReason::PermissionDenied)?;
                if existing.len() != sealed.len() {
                    return Err(PartReason::RecognitionFailed);
                }
                let decoded = Zeroizing::new(
                    self.protector
                        .unprotect(&existing)
                        .map_err(|_| PartReason::RecognitionFailed)?,
                );
                if decoded.as_slice() != plain {
                    return Err(PartReason::RecognitionFailed);
                }
                // Idempotent reuse allocates neither bytes nor another part.
                return Ok(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(_) => return Err(PartReason::PermissionDenied),
        }
        let prefix = format!("{}.", reference.message_key);
        let mut message_sizes = std::collections::HashMap::<String, u64>::new();
        let mut used = 0u64;
        for entry in std::fs::read_dir(&self.root).map_err(|_| PartReason::PermissionDenied)? {
            let entry = entry.map_err(|_| PartReason::PermissionDenied)?;
            if entry.file_name() == ".owner" {
                continue;
            }
            let m = std::fs::symlink_metadata(entry.path())
                .map_err(|_| PartReason::PermissionDenied)?;
            if !m.is_file() || m.file_type().is_symlink() {
                return Err(PartReason::PermissionDenied);
            }
            if entry
                .file_name()
                .to_str()
                .is_some_and(|n| n.starts_with(&prefix))
            {
                let name = entry.file_name();
                let pid = name
                    .to_str()
                    .and_then(|n| n.strip_prefix(&prefix))
                    .and_then(|n| n.split('.').next())
                    .filter(|id| uuid::Uuid::parse_str(id).is_ok())
                    .ok_or(PartReason::PermissionDenied)?;
                // Retained versions use global cache space, but represent one
                // logical part. The maximum is conservative for any revision.
                message_sizes
                    .entry(pid.to_owned())
                    .and_modify(|size| *size = (*size).max(m.len()))
                    .or_insert(m.len());
            }
            used = used.checked_add(m.len()).ok_or(PartReason::StorageFull)?;
        }
        limits
            .check(
                Resource::CacheBytes,
                used.checked_add(sealed.len() as u64)
                    .ok_or(PartReason::StorageFull)?,
            )
            .map_err(|_| PartReason::StorageFull)?;
        message_sizes
            .entry(reference.part_id.to_string())
            .and_modify(|size| *size = (*size).max(sealed.len() as u64))
            .or_insert(sealed.len() as u64);
        limits.check_message(&message_sizes.values().copied().collect::<Vec<_>>())?;
        let pending = self.root.join(format!(".pending-{}", uuid::Uuid::new_v4()));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&pending)
            .map_err(|_| PartReason::PermissionDenied)?;
        let published = file
            .write_all(&sealed)
            .and_then(|_| file.sync_all())
            .and_then(|_| std::fs::hard_link(&pending, &path))
            .and_then(|_| File::open(&self.root)?.sync_all());
        drop(file);
        // Final files are immutable. A crash before unlink is safe: startup
        // removes only the extra pending name, preserving the published link.
        let cleanup =
            std::fs::remove_file(&pending).and_then(|_| File::open(&self.root)?.sync_all());
        if let Err(e) = published.and(cleanup) {
            return Err(if e.kind() == std::io::ErrorKind::StorageFull {
                PartReason::StorageFull
            } else {
                PartReason::PermissionDenied
            });
        }
        Ok(path)
    }
}
fn io_error(e: std::io::Error) -> AppError {
    if e.kind() == std::io::ErrorKind::StorageFull {
        AppError::StorageFull
    } else {
        AppError::Disconnected
    }
}
pub struct DownloadService<T: DownloadPort, C: Clock> {
    pub transport: T,
    clock: C,
    cache: ProtectedCache,
    hosts: Vec<String>,
}
impl<T: DownloadPort, C: Clock> DownloadService<T, C> {
    pub fn new(transport: T, clock: C, cache: ProtectedCache, hosts: Vec<String>) -> Self {
        Self {
            transport,
            clock,
            cache,
            hosts,
        }
    }
    fn approve(&self, e: &Endpoint) -> Result<(), PartReason> {
        // Exact lower-case DNS hostname allowlist, not substring/suffix matching.
        if !self.hosts.contains(&e.host)
            || !e.host.contains('.')
            || e.host.len() > 253
            || e.host.parse::<std::net::IpAddr>().is_ok()
            || e.host.split('.').any(|label| {
                label.is_empty()
                    || label.len() > 63
                    || label.starts_with('-')
                    || label.ends_with('-')
                    || !label
                        .bytes()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
            })
            || !e.path.starts_with('/')
            || e.path.starts_with("//")
            || e.path.contains('\\')
            || e.path.chars().any(char::is_control)
        {
            return Err(PartReason::PermissionDenied);
        }
        Ok(())
    }
    pub fn fetch_detailed(
        &mut self,
        reference: &AttachmentRef,
        limits: &ParserLimits,
    ) -> Result<PathBuf, PartReason> {
        if reference.source_file_ref.is_none() {
            return Err(PartReason::DownloadUnavailable);
        }
        let grant = self.transport.resolve(reference)?;
        if self.clock.now() >= grant.expires_at {
            return Err(PartReason::Expired);
        }
        if grant.thumbnail_only {
            return Err(PartReason::PartialSource);
        }
        let mut endpoint = grant.endpoint;
        for redirects in 0..=3 {
            self.approve(&endpoint)?;
            if self.clock.now() >= grant.expires_at {
                return Err(PartReason::Expired);
            }
            match self.transport.get(&endpoint)? {
                Response::Redirect(next) => {
                    if redirects == 3 {
                        return Err(PartReason::PermissionDenied);
                    }
                    self.approve(&next)?;
                    endpoint = next;
                }
                Response::Body(mut stream) => {
                    let cap = limits
                        .max_file_bytes
                        .min(ParserLimits::v01().max_file_bytes);
                    let mut plain = Zeroizing::new(Vec::new());
                    let mut buf = Zeroizing::new([0u8; 8192]);
                    loop {
                        let remaining = cap.saturating_sub(plain.len() as u64);
                        let read = buf.len().min(remaining.saturating_add(1) as usize);
                        let n = stream
                            .read(&mut buf[..read])
                            .map_err(|_| PartReason::DownloadUnavailable)?;
                        if n == 0 {
                            break;
                        }
                        limits.check(Resource::FileBytes, plain.len() as u64 + n as u64)?;
                        plain.extend_from_slice(&buf[..n]);
                    }
                    // Header preflight only; full decoding and ZIP expansion belong exclusively
                    // to the isolated worker. ZIP acquisition stays unsupported until that gate.
                    if plain.starts_with(b"%PDF-") {
                    } else if plain.starts_with(b"\x89PNG\r\n\x1a\n") {
                        if plain.len() < 33 || plain[8..16] != [0, 0, 0, 13, b'I', b'H', b'D', b'R']
                        {
                            return Err(PartReason::RecognitionFailed);
                        }
                        let width = u32::from_be_bytes(
                            plain[16..20]
                                .try_into()
                                .map_err(|_| PartReason::RecognitionFailed)?,
                        );
                        let height = u32::from_be_bytes(
                            plain[20..24]
                                .try_into()
                                .map_err(|_| PartReason::RecognitionFailed)?,
                        );
                        limits.check_image(plain.len() as u64, width, height)?;
                    } else {
                        return Err(PartReason::FormatUnsupported);
                    }
                    if let Some(expected) = &reference.content_hash {
                        use sha2::{Digest, Sha256};
                        if Sha256::digest(&plain)
                            .iter()
                            .map(|b| format!("{b:02x}"))
                            .collect::<String>()
                            != *expected
                        {
                            return Err(PartReason::RecognitionFailed);
                        }
                    }
                    return self.cache.store(reference, &plain, limits);
                }
            }
        }
        Err(PartReason::PermissionDenied)
    }
    pub fn fetch_attachment(
        &mut self,
        reference: &AttachmentRef,
        limits: &ParserLimits,
    ) -> AppResult<PathBuf> {
        self.fetch_detailed(reference, limits).map_err(|r| match r {
            PartReason::AuthRequired => AppError::AuthFailed,
            PartReason::StorageFull => AppError::StorageFull,
            PartReason::FormatUnsupported => AppError::Unsupported,
            _ => AppError::ParseFailed,
        })
    }
}
/// Convert acquisition failures to durable visible per-part outcomes.
pub fn download_failure(part_id: PartId, reason: PartReason) -> PartResult {
    PartResult {
        part_id,
        status: match reason {
            PartReason::LimitExceeded => PartStatus::LimitExceeded,
            PartReason::FormatUnsupported => PartStatus::Unsupported,
            PartReason::PartialSource => PartStatus::PartialParse,
            _ => PartStatus::DownloadFailed,
        },
        blocks: vec![],
        reason_code: Some(reason),
    }
}
