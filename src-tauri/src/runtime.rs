//! Windows-only native adapter. Actual WebView IPC acceptance still requires Windows D7.
use crate::lifecycle::{DesktopLifecycle, Lifecycle, LifecycleEvent, UnavailableVault};
use crate::{
    app_state::AppState,
    commands::{self, CallingContext},
};
use shixu_core::contracts::error::AppError;
use shixu_core::{
    contracts::{AppResult, notification::*},
    runtime::{
        supervisor::AttachmentParser,
        workers::{BackgroundWorkers, WorkerPorts},
    },
};
use std::sync::{Arc, Mutex};
struct NativeWorkers(Mutex<Option<BackgroundWorkers>>);
struct InstanceOwner {
    _file: std::fs::File,
}
struct NativeParser;
impl AttachmentParser for NativeParser {
    fn parse(&self, p: &MessagePart, l: &ParserLimits) -> AppResult<PartResult> {
        shixu_native::attachments::host::parse_part(p, l)
    }
}
struct NativeDesktop(tauri::AppHandle);
impl DesktopLifecycle for NativeDesktop {
    fn hide_main(&self) -> AppResult<()> {
        self.0
            .get_webview_window("main")
            .ok_or(AppError::Unsupported)?
            .hide()
            .map_err(|_| AppError::Unsupported)
    }
    fn focus_main(&self) -> AppResult<()> {
        let w = self
            .0
            .get_webview_window("main")
            .ok_or(AppError::Unsupported)?;
        w.show()
            .and_then(|_| w.set_focus())
            .map_err(|_| AppError::Unsupported)
    }
    fn exit(&self) -> AppResult<()> {
        self.0.exit(0);
        Ok(())
    }
}
fn stop_workers(app: &tauri::AppHandle) {
    if let Some(workers) = app.try_state::<NativeWorkers>()
        && let Ok(mut workers) = workers.0.lock()
        && let Some(workers) = workers.take()
    {
        let _ = workers.stop();
    }
}
fn install_tray(app: &tauri::App) -> tauri::Result<()> {
    use tauri::{
        menu::{Menu, MenuItem},
        tray::TrayIconBuilder,
    };
    let show = MenuItem::with_id(app, "show", "打开拾序", true, None::<&str>)?;
    let exit = MenuItem::with_id(app, "exit", "退出拾序", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &exit])?;
    TrayIconBuilder::with_id("shixu-background")
        .icon(tauri::image::Image::new_owned(
            [47, 102, 85, 255].repeat(16 * 16),
            16,
            16,
        ))
        .tooltip("拾序 · 后台运行")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => {
                let _ = NativeDesktop(app.clone()).focus_main();
            }
            "exit" => {
                stop_workers(app);
                if let Some(lifecycle) = app.try_state::<Lifecycle>() {
                    let _ = lifecycle.handle_lifecycle(LifecycleEvent::Exit, 0);
                } else {
                    app.exit(0);
                }
            }
            _ => {}
        })
        .build(app)?;
    Ok(())
}
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};
fn local_url(url: &tauri::Url) -> bool {
    url.origin().ascii_serialization() == "http://tauri.localhost"
        && matches!(url.path(), "/" | "/index.html")
}
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let root = app.path().app_local_data_dir()?;
            std::fs::create_dir_all(&root)?;
            // Process exclusion is fail-closed. Cross-process focus/session/power
            // delivery still needs the Windows native integration gate.
            let owner_path = root.join(".instance-owner");
            if let Ok(metadata) = std::fs::symlink_metadata(&owner_path)
                && (!metadata.is_file() || metadata.file_type().is_symlink())
            {
                return Err(Box::new(AppError::InvalidInput));
            }
            let owner = std::fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(owner_path)?;
            owner.try_lock().map_err(|_| AppError::Conflict)?;
            app.manage(InstanceOwner { _file: owner });
            let directory = root.join("calendar");
            // Product storage is only the existing Windows DPAPI/ACL adapter.
            let state = match shixu_native::protection::DpapiProtector::open_database(&directory) {
                Ok(db) => AppState::from_database(Arc::new(db))
                    .with_backups(&directory.join("backups"))?,
                Err(_) => AppState::default(),
            };
            if let Ok(supervisor) = state.runtime() {
                if std::env::args().any(|arg| arg == "--login-start") {
                    supervisor.login_start()?;
                } else {
                    supervisor.resume()?;
                }
                let workers = supervisor.spawn_workers(WorkerPorts {
                    receiver: Some(state.take_qq_receiver()?),
                    parser: Some(Arc::new(NativeParser)),
                    backup: state.backup().ok(),
                    ..WorkerPorts::default()
                })?;
                app.manage(NativeWorkers(Mutex::new(Some(workers))));
                app.manage(Lifecycle {
                    supervisor,
                    vault: Arc::new(UnavailableVault),
                    desktop: Arc::new(NativeDesktop(app.handle().clone())),
                });
            }
            app.manage(state);
            install_tray(app)?;
            WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("拾序")
                .inner_size(1487.0, 1058.0)
                .min_inner_size(760.0, 600.0)
                .on_navigation(local_url)
                .build()?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main"
                && let tauri::WindowEvent::CloseRequested { api, .. } = event
            {
                api.prevent_close();
                if let Some(lifecycle) = window.app_handle().try_state::<Lifecycle>() {
                    let _ = lifecycle.handle_lifecycle(LifecycleEvent::WindowClose, 0);
                } else {
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(|invoke| {
            // Window creation must run outside the synchronous Windows WebView callback.
            tauri::async_runtime::spawn_blocking(move || {
                let view = invoke.message.webview();
                let command = invoke.message.command();
                let result = (|| {
                    let url = view.url().map_err(|_| AppError::AuthFailed)?;
                    let origin = url.origin().ascii_serialization();
                    let context = CallingContext {
                        label: view.label(),
                        origin: &origin,
                    };
                    commands::authorize(&context, command)?;
                    let tauri::ipc::InvokeBody::Json(payload) = invoke.message.payload() else {
                        return Err(AppError::InvalidInput);
                    };
                    // Boundary size check is structural, avoiding extra plaintext serialization.
                    crate::commands::validate_command_size(command, payload)?;
                    let state = view.state::<AppState>();
                    let result = commands::dispatch(&context, command, payload.clone(), &state)?;
                    if command == "show_vault_window" {
                        if let Some(window) = view.app_handle().get_webview_window("vault") {
                            window.show().map_err(|_| AppError::Unsupported)?;
                            window.set_focus().map_err(|_| AppError::Unsupported)?;
                        } else {
                            WebviewWindowBuilder::new(
                                view.app_handle(),
                                "vault",
                                WebviewUrl::App("index.html?window=vault".into()),
                            )
                            .title("拾序 · 密码库")
                            .inner_size(640.0, 620.0)
                            .on_navigation(local_url)
                            .build()
                            .map_err(|_| AppError::Unsupported)?;
                        }
                    }
                    Ok(result)
                })();
                match result {
                    Ok(value) => invoke.resolver.resolve(value),
                    Err(error) => invoke.resolver.reject(error.code()),
                }
            });
            true
        })
        .build(tauri::generate_context!())
        .expect("desktop runtime failed")
        .run(|app, event| {
            if matches!(event, tauri::RunEvent::Exit) {
                stop_workers(app);
            }
        });
}
