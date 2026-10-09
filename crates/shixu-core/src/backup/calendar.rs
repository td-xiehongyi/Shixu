//! Local snapshots contain protected SQLite payloads, never source credentials.
//! The SQLite backup API reads committed WAL and atomically replaces the SAME live
//! connection. No live database file rename or plaintext export scratch file exists.
use super::manifest::DiskManifest;
use crate::{
    contracts::{AppResult, SchemaVersion, backup::*, error::AppError, notification::*},
    notifications::consent::{ConsentStore, ModelConsent},
    storage::{Database, coordinator::PauseGuard, database::storage_error},
};
use rusqlite::{Connection, OpenFlags};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
};
const DAY: i64 = 86_400_000;
pub(crate) const MAX_DB: u64 = 256 * 1024 * 1024;
/// Implementations must use the same DB coordinator for publication and cleanup,
/// return immutable protected bytes for the exact part, and never overwrite blobs.
pub trait BackupBlobs: Send + Sync {
    fn coordinator(&self) -> Arc<crate::storage::coordinator::WriteCoordinator>;
    fn read(&self, part: &MessagePart, pause: &PauseGuard) -> AppResult<Option<Vec<u8>>>;
    fn publish(&self, part: &MessagePart, sealed: &[u8], pause: &PauseGuard) -> AppResult<()>;
}
struct Prepared {
    id: RestorePreviewId,
    path: PathBuf,
    digest: String,
    expires: i64,
}
pub struct CalendarBackup {
    _owner: std::fs::File,
    pub(crate) db: Arc<Database>,
    consent: Arc<ConsentStore>,
    pub(crate) root: PathBuf,
    blobs: Option<Arc<dyn BackupBlobs>>,
    prepared: Mutex<Option<Prepared>>,
    pub(crate) operation: Mutex<()>,
}
impl CalendarBackup {
    pub fn new(db: Arc<Database>, consent: Arc<ConsentStore>, root: &Path) -> AppResult<Self> {
        consent.bind(db.coordinator())?;
        let _permit = db.coordinator().enter()?;
        directory(root)?;
        let owner_path = root.join(".owner");
        match std::fs::symlink_metadata(&owner_path) {
            Ok(m) if !m.is_file() || m.file_type().is_symlink() => {
                return Err(AppError::InvalidInput);
            }
            Ok(_) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(io_error(e)),
        }
        let owner = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(owner_path)
            .map_err(io_error)?;
        owner.try_lock().map_err(|_| AppError::Conflict)?;
        for e in std::fs::read_dir(root).map_err(io_error)? {
            let p = e.map_err(io_error)?.path();
            let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if ["stage-", "import-"].iter().any(|prefix| {
                name.strip_prefix(prefix)
                    .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok())
            }) {
                remove_directory(&p)?;
            }
        }

