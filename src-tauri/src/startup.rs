//! Fixed observation selection happens before any normal application-store access.
use crate::app_state::AppState;
use shixu_core::contracts::{AppResult, error::AppError};
use std::{ffi::OsString, path::PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Observe,
}
/// Only the exact fixed flag selects observation. No caller-controlled root exists.
pub fn observation_mode(args: &[OsString]) -> AppResult<Option<Mode>> {
    if args
        .first()
        .is_some_and(|arg| arg == "--vault-lifecycle-observe")
    {
        if args.len() != 1 {
            return Err(AppError::InvalidInput);
        }
        return Ok(Some(Mode::Observe));
    }
    Ok(None)
}
/// Owns only a directory atomically created by this process. No existing root is opened.
pub struct ObservationRoot(PathBuf);
impl ObservationRoot {
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
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(_) => return Err(AppError::Unsupported),
            }
        }
        Err(AppError::Conflict)
    }
}
impl Drop for ObservationRoot {
    fn drop(&mut self) {
        // No recursive removal: a foreign child causes safe refusal of cleanup.
        let _ = std::fs::remove_dir(&self.0);
    }
}
pub fn initialize(
    mode: Mode,
    normal: impl FnOnce() -> AppResult<AppState>,
) -> AppResult<(AppState, Option<ObservationRoot>)> {
    match mode {
        Mode::Observe => Ok((AppState::default(), Some(ObservationRoot::fresh()?))),
        Mode::Normal => Ok((normal()?, None)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
