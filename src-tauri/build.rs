fn main() {
    release_identity();
    #[cfg(windows)]
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "backup_previous",
            "backup_list",
            "backup_snapshot",
            "backup_preview",
            "backup_restore",
            "backup_delete",
            "backup_export",
            "backup_import",
            "backup_vault",
            "calendar_create_manual",
            "calendar_details",
            "notification_list",
            "notification_parts",
            "retry_part",
            "settings_read",
            "save_source_config",
            "set_model_consent",
            "set_autostart",
            "calendar_query",
            "calendar_edit",
            "calendar_undo",
            "show_vault_window",
            "vault_create",
            "vault_change_master",
            "vault_copy",
            "vault_unlock",
            "vault_list",
            "vault_apply",
            "vault_reveal",
            "vault_lock",
        ]),
    ))
    .expect("desktop configuration invalid");
}

// Fingerprint the current checkout bytes, including non-ignored new task files.
// HEAD alone would misidentify a dirty development build as its predecessor.
fn release_identity() {
    use sha2::{Digest, Sha256};
    use std::{path::Path, process::Command};
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let output = Command::new("git")
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ])
        .current_dir(root)
        .output()
        .expect("git required for release source identity");
    assert!(output.status.success(), "git source listing failed");
    let mut paths: Vec<_> = output
        .stdout
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .collect();
    paths.sort();
    paths.dedup();
    let mut hash = Sha256::new();
    for path in paths {
        let name = std::str::from_utf8(path).expect("UTF-8 source paths required");
        let bytes = std::fs::read(root.join(name)).expect("listed source file unreadable");
        hash.update((path.len() as u64).to_le_bytes());
        hash.update(path);
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
        println!("cargo:rerun-if-changed={}", root.join(name).display());
    }
    // Detect new non-ignored files and commit changes too; outputs are ignored.
    for directory in [
        "src",
        "src-tauri/src",
        "src-tauri/tests",
        "crates",
        "docs",
        "scripts",
    ] {
        println!("cargo:rerun-if-changed={}", root.join(directory).display());
    }
    for path in [
        ".git/HEAD",
        ".git/refs/heads/codex/shixu-v0.1",
        ".git/index",
    ] {
        println!("cargo:rerun-if-changed={}", root.join(path).display());
    }
    let commit = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(root)
        .output()
        .expect("git HEAD unavailable");
    assert!(commit.status.success(), "git HEAD unavailable");
    println!(
        "cargo:rustc-env=SHIXU_SOURCE_SHA256={}",
        hash.finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    );
    println!(
        "cargo:rustc-env=SHIXU_SOURCE_COMMIT={}",
        String::from_utf8(commit.stdout).unwrap().trim()
    );
    println!(
        "cargo:rustc-env=SHIXU_BUILD_TARGET={}",
        std::env::var("TARGET").unwrap()
    );
    println!(
        "cargo:rustc-env=SHIXU_BUILD_PROFILE={}",
        std::env::var("PROFILE").unwrap()
    );
}
