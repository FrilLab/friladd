use friladd::registry_v2::{RegistryV2, TARGET_MAP_NAME, TargetMap};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use tempfile::tempdir;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("registry/examples/v2")
        .join(name)
        .join("registry.json")
}

fn asset() -> Value {
    serde_json::from_slice(&fs::read(fixture("assets")).unwrap()).unwrap()
}

fn parse(data: &Value) -> Result<RegistryV2, String> {
    RegistryV2::parse(&data.to_string())
}

#[test]
fn examples_round_trip_and_pin_the_exact_binary_snapshot() {
    let assets = fs::read(fixture("assets")).unwrap();
    let digest = format!("{:x}", Sha256::digest(&assets));
    let components = fs::read_to_string(fixture("components")).unwrap();
    let manifest = RegistryV2::parse(&components).unwrap();
    let item = manifest.select("model-link", Some("1.0.0")).unwrap();
    assert_eq!(
        item.dependencies[0].manifest_sha256.as_deref(),
        Some(digest.as_str())
    );
    assert!(
        manifest
            .select("model-link", Some("2.0.0"))
            .unwrap_err()
            .contains("requested")
    );
    assert!(manifest.select("model-link", Some("^1.0.0")).is_err());
    assert!(manifest.select("missing", None).is_err());

    let composition =
        RegistryV2::parse(&fs::read_to_string(fixture("compositions")).unwrap()).unwrap();
    assert_eq!(composition.items[0].packages.len(), 2);
    for provider in ["assets", "components", "compositions"] {
        let source = fs::read_to_string(fixture(provider)).unwrap();
        let manifest = RegistryV2::parse(&source).unwrap();
        let encoded = serde_json::to_string(&manifest).unwrap();
        assert_eq!(
            serde_json::to_string(&RegistryV2::parse(&encoded).unwrap()).unwrap(),
            encoded
        );
    }
}

#[test]
fn strict_contract_rejects_unsupported_or_unsafe_metadata() {
    for (pointer, value) in [
        ("/schemaVersion", json!(3)),
        ("/id", json!("UPPER")),
        ("/items/0/version", json!("latest")),
        ("/items/0/kind", json!("executable")),
        ("/items/0/license/id", json!("https://invalid/license")),
        ("/items/0/license/notice", json!("unlisted")),
        ("/items/0/files/0/source", json!("https://invalid/script")),
        ("/items/0/files/0/target", json!("nested/.git/hooks")),
        ("/items/0/files/0/size", json!(-1)),
        ("/items/0/files/0/size", json!(104857601)),
        ("/items/0/files/0/sha256", json!("A".repeat(64))),
    ] {
        let mut data = asset();
        *data.pointer_mut(pointer).unwrap() = value;
        assert!(parse(&data).is_err(), "{pointer}");
    }
    let mut data = asset();
    data.as_object_mut().unwrap().remove("id");
    assert!(
        parse(&data)
            .unwrap_err()
            .contains("Invalid registry v2 manifest")
    );
    let mut data = asset();
    data["items"][0]["unknown"] = json!([]);
    assert!(parse(&data).is_err());
    let mut data = asset();
    data["items"][0]["files"][1]["target"] = json!("Public/models/triangle/extra");
    assert!(parse(&data).unwrap_err().contains("collision"));
    let mut data = asset();
    data["items"][0]["tags"] = json!(["same", "same"]);
    assert!(parse(&data).unwrap_err().contains("tags"));
    let mut data = asset();
    data["items"]
        .as_array_mut()
        .unwrap()
        .push(asset()["items"][0].clone());
    assert!(parse(&data).unwrap_err().contains("Duplicate item"));
    let mut data = asset();
    data["items"][0]["files"][1]["source"] = data["items"][0]["files"][0]["source"].clone();
    assert!(parse(&data).unwrap_err().contains("Conflicting metadata"));
}

#[test]
fn dependencies_require_exact_versions_and_valid_provider_scope() {
    let own = json!({"providerId":"example.assets","itemId":"triangle","version":"1.0.0"});
    let mut data = asset();
    data["items"][0]["dependencies"] = json!([own]);
    assert!(parse(&data).unwrap_err().contains("Cycle"));
    data["items"][0]["dependencies"][0]["itemId"] = json!("absent");
    assert!(
        parse(&data)
            .unwrap_err()
            .contains("Missing internal dependency")
    );
    data["items"][0]["dependencies"][0]["itemId"] = json!("triangle");
    data["items"][0]["dependencies"][0]["version"] = json!("2.0.0");
    assert!(parse(&data).unwrap_err().contains("requires version"));
    data["items"][0]["dependencies"][0]["providerId"] = json!("example.external");
    assert!(parse(&data).unwrap_err().contains("exact manifest digest"));
    data["items"][0]["dependencies"][0]["manifestSha256"] = json!("0".repeat(64));
    assert!(parse(&data).is_ok());
    data["items"][0]["dependencies"][0]["providerId"] = json!("example.assets");
    assert!(parse(&data).unwrap_err().contains("enclosing snapshot"));

    let mut data = asset();
    for spec in [
        "latest",
        "file:../../relative",
        "git+https://invalid/package",
        "npm:other",
        "1.0.0 --script",
        "*",
    ] {
        data["items"][0]["packages"] = json!([{"name":"@owner/pkg","spec":spec,"kind":"peer"}]);
        assert!(parse(&data).unwrap_err().contains("Package spec"), "{spec}");
    }
    data["items"][0]["packages"][0]["spec"] = json!("~1.2.3");
    assert!(parse(&data).is_ok());
}

