//! Direct CLI output-preservation controls use synthetic evidence only.
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "shixu-release-output-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn evidence(&self) -> Vec<u8> {
        let result = Command::new(env!("CARGO_BIN_EXE_shixu-desktop"))
            .arg("--release-manifest")
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(0));
        let identity: Value = serde_json::from_slice(&result.stdout).unwrap();
        let m = &identity["manifest"];
        let bytes = serde_json::to_vec(&json!({"schema_version":1,"build_id":m["build_id"],"source_sha256":m["source_sha256"],"artifact_sha256":identity["artifact_sha256"],"target":m["target"],"versions":m["versions"],"gates":[]})).unwrap();
        fs::write(self.0.join("evidence.json"), &bytes).unwrap();
        bytes
    }
    fn run(&self, evidence: &str, output: &std::path::Path) -> i32 {
        Command::new(env!("CARGO_BIN_EXE_shixu-desktop"))
            .current_dir(&self.0)
            .arg("--verify-release")
            .arg(evidence)
            .arg(output)
            .output()
            .unwrap()
            .status
            .code()
            .unwrap()
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn existing_lexical_and_relative_evidence_aliases_are_preserved() {
    for output in ["evidence.json", "./evidence.json", "child/../evidence.json"] {
        let s = Scratch::new();
        fs::create_dir(s.0.join("child")).unwrap();
        let before = s.evidence();
        assert_eq!(s.run("evidence.json", output.as_ref()), 1, "{output}");
        assert_eq!(fs::read(s.0.join("evidence.json")).unwrap(), before);
    }
}

#[test]
fn unrelated_existing_output_is_preserved() {
    let s = Scratch::new();
    let before = s.evidence();
    fs::write(s.0.join("report.json"), b"original report").unwrap();
    assert_eq!(s.run("evidence.json", "report.json".as_ref()), 1);
    assert_eq!(
        fs::read(s.0.join("report.json")).unwrap(),
        b"original report"
    );
    assert_eq!(fs::read(s.0.join("evidence.json")).unwrap(), before);
}

#[test]
fn fresh_output_records_blocked_and_invalid_evidence_without_modifying_input() {
    let s = Scratch::new();
    let before = s.evidence();
    assert_eq!(s.run("evidence.json", "fresh.json".as_ref()), 2);
    let report: Value = serde_json::from_slice(&fs::read(s.0.join("fresh.json")).unwrap()).unwrap();
    assert_eq!(report["verdict"], "BLOCKED");
    assert_eq!(fs::read(s.0.join("evidence.json")).unwrap(), before);
    fs::write(s.0.join("invalid.json"), b"invalid evidence").unwrap();
    assert_eq!(s.run("invalid.json", "invalid.json".as_ref()), 1);
    assert_eq!(s.run("invalid.json", "./invalid.json".as_ref()), 1);
    assert_eq!(
        fs::read(s.0.join("invalid.json")).unwrap(),
        b"invalid evidence"
    );
    assert_eq!(s.run("invalid.json", "invalid-report.json".as_ref()), 1);
    let report: Value =
        serde_json::from_slice(&fs::read(s.0.join("invalid-report.json")).unwrap()).unwrap();
    assert_eq!(report["verdict"], "FAIL");
    assert_eq!(
        fs::read(s.0.join("invalid.json")).unwrap(),
        b"invalid evidence"
    );
}

#[test]
fn executable_destination_is_preserved() {
    let s = Scratch::new();
    let executable = std::path::Path::new(env!("CARGO_BIN_EXE_shixu-desktop"));
    let before = fs::read(executable).unwrap();
    assert_eq!(s.run("-", executable), 1);
    assert_eq!(fs::read(executable).unwrap(), before);
}

#[cfg(unix)]
#[test]
fn symlink_and_hardlink_evidence_aliases_are_preserved() {
    for symlink in [true, false] {
        let s = Scratch::new();
        let before = s.evidence();
        let output = s.0.join("alias.json");
        if symlink {
            std::os::unix::fs::symlink("evidence.json", &output).unwrap();
        } else {
            fs::hard_link(s.0.join("evidence.json"), &output).unwrap();
        }
        assert_eq!(s.run("evidence.json", &output), 1);
        assert_eq!(fs::read(s.0.join("evidence.json")).unwrap(), before);
        assert_eq!(fs::read(&output).unwrap(), before);
    }
}

#[cfg(unix)]
#[test]
fn dangling_symlink_output_does_not_create_its_target() {
    let s = Scratch::new();
    let before = s.evidence();
    std::os::unix::fs::symlink("missing.json", s.0.join("alias.json")).unwrap();
    assert_eq!(s.run("evidence.json", "alias.json".as_ref()), 1);
    assert!(!s.0.join("missing.json").exists());
    assert!(
        fs::symlink_metadata(s.0.join("alias.json"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read(s.0.join("evidence.json")).unwrap(), before);
}
