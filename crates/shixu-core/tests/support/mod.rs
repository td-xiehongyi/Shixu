use std::{
    fs,
    path::{Path, PathBuf},
};

pub fn fixture_bytes(name: &str) -> Vec<u8> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
    load_fixture(&root, name).expect("read allowlisted synthetic fixture")
}
pub fn fixture_json(name: &str) -> serde_json::Value {
    serde_json::from_slice(&fixture_bytes(name)).expect("decode synthetic JSON fixture")
}
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
const MAX_FIXTURE_BYTES: u64 = 50 * 1024 * 1024;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureManifest {
    schema_version: shixu_core::contracts::SchemaVersion,
    synthetic_only: bool,
    fixtures: Vec<String>,
}
fn relative_fixture_path(name: &str) -> Result<&Path, &'static str> {
    use std::path::Component;
    let path = Path::new(name);
    if name.is_empty()
        || name.contains(['\\', ':'])
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err("invalid fixture path");
    }
    Ok(path)
}
fn bounded_read(path: &Path, limit: u64) -> Result<Vec<u8>, &'static str> {
    use std::io::Read;
    let metadata = fs::symlink_metadata(path).map_err(|_| "fixture metadata unavailable")?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err("fixture type or size rejected");
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|_| "fixture read failed")?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "fixture read failed")?;
    if bytes.len() as u64 > limit {
        return Err("fixture size rejected");
    }
    Ok(bytes)
}
fn load_fixture(root: &Path, name: &str) -> Result<Vec<u8>, &'static str> {
    let relative = relative_fixture_path(name)?;
    let root_metadata = fs::symlink_metadata(root).map_err(|_| "fixture root unavailable")?;
    if !root_metadata.is_dir() || root_metadata.file_type().is_symlink() {
        return Err("invalid fixture root");
    }
    let root = fs::canonicalize(root).map_err(|_| "fixture root unavailable")?;
    let manifest: FixtureManifest = serde_json::from_slice(&bounded_read(
        &root.join("manifest.json"),
        MAX_MANIFEST_BYTES,
    )?)
    .map_err(|_| "invalid fixture manifest")?;
    if manifest.schema_version != shixu_core::contracts::SchemaVersion::V1
        || !manifest.synthetic_only
    {
        return Err("fixture provenance rejected");
    }
    // Validate the entire allowlist, including entries other than the requested one.
    for entry in &manifest.fixtures {
        relative_fixture_path(entry)?;
    }
    if !manifest.fixtures.iter().any(|entry| entry == name) {
        return Err("fixture is not allowlisted");
    }
    let mut path = root.clone();
    for component in relative.components() {
        path.push(component);
        if fs::symlink_metadata(&path)
            .map_err(|_| "fixture path unavailable")?
            .file_type()
            .is_symlink()
        {
            return Err("fixture symlink rejected");
        }
    }
    if !fs::canonicalize(&path)
        .map_err(|_| "fixture path unavailable")?
        .starts_with(&root)
    {
        return Err("fixture escaped root");
    }
    bounded_read(&path, MAX_FIXTURE_BYTES)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT_ID: AtomicU64 = AtomicU64::new(0);
    struct SyntheticSandbox(PathBuf);
    impl SyntheticSandbox {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "shixu-f0-synthetic-{}-{}",
                std::process::id(),
                NEXT_ID.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            fs::create_dir(path.join("fixtures")).unwrap();
            fs::create_dir(path.join("fixtures/contracts")).unwrap();
            fs::write(path.join("fixtures/contracts/event.json"), b"{}").unwrap();
            fs::write(path.join("fixtures/unlisted.json"), b"{}").unwrap();
            fs::write(path.join("outside-synthetic.json"), b"{}").unwrap();
            let sandbox = Self(path);
            sandbox.manifest(true, "contracts/event.json");
            sandbox
        }
        fn root(&self) -> PathBuf {
            self.0.join("fixtures")
        }
        fn manifest(&self, synthetic_only: bool, fixture: &str) {
            let value = serde_json::json!({"schema_version":1,"synthetic_only":synthetic_only,"fixtures":[fixture]});
            fs::write(
                self.root().join("manifest.json"),
                serde_json::to_vec(&value).unwrap(),
            )
            .unwrap();
        }
    }
    impl Drop for SyntheticSandbox {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn manifest_allowlist_rejects_unlisted_existing_files() {
        let sandbox = SyntheticSandbox::new();
        assert!(load_fixture(&sandbox.root(), "contracts/event.json").is_ok());
        assert!(load_fixture(&sandbox.root(), "unlisted.json").is_err());
    }
    #[test]
    fn manifest_requires_explicit_synthetic_provenance() {
        let sandbox = SyntheticSandbox::new();
        sandbox.manifest(false, "contracts/event.json");
        assert!(load_fixture(&sandbox.root(), "contracts/event.json").is_err());
    }
    #[test]
    fn manifest_cannot_allowlist_traversal_or_absolute_paths() {
        let sandbox = SyntheticSandbox::new();
        for name in [
            "../outside-synthetic.json",
            "contracts/../unlisted.json",
            "contracts\\event.json",
            "/nonexistent-shixu-synthetic-file",
            "C:\\personal.json",
        ] {
            sandbox.manifest(true, name);
            assert!(load_fixture(&sandbox.root(), name).is_err());
        }
    }
    #[cfg(unix)]
    #[test]
    fn allowlisted_symlinks_are_rejected() {
        use std::os::unix::fs::symlink;
        let sandbox = SyntheticSandbox::new();
        let target = sandbox.root().join("contracts/event.json");
        fs::remove_file(&target).unwrap();
        symlink(sandbox.0.join("outside-synthetic.json"), target).unwrap();
        assert!(load_fixture(&sandbox.root(), "contracts/event.json").is_err());
    }
}
