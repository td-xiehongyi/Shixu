fn main() {
    #[cfg(windows)]
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "calendar_query",
            "calendar_edit",
            "calendar_undo",
            "show_vault_window",
            "vault_unlock",
            "vault_list",
            "vault_apply",
            "vault_reveal",
            "vault_lock",
        ]),
    ))
    .expect("desktop configuration invalid");
}
