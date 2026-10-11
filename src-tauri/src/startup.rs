//! Fixed observation selection happens before any normal application-store access.
use crate::app_state::AppState;
use shixu_core::contracts::{AppResult, error::AppError};
use std::{ffi::OsString, path::PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Observe,
    Fictional,
}
/// Only the exact fixed flag selects observation. No caller-controlled root exists.
pub fn observation_mode(args: &[OsString]) -> AppResult<Option<Mode>> {
    let mode = match args.first().and_then(|arg| arg.to_str()) {
        Some("--vault-lifecycle-observe") => Some(Mode::Observe),
        Some("--vault-fictional") => Some(Mode::Fictional),
        _ => None,
    };
    if let Some(mode) = mode {
        if args.len() != 1 {
            return Err(AppError::InvalidInput);
        }
        return Ok(Some(mode));
    }
    Ok(None)
}
/// Owns only a directory atomically created by this process. No existing root is opened.
pub struct ObservationRoot(PathBuf, bool);
impl ObservationRoot {
    pub fn is_fictional(&self) -> bool {
        !self.1
    }
    pub fn vault_directory(&self) -> PathBuf {
        self.0.join("vault")
    }
    pub fn resources_directory(&self) -> AppResult<PathBuf> {
        let directory = self.0.parent().ok_or(AppError::Unsupported)?;
        #[cfg(windows)]
        return shixu_native::vault::engine::bundled_resources_directory(directory);
        #[cfg(not(windows))]
        Ok(directory.join("vault-win-x64"))
    }
    pub fn webview_directory(&self) -> PathBuf {
        self.0.join("webview")
    }
    fn fresh() -> AppResult<Self> {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| AppError::Unsupported)?
            .as_nanos();
        for attempt in 0..16 {
            let path = std::env::temp_dir().join(format!(
                "shixu-lifecycle-observation-{}-{nonce}-{attempt}",
                std::process::id()
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path, true)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(_) => return Err(AppError::Unsupported),
            }
        }
        Err(AppError::Conflict)
    }
    fn fictional() -> AppResult<Self> {
        // Fixed native-only workspace beside this executable, never a CLI path.
        // It intentionally persists for real process-restart acceptance.
        let exe = std::env::current_exe().map_err(|_| AppError::Unsupported)?;
        let path = exe
            .parent()
            .ok_or(AppError::Unsupported)?
            .join("shixu-fictional-desktop");
        match std::fs::create_dir(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err(AppError::Unsupported),
        }
        let metadata = std::fs::symlink_metadata(&path).map_err(|_| AppError::Unsupported)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(AppError::Unsupported);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err(AppError::Unsupported);
            }
        }
        Ok(Self(path, false))
    }
}
impl Drop for ObservationRoot {
    fn drop(&mut self) {
        // No recursive removal: a foreign child causes safe refusal of cleanup.
        if self.1 {
            let _ = std::fs::remove_dir(&self.0);
        }
    }
}
pub fn initialize(
    mode: Mode,
    normal: impl FnOnce() -> AppResult<AppState>,
) -> AppResult<(AppState, Option<ObservationRoot>)> {
    match mode {
        Mode::Observe => Ok((AppState::default(), Some(ObservationRoot::fresh()?))),
        Mode::Fictional => Ok((AppState::default(), Some(ObservationRoot::fictional()?))),
        Mode::Normal => Ok((normal()?, None)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    #[test]
    fn bundled_resources_accept_native_verbatim_disk_path() {
        let root = ObservationRoot(
            PathBuf::from(r"\\?\C:\拾序 空 格\shixu-fictional-desktop"),
            false,
        );
        assert_eq!(
            root.resources_directory().unwrap(),
            PathBuf::from(r"C:\拾序 空 格\vault-win-x64")
        );
    }
    #[cfg(windows)]
    #[test]
    fn bundled_resources_refuse_remote_device_and_aliased_paths() {
        for path in [
            r"\\server\share\app\shixu-fictional-desktop",
            r"\\?\UNC\server\share\app\shixu-fictional-desktop",
            r"\\.\C:\app\shixu-fictional-desktop",
            r"C:\app\..\other\shixu-fictional-desktop",
            r"C:\app.\shixu-fictional-desktop",
            r"C:\app \shixu-fictional-desktop",
            r"relative\app\shixu-fictional-desktop",
        ] {
            assert!(
                ObservationRoot(PathBuf::from(path), false)
                    .resources_directory()
                    .is_err(),
                "unexpected admission: {path}"
            );
        }
    }
    #[test]
    fn observation_never_initializes_production_and_owns_fresh_root() {
        let (state, root) =
            initialize(Mode::Observe, || panic!("production store touched")).unwrap();
        assert!(state.runtime().is_err());
        assert!(state.backup().is_err());
        assert!(state.database().is_err());
        assert!(state.calendar().is_err());
        assert!(state.qq().is_err());
        assert!(state.settings().is_err());
        let root = root.expect("observation must own its fresh root");
        let path = root.0.clone();
        assert!(path.is_dir());
        assert_eq!(root.webview_directory(), path.join("webview"));
        assert_eq!(std::fs::read_dir(&path).unwrap().count(), 0);
        let (_, other) = initialize(Mode::Observe, || panic!("production store touched")).unwrap();
        assert_ne!(path, other.unwrap().0);
        drop(root);
        assert!(!path.exists());
    }
    #[test]
    fn fictional_selection_is_explicit_and_cannot_override_store_paths() {
        let selected = observation_mode(&["--vault-fictional".into()]).unwrap();
        assert!(
            selected.is_some(),
            "fixed fictional flag must select isolated startup"
        );
        for extra in ["--root", "--login-start", "C:\\existing"] {
            assert!(observation_mode(&["--vault-fictional".into(), extra.into()]).is_err());
        }
        let (_, root) =
            initialize(selected.unwrap(), || panic!("production store touched")).unwrap();
        assert!(root.is_some());
    }
    #[test]
    fn exact_observation_argument_refuses_extensions() {
        assert_eq!(
            observation_mode(&["--vault-lifecycle-observe".into()]).unwrap(),
            Some(Mode::Observe)
        );
        for extra in [
            "--root",
            "--login-start",
            "--release-manifest",
            "C:\\existing",
        ] {
            assert!(observation_mode(&["--vault-lifecycle-observe".into(), extra.into()]).is_err());
        }
        assert_eq!(observation_mode(&[]).unwrap(), None);
        assert_eq!(observation_mode(&["--unknown".into()]).unwrap(), None);
    }
    #[test]
    fn normal_selection_retains_existing_initialization() {
        let called = std::cell::Cell::new(false);
        let (_, root) = initialize(Mode::Normal, || {
            called.set(true);
            Ok(AppState::default())
        })
        .unwrap();
        assert!(called.get());
        assert!(root.is_none());
    }
}
