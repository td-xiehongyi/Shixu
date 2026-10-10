//! Actual cached Tauri mapping, with every prepared reviewed runtime hash.
use sha2::{Digest, Sha256};
use std::{collections::HashMap, path::Path};
#[test]
fn tauri_map_preserves_all_198_runtime_keys_and_hashes() {
    let desktop = Path::new(env!("CARGO_MANIFEST_DIR"));
    let config: serde_json::Value =
        serde_json::from_slice(&std::fs::read(desktop.join("tauri.conf.json")).unwrap()).unwrap();
    assert_eq!(config["bundle"]["active"], false);
    assert_eq!(
        config["bundle"]["icon"],
        serde_json::json!(["icons/icon.ico"])
    );
    let configured: HashMap<String, String> =
        serde_json::from_value(config["bundle"]["resources"].clone()).unwrap();
    assert_eq!(
        configured,
        HashMap::from([(
            "../resources/vault-win-x64/".into(),
            "vault-win-x64/".into()
        )])
    );
    let absolute: HashMap<String, String> = configured
        .into_iter()
        .map(|(source, destination)| {
            (
                desktop.join(source).to_str().unwrap().to_owned(),
                destination,
            )
        })
        .collect();
    let manifest: serde_json::Value = serde_json::from_slice(
        &std::fs::read(desktop.join("../vault-helper/resources-win-x64.json")).unwrap(),
    )
    .unwrap();
    let expected = manifest["files"].as_object().unwrap();
    assert_eq!(expected.len(), 198);
    let mut actual = HashMap::new();
    for resource in tauri_utils::resources::ResourcePaths::from_map(&absolute, true).iter() {
        let resource = resource.unwrap();
        let key = resource
            .target()
            .strip_prefix("vault-win-x64")
            .unwrap()
            .to_str()
            .unwrap()
            .replace('\\', "/");
        let digest = Sha256::digest(std::fs::read(resource.path()).unwrap())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        assert!(actual.insert(key, digest).is_none());
    }
    assert_eq!(actual.len(), 198);
    for (key, digest) in actual {
        assert_eq!(expected[&key], digest);
    }
}
