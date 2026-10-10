//! Linux encrypted checkpoint + atomic rename. Windows enablement is blocked.
use sha2::{Digest, Sha256};
use shixu_core::contracts::{AppResult, error::AppError};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
pub(super) const MAX_FILE: u64 = 8 * 1024 * 1024;
pub(super) fn read_bounded(path: &Path) -> AppResult<Vec<u8>> {
    let metadata = fs::symlink_metadata(path).map_err(|_| AppError::AuthFailed)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > MAX_FILE {
        return Err(AppError::AuthFailed);
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|_| AppError::AuthFailed)?
        .take(MAX_FILE + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| AppError::AuthFailed)?;
    if bytes.len() as u64 > MAX_FILE {
        return Err(AppError::AuthFailed);
    }
    Ok(bytes)
}
pub(super) fn digest(path: &Path) -> AppResult<String> {
    Ok(Sha256::digest(read_bounded(path)?)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>())
}
pub(super) fn ensure_plain_directory(path: &Path) -> AppResult<PathBuf> {
    if !path.is_absolute() {
        return Err(AppError::Unsupported);
    }
    for ancestor in path.ancestors() {
        let metadata = fs::symlink_metadata(ancestor).map_err(|_| AppError::Unsupported)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(AppError::Unsupported);
        }
    }
    path.canonicalize().map_err(|_| AppError::Unsupported)
}
fn storage(error: std::io::Error) -> AppError {
    if error.kind() == std::io::ErrorKind::StorageFull {
        AppError::StorageFull
    } else {
        AppError::AuthFailed
    }
}
fn copy_checkpoint(work: &Path, bytes: &[u8]) -> AppResult<()> {
    let staging = work.join("vault.checkpoint.kdbx");
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut output = options.open(&staging).map_err(storage)?;
    output.write_all(bytes).map_err(storage)?;
    output.sync_all().map_err(storage)?;
    fs::rename(staging, work.join("vault.previous.kdbx")).map_err(storage)?;
    File::open(work)
        .map_err(storage)?
        .sync_all()
        .map_err(storage)
}
pub(super) fn commit(
    work: &Path,
    expected: Option<&str>,
    verified_digest: &str,
) -> AppResult<String> {
    let target = work.join("vault.kdbx");
    let pending = work.join("vault.pending.kdbx");
    let next = digest(&pending)?;
    if next != verified_digest {
        return Err(AppError::AuthFailed);
    }
    File::open(&pending)
        .map_err(storage)?
        .sync_all()
        .map_err(storage)?; // helper has already cryptographically reloaded it
    let old = match expected {
        Some(value) => {
            if digest(&target)? != value {
                return Err(AppError::Conflict);
            }
            Some(read_bounded(&target)?)
        }
        None => {
            if target.try_exists().map_err(storage)? {
                return Err(AppError::Conflict);
            }
            None
        }
    };
    if let Some(bytes) = &old {
        copy_checkpoint(work, bytes)?;
    }
    // Recheck immediately before rename. Same-user malicious races are outside
    // this trusted single-owner development API; production OS gate remains.
    if let Some(value) = expected {
        if digest(&target)? != value {
            return Err(AppError::Conflict);
        }
    } else if target.try_exists().map_err(storage)? {
        return Err(AppError::Conflict);
    }
    fs::rename(&pending, &target).map_err(storage)?;
    if let Err(error) = File::open(work).and_then(|dir| dir.sync_all()) {
        // Never report a saved success after a failed durability barrier.
        // Checkpoint retains old valid ciphertext; recovery is not yet UI-wired.
        return Err(storage(error));
    }
    Ok(next)
}
