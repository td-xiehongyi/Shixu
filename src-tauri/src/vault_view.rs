//! A single native window, with capabilities separated by actual WebView label.
use shixu_core::contracts::{AppResult, error::AppError};
#[derive(Default)]
pub struct NavigationState {
    revision: u64,
    visible: bool,
}
impl NavigationState {
    pub fn request(&mut self, visible: bool, revision: u64) -> bool {
        if revision <= self.revision {
            return false;
        }
        self.revision = revision;
        self.visible = visible;
        true
    }
    pub fn acknowledge(&self, revision: u64) -> bool {
        self.revision == revision && self.visible
    }
}
pub fn content_bounds(width: f64, height: f64) -> (f64, f64, f64, f64) {
    (214.0, 0.0, (width - 214.0).max(1.0), height.max(1.0))
}
#[cfg(windows)]
mod native {
    use super::*;
    use std::sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    };
    use tauri::{Emitter, Manager, WebviewBuilder, WebviewUrl};
    pub struct VaultViewHost {
        desired: Mutex<NavigationState>,
        operation: Mutex<()>,
        initialized: Mutex<bool>,
        suspended: AtomicBool,
        failed: AtomicBool,
    }
    impl Default for VaultViewHost {
        fn default() -> Self {
            Self {
                desired: Mutex::new(NavigationState::default()),
                operation: Mutex::new(()),
                initialized: Mutex::new(false),
                suspended: AtomicBool::new(false),
                failed: AtomicBool::new(false),
            }
        }
    }
    impl VaultViewHost {
        pub fn set_visible(
            &self,
            app: &tauri::AppHandle,
            visible: bool,
            revision: u64,
        ) -> AppResult<()> {
            if !self
                .desired
                .lock()
                .map_err(|_| AppError::Locked)?
                .request(visible, revision)
            {
                return Ok(());
            }
            let _operation = self.operation.lock().map_err(|_| AppError::Locked)?;
            if self.desired.lock().map_err(|_| AppError::Locked)?.revision != revision {
                return Ok(());
            }
            if visible && app.get_webview("vault").is_none() {
                let window = app.get_window("main").ok_or(AppError::Unsupported)?;
                let fictional = app
                    .try_state::<crate::startup::ObservationRoot>()
                    .is_some_and(|r| r.is_fictional());
                let url = if fictional {
                    "index.html?window=vault&fictional=1"
                } else {
                    "index.html?window=vault"
                };
                let mut builder = WebviewBuilder::new("vault", WebviewUrl::App(url.into()))
                    .on_navigation(crate::runtime::local_url);
                if let Some(root) = app.try_state::<crate::startup::ObservationRoot>() {
                    builder = builder.data_directory(root.webview_directory());
                }
                // Initial page contains no secrets. Create outside visible client bounds,
                // then wait for the UI redaction acknowledgement before showing it.
                let view = window
                    .add_child(
                        builder,
                        tauri::LogicalPosition::new(-10000., 0.),
                        tauri::LogicalSize::new(1., 1.),
                    )
                    .map_err(|_| AppError::Unsupported)?;
                view.hide().map_err(|_| AppError::Unsupported)?;
                self.resize(app)?;
            }
            if let Some(view) = app.get_webview("vault") {
                view.hide().map_err(|_| AppError::Unsupported)?;
                if *self.initialized.lock().map_err(|_| AppError::Locked)? {
                    self.send_visibility(app)?;
                } else if visible
                    && self.failed.load(Ordering::SeqCst)
                    && !self.suspended.load(Ordering::SeqCst)
                {
                    view.show().map_err(|_| AppError::Unsupported)?;
                }
            }
            Ok(())
        }
        fn send_visibility(&self, app: &tauri::AppHandle) -> AppResult<()> {
            let state = self.desired.lock().map_err(|_| AppError::Locked)?;
            app.emit_to(
                tauri::EventTarget::webview("vault"),
                "vault_view_visibility",
                serde_json::json!({"visible":state.visible,"revision":state.revision.to_string()}),
            )
            .map_err(|_| AppError::Unsupported)
        }
        pub fn acknowledge(
            &self,
            app: &tauri::AppHandle,
            revision: Option<u64>,
            failed: bool,
        ) -> AppResult<()> {
            let _operation = self.operation.lock().map_err(|_| AppError::Locked)?;
            if failed {
                self.failed.store(true, Ordering::SeqCst);
                *self.initialized.lock().map_err(|_| AppError::Locked)? = false;
                app.state::<crate::app_state::AppState>()
                    .vault()
                    .signal()
                    .notify(crate::vault::NativeVaultEvent::Revoked);
                app.emit_to(
                    tauri::EventTarget::webview("main"),
                    "vault_view_failed",
                    true,
                )
                .map_err(|_| AppError::Unsupported)?;
                let state = self.desired.lock().map_err(|_| AppError::Locked)?;
                if state.visible && !self.suspended.load(Ordering::SeqCst) {
                    app.get_webview("vault")
                        .ok_or(AppError::Unsupported)?
                        .show()
                        .map_err(|_| AppError::Unsupported)?;
                }
                return Ok(());
            }
            let Some(revision) = revision else {
                self.failed.store(false, Ordering::SeqCst);
                app.emit_to(
                    tauri::EventTarget::webview("main"),
                    "vault_view_failed",
                    false,
                )
                .map_err(|_| AppError::Unsupported)?;
                *self.initialized.lock().map_err(|_| AppError::Locked)? = true;
                return self.send_visibility(app);
            };
            let state = self.desired.lock().map_err(|_| AppError::Locked)?;
            if state.acknowledge(revision) && !self.suspended.load(Ordering::SeqCst) {
                let view = app.get_webview("vault").ok_or(AppError::Unsupported)?;
                view.show()
                    .and_then(|_| view.set_focus())
                    .map_err(|_| AppError::Unsupported)?;
            }
            Ok(())
        }
        pub fn resize(&self, app: &tauri::AppHandle) -> AppResult<()> {
            let Some(view) = app.get_webview("vault") else {
                return Ok(());
            };
            let window = app.get_window("main").ok_or(AppError::Unsupported)?;
            let scale = window.scale_factor().map_err(|_| AppError::Unsupported)?;
            let size = window
                .inner_size()
                .map_err(|_| AppError::Unsupported)?
                .to_logical::<f64>(scale);
            let (x, y, width, height) = content_bounds(size.width, size.height);
            view.set_bounds(tauri::Rect {
                position: tauri::LogicalPosition::new(x, y).into(),
                size: tauri::LogicalSize::new(width, height).into(),
            })
            .map_err(|_| AppError::Unsupported)
        }
        pub fn resume(&self, app: &tauri::AppHandle) -> AppResult<()> {
            if !self.suspended.swap(false, Ordering::SeqCst) {
                return Ok(());
            }
            if app.get_webview("vault").is_some()
                && *self.initialized.lock().map_err(|_| AppError::Locked)?
            {
                self.send_visibility(app)?;
            }
            Ok(())
        }
        pub fn redact_hidden(&self, app: &tauri::AppHandle) -> AppResult<()> {
            self.suspended.store(true, Ordering::SeqCst);
            // Window-to-tray keeps the selected module and native session; the
            // UI only drops temporary display/input. It must not unmount.
            if app.get_webview("vault").is_some() {
                app.get_webview("vault")
                    .ok_or(AppError::Unsupported)?
                    .hide()
                    .map_err(|_| AppError::Unsupported)?;
                app.emit_to(
                    tauri::EventTarget::webview("vault"),
                    "vault_view_redact",
                    (),
                )
                .map_err(|_| AppError::Unsupported)?;
            }
            Ok(())
        }
    }
}
#[cfg(windows)]
pub use native::VaultViewHost;
