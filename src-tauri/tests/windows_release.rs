//! Synthetic input documents test the validator; none represents native acceptance.
use serde_json::{Value, json};
use shixu_desktop::release::*;
fn evidence() -> Value {
    let m = compiled_manifest();
    json!({"schema_version":1,"build_id":m.build_id,"source_sha256":m.source_sha256,"artifact_sha256":"a".repeat(64),"target":m.target,"versions":m.versions,"gates":[]})
}
fn gate(id: &str) -> Value {
    let policy = policies().into_iter().find(|p| p.id == id).unwrap();
    json!({"id":id,"status":"PASS","kind":if policy.native {"native"} else {"portable"},"started_at":1000,"finished_at":2000,"coverage":policy.coverage,"metrics":{"messages":100,"duration_ms":28800000,"persist_p95_ms":5000,"rules_p95_ms":10000,"cases":157,"gold":132,"tp":123,"fp":3,"fn":9,"ambiguous_guesses":0,"sourced_events":1,"undone_events":1},"components":[{"name":"synthetic-component","version":"1","sha256":"b".repeat(64),"source":"synthetic-fixture","license":"test-only"}],"logs":[{"sha256":"c".repeat(64),"bytes":123,"exit_code":0}]})
}
fn check(v: &Value) -> ReleaseReport {
    validate(&serde_json::to_vec(v).unwrap(), &"a".repeat(64), 3000)
}
#[test]
fn absent_reports_block_all_required_gates() {
    let r = check(&evidence());
    assert!(!r.blockers.is_empty());
    assert_ne!(r.exit_code(), 0);
    assert_eq!(r.verdict, Verdict::Blocked);
}
#[test]
fn shell_scope_excludes_session_power_but_keeps_remaining_native_gates() {
    let policy = policies()
        .into_iter()
        .find(|p| p.id == "windows_shell")
        .unwrap();
    assert!(!policy.coverage.contains(&"session_lock_suspend_resume"));
    for required in [
        "windows_build_installer",
        "webview_ipc_authorization",
        "tray_exit",
        "autostart_registration",
        "second_process_focus",
    ] {
        assert!(policy.coverage.contains(&required));
    }
    let report = check(&evidence());
    assert_eq!(report.gates.len(), 12);
    assert_eq!(
        report
            .gates
            .iter()
            .find(|g| g.id == "windows_shell")
            .unwrap()
            .verdict,
        Verdict::Blocked
    );
}
#[test]
fn started_is_not_verified() {
    let mut v = evidence();
    let mut g = gate("quality_text");
    g["status"] = json!("STARTED");
    v["gates"] = json!([g]);
    assert_eq!(
        check(&v)
            .gates
            .iter()
            .find(|g| g.id == "quality_text")
            .unwrap()
            .verdict,
        Verdict::Blocked
    );
}
#[test]
fn mock_pass_does_not_clear_windows_gate() {
    let mut v = evidence();
    let mut g = gate("windows_shell");
    g["kind"] = json!("synthetic");
    v["gates"] = json!([g]);
    assert_ne!(check(&v).exit_code(), 0);
}
#[test]
fn current_build_cannot_self_assert_native_implementation() {
    let mut v = evidence();
    v["gates"] = json!(policies().iter().map(|p| gate(p.id)).collect::<Vec<_>>());
    let r = check(&v);
    assert_ne!(r.exit_code(), 0);
    assert!(
        r.gates
            .iter()
            .filter(|g| g.native)
            .all(|g| g.verdict != Verdict::Pass)
    );
}
#[test]
fn real_attachment_unavailable_blocks_coverage_claim() {
    let mut v = evidence();
    let mut g = gate("quality_image");
    g["coverage"] = json!([]);
    v["gates"] = json!([g]);
    assert!(
        check(&v)
            .gates
            .iter()
            .find(|g| g.id == "quality_image")
            .unwrap()
            .reasons
            .iter()
            .any(|s| s == "INCOMPLETE_REQUIRED_COVERAGE")
    );
}
#[test]
fn portable_text_pass_only_its_full_corpus() {
    let mut v = evidence();
    v["gates"] = json!([gate("quality_text")]);
    assert_eq!(
        check(&v)
            .gates
            .iter()
            .find(|g| g.id == "quality_text")
            .unwrap()
            .verdict,
        Verdict::Pass
    );
    v["gates"][0]["metrics"]["gold"] = json!(1);
    assert_ne!(
        check(&v)
            .gates
            .iter()
            .find(|g| g.id == "quality_text")
            .unwrap()
            .verdict,
        Verdict::Pass
    );
}
#[test]
fn malformed_unknown_duplicate_and_oversize_are_rejected() {
    for raw in [
        b"{}".to_vec(),
        b"{\"schema_version\":1,\"schema_version\":1}".to_vec(),
        vec![b' '; MAX_EVIDENCE_BYTES + 1],
        b"null".to_vec(),
    ] {
        assert_eq!(validate(&raw, &"a".repeat(64), 3000).verdict, Verdict::Fail);
    }
    let mut v = evidence();
    v["unknown"] = json!(true);
    assert_eq!(check(&v).verdict, Verdict::Fail);
    let mut v = evidence();
    v["gates"] = json!([gate("quality_text"), gate("quality_text")]);
    assert_eq!(check(&v).verdict, Verdict::Fail);
    let mut v = evidence();
    let mut g = gate("quality_text");
    g["id"] = json!("imaginary");
    v["gates"] = json!([g]);
    assert_eq!(check(&v).verdict, Verdict::Fail);
}
#[test]
fn stale_foreign_build_artifact_versions_and_future_fail_closed() {
    for key in ["build_id", "source_sha256", "artifact_sha256", "target"] {
        let mut v = evidence();
        v[key] = json!("foreign");
        assert_eq!(check(&v).verdict, Verdict::Fail);
    }
    let mut v = evidence();
    v["versions"] = json!({});
    assert_eq!(check(&v).verdict, Verdict::Fail);
    for (start, end, now) in [
        (1000, 2000, MAX_AGE_MS + 2001),
        (1000, 4000, 3000),
        (2000, 1000, 3000),
    ] {
        let mut v = evidence();
        let mut g = gate("quality_text");
        g["started_at"] = json!(start);
        g["finished_at"] = json!(end);
        v["gates"] = json!([g]);
        assert_ne!(
            validate(&serde_json::to_vec(&v).unwrap(), &"a".repeat(64), now).exit_code(),
            0
        );
        assert_ne!(
            validate(&serde_json::to_vec(&v).unwrap(), &"a".repeat(64), now)
                .gates
                .iter()
                .find(|g| g.id == "quality_text")
                .unwrap()
                .verdict,
            Verdict::Pass
        );
    }
}
#[test]
fn whole_flow_has_source_and_undo() {
    let mut v = evidence();
    let mut g = gate("whole_flow");
    g["metrics"]["sourced_events"] = json!(0);
    g["metrics"]["undone_events"] = json!(0);
    v["gates"] = json!([g]);
    assert_ne!(
        check(&v)
            .gates
            .iter()
            .find(|g| g.id == "whole_flow")
            .unwrap()
            .verdict,
        Verdict::Pass
    );
}
#[test]
#[ignore = "BLOCKED: requires genuine Windows engine/QQ/WebView/clipboard/containment and authorized synthetic native workflow"]
fn actual_windows_release_acceptance() {
    panic!(
        "BLOCKED: native production implementations and Windows evidence remain absent; passing portable reports cannot satisfy release acceptance"
    );
}