        Ok(Self {
            _owner: owner,
            db,
            consent,
            root: root.into(),
            blobs: None,
            prepared: Mutex::new(None),
            operation: Mutex::new(()),
        })
    }
    pub fn with_blobs(mut self, blobs: Arc<dyn BackupBlobs>) -> AppResult<Self> {
        if !Arc::ptr_eq(&blobs.coordinator(), &self.db.coordinator()) {
            return Err(AppError::Conflict);
        }
        self.blobs = Some(blobs);
        Ok(self)
    }
    pub fn snapshot(&self, now: i64, pause: &PauseGuard) -> AppResult<BackupManifest> {
        let _operation = self.operation.lock().map_err(|_| AppError::Conflict)?;
        self.db.paused(pause, |_| Ok(()))?;
        let day = now.div_euclid(DAY);
        let path = self.root.join(format!("day-{day}"));
        if path.exists() {
            return Ok(self.validate(&path)?.0.manifest);
        }
        let manifest = self.capture(&path, now, pause)?;
        let list = self.days()?;
        for day in list.iter().take(list.len().saturating_sub(7)) {
            remove_directory(&self.root.join(format!("day-{day}")))?;
        }
        Ok(manifest)
    }
    fn capture(&self, path: &Path, now: i64, pause: &PauseGuard) -> AppResult<BackupManifest> {
        let mut copy = Connection::open_in_memory().map_err(storage_error)?;
        self.db
            .paused(pause, |live| copy_database(live, &mut copy))?;
        // The isolated in-memory copy is sanitized before any snapshot reaches disk.
        copy.execute_batch("PRAGMA secure_delete=ON; DELETE FROM source_secrets; VACUUM;")
            .map_err(storage_error)?;
        self.publish_copy(&copy, path, now, Some(pause))
    }
    pub(crate) fn publish_copy(
        &self,
        copy: &Connection,
        path: &Path,
        now: i64,
        pause: Option<&PauseGuard>,
    ) -> AppResult<BackupManifest> {
        let staged = self.root.join(format!("stage-{}", uuid::Uuid::new_v4()));
        directory(&staged)?;
        let result = (|| {
            let mut manifest = super::manifest::describe(&self.db, copy, now)?;
            let parts = super::manifest::parts(&self.db, copy)?;
            let mut total = 0usize;
            for (entry, part) in manifest.blobs.iter_mut().zip(parts.iter()) {
                if entry.state == BlobState::Present {
                    let pause = pause.ok_or(AppError::InvalidInput)?;
                    let store = self.blobs.as_ref().ok_or(AppError::Unsupported)?;
                    match store.read(part, pause)? {
                        Some(bytes) => {
                            total = total
                                .checked_add(bytes.len())
                                .ok_or(AppError::InvalidInput)?;
                            if total > 1024 * 1024 * 1024 || bytes.len() > 25 * 1024 * 1024 {
                                return Err(AppError::InvalidInput);
                            }
                            entry.content_hash = Some(hash(&bytes));
                            entry.byte_size = Some(bytes.len() as u64);
                            write_new(&staged.join(entry.blob_id.to_string()), &bytes)?;
                        }
                        None => {
                            entry.state = BlobState::Cleaned;
                            entry.content_hash = None;
                            entry.byte_size = None;
                        }
                    }
                }
            }
            let db_path = staged.join("calendar.sqlite");
            let mut target = Connection::open(&db_path).map_err(storage_error)?;
            copy_database(copy, &mut target)?;
            drop(target);
            sync_file(&db_path)?;
            let disk = DiskManifest {
                transfer_schema_version: SchemaVersion::V1,
                physical_schema_version: 7,
                database_hash: hash_file(&db_path, MAX_DB)?,
                manifest: manifest.clone(),
            };
            write_new(
                &staged.join("manifest.json"),
                &serde_json::to_vec(&disk).map_err(|_| AppError::InvalidInput)?,
            )?;
            self.validate(&staged)?;
            sync_dir(&staged)?;
            if path.exists() {
                return Err(AppError::Conflict);
            }
            std::fs::rename(&staged, path).map_err(io_error)?;
            sync_dir(&self.root)?;
            Ok(manifest)
        })();
        if staged.exists() {
            let _ = remove_directory(&staged);
        }
        result
    }
    pub fn automatic_snapshot(&self, now: i64) -> AppResult<BackupManifest> {
        let pause = self.db.pause_writes()?;
        self.snapshot(now, &pause)
    }
    pub fn delete(&self, day: i64) -> AppResult<()> {
        let _op = self.operation.lock().map_err(|_| AppError::Conflict)?;
        let path = self.root.join(format!("day-{day}"));
        remove_directory(&path)?;
        sync_dir(&self.root)
    }
    pub fn list(&self) -> AppResult<Vec<BackupManifest>> {
        self.days()?
            .into_iter()
            .rev()
            .map(|d| {
                Ok(self
                    .validate(&self.root.join(format!("day-{d}")))?
                    .0
                    .manifest)
            })
            .collect()
    }
    fn days(&self) -> AppResult<Vec<i64>> {
        check_directory(&self.root)?;
        let mut days = vec![];
        for entry in std::fs::read_dir(&self.root).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            if let Some(s) = entry
                .file_name()
                .to_str()
                .and_then(|s| s.strip_prefix("day-"))
            {
                let d = s.parse::<i64>().map_err(|_| AppError::InvalidInput)?;
                if format!("day-{d}") != entry.file_name().to_string_lossy() {
                    return Err(AppError::InvalidInput);
                }
                check_directory(&entry.path())?;
                days.push(d)
            }
        }
        days.sort();
        Ok(days)
    }
    pub fn preview_previous(&self, now: i64) -> AppResult<RestorePreview> {
        let _op = self.operation.lock().map_err(|_| AppError::Conflict)?;
        let mut previous = Vec::new();
        for entry in std::fs::read_dir(&self.root).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            if entry
                .file_name()
                .to_str()
                .is_some_and(|s| s.starts_with("before-"))
            {
                let path = entry.path();
                let (disk, _) = self.validate(&path)?;
                previous.push((disk.manifest.created_at, path));
            }
        }
        previous.sort();
        let (_, path) = previous.pop().ok_or(AppError::InvalidInput)?;
        self.prepare(path, now)
    }
    pub fn preview(&self, day: i64, now: i64) -> AppResult<RestorePreview> {
        let _op = self.operation.lock().map_err(|_| AppError::Conflict)?;
        self.prepare(self.root.join(format!("day-{day}")), now)
    }
    pub(crate) fn prepare(&self, path: PathBuf, now: i64) -> AppResult<RestorePreview> {
        let (disk, _) = self.validate(&path)?;
        let id = RestorePreviewId::from_uuid(uuid::Uuid::new_v4());
        let mut current = self.prepared.lock().map_err(|_| AppError::Conflict)?;
        if let Some(old) = current.as_ref()
            && old.path != path
            && old
                .path
                .file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.starts_with("import-"))
        {
            remove_directory(&old.path)?;
        }
        *current = Some(Prepared {
            id,
            digest: bundle_digest(&path)?,
            path,
            expires: now.checked_add(600_000).ok_or(AppError::InvalidInput)?,
        });
        Ok(RestorePreview {
            preview_id: id,
            schema_version: disk.manifest.schema_version,
            created_at: disk.manifest.created_at,
            entity_counts: disk.manifest.entity_counts,
            blobs: disk.manifest.blobs,
        })
    }
    pub fn restore(
        &self,
        id: RestorePreviewId,
        confirmed: bool,
        now: i64,
        pause: &PauseGuard,
    ) -> AppResult<()> {
        if !confirmed {
            return Err(AppError::InvalidInput);
        }
        let _op = self.operation.lock().map_err(|_| AppError::Conflict)?;
        self.db.paused(pause, |_| Ok(()))?;
        let mut prepared = self.prepared.lock().map_err(|_| AppError::Conflict)?;
        let p = prepared.as_ref().ok_or(AppError::Conflict)?;
        if p.id != id || now > p.expires || bundle_digest(&p.path)? != p.digest {
            return Err(AppError::Conflict);
        }
        let (disk, mut copy) = self.validate(&p.path)?;
        super::manifest::deactivate(&self.db, &mut copy)?;
        super::manifest::mark_missing(&self.db, &copy, &disk.manifest.blobs)?;
        // Drain includes model sends and consent writers. Hold consent until the atomic
        // SQLite commit and default-off publication; failure leaves old consent intact.
        let mut consent = self
            .consent
            .current
            .lock()
            .map_err(|_| AppError::Conflict)?;
        let revision = consent.revision.checked_add(1).ok_or(AppError::Conflict)?;
        let before = self.root.join(format!("before-{}", uuid::Uuid::new_v4()));
        self.capture(&before, now, pause)?;
        // Bound safety checkpoints even when later blob publication fails. Keep
        // the active source when it is itself the previous checkpoint.
        for entry in std::fs::read_dir(&self.root).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            let path = entry.path();
            if path != before
                && path != p.path
                && entry.file_name().to_str().is_some_and(|s| {
                    s.strip_prefix("before-")
                        .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok())
                })
            {
                remove_directory(&path)?;
            }
        }
        let parts = super::manifest::parts(&self.db, &copy)?;
        for (b, part) in disk.manifest.blobs.iter().zip(parts.iter()) {
            if b.state == BlobState::Present {
                let bytes = read_file(&p.path.join(b.blob_id.to_string()), 25 * 1024 * 1024)?;
                self.blobs
                    .as_ref()
                    .ok_or(AppError::Unsupported)?
                    .publish(part, &bytes, pause)?;
            }
        }
        // SQLite backup's destination transaction commits only after the full copy.
        self.db.paused(pause, |live| copy_database(&copy, live))?;
        *consent = ModelConsent {
            revision,
            ..ModelConsent::default()
        };
        // Post-commit cleanup cannot turn successful restoration into an error.
        if p.path
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.starts_with("import-"))
        {
            let _ = remove_directory(&p.path);
        }
        *prepared = None;
        if let Ok(entries) = std::fs::read_dir(&self.root) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path != before
                    && entry
                        .file_name()
                        .to_str()
                        .is_some_and(|s| s.starts_with("before-"))
                {
                    let _ = remove_directory(&path);
                }
            }
        }
        Ok(())
    }
    pub fn vault_snapshot(&self) -> AppResult<BackupManifest> {
        Err(AppError::Unsupported)
    }
    pub(crate) fn validate(&self, path: &Path) -> AppResult<(DiskManifest, Connection)> {
        check_directory(path)?;
        let disk: DiskManifest =
            serde_json::from_slice(&read_file(&path.join("manifest.json"), 4 * 1024 * 1024)?)
                .map_err(|_| AppError::InvalidInput)?;
        if disk.physical_schema_version != 7
            || disk.database_hash != hash_file(&path.join("calendar.sqlite"), MAX_DB)?
        {
            return Err(AppError::InvalidInput);
        }
        let source = Connection::open_with_flags(
            path.join("calendar.sqlite"),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(storage_error)?;
        super::manifest::validate_database(&self.db, &source)?;
        let mut copy = Connection::open_in_memory().map_err(storage_error)?;
        copy_database(&source, &mut copy)?;
        let expected = super::manifest::describe(&self.db, &copy, disk.manifest.created_at)?;
        if expected.entity_counts != disk.manifest.entity_counts
            || expected.blobs.len() != disk.manifest.blobs.len()
        {
            return Err(AppError::InvalidInput);
        }
        let mut allowed = std::collections::HashSet::from([
            "calendar.sqlite".to_string(),
            "manifest.json".to_string(),
        ]);
        for (e, b) in expected.blobs.iter().zip(disk.manifest.blobs.iter()) {
            if e.blob_id != b.blob_id
                || e.state != b.state
                    && !(e.state == BlobState::Present && b.state == BlobState::Cleaned)
            {
                return Err(AppError::InvalidInput);
            }
            if b.state == BlobState::Present {
                let bytes = read_file(&path.join(b.blob_id.to_string()), 25 * 1024 * 1024)?;
                if b.content_hash.as_ref() != Some(&hash(&bytes))
                    || b.byte_size != Some(bytes.len() as u64)
                {
                    return Err(AppError::InvalidInput);
                }
                allowed.insert(b.blob_id.to_string());
            } else if b.content_hash.is_some() || b.byte_size.is_some() {
                return Err(AppError::InvalidInput);
            }
        }
        for entry in std::fs::read_dir(path).map_err(io_error)? {
            if !allowed.contains(
                &entry
                    .map_err(io_error)?
                    .file_name()
                    .to_string_lossy()
                    .to_string(),
            ) {
                return Err(AppError::InvalidInput);
            }
        }
        Ok((disk, copy))
    }
}
pub(crate) fn copy_database(source: &Connection, target: &mut Connection) -> AppResult<()> {
    rusqlite::backup::Backup::new(source, target)
        .map_err(storage_error)?
        .run_to_completion(128, std::time::Duration::from_millis(1), None)
        .map_err(storage_error)
}
pub(crate) fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn bundle_digest(path: &Path) -> AppResult<String> {
    let bytes = read_file(&path.join("manifest.json"), 4 * 1024 * 1024)?;
    let mut data = bytes;
    data.extend(hash_file(&path.join("calendar.sqlite"), MAX_DB)?.as_bytes());
    Ok(hash(&data))
}
pub(crate) fn io_error(e: std::io::Error) -> AppError {
    if e.kind() == std::io::ErrorKind::StorageFull {
        AppError::StorageFull
    } else {
        AppError::InvalidInput
    }
}
pub(crate) fn check_directory(path: &Path) -> AppResult<()> {
    if !path.is_absolute()
        || path
            .components()
            .any(|p| matches!(p, Component::ParentDir | Component::CurDir))
    {
        return Err(AppError::InvalidInput);
    }
    for p in path.ancestors() {
        let m = std::fs::symlink_metadata(p).map_err(io_error)?;
        if !m.is_dir() || m.file_type().is_symlink() {
            return Err(AppError::InvalidInput);
        }
    }
    Ok(())
}
fn directory(path: &Path) -> AppResult<()> {
    check_directory(path.parent().ok_or(AppError::InvalidInput)?)?;
    if !path.exists() {
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(path).map_err(io_error)?;
    }
    check_directory(path)
}
fn remove_directory(path: &Path) -> AppResult<()> {
    check_directory(path)?;
    for e in std::fs::read_dir(path).map_err(io_error)? {
        let p = e.map_err(io_error)?.path();
        let m = std::fs::symlink_metadata(&p).map_err(io_error)?;
        if !m.is_file() || m.file_type().is_symlink() {
            return Err(AppError::InvalidInput);
        }
    }
    std::fs::remove_dir_all(path).map_err(io_error)
}
pub(crate) fn read_file(path: &Path, max: u64) -> AppResult<Vec<u8>> {
    check_directory(path.parent().ok_or(AppError::InvalidInput)?)?;
    let m = std::fs::symlink_metadata(path).map_err(io_error)?;
    if !m.is_file() || m.file_type().is_symlink() || m.len() > max {
        return Err(AppError::InvalidInput);
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(io_error)?
        .take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() as u64 > max {
        return Err(AppError::InvalidInput);
    }
    Ok(bytes)
}
fn hash_file(path: &Path, max: u64) -> AppResult<String> {
    Ok(hash(&read_file(path, max)?))
}
fn write_new(path: &Path, bytes: &[u8]) -> AppResult<()> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    let mut f = o.open(path).map_err(io_error)?;
    f.write_all(bytes).map_err(io_error)?;
    f.sync_all().map_err(io_error)
}
fn sync_file(path: &Path) -> AppResult<()> {
    std::fs::File::open(path)
        .map_err(io_error)?
        .sync_all()
        .map_err(io_error)
}
fn sync_dir(path: &Path) -> AppResult<()> {
    #[cfg(unix)]
    {
        sync_file(path)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn destination_failure_rolls_back_existing_connection() {
        let mut live = Connection::open_in_memory().unwrap();
        live.execute_batch("CREATE TABLE original(value TEXT); INSERT INTO original VALUES('still usable'); PRAGMA max_page_count=2;").unwrap();
        let source = Connection::open_in_memory().unwrap();
        source.execute_batch("CREATE TABLE replacement(value BLOB); INSERT INTO replacement VALUES(zeroblob(1048576));").unwrap();
        assert_eq!(
            copy_database(&source, &mut live),
            Err(AppError::StorageFull)
        );
        assert_eq!(
            live.query_row("SELECT value FROM original", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "still usable"
        );
        live.execute("UPDATE original SET value='new write'", [])
            .unwrap();
    }
}
