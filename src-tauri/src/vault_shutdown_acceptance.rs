#![cfg(windows)]
use crate::vault::{Operation, VaultController};
use shixu_core::contracts::vault::SecretBytes;
use std::{path::PathBuf, process::Command};

fn powershell(script: &str) -> String {
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .unwrap();
    assert!(output.status.success(), "process metadata query failed");
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

#[test]
#[ignore = "Uses an existing synthetic acceptance fixture and actual contained helper"]
fn real_helper_terminates_when_controller_shuts_down() {
    // The caller supplies only its previous synthetic fixture; never formal storage.
    let work = PathBuf::from(std::env::var("SHIXU_ACCEPTANCE_FIXTURE_ROOT").unwrap());
    assert!(
        work.to_string_lossy()
            .contains("shixu production admission ")
    );
    let manifest: PathBuf = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .components()
        .collect();
    let resources = manifest
        .parent()
        .unwrap()
        .join("resources")
        .join("vault-win-x64");
    // Distinguish fixture admission from shutdown behavior in failure reports.
    let admission =
        shixu_native::vault::engine::KdbxWebEngine::prepared(resources.clone(), work.clone())
            .expect("synthetic fixture must pass native admission");
    drop(admission);
    let controller = VaultController::prepared(resources, work);
    controller
        .execute(Operation::Unlock(SecretBytes::new(
            b"test-only-production-master".to_vec(),
        )))
        .unwrap();
    assert!(controller.is_unlocked().unwrap());
    let children = powershell(&format!(
        "Get-CimInstance Win32_Process -Filter 'ParentProcessId = {}' | Where-Object Name -eq 'node.exe' | ForEach-Object ProcessId",
        std::process::id()
    ));
    let pids: Vec<u32> = children
        .lines()
        .map(|line| line.trim().parse().unwrap())
        .collect();
    assert_eq!(
        pids.len(),
        1,
        "must observe one real live crypto helper before shutdown"
    );
    controller.shutdown();
    assert!(controller.is_unlocked().is_err());
    assert!(controller.execute(Operation::List).is_err());
    let remaining = powershell(&format!(
        "Get-Process -Id {} -ErrorAction SilentlyContinue | ForEach-Object Id; exit 0",
        pids[0]
    ));
    assert!(
        remaining.is_empty(),
        "crypto helper survived controller shutdown"
    );
    println!("live_helper_before=true; helper_after=false; authority_revoked=true");
}