#[test]
fn executable_manifest_and_missing_report_fail_closed() {
    let executable = env!("CARGO_BIN_EXE_shixu-desktop");
    let output = std::process::Command::new(executable)
        .arg("--release-manifest")
        .output()
        .unwrap();
    assert!(output.status.success());
    let identity: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        identity["manifest"]["source_sha256"],
        compiled_manifest().source_sha256
    );
    assert_eq!(identity["artifact_sha256"].as_str().unwrap().len(), 64);
    let path =
        std::env::temp_dir().join(format!("shixu-release-report-{}.json", std::process::id()));
    let output = std::process::Command::new(executable)
        .args(["--verify-release", "-", path.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let report: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    std::fs::remove_file(path).unwrap();
    assert_eq!(report["artifact_sha256"], identity["artifact_sha256"]);
    assert_eq!(report["verdict"], "BLOCKED");
    assert!(report["blockers"].as_array().unwrap().len() >= 12);
}

#[test]
fn extreme_timestamps_are_blocked_without_panicking() {
    for (start, end) in [
        (i64::MIN, 0),
        (i64::MIN, i64::MAX),
        (0, i64::MAX),
        (i64::MAX, i64::MIN),
    ] {
        let mut v = evidence();
        let mut g = gate("g2_qq");
        g["started_at"] = json!(start);
        g["finished_at"] = json!(end);
        v["gates"] = json!([g]);
        let r = check(&v);
        let gate = r.gates.iter().find(|g| g.id == "g2_qq").unwrap();
        assert_ne!(gate.verdict, Verdict::Pass);
        assert!(gate.reasons.iter().any(|s| s == "STALE_OR_INVALID_TIME"));
    }
}
#[test]
fn strict_nested_keys_counts_status_and_command_evidence() {
    let mut v = evidence();
    v["gates"] = json!([gate("quality_text")]);
    let serialized = serde_json::to_string(&v).unwrap();
    let duplicate = serialized.replace("\"tp\":123", "\"tp\":123,\"tp\":123");
    assert_ne!(duplicate, serialized);
    assert_eq!(
        validate(duplicate.as_bytes(), &"a".repeat(64), 3000).verdict,
        Verdict::Fail
    );
    for (key, value) in [
        ("logs", json!([])),
        ("status", json!("FAIL")),
        ("metrics", json!({})),
        ("coverage", json!([])),
    ] {
        let mut v = v.clone();
        v["gates"][0][key] = value;
        assert_ne!(
            check(&v)
                .gates
                .iter()
                .find(|g| g.id == "quality_text")
                .unwrap()
                .verdict,
            Verdict::Pass
        );
    }
    let mut v = v.clone();
    v["gates"][0]["passed"] = json!(true);
    assert_eq!(check(&v).verdict, Verdict::Fail);
    let mut v = v.clone();
    v["gates"][0]["coverage"] = json!(["full_frozen_text_corpus", "full_frozen_text_corpus"]);
    assert_eq!(check(&v).verdict, Verdict::Fail);
}

#[test]
fn duplicate_log_references_cannot_inflate_evidence() {
    let mut v = evidence();
    let mut g = gate("quality_text");
    g["logs"] = json!([g["logs"][0].clone(), g["logs"][0].clone()]);
    v["gates"] = json!([g]);
    assert_eq!(check(&v).verdict, Verdict::Fail);
}

#[test]
fn stale_automatic_clipboard_cleanup_coverage_is_rejected() {
    let mut v = evidence();
    let mut g = gate("clipboard");
    g["coverage"] = json!([
        "reveal_15_seconds",
        "copy_30_seconds_generation",
        "clipboard_history",
        "idle_5_minutes_background_independent"
    ]);
    v["gates"] = json!([g]);
    assert_eq!(check(&v).verdict, Verdict::Fail);
}
#[test]
fn current_clipboard_coverage_does_not_claim_native_acceptance() {
    let mut v = evidence();
    v["gates"] = json!([gate("clipboard")]);
    let r = check(&v);
    let clipboard = r.gates.iter().find(|g| g.id == "clipboard").unwrap();
    assert_eq!(clipboard.verdict, Verdict::Blocked);
    assert!(
        clipboard
            .reasons
            .iter()
            .any(|s| s == "BUILD_CAPABILITY_BLOCKED: clipboard=written_untested")
    );
}