#[test]
fn mapping_preserves_relative_layout_and_refuses_unsafe_paths() {
    let project = tempdir().unwrap();
    let manifest = parse(&asset()).unwrap();
    let item = manifest.select("triangle", None).unwrap();
    assert_eq!(
        TargetMap::load(project.path())
            .unwrap()
            .targets(item, None)
            .unwrap()[0],
        "public/models/triangle/model.glb"
    );
    let file = project.path().join(TARGET_MAP_NAME);
    fs::write(
        &file,
        br#"{"schemaVersion":1,"defaultRoot":"vendor","kindRoots":{"asset":"static"}}"#,
    )
    .unwrap();
    let map = TargetMap::load(project.path()).unwrap();
    let paths = map.check_destinations(project.path(), item, None).unwrap();
    assert_eq!(
        paths,
        [
            "static/public/models/triangle/model.glb",
            "static/public/models/triangle/textures/albedo.png",
            "static/public/models/triangle/NOTICE.txt"
        ]
    );
    assert_eq!(
        map.targets(item, Some("override")).unwrap()[0],
        "override/public/models/triangle/model.glb"
    );
    assert!(map.targets(item, Some("../outside")).is_err());
    assert!(map.targets(item, Some("prefix/.Git")).is_err());
    assert_eq!(fs::read_dir(project.path()).unwrap().count(), 1);
    fs::write(
        &file,
        br#"{"schemaVersion":1,"kindRoots":{"asset":"one","asset":"two"}}"#,
    )
    .unwrap();
    assert!(TargetMap::load(project.path()).is_err());
    fs::write(&file, br#"{"schemaVersion":1,"defaultRoot":"C:/absolute"}"#).unwrap();
    assert!(TargetMap::load(project.path()).is_err());
}

#[cfg(unix)]
#[test]
fn mapping_rejects_symlinked_target_and_configuration() {
    use std::os::unix::fs::symlink;
    let project = tempdir().unwrap();
    let outside = tempdir().unwrap();
    symlink(outside.path(), project.path().join("static")).unwrap();
    let manifest = parse(&asset()).unwrap();
    assert!(
        TargetMap::default()
            .check_destinations(project.path(), &manifest.items[0], Some("static"))
            .unwrap_err()
            .contains("symlink")
    );
    let config = outside.path().join("mapping.json");
    fs::write(&config, br#"{"schemaVersion":1}"#).unwrap();
    symlink(config, project.path().join(TARGET_MAP_NAME)).unwrap();
    assert!(
        TargetMap::load(project.path())
            .unwrap_err()
            .contains("symlink")
    );
}

#[test]
fn cli_reads_provider_fixtures_and_verifies_sizes() {
    let project = tempdir().unwrap();
    let global = tempdir().unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_friladd"))
            .current_dir(project.path())
            .env("FRILADD_CONFIG_DIR", global.path())
            .args(args)
            .output()
            .unwrap()
    };
    for name in ["assets", "components", "compositions"] {
        let manifest = fixture(name);
        let locator = manifest.to_str().unwrap();
        let output = run(&["registry", "verify", "--registry", locator]);
        assert_eq!(output.status.code(), Some(0), "{output:?}");
        assert!(String::from_utf8_lossy(&output.stdout).contains("Manifest SHA-256:"));
        let listed = run(&["list", "--registry", locator]);
        assert_eq!(listed.status.code(), Some(0), "{listed:?}");
        assert!(String::from_utf8_lossy(&listed.stdout).contains("Available items"));
    }
    assert!(fs::read_dir(project.path()).unwrap().next().is_none());
    let manifest = tempdir().unwrap();
    let mut data = asset();
    let payload = b"asset";
    data["items"][0]["files"] = json!([{
        "source":"file.bin","target":"asset.bin","sha256":format!("{:x}", Sha256::digest(payload)),"size":2
    }]);
    data["items"][0]["license"] = Value::Null;
    fs::write(manifest.path().join("file.bin"), payload).unwrap();
    let path = manifest.path().join("registry.json");
    fs::write(&path, data.to_string()).unwrap();
    let output = run(&["registry", "verify", "--registry", path.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("Size mismatch"));
    assert!(output.stdout.is_empty());
    assert!(fs::read_dir(project.path()).unwrap().next().is_none());

    let valid = fixture("assets");
    let locator = valid.to_str().unwrap();
    let registered = run(&["registry", "add", "fixture", locator, "--default"]);
    assert_eq!(registered.status.code(), Some(0), "{registered:?}");
    let listed = run(&["list"]);
    assert_eq!(listed.status.code(), Some(0), "{listed:?}");
    assert!(String::from_utf8_lossy(&listed.stdout).contains("triangle"));
    let add = run(&["add", "fixture/triangle"]);
    assert_eq!(add.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&add.stderr).contains("requires managed add"));
    assert_eq!(fs::read_dir(project.path()).unwrap().count(), 1);
}
