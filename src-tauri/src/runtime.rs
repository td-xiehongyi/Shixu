//! Windows-only native adapter. Actual WebView IPC acceptance still requires Windows D7.
use crate::lifecycle::{DesktopLifecycle, Lifecycle, LifecycleEvent};
use crate::vault::NativeVaultEvent;
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
use tauri::Emitter;
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
                let handle = app.clone();
                if app
                    .run_on_main_thread(move || {
                        if let Some(state) = handle.try_state::<AppState>() {
                            state.vault().shutdown();
                        }
                        stop_workers(&handle);
                        if let Some(lifecycle) = handle.try_state::<Lifecycle>() {
                            let _ = lifecycle.handle_lifecycle(LifecycleEvent::Exit, 0);
                        } else {
                            handle.exit(0);
                        }
                    })
                    .is_err()
                {
                    if let Some(state) = app.try_state::<AppState>() {
                        state.vault().signal().fail();
                    }
                    eprintln!("SHIXU_VAULT_LIFECYCLE cleanup_schedule=failed availability=blocked");
                    app.exit(1);
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
fn normal_state(app: &mut tauri::App) -> AppResult<AppState> {
    let root = app
        .path()
        .app_local_data_dir()
        .map_err(|_| AppError::Unsupported)?;
    std::fs::create_dir_all(&root).map_err(|_| AppError::Unsupported)?;
    // Process exclusion is fail-closed. Cross-process focus still needs
    // the Windows native integration gate.
    let owner_path = root.join(".instance-owner");
    if let Ok(metadata) = std::fs::symlink_metadata(&owner_path)
        && (!metadata.is_file() || metadata.file_type().is_symlink())
    {
        return Err(AppError::InvalidInput);
    }
    let owner = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(owner_path)
        .map_err(|_| AppError::Unsupported)?;
    owner.try_lock().map_err(|_| AppError::Conflict)?;
    app.manage(InstanceOwner { _file: owner });
    let directory = root.join("calendar");
    // Product storage is only the existing Windows DPAPI/ACL adapter.
    let state = match shixu_native::protection::DpapiProtector::open_database(&directory) {
        Ok(db) => AppState::from_database(Arc::new(db)).with_backups(&directory.join("backups"))?,
        Err(_) => AppState::default(),
    };
    let vault_root = root.join("vault");
    // Windows vault admission must create its private leaf atomically.
    // The public constructor remains blocked pending native proof.
    let state = if let Ok(resources) = app.path().resource_dir() {
        state.with_vault(resources.join("vault-win-x64"), vault_root)
    } else {
        state
    };
    Ok(state)
}
pub fn run(mode: crate::startup::Mode) {
    tauri::Builder::default()
        .setup(move |app| {
            let (state, observation_root) = crate::startup::initialize(mode, || normal_state(app))?;
            if let Some(root) = observation_root {
                app.manage(root);
                eprintln!("SHIXU_VAULT_OBSERVATION mode=fresh-synthetic-unavailable production_store=untouched");
            }
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
                    vault: state.vault(),
                    desktop: Arc::new(NativeDesktop(app.handle().clone())),
                });
            }
            let vault = state.vault();
            let signal = vault.signal();
            let handle = app.handle().clone();
            let redaction_signal = signal.clone();
            vault.set_lifecycle_handler(Arc::new(move |event| {
                eprintln!("SHIXU_VAULT_LIFECYCLE deferred={event:?} production=Unsupported");
                if event == NativeVaultEvent::Revoked
                    && let Some(window) = handle.get_webview_window("vault")
                {
                    // Fixed empty native event; never a session/secret payload.
                    if window.emit_to(tauri::EventTarget::webview_window("vault"), "vault_locked", ()).is_err() {
                        redaction_signal.fail();
                        eprintln!("SHIXU_VAULT_LIFECYCLE redaction_submit=failed availability=blocked");
                    }
                }
            }))?;
            app.manage(state);
            install_tray(app)?;
            let mut main_builder = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()));
            if let Some(root) = app.try_state::<crate::startup::ObservationRoot>() {
                main_builder = main_builder.data_directory(root.webview_directory());
            }
            main_builder
                .title("拾序")
                .inner_size(1487.0, 1058.0)
                .min_inner_size(760.0, 600.0)
                .on_navigation(local_url)
                .build()?;
            eprintln!("SHIXU_VAULT_LIFECYCLE dispatcher=ready session_power=out_of_scope production=Unsupported");
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main"
                && let tauri::WindowEvent::CloseRequested { api, .. } = event
            {
                api.prevent_close();
                if let Some(state) = window.app_handle().try_state::<AppState>() {
                    state
                        .vault()
                        .signal()
                        .notify(NativeVaultEvent::WindowClosed);
                }
                let _ = window.hide();
            }
            if window.label() == "vault"
                && matches!(event, tauri::WindowEvent::CloseRequested { .. })
                && let Some(state) = window.app_handle().try_state::<AppState>()
            {
                state
                    .vault()
                    .signal()
                    .notify(NativeVaultEvent::WindowClosed);
            }
        })
        .invoke_handler(|invoke| {
            // Window creation must run outside the synchronous Windows WebView callback.
            tauri::async_runtime::spawn_blocking(move || {
                let view = invoke.message.webview();
                let command = invoke.message.command();
                let mut resolver = Some(invoke.resolver);
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
                    commands::dispatch_guarded(
                        &context,
                        command,
                        payload.clone(),
                        &state,
                        |value, check| {
                            if command == "show_vault_window" {
                                if let Some(window) = view.app_handle().get_webview_window("vault")
                                {
                                    window.show().map_err(|_| AppError::Unsupported)?;
                                    window.set_focus().map_err(|_| AppError::Unsupported)?;
                                } else {
                                    let mut builder = WebviewWindowBuilder::new(
                                        view.app_handle(),
                                        "vault",
                                        WebviewUrl::App("index.html?window=vault".into()),
                                    );
                                    if let Some(root) = view.app_handle().try_state::<crate::startup::ObservationRoot>() {
                                        builder = builder.data_directory(root.webview_directory());
                                    }
                                    builder.title("拾序 · 密码库")
                                    .inner_size(640.0, 620.0)
                                    .on_navigation(local_url)
                                    .build()
                                    .map_err(|_| AppError::Unsupported)?;
                                }
                            }
                            // Catch serialization failure before consuming the resolver;
                            // this remains inside the original vault publication guard.
                            let body = tauri::ipc::IpcResponse::body(value)
                                .map_err(|_| AppError::Unsupported)?;
                            check()?; // after actual IpcResponse serialization, before resolver consumption
                            resolver
                                .take()
                                .ok_or(AppError::Disconnected)?
                                .resolve(tauri::ipc::Response::new(body));
                            Ok(())
                        },
                    )
                })();
                if let Err(error) = result
                    && let Some(resolver) = resolver.take()
                {
                    resolver.reject(error.code());
                }
            });
            true
        })
        .build(tauri::generate_context!())
        .expect("desktop runtime failed")
        .run(|app, event| {
            if matches!(event, tauri::RunEvent::Exit) {
                if let Some(state) = app.try_state::<AppState>() {
                    state.vault().shutdown();
                }
                stop_workers(app);
            }
        });
}
