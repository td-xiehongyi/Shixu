//! Strict operator-evidence validation, not attestation or execution of native probes.
use serde::{
    Deserialize, Serialize,
    de::{self, MapAccess, SeqAccess, Visitor},
};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};
pub const MAX_EVIDENCE_BYTES: usize = 1024 * 1024;
pub const MAX_AGE_MS: i64 = 24 * 60 * 60 * 1000;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Verdict {
    Pass,
    Blocked,
    Fail,
}
#[derive(Clone, Debug, Serialize)]
pub struct BuildManifest {
    pub build_id: String,
    pub source_sha256: String,
    pub source_commit: String,
    pub target: String,
    pub versions: BTreeMap<String, String>,
    pub capabilities: BTreeMap<String, String>,
}
pub struct Policy {
    pub id: &'static str,
    pub native: bool,
    pub capability: &'static str,
    pub coverage: Vec<&'static str>,
}
#[derive(Debug, Serialize)]
pub struct GateResult {
    pub id: String,
    pub native: bool,
    pub verdict: Verdict,
    pub reasons: Vec<String>,
}
#[derive(Debug, Serialize)]
pub struct ReleaseReport {
    pub build_id: String,
    pub versions: BTreeMap<String, String>,
    pub manifest: BuildManifest,
    pub artifact_sha256: String,
    pub verdict: Verdict,
    pub gates: Vec<GateResult>,
    pub unsupported_cases: Vec<String>,
    pub blockers: Vec<String>,
}
impl ReleaseReport {
    pub fn exit_code(&self) -> i32 {
        match self.verdict {
            Verdict::Pass => 0,
            Verdict::Blocked => 2,
            Verdict::Fail => 1,
        }
    }
}
pub fn compiled_manifest() -> BuildManifest {
    let source = env!("SHIXU_SOURCE_SHA256");
    let target = env!("SHIXU_BUILD_TARGET");
    let version = env!("CARGO_PKG_VERSION");
    let versions = BTreeMap::from([
        ("app".into(), version.into()),
        ("tauri".into(), "2.12.2".into()),
        (
            "frontend_lock_sha256".into(),
            hex(&Sha256::digest(include_bytes!("../../pnpm-lock.yaml"))),
        ),
        (
            "cargo_lock_sha256".into(),
            hex(&Sha256::digest(include_bytes!("../../Cargo.lock"))),
        ),
    ]);
    let capabilities = BTreeMap::from([
        (
            "model_provider_optional".into(),
            "optional_off_unimplemented".into(),
        ),
        ("vault_engine".into(), "unimplemented".into()),
        ("qq_wire".into(), "unimplemented".into()),
        ("windows_shell".into(), "written_untested_incomplete".into()),
        (
            "windows_storage".into(),
            "written_untested_incomplete".into(),
        ),
        ("clipboard".into(), "unimplemented".into()),
        ("parser_isolation".into(), "unimplemented".into()),
        ("whole_flow".into(), "unimplemented".into()),
        ("quality_text".into(), "portable_implemented".into()),
        ("quality_image".into(), "quality_open".into()),
        ("quality_pdf".into(), "quality_open".into()),
        ("quality_docx".into(), "quality_open".into()),
        ("quality_xlsx".into(), "quality_open".into()),
    ]);
    let build_id = hex(&Sha256::digest(format!(
        "shixu-release-v1:{source}:{target}:{}:{version}",
        env!("SHIXU_BUILD_PROFILE")
    )));
    BuildManifest {
        build_id,
        source_sha256: source.into(),
        source_commit: env!("SHIXU_SOURCE_COMMIT").into(),
        target: target.into(),
        versions,
        capabilities,
    }
}
pub fn policies() -> Vec<Policy> {
    [
        (
            "g1_vault",
            "vault_engine",
            vec![
                "private_session",
                "uuid_unicode_newline_gui_roundtrip",
                "wrong_password_tamper",
                "atomic_write_conflict_diskfull",
                "change_master_old_backup",
                "vault_backup_restore",
                "session_secret_cleanup",
            ],
        ),
        (
            "g2_qq",
            "qq_wire",
            vec![
                "ordinary_group_live",
                "original_attachments_all_formats",
                "disconnect_login_expiry_reconnect",
                "100_messages_8_hours",
                "persist_rule_latency",
            ],
        ),
        (
            "windows_shell",
            "windows_shell",
            vec![
                "windows_build_installer",
                "webview_ipc_authorization",
                "tray_exit",
                "session_lock_suspend_resume",
                "autostart_registration",
                "second_process_focus",
            ],
        ),
        (
            "windows_storage",
            "windows_storage",
            vec![
                "dpapi_same_user",
                "dpapi_second_identity",
                "private_acl_reparse",
                "file_lock_diskfull_crash_restore",
                "protected_cache_handoff",
            ],
        ),
        (
            "clipboard",
            "clipboard",
            vec![
                "reveal_15_seconds",
                "copy_persists_until_user_overwrite_or_manual_clear",
                "clipboard_history",
                "clipboard_cloud_exclusion",
                "vault_copy_permission",
                "idle_5_minutes_background_independent",
            ],
        ),
        (
            "parser_isolation",
            "parser_isolation",
            vec![
                "job_memory_512_mib",
                "network_vault_denied",
                "image_timeout_30_seconds",
                "file_timeout_120_seconds",
                "kill_on_close_child_limit",
            ],
        ),
        (
            "whole_flow",
            "whole_flow",
            vec![
                "create_lock_fictional_vault",
                "auto_calendar_source_evidence",
                "reschedule_cancel",
                "edit_undo_override",
                "restart_replay_suppression",
                "backup_confirm_restore",
            ],
        ),
        (
            "quality_text",
            "quality_text",
            vec![
                "full_frozen_text_corpus",
                "precision_recall",
                "ambiguous_guesses_zero",
            ],
        ),
        (
            "quality_image",
            "quality_image",
            vec![
                "full_frozen_image_corpus",
                "all_gold_and_unsupported",
                "source_location",
                "precision_recall",
                "ambiguous_guesses_zero",
            ],
        ),
        (
            "quality_pdf",
            "quality_pdf",
            vec![
                "full_frozen_pdf_corpus",
                "all_gold_and_unsupported",
                "source_location",
                "precision_recall",
                "ambiguous_guesses_zero",
            ],
        ),
        (
            "quality_docx",
            "quality_docx",
            vec![
                "full_frozen_docx_corpus",
                "all_gold_and_unsupported",
                "source_location",
                "precision_recall",
                "ambiguous_guesses_zero",
            ],
        ),
        (
            "quality_xlsx",
            "quality_xlsx",
            vec![
                "full_frozen_xlsx_corpus",
                "all_gold_and_unsupported",
                "source_location",
                "precision_recall",
                "ambiguous_guesses_zero",
            ],
        ),
    ]
    .into_iter()
    .map(|(id, capability, coverage)| Policy {
        id,
        native: id != "quality_text",
        capability,
        coverage,
    })
    .collect()
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Evidence {
    schema_version: u32,
    build_id: String,
    source_sha256: String,
    artifact_sha256: String,
    target: String,
    versions: BTreeMap<String, String>,
    gates: Vec<GateEvidence>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GateEvidence {
    id: String,
    status: Status,
    kind: Kind,
    started_at: i64,
    finished_at: i64,
    coverage: Vec<String>,
    metrics: Metrics,
    components: Vec<Component>,
    logs: Vec<Log>,
}
#[derive(Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum Status {
    Pass,
    Fail,
    Blocked,
    Unknown,
    Started,
}
#[derive(Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Kind {
    Native,
    Portable,
    Synthetic,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Metrics {
    messages: Option<u64>,
    duration_ms: Option<u64>,
    persist_p95_ms: Option<u64>,
    rules_p95_ms: Option<u64>,
    cases: Option<u64>,
    gold: Option<u64>,
    tp: Option<u64>,
    fp: Option<u64>,
    #[serde(rename = "fn")]
    missed: Option<u64>,
    ambiguous_guesses: Option<u64>,
    sourced_events: Option<u64>,
    undone_events: Option<u64>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Component {
    name: String,
    version: String,
    sha256: String,
    source: String,
    license: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Log {
    sha256: String,
    bytes: u64,
    exit_code: i32,
}
fn digest(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn label(s: &str) -> bool {
    !s.is_empty() && s.len() <= 256 && !s.chars().any(char::is_control)
}
// serde_json::Value normally discards duplicate map keys. Preserve rejection at
// every nesting level, including versions/metrics, before typed decoding.
struct Unique(serde_json::Value);
impl<'de> Deserialize<'de> for Unique {
    fn deserialize<D: de::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Unique;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("JSON with unique object keys")
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Unique, E> {
                serde_json::Number::from_f64(v)
                    .map(|n| Unique(n.into()))
                    .ok_or_else(|| E::custom("invalid number"))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_string<E: de::Error>(self, v: String) -> Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Unique, E> {
                Ok(Unique(serde_json::Value::Null))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Unique, A::Error> {
                let mut v = vec![];
                while let Some(x) = a.next_element::<Unique>()? {
                    v.push(x.0);
                }
                Ok(Unique(v.into()))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Unique, A::Error> {
                let mut m = serde_json::Map::new();
                while let Some((k, v)) = a.next_entry::<String, Unique>()? {
                    if m.insert(k, v.0).is_some() {
                        return Err(de::Error::custom("duplicate key"));
                    }
                }
                Ok(Unique(m.into()))
            }
        }
        d.deserialize_any(V)
    }
}
fn report(artifact: &str) -> ReleaseReport {
    let m = compiled_manifest();
    ReleaseReport {
        build_id: m.build_id.clone(),
        versions: m.versions.clone(),
        manifest: m,
        artifact_sha256: artifact.into(),
        verdict: Verdict::Blocked,
        gates: vec![],
        unsupported_cases: vec![
            "Real VaultEngine/private session/KDBX backup and native clipboard writer/history/cloud exclusion remain unimplemented; copy must persist until user overwrite or manual clear".into(),
            "QQ wire, isolated parser process and Windows protected cache/handoff are unimplemented".into(),
            "Windows build/installer/WebView/DPAPI/ACL and lifecycle hooks lack genuine acceptance".into(),
            "N4 full 20 image / 20 PDF corpus: image regions29/31 boxes39/47; PDF eligible regions72/73 boxes77/81, plus44 rejected gold regions with0 output. Chinese cases1/2 regions and6/10 boxes; event-quality OPEN".into(),
            "N5 full25 DOCX: chars TP194 FP0 FN179 P100 R52.01; titles8/0/9 P100 R47.06; calendar6/2/11 P75 R35.29. Full25 XLSX: chars196/0/114 P100 R63.23; titles7/0/6 P100 R53.85; calendar4/3/9 P57.14 R30.77. Ambiguous guesses0; unsupported gold retained, quality OPEN".into(),
            "N6 portable text157 cases132 gold TP123 FP3 FN9 P97.619 R93.182 ambiguous guesses0; scope only frozen portable text corpus".into(),
            "Optional cloud model is off/unselected; no mandatory baseline provider gate".into()
        ],
        blockers: vec![],
    }
}
fn reject(mut r: ReleaseReport) -> ReleaseReport {
    r.verdict = Verdict::Fail;
    r.blockers.push("INVALID_EVIDENCE: malformed, duplicate, unknown, oversize or foreign build/artifact/version".into());
    r
}
pub fn validate(raw: &[u8], artifact: &str, now: i64) -> ReleaseReport {
    let mut r = report(artifact);
    if raw.len() > MAX_EVIDENCE_BYTES || !digest(artifact) || now < 0 {
        return reject(r);
    }
    let missing=serde_json::to_vec(&serde_json::json!({"schema_version":1,"build_id":r.build_id,"source_sha256":r.manifest.source_sha256,"artifact_sha256":artifact,"target":r.manifest.target,"versions":r.versions,"gates":[]})).unwrap();
    let raw = if raw.is_empty() {
        missing.as_slice()
    } else {
        raw
    };
    let parsed =
        serde_json::from_slice::<Unique>(raw).and_then(|v| serde_json::from_value::<Evidence>(v.0));
    let e = match parsed {
        Ok(e) => e,
        Err(_) => return reject(r),
    };
    if e.schema_version != 1
        || e.build_id != r.build_id
        || e.source_sha256 != r.manifest.source_sha256
        || e.artifact_sha256 != artifact
        || e.target != r.manifest.target
        || e.versions != r.versions
        || e.gates.len() > policies().len()
    {
        return reject(r);
    }
    let policies = policies();
    let mut ids = BTreeSet::new();
    for g in &e.gates {
        if !ids.insert(&g.id)
            || !policies.iter().any(|p| p.id == g.id)
            || g.coverage.len() > 32
            || g.components.len() > 16
            || g.logs.len() > 32
        {
            return reject(r);
        }
        let p = policies.iter().find(|p| p.id == g.id).unwrap();
        let mut coverage = BTreeSet::new();
        if g.coverage
            .iter()
            .any(|c| !coverage.insert(c.as_str()) || !p.coverage.contains(&c.as_str()))
        {
            return reject(r);
        }
        let mut components = BTreeSet::new();
        let mut logs = BTreeSet::new();
        if g.components.iter().any(|c| {
            !components.insert(&c.name)
                || !label(&c.name)
                || !label(&c.version)
                || !digest(&c.sha256)
                || !label(&c.source)
                || !label(&c.license)
        }) || g.logs.iter().any(|l| {
            !logs.insert(&l.sha256)
                || !digest(&l.sha256)
                || l.bytes == 0
                || l.bytes > 1024 * 1024 * 1024
        }) {
            return reject(r);
        }
    }
    for p in policies {
        let mut reasons = vec![];
        let mut failed = false;
        if let Some(g) = e.gates.iter().find(|g| g.id == p.id) {
            if g.status != Status::Pass {
                reasons.push("NOT_VERIFIED: started/unknown/blocked/failed observation".into());
                failed = g.status == Status::Fail;
            }
            if (p.native && g.kind != Kind::Native) || (!p.native && g.kind != Kind::Portable) {
                reasons.push("EVIDENCE_SCOPE: mock/synthetic/wrong execution scope".into());
            }
            if g.started_at < 0
                || g.finished_at < g.started_at
                || g.finished_at > now
                || now - g.finished_at > MAX_AGE_MS
            {
                reasons.push("STALE_OR_INVALID_TIME".into());
            }
            if g.coverage.len() != p.coverage.len() {
                reasons.push("INCOMPLETE_REQUIRED_COVERAGE".into());
            }
            if g.logs.is_empty()
                || g.logs.iter().any(|l| l.exit_code != 0)
                || (p.native && g.components.is_empty())
            {
                reasons.push("INCOMPLETE_COMPONENT_OR_COMMAND_EVIDENCE".into());
            }
            if p.id == "g2_qq" {
                let m = &g.metrics;
                if m.messages.is_none_or(|n| n < 100)
                    || m.duration_ms.is_none_or(|n| {
                        n < 28_800_000
                            || n > g.finished_at.checked_sub(g.started_at).unwrap_or(-1).max(0)
                                as u64
                    })
                    || m.persist_p95_ms.is_none_or(|n| n > 5000)
                    || m.rules_p95_ms.is_none_or(|n| n > 10000)
                {
                    reasons.push("QQ_COUNT_DURATION_OR_P95_INCOMPLETE".into());
                }
            }
            if p.id == "whole_flow"
                && (g.metrics.sourced_events.is_none_or(|n| n == 0)
                    || g.metrics.undone_events.is_none_or(|n| n == 0))
            {
                reasons.push("SOURCE_AND_UNDO_OBSERVATIONS_REQUIRED".into());
            }
            if p.id.starts_with("quality_") && !quality(&g.metrics, p.id) {
                reasons.push("FULL_CORPUS_QUALITY_INCOMPLETE_OR_FAILED".into());
                failed = true;
            }
        } else {
            reasons.push("MISSING_GATE_EVIDENCE".into());
        }
        if p.native {
            if !r.manifest.target.contains("windows") {
                reasons.push("WINDOWS_BLOCKED: validator build is not Windows".into());
            }
            if r.manifest.capabilities[p.capability] != "native_implemented" {
                reasons.push(format!(
                    "BUILD_CAPABILITY_BLOCKED: {}={}",
                    p.capability, r.manifest.capabilities[p.capability]
                ));
            }
        }
        let verdict = if failed {
            Verdict::Fail
        } else if reasons.is_empty() {
            Verdict::Pass
        } else {
            Verdict::Blocked
        };
        if verdict != Verdict::Pass {
            r.blockers.push(format!("{}: {:?}", p.id, verdict));
        }
        r.gates.push(GateResult {
            id: p.id.into(),
            native: p.native,
            verdict,
            reasons,
        });
    }
    r.verdict = if r.gates.iter().any(|g| g.verdict == Verdict::Fail) {
        Verdict::Fail
    } else if r.blockers.is_empty() {
        Verdict::Pass
    } else {
        Verdict::Blocked
    };
    r
}
fn quality(m: &Metrics, id: &str) -> bool {
    let (Some(cases), Some(gold), Some(tp), Some(fp), Some(missed), Some(guesses)) =
        (m.cases, m.gold, m.tp, m.fp, m.missed, m.ambiguous_guesses)
    else {
        return false;
    };
    let minimum = if id == "quality_text" {
        157
    } else if matches!(id, "quality_docx" | "quality_xlsx") {
        25
    } else {
        20
    };
    if cases < minimum
        || gold == 0
        || [cases, gold, tp, fp, missed]
            .into_iter()
            .any(|n| n > 1_000_000)
        || tp + missed != gold
        || tp + fp == 0
        || guesses != 0
    {
        return false;
    }
    if id == "quality_text" && (cases != 157 || gold != 132 || tp != 123 || fp != 3 || missed != 9)
    {
        return false;
    }
    tp * 100 >= (tp + fp) * 95 && tp * 100 >= gold * 90
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
