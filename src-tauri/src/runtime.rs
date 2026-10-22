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
            .get_window("main")
            .ok_or(AppError::Unsupported)?
            .hide()
            .map_err(|_| AppError::Unsupported)
    }
    fn focus_main(&self) -> AppResult<()> {
        let w = self.0.get_window("main").ok_or(AppError::Unsupported)?;
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
use tauri::{Manager, WebviewBuilder, WebviewUrl, WindowBuilder};
pub(crate) fn local_url(url: &tauri::Url) -> bool {
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
        Ok(db) => AppState::from_database(Arc::new(db))
            .with_backups(&directory.join("backups"))?
            .with_model(&root)?,
        Err(_) => AppState::default(),
    };
    let vault_root = root.join("vault");
    // Windows vault admission must create its private leaf atomically.
    // Keep normal storage under the application-data root, independent of the
    // EXE-adjacent fictional fixture. Native admission remains fail-closed.
    let resources = app
        .path()
        .resource_dir()
        .map_err(|_| AppError::Unsupported)
        .and_then(|directory| shixu_native::vault::engine::bundled_resources_directory(&directory));
    let state = if let Ok(resources) = resources {
        state.with_vault(resources, vault_root)
    } else {
        state
    };
    Ok(state)
}
pub fn run(mode: crate::startup::Mode) {
    tauri::Builder::default()
        .setup(move |app| {
            let (mut state, observation_root) = crate::startup::initialize(mode, || normal_state(app))?;
            if mode == crate::startup::Mode::Fictional {
                let root = observation_root.as_ref().ok_or(AppError::Unsupported)?;
                state = state.with_fictional_vault(root.resources_directory()?, root.vault_directory());
                eprintln!("SHIXU_VAULT_FICTIONAL production_store=untouched qq=disabled persistence=dedicated-native-vault release=BLOCKED");
            }
            if let Some(root) = observation_root {
                let fictional = root.is_fictional();
                app.manage(root);
                if !fictional {
                    eprintln!("SHIXU_VAULT_OBSERVATION mode=fresh-synthetic-unavailable production_store=untouched");
                }
            }
            if let Ok(supervisor) = state.runtime() {
                if std::env::args().any(|arg| arg == "--login-start") {
                    supervisor.login_start()?;
                } else {
                    supervisor.resume()?;
                }
                if mode == crate::startup::Mode::Normal
                    && std::env::args().any(|arg| arg == "--qq-auto-connect")
                {
                    // Connection failure must remain visible without blocking the app or vault.
                    let _ = state.qq()?.start_auto_connect(std::time::Duration::from_secs(300));
                }
                let workers = supervisor.spawn_workers(WorkerPorts {
                    receiver: Some(state.take_qq_receiver()?),
                    parser: Some(Arc::new(NativeParser)),
                    backup: state.backup().ok(),
                    model: state.model_transport().ok().map(|v| v as Arc<dyn shixu_core::notifications::model::ModelTransport+Send+Sync>),
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
                eprintln!("SHIXU_VAULT_LIFECYCLE deferred={event:?} release=BLOCKED");
                if event == NativeVaultEvent::Revoked {
                    if handle.get_webview("vault").is_some() && handle.emit_to(tauri::EventTarget::webview("vault"), "vault_locked", ()).is_err() { redaction_signal.fail(); }
                    let _=handle.emit_to(tauri::EventTarget::webview("main"), "vault_lock_state", false);
                }
            }))?;
            app.manage(state);
            app.manage(crate::vault_view::VaultViewHost::default());
            install_tray(app)?;
            let fictional = mode == crate::startup::Mode::Fictional;
            let main_url=if fictional { "index.html?fictional=1" }else{ "index.html" };
            let window=WindowBuilder::new(app,"main")
                .title(if fictional { "拾序 · 虚构密码库验收" }else{ "拾序" })
                .inner_size(1487.0,1058.0).min_inner_size(760.0,600.0).build()?;
            let mut builder=WebviewBuilder::new("main",WebviewUrl::App(main_url.into())).on_navigation(local_url).auto_resize();
            if let Some(root)=app.try_state::<crate::startup::ObservationRoot>() { builder=builder.data_directory(root.webview_directory()); }
            window.add_child(builder,tauri::LogicalPosition::new(0.,0.),tauri::LogicalSize::new(1487.,1058.))?;
            eprintln!("SHIXU_VAULT_LIFECYCLE dispatcher=ready session_power=out_of_scope release=BLOCKED");
            Ok(())
        })
        .on_window_event(|window,event| {
            if window.label() != "main" { return; }
            if let tauri::WindowEvent::CloseRequested { api, .. }=event {
                api.prevent_close();
                let app=window.app_handle();
                if app.state::<crate::vault_view::VaultViewHost>().redact_hidden(app).is_err() { app.state::<AppState>().vault().signal().notify(NativeVaultEvent::Revoked); }
                let _=window.hide();
            }
            if matches!(event,tauri::WindowEvent::Resized(_) | tauri::WindowEvent::ScaleFactorChanged {..} | tauri::WindowEvent::Focused(true)) {
                let app=window.app_handle().clone();
                let focused=matches!(event,tauri::WindowEvent::Focused(true));
                tauri::async_runtime::spawn_blocking(move || {
                    if let Some(host)=app.try_state::<crate::vault_view::VaultViewHost>() {
                        let result=if focused {host.resume(&app)}else{host.resize(&app)};
                        if result.is_err() {app.state::<AppState>().vault().signal().notify(NativeVaultEvent::Revoked);}
                    }
                });
            }
            if matches!(event,tauri::WindowEvent::Destroyed) && let Some(state)=window.app_handle().try_state::<AppState>() { state.vault().signal().notify(NativeVaultEvent::WindowClosed); }
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
                            if command == "set_vault_view_visible" {
                                let args:commands::ViewArgs=serde_json::from_value(payload.clone()).map_err(|_|AppError::InvalidInput)?;
                                view.state::<crate::vault_view::VaultViewHost>().set_visible(view.app_handle(),args.visible,crate::wire::revision(&args.revision)?)?;
                            }
                            if command == "ack_vault_view" {
                                let args:commands::ViewAckArgs=serde_json::from_value(payload.clone()).map_err(|_|AppError::InvalidInput)?;
                                let revision=args.revision.as_deref().map(crate::wire::revision).transpose()?;
                                view.state::<crate::vault_view::VaultViewHost>().acknowledge(view.app_handle(),revision,args.failed)?;
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
                if let Ok(unlocked)=view.state::<AppState>().vault().is_unlocked() {
                    let _=view.emit_to(tauri::EventTarget::webview("main"),"vault_lock_state",unlocked);
                }
                if result.is_err() && matches!(command,"set_vault_view_visible" | "ack_vault_view") { view.state::<AppState>().vault().signal().notify(NativeVaultEvent::Revoked); }
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
