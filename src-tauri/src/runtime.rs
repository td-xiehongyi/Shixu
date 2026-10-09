//! Windows-only native adapter. Actual WebView IPC acceptance still requires Windows D7.
use crate::{
    app_state::AppState,
    commands::{self, CallingContext},
};
use shixu_core::contracts::error::AppError;
use std::sync::Arc;
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
            let directory = root.join("calendar");
            // Product storage is only the existing Windows DPAPI/ACL adapter.
            let state = match shixu_native::protection::DpapiProtector::open_database(&directory) {
                Ok(db) => AppState::from_database(Arc::new(db)),
                Err(_) => AppState::default(),
            };
            app.manage(state);
            WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("拾序")
                .inner_size(1487.0, 1058.0)
                .min_inner_size(760.0, 600.0)
                .on_navigation(local_url)
                .build()?;
            Ok(())
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
                    crate::commands::validate_json_size(payload)?;
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
        .run(tauri::generate_context!())
        .expect("desktop runtime failed");
}
