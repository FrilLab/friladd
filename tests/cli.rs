use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
use tempfile::tempdir;

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_friladd")
}

fn run(args: &[&str]) -> Output {
    run_in(Path::new(env!("CARGO_MANIFEST_DIR")), args)
}

fn run_in(directory: &Path, args: &[&str]) -> Output {
    let config = tempdir().unwrap();
    Command::new(binary())
        .env("FRILADD_CONFIG_DIR", config.path())
        .current_dir(directory)
        .args(args)
        .output()
        .expect("run friladd")
}

fn run_with_config(directory: &Path, args: &[&str], config_dir: &Path) -> Output {
    Command::new(binary())
        .env("FRILADD_CONFIG_DIR", config_dir)
        .current_dir(directory)
        .args(args)
        .output()
        .expect("run friladd")
}

fn repo_path(relative: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(relative)
        .display()
        .to_string()
}

fn copy_consumer_fixture(destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    fs::copy(
        repo_path("fixtures/consumer-project/package.json"),
        destination.join("package.json"),
    )
    .unwrap();
    fs::copy(
        repo_path("fixtures/consumer-project/README.md"),
        destination.join("README.md"),
    )
    .unwrap();
}

fn write_lock(project: &Path, targets: &[(&str, &[u8])]) {
    use friladd::lockfile::{Content, Item, Lock, OwnedFile, Provider, v1_provider_id};
    use sha2::{Digest, Sha256};
    let manifest = "https://example.invalid/registry.json";
    let provider_id = v1_provider_id(manifest).unwrap();
    fs::create_dir_all(project).unwrap();
    let files = targets
        .iter()
        .map(|(target, bytes)| {
            let path = project.join(target);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, bytes).unwrap();
            let source = Path::new(target)
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .to_owned();
            let resolved = url::Url::parse(manifest)
                .unwrap()
                .join(&source)
                .unwrap()
                .to_string();
            let content = Content::of(bytes);
            OwnedFile {
                source,
                resolved_source: resolved,
                declared_target: (*target).to_owned(),
                target: (*target).to_owned(),
                expected_sha256: content.sha256.clone(),
                installed: content,
            }
        })
        .collect();
    let lock = Lock {
        schema_version: 1,
        providers: vec![Provider {
            provider_id: provider_id.clone(),
            declared_source: manifest.into(),
            resolved_source: manifest.into(),
            manifest_sha256: format!("{:x}", Sha256::digest(b"manifest")),
        }],
        items: vec![Item {
            provider_id,
            item_id: "sample".into(),
            version: None,
            manifest_source: manifest.into(),
            manifest_sha256: format!("{:x}", Sha256::digest(b"manifest")),
            target_root: None,
            dependencies: Vec::new(),
            files,
        }],
    };
    fs::write(project.join("friladd.lock.json"), lock.encode().unwrap()).unwrap();
}

#[test]
fn help_version_and_invalid_arguments_have_expected_exit_codes() {
    assert!(run(&["--help"]).status.success());
    assert!(String::from_utf8_lossy(&run(&["installed", "--help"]).stdout).contains("--json"));
    assert!(
        String::from_utf8_lossy(&run(&["search", "--help"]).stdout).contains("--all-registries")
    );
    assert!(String::from_utf8_lossy(&run(&["info", "--help"]).stdout).contains("--json"));
    assert!(String::from_utf8_lossy(&run(&["status", "--help"]).stdout).contains("--check"));
    assert!(
        String::from_utf8_lossy(&run(&["--version"]).stdout).contains(env!("CARGO_PKG_VERSION"))
    );
    assert!(!run(&["unknown"]).status.success());
    assert!(
        !run(&["add", "--registry", "registry/registry.json"])
            .status
            .success()
    );
    assert!(
        !run(&["list", "--registry", "/missing/registry.json"])
            .status
            .success()
    );
}

#[test]
fn catalog_search_info_and_json_are_deterministic_read_only_and_provider_neutral() {
    let project = tempdir().unwrap();
    let global = tempdir().unwrap();
    let one = project.path().join("one.json");
    let two = project.path().join("two.json");
    let three = project.path().join("three.json");
    let broken = project.path().join("broken.json");
    let manifest = |registry: &str, description: &str| {
        serde_json::json!({
            "schemaVersion": 1,
            "name": registry,
            "items": [{
                "name": "same",
                "description": description,
                "files": [{"source":"unavailable.bin", "target":"src/unavailable.bin", "sha256":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"}]
            }]
        })
    };
    fs::write(
        &one,
        serde_json::to_vec(&manifest("one", "Needle from global")).unwrap(),
    )
    .unwrap();
    fs::write(
        &two,
        serde_json::to_vec(&manifest("two", "Needle from project")).unwrap(),
    )
    .unwrap();
    fs::write(
        &three,
        serde_json::to_vec(&manifest("three", "Needle \u{1b}[31mfrom another provider")).unwrap(),
    )
    .unwrap();
    fs::write(
        &broken,
        serde_json::to_vec(&manifest("valid", "Not a match")).unwrap(),
    )
    .unwrap();
    let project_config = serde_json::json!({"schemaVersion":1,"defaultRegistry":"shared","registries":{"shared":{"source":two}}});
    fs::write(
        project.path().join(".friladd.json"),
        serde_json::to_vec(&project_config).unwrap(),
    )
    .unwrap();
    let global_config = serde_json::json!({"schemaVersion":1,"defaultRegistry":"global","registries":{"shared":{"source":one},"global":{"source":three},"selected":{"source":broken},"broken":{"source":broken}}});
    fs::write(
        global.path().join(".friladd.json"),
        serde_json::to_vec(&global_config).unwrap(),
    )
    .unwrap();

    let project_path = project.path().display().to_string();
    let no_default_project = tempdir().unwrap();
    let no_default_global = tempdir().unwrap();
    let no_default = run_with_config(
        no_default_project.path(),
        &["search", "needle", "--json"],
        no_default_global.path(),
    );
    assert_eq!(no_default.status.code(), Some(2));
    assert!(no_default.stdout.is_empty());
    assert!(String::from_utf8_lossy(&no_default.stderr).contains("No default registry"));

    let explicit_search = run_with_config(
        project.path(),
        &[
            "search",
            "project",
            "--registry",
            &two.display().to_string(),
            "--json",
        ],
        global.path(),
    );
    assert!(explicit_search.status.success(), "{explicit_search:?}");
    let explicit_json: serde_json::Value = serde_json::from_slice(&explicit_search.stdout).unwrap();
    assert_eq!(explicit_json["data"]["items"][0]["registry"], "two");
    let human_search = run_with_config(
        project.path(),
        &["search", "needle", "--all-registries"],
        global.path(),
    );
    assert!(human_search.status.success());
    assert!(String::from_utf8_lossy(&human_search.stdout).contains("two/same"));
    assert!(!human_search.stdout.contains(&0x1b));
    let human_info = run_with_config(
        project.path(),
        &["info", "same", "--registry", &two.display().to_string()],
        global.path(),
    );
    assert!(human_info.status.success());
    assert!(String::from_utf8_lossy(&human_info.stdout).contains("File: unavailable.bin"));

    let all = run_with_config(
        project.path(),
        &[
            "search",
            "NEEDLE",
            "--all-registries",
            "--json",
            "--project",
            &project_path,
        ],
        global.path(),
    );
    assert_eq!(all.status.code(), Some(0), "{all:?}");
    assert!(all.stderr.is_empty());
    let result: serde_json::Value = serde_json::from_slice(&all.stdout).unwrap();
    assert_eq!(result["schemaVersion"], 1);
    assert_eq!(result["command"], "search");
    assert_eq!(result["data"]["items"].as_array().unwrap().len(), 2);
    assert_eq!(result["data"]["items"][0]["provider"], "three");
    assert_eq!(result["data"]["items"][1]["provider"], "two");
    assert_eq!(result["data"]["items"][0]["id"], "same");

    let default_search = run_with_config(
        project.path(),
        &["search", "project", "--json", "--project", &project_path],
        global.path(),
    );
    assert!(default_search.status.success(), "{default_search:?}");
    let default_json: serde_json::Value = serde_json::from_slice(&default_search.stdout).unwrap();
    assert_eq!(default_json["data"]["items"][0]["registry"], "two");
    let default_list = run_with_config(
        project.path(),
        &["list", "--json", "--project", &project_path],
        global.path(),
    );
    assert!(default_list.status.success(), "{default_list:?}");
    let default_list_json: serde_json::Value =
        serde_json::from_slice(&default_list.stdout).unwrap();
    assert_eq!(default_list_json["data"]["items"][0]["alias"], "shared");
    let selected_list = run_with_config(
        project.path(),
        &["list", "selected", "--json", "--project", &project_path],
        global.path(),
    );
    assert!(selected_list.status.success(), "{selected_list:?}");
    let selected_list_json: serde_json::Value =
        serde_json::from_slice(&selected_list.stdout).unwrap();
    assert_eq!(selected_list_json["data"]["items"][0]["alias"], "selected");
    let no_match = run_with_config(
        project.path(),
        &["search", "absent", "--json", "--project", &project_path],
        global.path(),
    );
    assert!(no_match.status.success());
    let no_match_json: serde_json::Value = serde_json::from_slice(&no_match.stdout).unwrap();
    assert_eq!(no_match_json["data"]["items"].as_array().unwrap().len(), 0);

    let info = run_with_config(
        project.path(),
        &["info", "shared/same", "--json", "--project", &project_path],
        global.path(),
    );
    assert!(info.status.success(), "{info:?}");
    let info_json: serde_json::Value = serde_json::from_slice(&info.stdout).unwrap();
    assert_eq!(info_json["data"]["item"]["provider"], "two");
    assert_eq!(
        info_json["data"]["item"]["manifestSha256"]
            .as_str()
            .unwrap()
            .len(),
        64
    );
    assert_eq!(
        info_json["data"]["item"]["files"][0]["sha256"]
            .as_str()
            .unwrap()
            .len(),
        64
    );

    let empty_query = run_with_config(
        project.path(),
        &["search", "   ", "--project", &project_path],
        global.path(),
    );
    assert_eq!(empty_query.status.code(), Some(2));
    assert!(empty_query.stdout.is_empty());
    let conflict = run_with_config(
        project.path(),
        &["search", "x", "--registry", "shared", "--all-registries"],
        global.path(),
    );
    assert_eq!(conflict.status.code(), Some(2));
    fs::write(&broken, b"not json").unwrap();
    let unavailable = run_with_config(
        project.path(),
        &[
            "search",
            "x",
            "--all-registries",
            "--json",
            "--project",
            &project_path,
        ],
        global.path(),
    );
    assert_eq!(unavailable.status.code(), Some(2));
    assert!(unavailable.stdout.is_empty());
    assert!(String::from_utf8_lossy(&unavailable.stderr).contains("Registry 'broken' failed"));

    let v2_path = repo_path("registry/examples/v2/assets/registry.json");
    let v2_info = run_with_config(
        project.path(),
        &[
            "info",
            "triangle",
            "--registry",
            &v2_path,
            "--json",
            "--project",
            &project_path,
        ],
        global.path(),
    );
    assert!(v2_info.status.success(), "{v2_info:?}");
    let v2_json: serde_json::Value = serde_json::from_slice(&v2_info.stdout).unwrap();
    assert_eq!(v2_json["data"]["item"]["kind"], "asset");
    assert_eq!(v2_json["data"]["item"]["files"][0]["size"], 752);
    assert!(v2_json["data"]["item"]["license"].is_object());

    fs::write(
        &broken,
        serde_json::to_vec(&manifest("valid", "valid")).unwrap(),
    )
    .unwrap();
    let listing = run_with_config(
        project.path(),
        &[
            "list",
            "--all-registries",
            "--json",
            "--project",
            &project_path,
        ],
        global.path(),
    );
    assert!(listing.status.success(), "{listing:?}");
    let listing_json: serde_json::Value = serde_json::from_slice(&listing.stdout).unwrap();
    assert_eq!(listing_json["data"]["items"].as_array().unwrap().len(), 4);
    assert!(
        listing_json["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["alias"] == "selected")
    );
    let before_entries = fs::read_dir(project.path()).unwrap().count();
    fs::write(&broken, b"not json").unwrap();
    let listing = run_with_config(
        project.path(),
        &[
            "list",
            "--all-registries",
            "--json",
            "--project",
            &project_path,
        ],
        global.path(),
    );
    assert_eq!(listing.status.code(), Some(2)); // fail all: the configured broken registry is visible
    assert!(listing.stdout.is_empty());
    assert_eq!(
        fs::read_dir(project.path()).unwrap().count(),
        before_entries
    );
    assert!(!project.path().join("friladd.lock.json").exists());
}

#[test]
fn all_registry_catalog_is_bounded_before_loading_manifests() {
    let project = tempdir().unwrap();
    let global = tempdir().unwrap();
    let mut registries = serde_json::Map::new();
    for index in 0..65 {
        registries.insert(
            format!("r{index:02}"),
            serde_json::json!({"source": project.path().join("missing.json")}),
        );
    }
    fs::write(
        project.path().join(".friladd.json"),
        serde_json::to_vec(&serde_json::json!({"schemaVersion":1,"registries":registries}))
            .unwrap(),
    )
    .unwrap();
    let project_path = project.path().display().to_string();
    let output = run_with_config(
        project.path(),
        &[
            "list",
            "--all-registries",
            "--json",
            "--project",
            &project_path,
        ],
        global.path(),
    );
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("limited to 64"));
}

#[test]
fn doctor_is_offline_read_only_and_reports_warnings_without_failing() {
    use std::net::TcpListener;

    let project = tempdir().unwrap();
    let global = tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let source = format!("https://{}/registry.json", listener.local_addr().unwrap());
    fs::write(
        project.path().join(".friladd.json"),
        serde_json::to_vec(&serde_json::json!({
            "schemaVersion": 1,
            "defaultRegistry": "local",
            "registries": {"local": {"source": source}}
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        project.path().join("package.json"),
        br#"{"name":"consumer"}"#,
    )
    .unwrap();
    let project_path = project.path().display().to_string();
    let before_config = fs::read(project.path().join(".friladd.json")).unwrap();
    let output = run_with_config(
        project.path(),
        &["doctor", "--project", &project_path, "--json"],
        global.path(),
    );
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(output.stderr.is_empty());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schemaVersion"], 1);
    assert_eq!(report["command"], "doctor");
    assert_eq!(report["data"]["defaultRegistry"], "local");
    assert_eq!(report["data"]["summary"]["warn"], 1);
    assert_eq!(report["data"]["summary"]["fail"], 0);
    assert_eq!(
        report["data"]["packageManagers"].as_array().unwrap().len(),
        0
    );
    assert!(
        report["data"]["checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|check| check["code"] == "registry.online" && check["state"] == "skip")
    );
    assert!(
        listener.accept().is_err(),
        "offline doctor contacted the remote registry"
    );
    assert_eq!(
        fs::read(project.path().join(".friladd.json")).unwrap(),
        before_config
    );
    assert!(!project.path().join("friladd.lock.json").exists());
    assert!(!project.path().join(".friladd").exists());

    let online_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let online_source = format!(
        "https://{}/registry.json",
        online_listener.local_addr().unwrap()
    );
    let server = std::thread::spawn(move || {
        if let Ok((stream, _)) = online_listener.accept() {
            drop(stream);
        }
    });
    let online_config = serde_json::to_vec(&serde_json::json!({
        "schemaVersion": 1,
        "defaultRegistry": "local",
        "registries": {"local": {"source": online_source}}
    }))
    .unwrap();
    fs::write(project.path().join(".friladd.json"), &online_config).unwrap();
    let online = run_with_config(
        project.path(),
        &["doctor", "--project", &project_path, "--online", "--json"],
        global.path(),
    );
    assert_eq!(online.status.code(), Some(2));
    assert!(!online.stderr.is_empty());
    let online_report: serde_json::Value = serde_json::from_slice(&online.stdout).unwrap();
    assert_eq!(
        online_report["data"]["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|check| check["code"] == "registry.online.local")
            .unwrap()["state"],
        "fail"
    );
    server.join().unwrap();
    assert_eq!(
        fs::read(project.path().join(".friladd.json")).unwrap(),
        online_config
    );
    assert!(!project.path().join(".friladd").exists());
}

#[test]
fn doctor_reports_corrupt_state_drift_and_interrupted_transactions_without_repairing() {
    let project = tempdir().unwrap();
    let global = tempdir().unwrap();
    write_lock(project.path(), &[("assets/managed.bin", b"expected")]);
    let target = project.path().join("assets/managed.bin");
    fs::write(&target, b"local edit").unwrap();
    let project_path = project.path().display().to_string();
    let lock_bytes = fs::read(project.path().join("friladd.lock.json")).unwrap();
    let edited_bytes = fs::read(&target).unwrap();
    let output = run_with_config(
        project.path(),
        &["doctor", "--project", &project_path, "--json"],
        global.path(),
    );
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["data"]["managed"]["fileStates"]["modified"], 1);
    assert_eq!(report["data"]["summary"]["warn"], 2); // drift and missing default
    assert_eq!(
        fs::read(project.path().join("friladd.lock.json")).unwrap(),
        lock_bytes
    );
    assert_eq!(fs::read(&target).unwrap(), edited_bytes);

    fs::create_dir_all(project.path().join(".friladd/transaction")).unwrap();
    let transaction = project.path().join(".friladd/transaction");
    fs::write(transaction.join("sentinel"), b"preserve me").unwrap();
    let pending = run_with_config(
        project.path(),
        &["doctor", "--project", &project_path, "--json"],
        global.path(),
    );
    assert_eq!(pending.status.code(), Some(2));
    let pending_report: serde_json::Value = serde_json::from_slice(&pending.stdout).unwrap();
    assert_eq!(pending_report["data"]["transactionPending"], true);
    assert_eq!(
        pending_report["data"]["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|check| check["code"] == "transaction.recovery")
            .unwrap()["state"],
        "fail"
    );
    assert_eq!(
        fs::read(transaction.join("sentinel")).unwrap(),
        b"preserve me"
    );
    assert_eq!(fs::read(&target).unwrap(), edited_bytes);

    fs::remove_dir_all(project.path().join(".friladd")).unwrap();
    fs::write(project.path().join("friladd.lock.json"), b"{").unwrap();
    let corrupt = run_with_config(
        project.path(),
        &["doctor", "--project", &project_path, "--json"],
        global.path(),
    );
    assert_eq!(corrupt.status.code(), Some(2));
    let corrupt_report: serde_json::Value = serde_json::from_slice(&corrupt.stdout).unwrap();
    assert_eq!(
        corrupt_report["data"]["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|check| check["code"] == "lock.schema")
            .unwrap()["state"],
        "fail"
    );
    assert_eq!(
        fs::read(project.path().join("friladd.lock.json")).unwrap(),
        b"{"
    );

    fs::write(
        project.path().join("friladd.lock.json"),
        b"{\"schemaVersion\":1,\"providers\":[],\"items\":[]}",
    )
    .unwrap();
    fs::write(project.path().join(".friladd.json"), b"{").unwrap();
    let invalid_config = run_with_config(
        project.path(),
        &["doctor", "--project", &project_path, "--json"],
        global.path(),
    );
    assert_eq!(invalid_config.status.code(), Some(2));
    let config_report: serde_json::Value = serde_json::from_slice(&invalid_config.stdout).unwrap();
    assert_eq!(
        config_report["data"]["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|check| check["code"] == "config.project")
            .unwrap()["state"],
        "fail"
    );
    assert_eq!(
        fs::read(project.path().join(".friladd.json")).unwrap(),
        b"{"
    );
}

#[cfg(unix)]
#[test]
fn doctor_reports_unsafe_owned_symlinks_and_symlink_project_roots() {
    use std::os::unix::fs::symlink;

    let project = tempdir().unwrap();
    let global = tempdir().unwrap();
    let outside = tempdir().unwrap();
    write_lock(project.path(), &[("assets/managed.bin", b"expected")]);
    let target = project.path().join("assets/managed.bin");
    fs::remove_file(&target).unwrap();
    let secret = outside.path().join("secret.bin");
    fs::write(&secret, b"private").unwrap();
    symlink(&secret, &target).unwrap();
    let project_path = project.path().display().to_string();
    let output = run_with_config(
        project.path(),
        &["doctor", "--project", &project_path, "--json"],
        global.path(),
    );
    assert_eq!(output.status.code(), Some(2));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["data"]["managed"]["fileStates"]["unsafe"], 1);
    assert_eq!(fs::read_link(&target).unwrap(), secret);
    assert_eq!(fs::read(&secret).unwrap(), b"private");

    let root = tempdir().unwrap();
    let alias = root.path().join("project-link");
    symlink(project.path(), &alias).unwrap();
    let alias_path = alias.display().to_string();
    let symlink_root = run_with_config(
        root.path(),
        &["doctor", "--project", &alias_path, "--json"],
        global.path(),
    );
    assert_eq!(symlink_root.status.code(), Some(2));
    let root_report: serde_json::Value = serde_json::from_slice(&symlink_root.stdout).unwrap();
    assert_eq!(
        root_report["data"]["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|check| check["code"] == "project.path")
            .unwrap()["state"],
        "fail"
    );
    assert_eq!(fs::read_link(&alias).unwrap(), project.path());
}

#[test]
fn registry_commands_emit_versioned_json_and_verify_without_project_writes() {
    let project = tempdir().unwrap();
    let global = tempdir().unwrap();
    let registry_path = repo_path("registry/registry.json");
    fs::write(
        project.path().join(".friladd.json"),
        serde_json::to_vec(&serde_json::json!({
            "schemaVersion": 1,
            "registries": {
                "local": {"source": registry_path},
                "private": {"source": "https://user:secret@example.test/registry.json?token=hidden"}
            }
        }))
        .unwrap(),
    )
    .unwrap();
    let project_path = project.path().display().to_string();
    let before_config = fs::read(project.path().join(".friladd.json")).unwrap();
    let listing = run_with_config(
        project.path(),
        &["registry", "list", "--project", &project_path, "--json"],
        global.path(),
    );
    assert!(listing.status.success(), "{listing:?}");
    let listing_json: serde_json::Value = serde_json::from_slice(&listing.stdout).unwrap();
    assert_eq!(listing_json["command"], "registry list");
    assert_eq!(listing_json["data"]["registries"][0]["alias"], "local");
    assert_eq!(
        listing_json["data"]["registries"][1]["source"],
        "https://example.test/[redacted]"
    );
    assert!(!String::from_utf8_lossy(&listing.stdout).contains("secret"));
    assert!(!String::from_utf8_lossy(&listing.stdout).contains("hidden"));
    let verify = run_with_config(
        project.path(),
        &[
            "registry",
            "verify",
            "local",
            "--project",
            &project_path,
            "--json",
        ],
        global.path(),
    );
    assert_eq!(verify.status.code(), Some(0), "{verify:?}");
    let verify_json: serde_json::Value = serde_json::from_slice(&verify.stdout).unwrap();
    assert_eq!(verify_json["command"], "registry verify");
    assert!(
        verify_json["data"]["manifestSha256"]
            .as_str()
            .unwrap()
            .len()
            == 64
    );
    assert!(verify_json["data"]["fileCount"].as_u64().unwrap() > 0);
    assert_eq!(
        fs::read(project.path().join(".friladd.json")).unwrap(),
        before_config
    );
    let metadata_only = project.path().join("metadata-only/registry.json");
    fs::create_dir_all(metadata_only.parent().unwrap()).unwrap();
    fs::write(
        &metadata_only,
        serde_json::to_vec(&serde_json::json!({
            "schemaVersion": 1,
            "name": "metadata-only",
            "items": [{"name":"example","description":"Manifest only","files":[{"source":"missing.bin","target":"src/missing.bin","sha256":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"}]}]
        }))
        .unwrap(),
    )
    .unwrap();
    let online_config = serde_json::to_vec(&serde_json::json!({
        "schemaVersion": 1,
        "defaultRegistry": "local",
        "registries": {"local": {"source": metadata_only}}
    }))
    .unwrap();
    fs::write(project.path().join(".friladd.json"), &online_config).unwrap();
    let online_doctor = run_with_config(
        project.path(),
        &["doctor", "--project", &project_path, "--online", "--json"],
        global.path(),
    );
    assert_eq!(online_doctor.status.code(), Some(0), "{online_doctor:?}");
    let doctor_json: serde_json::Value = serde_json::from_slice(&online_doctor.stdout).unwrap();
    assert_eq!(
        doctor_json["data"]["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|check| check["code"] == "registry.online.local")
            .unwrap()["state"],
        "ok"
    );
    assert_eq!(
        fs::read(project.path().join(".friladd.json")).unwrap(),
        online_config
    );
    assert!(!project.path().join("friladd.lock.json").exists());
    assert!(!project.path().join(".friladd").exists());

    fs::write(
        global.path().join(".friladd.json"),
        serde_json::to_vec(&serde_json::json!({
            "schemaVersion": 1,
            "registries": {"global": {"source": "https://example.test/registry.json"}}
        }))
        .unwrap(),
    )
    .unwrap();
    let global_listing = run_with_config(
        project.path(),
        &["registry", "list", "--global", "--json"],
        global.path(),
    );
    assert!(global_listing.status.success(), "{global_listing:?}");
    let global_json: serde_json::Value = serde_json::from_slice(&global_listing.stdout).unwrap();
    assert_eq!(global_json["data"]["registries"][0]["alias"], "global");
    assert_eq!(global_json["data"]["registries"][0]["scope"], "global");
}

#[test]
fn installed_and_status_are_offline_read_only_and_report_drift() {
    let project = tempdir().unwrap();
    write_lock(project.path(), &[("assets/model.bin", b"\0binary\xff")]);
    let path = project.path().display().to_string();
    let before_lock = fs::read(project.path().join("friladd.lock.json")).unwrap();
    let before_file = fs::read(project.path().join("assets/model.bin")).unwrap();
    let before_entries = fs::read_dir(project.path()).unwrap().count();

    let installed = run(&["installed", "--project", &path, "--json"]);
    assert_eq!(installed.status.code(), Some(0), "{installed:?}");
    assert!(installed.stderr.is_empty());
    let installed_json: serde_json::Value = serde_json::from_slice(&installed.stdout).unwrap();
    assert_eq!(installed_json["schemaVersion"], 1);
    assert_eq!(installed_json["command"], "installed");
    assert_eq!(installed_json["data"]["items"][0]["fileCount"], 1);
    let installed_human = run(&["installed", "--project", &path]);
    assert_eq!(installed_human.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&installed_human.stdout).contains("sample"));

    let status = run(&["status", "--project", &path, "--json"]);
    assert_eq!(status.status.code(), Some(0), "{status:?}");
    let status_json: serde_json::Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(
        status_json["data"]["items"][0]["files"][0]["state"],
        "unchanged"
    );
    let status_human = run(&["status", "--project", &path]);
    assert_eq!(status_human.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&status_human.stdout).contains("unchanged"));
    assert_eq!(
        fs::read(project.path().join("friladd.lock.json")).unwrap(),
        before_lock
    );
    assert_eq!(
        fs::read(project.path().join("assets/model.bin")).unwrap(),
        before_file
    );
    assert_eq!(
        fs::read_dir(project.path()).unwrap().count(),
        before_entries
    );
    assert!(!project.path().join(".friladd").exists());

    fs::write(
        project.path().join(".friladd.json"),
        br#"{"schemaVersion":1,"registries":{"remote":{"source":"https://example.invalid/registry.json"}}}"#,
    )
    .unwrap();
    let config_bytes = fs::read(project.path().join(".friladd.json")).unwrap();
    let aliased = run(&["status", "remote/sample", "--project", &path, "--json"]);
    assert_eq!(aliased.status.code(), Some(0), "{aliased:?}");
    let aliased_json: serde_json::Value = serde_json::from_slice(&aliased.stdout).unwrap();
    assert_eq!(
        aliased_json["data"]["items"][0]["files"][0]["state"],
        "unchanged"
    );
    assert_eq!(
        fs::read(project.path().join(".friladd.json")).unwrap(),
        config_bytes
    );
    assert_eq!(
        fs::read(project.path().join("friladd.lock.json")).unwrap(),
        before_lock
    );
    assert!(!project.path().join(".friladd").exists());

    fs::write(project.path().join("assets/model.bin"), b"\0binary\x00").unwrap();
    let drift = run(&["status", "sample", "--project", &path, "--json"]);
    assert_eq!(drift.status.code(), Some(0));
    let drift_json: serde_json::Value = serde_json::from_slice(&drift.stdout).unwrap();
    assert_eq!(
        drift_json["data"]["items"][0]["files"][0]["state"],
        "modified"
    );
    assert_ne!(
        drift_json["data"]["items"][0]["files"][0]["actualSha256"],
        drift_json["data"]["items"][0]["files"][0]["expectedSha256"]
    );
    let checked = run(&["status", "sample", "--project", &path, "--check", "--json"]);
    assert_eq!(checked.status.code(), Some(2));
    assert!(serde_json::from_slice::<serde_json::Value>(&checked.stdout).is_ok());
    assert!(String::from_utf8_lossy(&checked.stderr).contains("drift"));

    fs::remove_file(project.path().join("assets/model.bin")).unwrap();
    let missing = run(&["status", "--project", &path, "--json"]);
    let missing_json: serde_json::Value = serde_json::from_slice(&missing.stdout).unwrap();
    assert_eq!(
        missing_json["data"]["items"][0]["files"][0]["state"],
        "missing"
    );
}

#[test]
fn installed_and_status_handle_empty_corrupt_and_unmanaged_queries() {
    let project = tempdir().unwrap();
    let path = project.path().display().to_string();
    let empty = run(&["installed", "--project", &path, "--json"]);
    assert_eq!(empty.status.code(), Some(0));
    let empty_json: serde_json::Value = serde_json::from_slice(&empty.stdout).unwrap();
    assert!(empty_json["data"]["items"].as_array().unwrap().is_empty());
    let unknown = run(&["status", "not-installed", "--project", &path, "--json"]);
    assert_eq!(unknown.status.code(), Some(0));
    let unknown_json: serde_json::Value = serde_json::from_slice(&unknown.stdout).unwrap();
    assert_eq!(unknown_json["data"]["items"][0]["state"], "unmanaged");
    fs::write(project.path().join("friladd.lock.json"), b"{}").unwrap();
    let corrupt = run(&["installed", "--project", &path, "--json"]);
    assert_eq!(corrupt.status.code(), Some(2));
    assert!(corrupt.stdout.is_empty());
}

#[test]
fn qualified_status_uses_registry_alias_even_when_a_same_named_path_exists() {
    let provider = tempdir().unwrap();
    let project = tempdir().unwrap();
    let manifest = provider.path().join("registry.json");
    fs::write(
        &manifest,
        manifest_with_targets(&["assets/model.bin"]).to_string(),
    )
    .unwrap();
    fs::write(provider.path().join("source.bin"), b"asset").unwrap();
    let manifest = manifest.display().to_string();
    let project_path = project.path().display().to_string();
    let registered = run_in(project.path(), &["registry", "add", "registry", &manifest]);
    assert_eq!(registered.status.code(), Some(0), "{registered:?}");

    // The repository working directory has a `registry/` directory. A qualified
    // alias must still be resolved from project configuration.
    let added = run(&["add", "registry/sample", "--project", &project_path]);
    assert_eq!(added.status.code(), Some(0), "{added:?}");
    let status = run(&[
        "status",
        "registry/sample",
        "--project",
        &project_path,
        "--json",
    ]);
    assert_eq!(status.status.code(), Some(0), "{status:?}");
    let output: serde_json::Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(output["data"]["items"][0]["itemId"], "sample");
    assert_eq!(output["data"]["items"][0]["files"][0]["state"], "unchanged");
}

#[test]
fn qualified_status_keeps_local_lock_ownership_when_registry_is_removed() {
    let provider = tempdir().unwrap();
    let project = tempdir().unwrap();
    let manifest = provider.path().join("registry.json");
    fs::write(
        &manifest,
        manifest_with_targets(&["assets/model.bin"]).to_string(),
    )
    .unwrap();
    fs::write(provider.path().join("source.bin"), b"asset").unwrap();
    let manifest = manifest.display().to_string();
    let registered = run_in(project.path(), &["registry", "add", "fixture", &manifest]);
    assert_eq!(registered.status.code(), Some(0), "{registered:?}");
    let added = run_in(project.path(), &["add", "fixture/sample"]);
    assert_eq!(added.status.code(), Some(0), "{added:?}");
    fs::remove_file(&manifest).unwrap();

    let status = run_in(
        project.path(),
        &[
            "status",
            "fixture/sample",
            "--project",
            project.path().to_str().unwrap(),
            "--json",
        ],
    );
    assert_eq!(status.status.code(), Some(0), "{status:?}");
    let output: serde_json::Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(output["data"]["items"][0]["itemId"], "sample");
    assert_eq!(output["data"]["items"][0]["files"][0]["state"], "unchanged");
}

#[test]
fn qualified_status_matches_declared_manifest_locator_without_following_redirects() {
    use friladd::lockfile::{Lock, v1_provider_id};
    let project = tempdir().unwrap();
    write_lock(project.path(), &[("assets/model.bin", b"asset")]);
    let declared = "https://registry.example.test/v1/registry.json";
    let resolved = "https://cdn.example.test/snapshots/registry.json";
    let lock_path = project.path().join("friladd.lock.json");
    let mut lock = Lock::parse(&fs::read(&lock_path).unwrap()).unwrap();
    let provider_id = v1_provider_id(resolved).unwrap();
    lock.providers[0].provider_id = provider_id.clone();
    lock.providers[0].declared_source = declared.into();
    lock.providers[0].resolved_source = resolved.into();
    lock.items[0].provider_id = provider_id;
    lock.items[0].manifest_source = resolved.into();
    lock.items[0].files[0].resolved_source = url::Url::parse(resolved)
        .unwrap()
        .join("source.bin")
        .unwrap()
        .to_string();
    fs::write(&lock_path, lock.encode().unwrap()).unwrap();
    fs::write(
        project.path().join(".friladd.json"),
        serde_json::json!({
            "schemaVersion": 1,
            "registries": { "upstream": { "source": declared } }
        })
        .to_string(),
    )
    .unwrap();

    let output = run_in(
        project.path(),
        &[
            "status",
            "upstream/sample",
            "--project",
            project.path().to_str().unwrap(),
            "--json",
        ],
    );
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["data"]["items"][0]["itemId"], "sample");
    assert_eq!(json["data"]["items"][0]["files"][0]["state"], "unchanged");
}

#[test]
fn unqualified_duplicate_item_ids_require_a_provider_selector() {
    use friladd::lockfile::{Content, Item, Lock, OwnedFile, Provider, v1_provider_id};
    use sha2::{Digest, Sha256};
    let project = tempdir().unwrap();
    write_lock(project.path(), &[("assets/one.bin", b"one")]);
    let path = project.path().display().to_string();
    let lock_path = project.path().join("friladd.lock.json");
    let mut lock = Lock::parse(&fs::read(&lock_path).unwrap()).unwrap();
    let manifest = "https://other.invalid/registry.json";
    let provider_id = v1_provider_id(manifest).unwrap();
    let content = Content::of(b"two");
    fs::write(project.path().join("assets/two.bin"), b"two").unwrap();
    lock.providers.push(Provider {
        provider_id: provider_id.clone(),
        declared_source: manifest.into(),
        resolved_source: manifest.into(),
        manifest_sha256: format!("{:x}", Sha256::digest(b"other manifest")),
    });
    lock.items.push(Item {
        provider_id,
        item_id: "sample".into(),
        version: None,
        manifest_source: manifest.into(),
        manifest_sha256: format!("{:x}", Sha256::digest(b"other manifest")),
        target_root: None,
        dependencies: Vec::new(),
        files: vec![OwnedFile {
            source: "two.bin".into(),
            resolved_source: url::Url::parse(manifest)
                .unwrap()
                .join("two.bin")
                .unwrap()
                .to_string(),
            declared_target: "assets/two.bin".into(),
            target: "assets/two.bin".into(),
            expected_sha256: content.sha256.clone(),
            installed: content,
        }],
    });
    fs::write(&lock_path, lock.encode().unwrap()).unwrap();
    let ambiguous = run(&["status", "sample", "--project", &path, "--json"]);
    assert_eq!(ambiguous.status.code(), Some(2));
    assert!(ambiguous.stdout.is_empty());
    assert!(String::from_utf8_lossy(&ambiguous.stderr).contains("ambiguous"));
}

#[cfg(unix)]
#[test]
fn status_reports_symlinked_owned_paths_as_unsafe_without_following_them() {
    use std::os::unix::fs::symlink;
    let project = tempdir().unwrap();
    let outside = tempdir().unwrap();
    write_lock(project.path(), &[("assets/model.bin", b"expected")]);
    let path = project.path().display().to_string();
    let target = project.path().join("assets/model.bin");
    fs::remove_file(&target).unwrap();
    fs::write(outside.path().join("secret.bin"), b"outside secret").unwrap();
    symlink(outside.path().join("secret.bin"), &target).unwrap();
    let output = run(&["status", "--project", &path, "--json"]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["data"]["items"][0]["files"][0]["state"], "unsafe");
    assert!(json["data"]["items"][0]["files"][0]["actualSha256"].is_null());
    assert_eq!(
        fs::read_link(target).unwrap(),
        outside.path().join("secret.bin")
    );
}

#[cfg(unix)]
#[test]
fn status_reports_unreadable_owned_files() {
    use std::os::unix::fs::PermissionsExt;
    let project = tempdir().unwrap();
    write_lock(project.path(), &[("assets/locked.bin", b"private")]);
    let path = project.path().display().to_string();
    let target = project.path().join("assets/locked.bin");
    let original = fs::metadata(&target).unwrap().permissions();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o0)).unwrap();
    let output = run(&["status", "--project", &path, "--json"]);
    fs::set_permissions(&target, original).unwrap();
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["data"]["items"][0]["files"][0]["state"], "unreadable");
}

#[test]
fn list_and_unknown_component_work_through_cli() {
    let registry = repo_path("registry/registry.json");
    let output = run(&["list", "--registry", &registry]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("envelope-reveal"));
    assert!(stdout.contains("photo-stack"));

    let unknown = run(&["add", "not-here", "--registry", &registry]);
    assert!(!unknown.status.success());
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("Component not found"));

    let missing_root = tempdir().unwrap();
    let missing_project = missing_root.path().join("not-created");
    let missing = run(&[
        "add",
        "unknown/sample",
        "--project",
        missing_project.to_str().unwrap(),
    ]);
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("not registered"));
}

#[test]
fn dry_run_creates_no_project_files_and_install_is_byte_identical() {
    let registry = repo_path("registry/registry.json");
    let project = tempdir().unwrap();
    copy_consumer_fixture(project.path());
    let before: Vec<_> = fs::read_dir(project.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    let path = project.path().display().to_string();
    let dry_run = run(&[
        "add",
        "envelope-reveal",
        "--registry",
        &registry,
        "--project",
        &path,
        "--dry-run",
    ]);
    assert!(
        dry_run.status.success(),
        "{}",
        String::from_utf8_lossy(&dry_run.stderr)
    );
    let stdout = String::from_utf8(dry_run.stdout).unwrap();
    assert!(stdout.contains("2 file(s) planned."));
    assert!(stdout.contains("Dry run: no project files were changed."));
    assert_eq!(fs::read_dir(project.path()).unwrap().count(), before.len());
    assert!(!project.path().join("src").exists());

    let install = run(&[
        "add",
        "envelope-reveal",
        "--registry",
        &registry,
        "--project",
        &path,
    ]);
    assert!(
        install.status.success(),
        "{}",
        String::from_utf8_lossy(&install.stderr)
    );
    for (source, target) in [
        (
            "registry/fixtures/envelope-reveal/EnvelopeReveal.tsx",
            "src/components/envelope-reveal/EnvelopeReveal.tsx",
        ),
        (
            "registry/fixtures/envelope-reveal/envelope-reveal.css",
            "src/components/envelope-reveal/envelope-reveal.css",
        ),
    ] {
        assert_eq!(
            fs::read(repo_path(source)).unwrap(),
            fs::read(project.path().join(target)).unwrap()
        );
    }
    assert_eq!(
        fs::read(project.path().join("package.json")).unwrap(),
        fs::read(repo_path("fixtures/consumer-project/package.json")).unwrap()
    );
}

#[test]
fn managed_add_records_ownership_is_idempotent_and_refuses_local_edits() {
    let registry = repo_path("registry/registry.json");
    let project = tempdir().unwrap();
    let path = project.path().display().to_string();
    let install = run(&[
        "add",
        "envelope-reveal",
        "--registry",
        &registry,
        "--project",
        &path,
    ]);
    assert!(
        install.status.success(),
        "{}",
        String::from_utf8_lossy(&install.stderr)
    );
    let lock_path = project.path().join("friladd.lock.json");
    let lock_bytes = fs::read(&lock_path).unwrap();
    let lock: serde_json::Value = serde_json::from_slice(&lock_bytes).unwrap();
    assert_eq!(lock["schemaVersion"], 1);
    assert_eq!(lock["items"].as_array().unwrap().len(), 1);
    assert_eq!(lock["items"][0]["files"].as_array().unwrap().len(), 2);

    let repeated = run(&[
        "add",
        "envelope-reveal",
        "--registry",
        &registry,
        "--project",
        &path,
    ]);
    assert!(
        repeated.status.success(),
        "{}",
        String::from_utf8_lossy(&repeated.stderr)
    );
    assert!(String::from_utf8_lossy(&repeated.stdout).contains("no changes"));
    assert_eq!(fs::read(&lock_path).unwrap(), lock_bytes);

    let target = project
        .path()
        .join("src/components/envelope-reveal/EnvelopeReveal.tsx");
    fs::write(&target, b"user edit").unwrap();
    let drifted = run(&[
        "add",
        "envelope-reveal",
        "--registry",
        &registry,
        "--project",
        &path,
    ]);
    assert!(!drifted.status.success());
    assert!(String::from_utf8_lossy(&drifted.stderr).contains("status"));
    assert_eq!(fs::read(&target).unwrap(), b"user edit");
    assert_eq!(fs::read(&lock_path).unwrap(), lock_bytes);
    fs::remove_file(&target).unwrap();
    let missing = run(&[
        "add",
        "envelope-reveal",
        "--registry",
        &registry,
        "--project",
        &path,
    ]);
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("restore"));
    assert!(!target.exists());
    assert_eq!(fs::read(&lock_path).unwrap(), lock_bytes);
}

#[test]
fn project_defaults_to_the_current_working_directory() {
    let registry = repo_path("registry/registry.json");
    let project = tempdir().unwrap();
    let output = run_in(
        project.path(),
        &["add", "envelope-reveal", "--registry", &registry],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        project
            .path()
            .join("src/components/envelope-reveal/EnvelopeReveal.tsx")
            .is_file()
    );
}

#[test]
fn existing_targets_are_reported_and_never_overwritten() {
    let registry = repo_path("registry/registry.json");
    let project = tempdir().unwrap();
    let target = project
        .path()
        .join("src/components/envelope-reveal/EnvelopeReveal.tsx");
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(&target, b"existing").unwrap();
    let path = project.path().display().to_string();
    let output = run(&[
        "add",
        "envelope-reveal",
        "--registry",
        &registry,
        "--project",
        &path,
        "--dry-run",
    ]);
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("! src/components/envelope-reveal/EnvelopeReveal.tsx (already exists)")
    );
    assert_eq!(fs::read(target).unwrap(), b"existing");
}

#[test]
fn project_registries_defaults_and_qualified_aliases_install_correct_bytes() {
    use sha2::{Digest, Sha256};
    let root = tempdir().unwrap();
    let project = root.path().join("consumer");
    fs::create_dir_all(&project).unwrap();
    let make_registry = |dir: &Path, item: &str, bytes: &[u8]| {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join("component.txt"), bytes).unwrap();
        let hash = Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let json = format!(
            r#"{{"schemaVersion":1,"name":"fixture","items":[{{"name":"{item}","description":"fixture","files":[{{"source":"component.txt","target":"out/{item}.txt","sha256":"{hash}"}}]}}]}}"#
        );
        fs::write(dir.join("registry.json"), json).unwrap();
    };
    let a = root.path().join("registry-a");
    let b = root.path().join("registry-b");
    make_registry(&a, "button", b"from-a");
    make_registry(&b, "envelope-reveal", b"from-b");
    let pa = a.join("registry.json").display().to_string();
    let pb = b.join("registry.json").display().to_string();
    let add_a = run_in(
        &project,
        &["registry", "add", "registry-a", &pa, "--default"],
    );
    assert!(
        add_a.status.success(),
        "{}",
        String::from_utf8_lossy(&add_a.stderr)
    );
    let add_b = run_in(&project, &["registry", "add", "registry-b", &pb]);
    assert!(
        add_b.status.success(),
        "{}",
        String::from_utf8_lossy(&add_b.stderr)
    );
    let install_a = run_in(&project, &["add", "button"]);
    assert!(
        install_a.status.success(),
        "{}",
        String::from_utf8_lossy(&install_a.stderr)
    );
    assert_eq!(fs::read(project.join("out/button.txt")).unwrap(), b"from-a");
    let install_b = run_in(&project, &["add", "registry-b/envelope-reveal"]);
    assert!(
        install_b.status.success(),
        "{}",
        String::from_utf8_lossy(&install_b.stderr)
    );
    assert_eq!(
        fs::read(project.join("out/envelope-reveal.txt")).unwrap(),
        b"from-b"
    );
    assert!(
        run_in(&project, &["registry", "default", "registry-b"])
            .status
            .success()
    );
    let explicit_project = tempdir().unwrap();
    let explicit_project_path = explicit_project.path().display().to_string();
    let explicit = run_in(
        &project,
        &[
            "add",
            "button",
            "--registry",
            &pa,
            "--project",
            &explicit_project_path,
            "--dry-run",
        ],
    );
    assert!(
        explicit.status.success(),
        "{}",
        String::from_utf8_lossy(&explicit.stderr)
    );
}

#[test]
fn corrupt_project_configuration_blocks_install_without_writing_files() {
    let project = tempdir().unwrap();
    fs::write(project.path().join(".friladd.json"), "not json").unwrap();
    let output = run_in(project.path(), &["add", "unknown"]);
    assert!(!output.status.success());
    assert!(!project.path().join("out").exists());
}

#[test]
fn global_registration_uses_isolated_overridden_config_directory() {
    use sha2::{Digest, Sha256};
    let root = tempdir().unwrap();
    let project = root.path().join("consumer");
    fs::create_dir_all(&project).unwrap();
    let registry_dir = root.path().join("global-registry");
    fs::create_dir_all(&registry_dir).unwrap();
    let contents = b"global bytes";
    fs::write(registry_dir.join("component.txt"), contents).unwrap();
    let hash = Sha256::digest(contents)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    fs::write(registry_dir.join("registry.json"), format!(r#"{{"schemaVersion":1,"name":"global-fixture","items":[{{"name":"sample","description":"fixture","files":[{{"source":"component.txt","target":"sample.txt","sha256":"{hash}"}}]}}]}}"#)).unwrap();
    let config_dir = root.path().join("isolated-config");
    let source = registry_dir.join("registry.json").display().to_string();
    let add = std::process::Command::new(binary())
        .env("FRILADD_CONFIG_DIR", &config_dir)
        .current_dir(&project)
        .args([
            "registry",
            "add",
            "global-fixture",
            &source,
            "--global",
            "--default",
        ])
        .output()
        .unwrap();
    assert!(
        add.status.success(),
        "{}",
        String::from_utf8_lossy(&add.stderr)
    );
    let install = std::process::Command::new(binary())
        .env("FRILADD_CONFIG_DIR", &config_dir)
        .current_dir(&project)
        .args(["add", "sample"])
        .output()
        .unwrap();
    assert!(
        install.status.success(),
        "{}",
        String::from_utf8_lossy(&install.stderr)
    );
    assert_eq!(fs::read(project.join("sample.txt")).unwrap(), contents);
    assert!(!project.join(".friladd.json").exists());
}

#[test]
fn bare_registry_json_path_remains_a_local_manifest() {
    let root = tempdir().unwrap();
    fs::copy(
        repo_path("registry/registry.json"),
        root.path().join("registry.json"),
    )
    .unwrap();
    fs::create_dir_all(root.path().join("fixtures/envelope-reveal")).unwrap();
    fs::copy(
        repo_path("registry/fixtures/envelope-reveal/EnvelopeReveal.tsx"),
        root.path()
            .join("fixtures/envelope-reveal/EnvelopeReveal.tsx"),
    )
    .unwrap();
    fs::copy(
        repo_path("registry/fixtures/envelope-reveal/envelope-reveal.css"),
        root.path()
            .join("fixtures/envelope-reveal/envelope-reveal.css"),
    )
    .unwrap();
    let listed = run_in(root.path(), &["list", "--registry", "registry.json"]);
    assert!(
        listed.status.success(),
        "{}",
        String::from_utf8_lossy(&listed.stderr)
    );
    let project = root.path().join("consumer");
    fs::create_dir(&project).unwrap();
    let project = project.display().to_string();
    let installed = run_in(
        root.path(),
        &[
            "add",
            "envelope-reveal",
            "--registry",
            "registry.json",
            "--project",
            &project,
        ],
    );
    assert!(
        installed.status.success(),
        "{}",
        String::from_utf8_lossy(&installed.stderr)
    );
    assert_eq!(
        fs::read(Path::new(&project).join("src/components/envelope-reveal/EnvelopeReveal.tsx"))
            .unwrap(),
        fs::read(repo_path(
            "registry/fixtures/envelope-reveal/EnvelopeReveal.tsx"
        ))
        .unwrap()
    );
}

#[test]
fn registry_list_global_ignores_corrupt_project_config_and_marks_scope() {
    let root = tempdir().unwrap();
    let project = root.path().join("project");
    fs::create_dir(&project).unwrap();
    let global = root.path().join("global");
    let registry = repo_path("registry/registry.json");
    let add = std::process::Command::new(binary())
        .env("FRILADD_CONFIG_DIR", &global)
        .current_dir(&project)
        .args(["registry", "add", "global-one", &registry, "--global"])
        .output()
        .unwrap();
    assert!(
        add.status.success(),
        "{}",
        String::from_utf8_lossy(&add.stderr)
    );
    fs::write(project.join(".friladd.json"), "broken").unwrap();
    let listed = std::process::Command::new(binary())
        .env("FRILADD_CONFIG_DIR", &global)
        .current_dir(&project)
        .args(["registry", "list", "--global"])
        .output()
        .unwrap();
    assert!(
        listed.status.success(),
        "{}",
        String::from_utf8_lossy(&listed.stderr)
    );
    assert!(String::from_utf8_lossy(&listed.stdout).contains("\tglobal"));
}

#[test]
fn registry_verify_and_add_preserve_binary_assets_and_detect_tampering() {
    use sha2::{Digest, Sha256};

    let root = tempdir().unwrap();
    let provider = root.path().join("provider");
    let consumer = root.path().join("consumer");
    fs::create_dir(&provider).unwrap();
    fs::create_dir(&consumer).unwrap();
    // Opaque binary bytes (not a renderable model) test the transport contract.
    let bytes = b"glTF\x00\xff\x80\r\n";
    let hash = Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    fs::write(provider.join("model.glb"), bytes).unwrap();
    fs::write(
        provider.join("registry.json"),
        serde_json::json!({
            "schemaVersion": 1,
            "name": "assets",
            "items": [{
                "name": "model",
                "description": "Binary fixture",
                "files": [{
                    "source": "model.glb",
                    "target": "public/assets/model.glb",
                    "sha256": hash
                }]
            }]
        })
        .to_string(),
    )
    .unwrap();

    let manifest = provider.join("registry.json");
    let manifest = manifest.to_str().unwrap();
    let verified = run_in(&consumer, &["registry", "verify", "--registry", manifest]);
    assert!(
        verified.status.success(),
        "{}",
        String::from_utf8_lossy(&verified.stderr)
    );
    assert!(
        String::from_utf8_lossy(&verified.stdout)
            .contains("Verified registry assets: 1 item(s), 1 file(s).")
    );
    assert_eq!(fs::read_dir(&consumer).unwrap().count(), 0);

    let added = run_in(&consumer, &["add", "model", "--registry", manifest]);
    assert!(
        added.status.success(),
        "{}",
        String::from_utf8_lossy(&added.stderr)
    );
    assert_eq!(
        fs::read(consumer.join("public/assets/model.glb")).unwrap(),
        bytes
    );

    fs::write(provider.join("model.glb"), b"tampered").unwrap();
    let rejected = run_in(&consumer, &["registry", "verify", "--registry", manifest]);
    assert!(!rejected.status.success());
    assert!(
        String::from_utf8_lossy(&rejected.stderr).contains("SHA-256 mismatch"),
        "{rejected:?}"
    );
    fs::remove_file(provider.join("model.glb")).unwrap();
    let missing = run_in(&consumer, &["registry", "verify", "--registry", manifest]);
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("model.glb"));
    assert_eq!(
        fs::read(consumer.join("public/assets/model.glb")).unwrap(),
        bytes
    );
}

#[test]
fn registry_verify_resolves_registered_alias_and_default() {
    let project = tempdir().unwrap();
    let manifest = repo_path("registry/registry.json");
    let registered = run_in(
        project.path(),
        &["registry", "add", "fixture", &manifest, "--default"],
    );
    assert!(
        registered.status.success(),
        "{}",
        String::from_utf8_lossy(&registered.stderr)
    );
    for args in [
        vec!["registry", "verify"],
        vec!["registry", "verify", "fixture"],
    ] {
        let verified = run_in(project.path(), &args);
        assert!(
            verified.status.success(),
            "{}",
            String::from_utf8_lossy(&verified.stderr)
        );
    }
    assert_eq!(fs::read_dir(project.path()).unwrap().count(), 1);
}

fn assert_cli_error(output: Output, message: &str) {
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains(message), "{stderr}");
}

#[test]
fn subcommand_help_and_ambiguous_flags_follow_the_contract() {
    let root = tempdir().unwrap();
    for command in [
        &["list"][..],
        &["add"],
        &["registry"],
        &["registry", "add"],
        &["registry", "default"],
        &["registry", "remove"],
        &["registry", "list"],
        &["registry", "verify"],
    ] {
        let args = [command, &["--help"]].concat();
        let output = run_in(root.path(), &args);
        assert_eq!(output.status.code(), Some(0), "{output:?}");
        assert!(output.stderr.is_empty(), "{output:?}");
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(stdout.contains("Usage: friladd"), "{stdout}");
        assert!(stdout.contains("Example: friladd"), "{stdout}");
        if *command != ["registry"] {
            assert!(stdout.contains("--project"), "{stdout}");
        }
    }
    for command in [
        &["list", "one", "--registry", "two"][..],
        &["registry", "verify", "one", "--registry", "two"],
        &["add", "one/thing", "--registry", "two"],
        &["registry", "list", "--global", "--project", "."],
    ] {
        assert_cli_error(run_in(root.path(), command), "cannot be");
    }
    assert_cli_error(
        run_in(root.path(), &["list", "--bogus"]),
        "unexpected argument",
    );
    assert_cli_error(
        run_in(root.path(), &["registry", "remove", "a", "b"]),
        "unexpected argument",
    );
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn explicit_project_registration_validates_paths_from_the_invocation_directory() {
    let root = tempdir().unwrap();
    let consumer = root.path().join("consumer");
    let provider = root.path().join("provider");
    fs::create_dir(&consumer).unwrap();
    fs::create_dir(&provider).unwrap();
    fs::write(
        provider.join("manifest.json"),
        r#"{"schemaVersion":1,"name":"remote-independent","items":[{"name":"z","description":"Z","files":[{"source":"missing","target":"z","sha256":"0000000000000000000000000000000000000000000000000000000000000000"}]},{"name":"a","description":"A","files":[{"source":"missing","target":"a","sha256":"0000000000000000000000000000000000000000000000000000000000000000"}]}]}"#,
    ).unwrap();

    let register = run_in(
        root.path(),
        &[
            "registry",
            "add",
            "demo",
            "./provider/manifest.json",
            "--project",
            "consumer",
        ],
    );
    assert_eq!(register.status.code(), Some(0), "{register:?}");
    assert!(!root.path().join(".friladd.json").exists());
    let config_path = consumer.join(".friladd.json");
    let original = fs::read(&config_path).unwrap();
    let config: serde_json::Value = serde_json::from_slice(&original).unwrap();
    assert_eq!(
        Path::new(config["registries"]["demo"]["source"].as_str().unwrap()),
        Path::new("..").join("provider").join("manifest.json")
    );
    assert_eq!(fs::read_dir(&consumer).unwrap().count(), 1);
    let listed = run_in(root.path(), &["list", "demo", "--project", "consumer"]);
    assert_eq!(listed.status.code(), Some(0), "{listed:?}");
    assert_eq!(
        String::from_utf8(listed.stdout).unwrap(),
        "Available items (remote-independent):\n- a: A\n- z: Z\n"
    );

    assert_cli_error(
        run_in(
            root.path(),
            &[
                "registry",
                "add",
                "demo",
                "./not-there",
                "--project",
                "consumer",
            ],
        ),
        "already exists",
    );
    assert_eq!(fs::read(&config_path).unwrap(), original);
    let set = run_in(
        root.path(),
        &["registry", "default", "demo", "--project", "consumer"],
    );
    assert_eq!(set.status.code(), Some(0), "{set:?}");
    assert_cli_error(
        run_in(
            root.path(),
            &["registry", "verify", "--project", "consumer"],
        ),
        "missing",
    );

    let removed = run_in(
        root.path(),
        &["registry", "remove", "demo", "--project", "consumer"],
    );
    assert_eq!(removed.status.code(), Some(0), "{removed:?}");
    let config: serde_json::Value =
        serde_json::from_slice(&fs::read(config_path).unwrap()).unwrap();
    assert_eq!(
        config,
        serde_json::json!({"schemaVersion": 1, "registries": {}})
    );
    assert_cli_error(
        run_in(root.path(), &["list", "--project", "consumer"]),
        "No default registry",
    );
    assert_eq!(fs::read_dir(&consumer).unwrap().count(), 1);
}

#[test]
fn malformed_and_ambiguous_configs_fail_without_changes() {
    let root = tempdir().unwrap();
    let path = root.path().join(".friladd.json");
    for (config, expected) in [
        ("{", "Invalid configuration"),
        (r#"{"schemaVersion":2}"#, "Unsupported"),
        (
            r#"{"schemaVersion":1,"extra":true}"#,
            "Invalid configuration",
        ),
        (
            r#"{"schemaVersion":1,"defaultRegistry":"missing"}"#,
            "is not registered",
        ),
        (
            r#"{"schemaVersion":1,"registries":{"same":{"source":"one"},"same":{"source":"two"}}}"#,
            "duplicate registry alias",
        ),
    ] {
        fs::write(&path, config).unwrap();
        assert_cli_error(run_in(root.path(), &["registry", "list"]), expected);
        assert_eq!(fs::read(&path).unwrap(), config.as_bytes());
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }
    fs::write(&path, " ".repeat(1024 * 1024 + 1)).unwrap();
    assert_cli_error(run_in(root.path(), &["registry", "list"]), "byte limit");
    assert_eq!(fs::metadata(path).unwrap().len(), 1024 * 1024 + 1);
}

fn manifest_with_targets(targets: &[&str]) -> serde_json::Value {
    use sha2::{Digest, Sha256};
    let hash = format!("{:x}", Sha256::digest(b"asset"));
    serde_json::json!({
        "schemaVersion":1,
        "name":"test",
        "items":[{
            "name":"sample",
            "description":"Test asset",
            "files":targets.iter().map(|target| serde_json::json!({
                "source":"source.bin", "target":target, "sha256":hash
            })).collect::<Vec<_>>()
        }]
    })
}

#[test]
fn malformed_registry_and_unsafe_targets_never_write_even_during_install() {
    let provider = tempdir().unwrap();
    let project = tempdir().unwrap();
    let manifest = provider.path().join("registry.json");
    let source = manifest.to_str().unwrap();
    fs::write(provider.path().join("source.bin"), b"asset").unwrap();
    for (targets, expected) in [
        (&["../outside"][..], "Invalid target"),
        (&["café/file"], "Invalid target"),
        (&["LPT9.txt"], "Invalid target"),
        (&["src./file"], "Invalid target"),
        (&["src/file:stream"], "Invalid target"),
        (&[".GIT/config"], "Reserved target"),
        (&["src/.friladd.json"], "Reserved target"),
        (&["friladd.lock.json"], "Reserved target"),
        (&["src/.friladd-journal/entry"], "Reserved target"),
        (&["src/File", "src/file"], "collision"),
        (
            &["src/File", "src/file-other", "src/file/child"],
            "collision",
        ),
        (&["src/File/child", "src/file"], "collision"),
        (&["src/Dir/one", "src/dir/two"], "collision"),
    ] {
        fs::write(&manifest, manifest_with_targets(targets).to_string()).unwrap();
        for operation in [
            &["add", "sample", "--registry", source][..],
            &["add", "sample", "--registry", source, "--dry-run"],
            &["registry", "verify", "--registry", source],
        ] {
            assert_cli_error(run_in(project.path(), operation), expected);
            assert_eq!(fs::read_dir(project.path()).unwrap().count(), 0);
        }
    }
    for (contents, expected) in [
        ("{", "Invalid registry manifest"),
        (
            r#"{"schemaVersion":99,"name":"test","items":[]}"#,
            "Unsupported",
        ),
        (
            r#"{"schemaVersion":1,"name":"test","items":[],"extra":true}"#,
            "Invalid registry manifest",
        ),
    ] {
        fs::write(&manifest, contents).unwrap();
        assert_cli_error(
            run_in(project.path(), &["registry", "add", "demo", source]),
            expected,
        );
        assert_eq!(fs::read_dir(project.path()).unwrap().count(), 0);
    }
}

#[test]
fn dry_run_requires_verified_content_and_keeps_the_project_empty() {
    let provider = tempdir().unwrap();
    let project = tempdir().unwrap();
    let manifest = provider.path().join("registry.json");
    let source = manifest.to_str().unwrap();
    fs::write(
        &manifest,
        manifest_with_targets(&["nested/asset", "nested/copy"]).to_string(),
    )
    .unwrap();
    let check = || {
        run_in(
            project.path(),
            &["add", "sample", "--registry", source, "--dry-run"],
        )
    };
    let missing = check();
    assert_eq!(missing.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("source.bin"));
    fs::write(provider.path().join("source.bin"), b"changed").unwrap();
    let changed = check();
    assert_eq!(changed.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&changed.stderr).contains("SHA-256 mismatch"));
    fs::write(provider.path().join("source.bin"), b"asset").unwrap();
    assert_eq!(check().status.code(), Some(0));
    assert_eq!(fs::read_dir(project.path()).unwrap().count(), 0);
    let install = run_in(project.path(), &["add", "sample", "--registry", source]);
    assert_eq!(install.status.code(), Some(0), "{install:?}");
    for target in ["nested/asset", "nested/copy"] {
        assert_eq!(fs::read(project.path().join(target)).unwrap(), b"asset");
    }
}

#[test]
fn add_json_reports_dry_run_install_and_unchanged_without_human_output() {
    let provider = tempdir().unwrap();
    let project = tempdir().unwrap();
    let manifest = provider.path().join("registry.json");
    let source = manifest.to_str().unwrap();
    fs::write(
        &manifest,
        manifest_with_targets(&["nested/asset"]).to_string(),
    )
    .unwrap();
    fs::write(provider.path().join("source.bin"), b"asset").unwrap();

    let dry_run = run_in(
        project.path(),
        &["add", "sample", "--registry", source, "--dry-run", "--json"],
    );
    assert_eq!(dry_run.status.code(), Some(0), "{dry_run:?}");
    assert!(dry_run.stderr.is_empty());
    let output: serde_json::Value = serde_json::from_slice(&dry_run.stdout).unwrap();
    assert_eq!(output["command"], "add");
    assert_eq!(output["data"]["dryRun"], true);
    assert_eq!(output["data"]["files"][0]["action"], "install");
    assert!(output["data"]["files"][0]["ownerBefore"].is_null());
    assert_eq!(output["data"]["files"][0]["ownerAfter"]["itemId"], "sample");
    assert_eq!(fs::read_dir(project.path()).unwrap().count(), 0);

    let install = run_in(
        project.path(),
        &["add", "sample", "--registry", source, "--json"],
    );
    assert_eq!(install.status.code(), Some(0), "{install:?}");
    assert!(install.stderr.is_empty());
    let output: serde_json::Value = serde_json::from_slice(&install.stdout).unwrap();
    assert_eq!(output["data"]["dryRun"], false);
    assert_eq!(output["data"]["files"][0]["action"], "install");
    assert_eq!(
        fs::read(project.path().join("nested/asset")).unwrap(),
        b"asset"
    );

    let lock_before_noop = fs::read(project.path().join("friladd.lock.json")).unwrap();
    let unchanged_dry_run = run_in(
        project.path(),
        &["add", "sample", "--registry", source, "--dry-run", "--json"],
    );
    assert_eq!(
        unchanged_dry_run.status.code(),
        Some(0),
        "{unchanged_dry_run:?}"
    );
    assert!(unchanged_dry_run.stderr.is_empty());
    let output: serde_json::Value = serde_json::from_slice(&unchanged_dry_run.stdout).unwrap();
    assert_eq!(output["data"]["dryRun"], true);
    assert_eq!(output["data"]["files"][0]["action"], "unchanged");
    assert_eq!(
        fs::read(project.path().join("friladd.lock.json")).unwrap(),
        lock_before_noop
    );

    let unchanged = run_in(
        project.path(),
        &["add", "sample", "--registry", source, "--json"],
    );
    assert_eq!(unchanged.status.code(), Some(0), "{unchanged:?}");
    assert!(unchanged.stderr.is_empty());
    let output: serde_json::Value = serde_json::from_slice(&unchanged.stdout).unwrap();
    assert_eq!(output["data"]["files"][0]["action"], "unchanged");
    assert_eq!(
        output["data"]["files"][0]["ownerBefore"]["itemId"],
        "sample"
    );
}

#[test]
fn remote_source_diagnostics_do_not_disclose_credentials() {
    let root = tempdir().unwrap();
    let fixture = r#"{"schemaVersion":1,"registries":{"a":{"source":"https://username:password@example.invalid/token-path?key=private-query#fragment-secret"}}}"#;
    fs::write(root.path().join(".friladd.json"), fixture).unwrap();
    let list = run_in(root.path(), &["registry", "list"]);
    assert_eq!(list.status.code(), Some(0), "{list:?}");
    assert_eq!(
        list.stdout,
        b"a\thttps://example.invalid/[redacted]\tproject\n"
    );
    let error = run_in(root.path(), &["list", "a"]);
    let stderr = String::from_utf8_lossy(&error.stderr);
    for secret in [
        "username",
        "password",
        "token-path",
        "private-query",
        "fragment-secret",
    ] {
        assert!(!stderr.contains(secret), "{stderr}");
    }
    assert_cli_error(error, "HTTPS URL without embedded credentials");
    assert_eq!(
        fs::read(root.path().join(".friladd.json")).unwrap(),
        fixture.as_bytes()
    );
}

#[cfg(unix)]
#[test]
fn rejects_symlink_config_manifest_source_and_destination() {
    use std::os::unix::fs::symlink;
    let provider = tempdir().unwrap();
    let project = tempdir().unwrap();
    let root = tempdir().unwrap();
    let manifest = provider.path().join("registry.json");
    fs::write(
        &manifest,
        manifest_with_targets(&["nested/asset"]).to_string(),
    )
    .unwrap();
    let original = root.path().join("keep");
    fs::write(&original, b"asset").unwrap();
    symlink(&original, project.path().join(".friladd.json")).unwrap();
    assert_cli_error(
        run_in(project.path(), &["registry", "list"]),
        "non-symlink file",
    );

    let project_link = root.path().join("project");
    symlink(project.path(), &project_link).unwrap();
    let project_link_with_slash = format!("{}/", project_link.display());
    assert_cli_error(
        run_in(
            root.path(),
            &["registry", "list", "--project", &project_link_with_slash],
        ),
        "non-symlink directory",
    );

    let linked_manifest = root.path().join("manifest.json");
    symlink(&manifest, &linked_manifest).unwrap();
    assert_cli_error(
        run_in(
            project.path(),
            &["list", "--registry", linked_manifest.to_str().unwrap()],
        ),
        "non-symlink file",
    );

    let source = provider.path().join("source.bin");
    symlink(&original, &source).unwrap();
    assert_cli_error(
        run_in(
            project.path(),
            &[
                "registry",
                "verify",
                "--registry",
                manifest.to_str().unwrap(),
            ],
        ),
        "symlink",
    );
    fs::remove_file(&source).unwrap();
    fs::write(source, b"asset").unwrap();

    symlink(root.path(), project.path().join("nested")).unwrap();
    assert_cli_error(
        run_in(
            project.path(),
            &["add", "sample", "--registry", manifest.to_str().unwrap()],
        ),
        "symlink",
    );
    assert_eq!(fs::read(original).unwrap(), b"asset");
    assert!(!root.path().join("asset").exists());
}

#[test]
fn conflicting_hashes_for_a_repeated_source_fail_before_writes() {
    let provider = tempdir().unwrap();
    let project = tempdir().unwrap();
    let manifest = provider.path().join("registry.json");
    let source = manifest.to_str().unwrap();
    let mut data = manifest_with_targets(&["assets/one", "assets/two"]);
    data["items"][0]["files"][1]["sha256"] = serde_json::Value::String("0".repeat(64));
    fs::write(&manifest, data.to_string()).unwrap();
    fs::write(provider.path().join("source.bin"), b"asset").unwrap();

    assert_cli_error(
        run_in(
            project.path(),
            &["registry", "verify", "--registry", source],
        ),
        "Conflicting SHA-256 digests",
    );
    let install = run_in(project.path(), &["add", "sample", "--registry", source]);
    assert_eq!(install.status.code(), Some(2), "{install:?}");
    assert!(String::from_utf8_lossy(&install.stderr).contains("Conflicting SHA-256 digests"));
    assert_eq!(fs::read_dir(project.path()).unwrap().count(), 0);
}

#[test]
fn update_plans_replaces_additions_and_obsolete_files_in_one_transaction() {
    let provider = tempdir().unwrap();
    let project = tempdir().unwrap();
    let manifest = provider.path().join("registry.json");
    let source = manifest.to_str().unwrap().to_owned();
    fs::write(provider.path().join("old.bin"), b"old").unwrap();
    fs::write(provider.path().join("stale.bin"), b"stale").unwrap();
    fs::write(
        &manifest,
        v1_manifest(&[
            ("old.bin", "assets/changed.bin", b"old"),
            ("stale.bin", "assets/obsolete.bin", b"stale"),
        ]),
    )
    .unwrap();
    let added = run_in(project.path(), &["add", "sample", "--registry", &source]);
    assert_eq!(added.status.code(), Some(0), "{added:?}");
    let lock_path = project.path().join("friladd.lock.json");
    let lock_before = fs::read(&lock_path).unwrap();

    fs::write(provider.path().join("new.bin"), b"new bytes").unwrap();
    fs::write(provider.path().join("added.bin"), b"added bytes").unwrap();
    fs::write(
        &manifest,
        v1_manifest(&[
            ("new.bin", "assets/changed.bin", b"new bytes"),
            ("added.bin", "assets/new.bin", b"added bytes"),
        ]),
    )
    .unwrap();
    let path = project.path().to_str().unwrap();
    let dry_run = run_in(
        project.path(),
        &[
            "update",
            "sample",
            "--registry",
            &source,
            "--project",
            path,
            "--dry-run",
            "--json",
        ],
    );
    assert_eq!(dry_run.status.code(), Some(0), "{dry_run:?}");
    assert!(dry_run.stderr.is_empty());
    let plan: serde_json::Value = serde_json::from_slice(&dry_run.stdout).unwrap();
    assert_eq!(plan["command"], "update");
    assert_eq!(plan["data"]["dryRun"], true);
    let actions = &plan["data"]["items"][0]["actions"];
    assert!(
        actions
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["action"] == "replace")
    );
    assert!(
        actions
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["action"] == "install")
    );
    assert!(
        actions
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["action"] == "remove")
    );
    assert_eq!(fs::read(&lock_path).unwrap(), lock_before);
    assert_eq!(
        fs::read(project.path().join("assets/changed.bin")).unwrap(),
        b"old"
    );
    assert!(!project.path().join("assets/new.bin").exists());

    let updated = run_in(
        project.path(),
        &[
            "update",
            "sample",
            "--registry",
            &source,
            "--project",
            path,
            "--json",
        ],
    );
    assert_eq!(updated.status.code(), Some(0), "{updated:?}");
    let output: serde_json::Value = serde_json::from_slice(&updated.stdout).unwrap();
    assert_eq!(output["data"]["dryRun"], false);
    assert_eq!(
        fs::read(project.path().join("assets/changed.bin")).unwrap(),
        b"new bytes"
    );
    assert_eq!(
        fs::read(project.path().join("assets/new.bin")).unwrap(),
        b"added bytes"
    );
    assert!(!project.path().join("assets/obsolete.bin").exists());
    let lock = friladd::lockfile::Lock::parse(&fs::read(&lock_path).unwrap()).unwrap();
    assert_eq!(lock.items[0].files.len(), 2);
    let installed_lock = fs::read(&lock_path).unwrap();
    let repeated = run_in(
        project.path(),
        &[
            "update",
            "sample",
            "--registry",
            &source,
            "--project",
            path,
            "--json",
        ],
    );
    assert_eq!(repeated.status.code(), Some(0), "{repeated:?}");
    let repeated_json: serde_json::Value = serde_json::from_slice(&repeated.stdout).unwrap();
    assert_eq!(
        repeated_json["data"]["items"][0]["actions"][0]["action"],
        "unchanged"
    );
    assert_eq!(fs::read(&lock_path).unwrap(), installed_lock);
}

#[test]
fn update_refuses_edits_provider_changes_and_implicit_candidate_selection() {
    let provider = tempdir().unwrap();
    let other = tempdir().unwrap();
    let project = tempdir().unwrap();
    let manifest = provider.path().join("registry.json");
    let other_manifest = other.path().join("registry.json");
    let source = manifest.to_str().unwrap().to_owned();
    fs::write(provider.path().join("source.bin"), b"asset").unwrap();
    fs::write(
        &manifest,
        manifest_with_targets(&["assets/model.bin"]).to_string(),
    )
    .unwrap();
    fs::write(other.path().join("source.bin"), b"asset").unwrap();
    fs::write(
        &other_manifest,
        manifest_with_targets(&["assets/model.bin"]).to_string(),
    )
    .unwrap();
    let added = run_in(project.path(), &["add", "sample", "--registry", &source]);
    assert_eq!(added.status.code(), Some(0), "{added:?}");
    let path = project.path().to_str().unwrap();
    let no_candidate = run_in(project.path(), &["update", "sample", "--project", path]);
    assert_eq!(no_candidate.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&no_candidate.stderr).contains("--registry"));

    let changed_provider = run_in(
        project.path(),
        &[
            "update",
            "sample",
            "--registry",
            other_manifest.to_str().unwrap(),
            "--project",
            path,
        ],
    );
    assert_eq!(changed_provider.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&changed_provider.stderr).contains("refusing provider switch"));

    fs::write(project.path().join("assets/model.bin"), b"user edit").unwrap();
    let before = fs::read(project.path().join("assets/model.bin")).unwrap();
    let edited = run_in(
        project.path(),
        &["update", "sample", "--registry", &source, "--project", path],
    );
    assert_eq!(edited.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&edited.stderr).contains("was modified"));
    assert_eq!(
        fs::read(project.path().join("assets/model.bin")).unwrap(),
        before
    );
}

#[test]
fn remove_is_transactional_and_allows_safe_missing_file_cleanup() {
    let provider = tempdir().unwrap();
    let project = tempdir().unwrap();
    let manifest = provider.path().join("registry.json");
    let source = manifest.to_str().unwrap().to_owned();
    fs::write(provider.path().join("source.bin"), b"asset").unwrap();
    fs::write(
        &manifest,
        manifest_with_targets(&["assets/model.bin"]).to_string(),
    )
    .unwrap();
    let added = run_in(project.path(), &["add", "sample", "--registry", &source]);
    assert_eq!(added.status.code(), Some(0), "{added:?}");
    let path = project.path().to_str().unwrap();
    let lock_path = project.path().join("friladd.lock.json");
    let lock_before = fs::read(&lock_path).unwrap();
    let dry_run = run_in(
        project.path(),
        &["remove", "sample", "--project", path, "--dry-run", "--json"],
    );
    assert_eq!(dry_run.status.code(), Some(0), "{dry_run:?}");
    let report: serde_json::Value = serde_json::from_slice(&dry_run.stdout).unwrap();
    assert_eq!(report["data"]["items"][0]["actions"][0]["action"], "remove");
    assert_eq!(fs::read(&lock_path).unwrap(), lock_before);
    assert!(project.path().join("assets/model.bin").exists());

    fs::write(project.path().join("assets/model.bin"), b"user edit").unwrap();
    let edited_before = fs::read(project.path().join("assets/model.bin")).unwrap();
    let edited = run_in(project.path(), &["remove", "sample", "--project", path]);
    assert_eq!(edited.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&edited.stderr).contains("was modified"));
    assert_eq!(
        fs::read(project.path().join("assets/model.bin")).unwrap(),
        edited_before
    );

    fs::remove_file(project.path().join("assets/model.bin")).unwrap();
    let removed = run_in(
        project.path(),
        &["remove", "sample", "--project", path, "--json"],
    );
    assert_eq!(removed.status.code(), Some(0), "{removed:?}");
    let report: serde_json::Value = serde_json::from_slice(&removed.stdout).unwrap();
    assert_eq!(
        report["data"]["items"][0]["actions"][0]["action"],
        "already-missing"
    );
    let lock = friladd::lockfile::Lock::parse(&fs::read(&lock_path).unwrap()).unwrap();
    assert!(lock.items.is_empty());
    assert!(lock.providers.is_empty());
}

#[test]
fn restore_recreates_exact_locked_bytes_and_refuses_changed_source() {
    use sha2::Digest;
    let provider = tempdir().unwrap();
    let project = tempdir().unwrap();
    let manifest = provider.path().join("registry.json");
    let source = manifest.to_str().unwrap().to_owned();
    fs::write(provider.path().join("source.bin"), b"\0exact\xffbytes").unwrap();
    let hash = format!("{:x}", sha2::Sha256::digest(b"\0exact\xffbytes"));
    fs::write(
        &manifest,
        serde_json::json!({
            "schemaVersion": 1,
            "name": "test",
            "items": [{
                "name": "sample", "description": "test",
                "files": [{"source":"source.bin","target":"assets/model.bin","sha256":hash}]
            }]
        })
        .to_string(),
    )
    .unwrap();
    let added = run_in(project.path(), &["add", "sample", "--registry", &source]);
    assert_eq!(added.status.code(), Some(0), "{added:?}");
    let path = project.path().to_str().unwrap();
    let lock_path = project.path().join("friladd.lock.json");
    let lock_before = fs::read(&lock_path).unwrap();
    fs::remove_file(project.path().join("assets/model.bin")).unwrap();
    let dry_run = run_in(
        project.path(),
        &["restore", "--project", path, "--dry-run", "--json"],
    );
    assert_eq!(dry_run.status.code(), Some(0), "{dry_run:?}");
    let report: serde_json::Value = serde_json::from_slice(&dry_run.stdout).unwrap();
    assert_eq!(
        report["data"]["items"][0]["actions"][0]["action"],
        "restore"
    );
    assert!(!project.path().join("assets/model.bin").exists());
    assert_eq!(fs::read(&lock_path).unwrap(), lock_before);

    let restored = run_in(project.path(), &["restore", "--project", path, "--json"]);
    assert_eq!(restored.status.code(), Some(0), "{restored:?}");
    assert_eq!(
        fs::read(project.path().join("assets/model.bin")).unwrap(),
        b"\0exact\xffbytes"
    );
    assert_eq!(fs::read(&lock_path).unwrap(), lock_before);

    fs::write(project.path().join("assets/model.bin"), b"user edit").unwrap();
    let edited_before = fs::read(project.path().join("assets/model.bin")).unwrap();
    let edited = run_in(project.path(), &["restore", "--project", path]);
    assert_eq!(edited.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&edited.stderr).contains("never overwrites edits"));
    assert_eq!(
        fs::read(project.path().join("assets/model.bin")).unwrap(),
        edited_before
    );

    fs::write(project.path().join("assets/model.bin"), b"\0exact\xffbytes").unwrap();
    fs::remove_file(project.path().join("assets/model.bin")).unwrap();
    fs::write(provider.path().join("source.bin"), b"different bytes").unwrap();
    let rejected = run_in(project.path(), &["restore", "--project", path]);
    assert_eq!(rejected.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&rejected.stderr).contains("Size mismatch for locked artifact"),
        "{rejected:?}"
    );
    assert!(!project.path().join("assets/model.bin").exists());
}

#[test]
fn update_requires_explicit_v2_downgrade_confirmation() {
    use sha2::Digest;
    let provider = tempdir().unwrap();
    let project = tempdir().unwrap();
    let manifest = provider.path().join("registry-v2.json");
    let source = manifest.to_str().unwrap().to_owned();
    fs::write(provider.path().join("asset.bin"), b"v2!").unwrap();
    let hash = format!("{:x}", sha2::Sha256::digest(b"v2!"));
    let write_manifest = |version: &str| {
        fs::write(
            &manifest,
            serde_json::json!({
                "schemaVersion": 2,
                "id": "org.example.assets",
                "name": "assets",
                "items": [{
                    "id": "sample", "version": version, "kind": "asset",
                    "files": [{"source":"asset.bin","target":"assets/model.bin","sha256":hash,"size":3}]
                }]
            })
            .to_string(),
        )
        .unwrap();
    };
    write_manifest("2.0.0");
    let added = run_in(project.path(), &["add", "sample", "--registry", &source]);
    assert_eq!(added.status.code(), Some(0), "{added:?}");
    write_manifest("1.0.0");
    let path = project.path().to_str().unwrap();
    let denied = run_in(
        project.path(),
        &["update", "sample", "--registry", &source, "--project", path],
    );
    assert_eq!(denied.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&denied.stderr).contains("--allow-downgrade"));
    let allowed = run_in(
        project.path(),
        &[
            "update",
            "sample",
            "--registry",
            &source,
            "--project",
            path,
            "--allow-downgrade",
            "--dry-run",
            "--json",
        ],
    );
    assert_eq!(allowed.status.code(), Some(0), "{allowed:?}");
    assert_eq!(
        fs::read(project.path().join("assets/model.bin")).unwrap(),
        b"v2!"
    );
}

#[test]
fn update_preserves_repeated_source_target_declarations() {
    use sha2::Digest;
    let provider = tempdir().unwrap();
    let project = tempdir().unwrap();
    let manifest = provider.path().join("registry.json");
    let source = manifest.to_str().unwrap().to_owned();
    fs::write(provider.path().join("shared.bin"), b"same bytes").unwrap();
    let hash = format!("{:x}", sha2::Sha256::digest(b"same bytes"));
    fs::write(
        &manifest,
        serde_json::json!({
            "schemaVersion": 1,
            "name": "test",
            "items": [{
                "name": "sample", "description": "test",
                "files": [
                    {"source":"shared.bin","target":"first.bin","sha256":hash},
                    {"source":"shared.bin","target":"nested/second.bin","sha256":hash}
                ]
            }]
        })
        .to_string(),
    )
    .unwrap();
    let add = run_in(project.path(), &["add", "sample", "--registry", &source]);
    assert_eq!(add.status.code(), Some(0), "{add:?}");
    let before = fs::read(project.path().join("friladd.lock.json")).unwrap();
    let update = run_in(
        project.path(),
        &["update", "sample", "--registry", &source, "--json"],
    );
    assert_eq!(update.status.code(), Some(0), "{update:?}");
    assert_eq!(
        fs::read(project.path().join("first.bin")).unwrap(),
        b"same bytes"
    );
    assert_eq!(
        fs::read(project.path().join("nested/second.bin")).unwrap(),
        b"same bytes"
    );
    assert_eq!(
        fs::read(project.path().join("friladd.lock.json")).unwrap(),
        before
    );
    let report: serde_json::Value = serde_json::from_slice(&update.stdout).unwrap();
    assert_eq!(
        report["data"]["items"][0]["actions"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn v2_large_binary_streams_through_verified_staging_and_v1_cap_is_preserved() {
    use sha2::Digest;
    let provider = tempdir().unwrap();
    let project = tempdir().unwrap();
    let data: Vec<u8> = (0..12 * 1024 * 1024)
        .map(|index| ((index * 31) % 251) as u8)
        .collect();
    let texture: Vec<u8> = (0..1024 * 1024)
        .map(|index| ((index * 17) % 253) as u8)
        .collect();
    fs::write(provider.path().join("model.glb"), &data).unwrap();
    fs::write(provider.path().join("albedo.png"), &texture).unwrap();
    let digest = format!("{:x}", sha2::Sha256::digest(&data));
    let texture_digest = format!("{:x}", sha2::Sha256::digest(&texture));
    let v2_manifest = provider.path().join("registry-v2.json");
    fs::write(
        &v2_manifest,
        serde_json::json!({
            "schemaVersion": 2,
            "id": "org.example.large-assets",
            "name": "large-assets",
            "items": [{
                "id": "model", "version": "1.0.0", "kind": "asset",
                "files": [
                    {"source":"model.glb","target":"models/model.glb","sha256":digest,"size":data.len()},
                    {"source":"albedo.png","target":"textures/albedo.png","sha256":texture_digest,"size":texture.len()}
                ]
            }]
        })
        .to_string(),
    )
    .unwrap();
    let v2_source = v2_manifest.to_str().unwrap();
    let project_path = project.path().to_str().unwrap();
    let verified = run_in(
        project.path(),
        &[
            "registry",
            "verify",
            "--registry",
            v2_source,
            "--project",
            project_path,
        ],
    );
    assert_eq!(verified.status.code(), Some(0), "{verified:?}");
    assert!(!project.path().join("friladd.lock.json").exists());
    let dry_run = run_in(
        project.path(),
        &[
            "add",
            "model",
            "--registry",
            v2_source,
            "--project",
            project_path,
            "--dry-run",
        ],
    );
    assert_eq!(dry_run.status.code(), Some(0), "{dry_run:?}");
    assert!(!project.path().join("models/model.glb").exists());
    assert!(!project.path().join("friladd.lock.json").exists());

    let added = run_in(
        project.path(),
        &[
            "add",
            "model",
            "--registry",
            v2_source,
            "--project",
            project_path,
        ],
    );
    assert_eq!(added.status.code(), Some(0), "{added:?}");
    assert_eq!(
        fs::read(project.path().join("models/model.glb")).unwrap(),
        data
    );
    assert_eq!(
        fs::read(project.path().join("textures/albedo.png")).unwrap(),
        texture
    );
    let lock: serde_json::Value =
        serde_json::from_slice(&fs::read(project.path().join("friladd.lock.json")).unwrap())
            .unwrap();
    assert_eq!(
        lock["items"][0]["files"][0]["installed"]["size"],
        12 * 1024 * 1024
    );
    fs::remove_file(project.path().join("models/model.glb")).unwrap();
    let restored = run_in(project.path(), &["restore", "--project", project_path]);
    assert_eq!(restored.status.code(), Some(0), "{restored:?}");
    assert_eq!(
        fs::read(project.path().join("models/model.glb")).unwrap(),
        data
    );
    assert_eq!(
        fs::read(project.path().join("textures/albedo.png")).unwrap(),
        texture
    );

    let v1_manifest = provider.path().join("registry-v1.json");
    fs::write(
        &v1_manifest,
        serde_json::json!({
            "schemaVersion": 1, "name": "large-v1",
            "items": [{"name":"model","description":"large model","files":[{"source":"model.glb","target":"v1-model.glb","sha256":digest}]}]
        }).to_string(),
    ).unwrap();
    let v1_source = v1_manifest.to_str().unwrap();
    let rejected = run_in(project.path(), &["add", "model", "--registry", v1_source]);
    assert_eq!(rejected.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("10485760 byte limit"));
    assert!(!project.path().join("v1-model.glb").exists());
}

#[test]
fn add_resolves_and_commits_a_cross_registry_dependency_graph_atomically() {
    use sha2::{Digest, Sha256};

    let ui = tempdir().unwrap();
    let assets = tempdir().unwrap();
    let project = tempdir().unwrap();
    let global = tempdir().unwrap();
    let component_bytes = b"export const Widget = () => null;\n";
    let asset_bytes = b"\0binary-model\xff";
    fs::write(ui.path().join("widget.tsx"), component_bytes).unwrap();
    fs::write(assets.path().join("model.glb"), asset_bytes).unwrap();
    let asset_manifest = serde_json::to_vec(&serde_json::json!({
        "schemaVersion": 2,
        "id": "org.example.assets",
        "name": "assets",
        "items": [{
            "id": "model", "version": "1.0.0", "kind": "asset",
            "files": [{
                "source": "model.glb", "target": "models/model.glb",
                "sha256": format!("{:x}", Sha256::digest(asset_bytes)),
                "size": asset_bytes.len()
            }]
        }]
    }))
    .unwrap();
    let asset_manifest_path = assets.path().join("registry.json");
    fs::write(&asset_manifest_path, &asset_manifest).unwrap();
    let ui_manifest_path = ui.path().join("registry.json");
    fs::write(
        &ui_manifest_path,
        serde_json::to_vec(&serde_json::json!({
            "schemaVersion": 2,
            "id": "org.example.ui",
            "name": "ui",
            "items": [{
                "id": "widget", "version": "1.0.0", "kind": "component",
                "files": [{
                    "source": "widget.tsx", "target": "components/widget.tsx",
                    "sha256": format!("{:x}", Sha256::digest(component_bytes)),
                    "size": component_bytes.len()
                }],
                "dependencies": [{
                    "providerId": "org.example.assets", "itemId": "model",
                    "version": "1.0.0",
                    "manifestSha256": format!("{:x}", Sha256::digest(&asset_manifest))
                }],
                "packages": [{"name":"react","spec":"^18.2.0","kind":"peer"}]
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        project.path().join(".friladd.json"),
        serde_json::to_vec(&serde_json::json!({
            "schemaVersion": 1,
            "registries": {
                "ui": {"source": ui_manifest_path},
                "assets": {"source": asset_manifest_path}
            }
        }))
        .unwrap(),
    )
    .unwrap();
    let project_path = project.path().display().to_string();
    let global_path = global.path();
    let output = run_with_config(
        project.path(),
        &[
            "add",
            "widget",
            "--registry",
            "ui",
            "--project",
            &project_path,
            "--json",
        ],
        global_path,
    );
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["command"], "add");
    assert_eq!(report["data"]["items"].as_array().unwrap().len(), 2);
    assert_eq!(report["data"]["items"][0]["itemId"], "model");
    assert_eq!(report["data"]["items"][1]["itemId"], "widget");
    assert_eq!(report["data"]["packages"][0]["name"], "react");
    assert_eq!(
        report["data"]["packages"][0]["requirements"][0]["kind"],
        "peer"
    );
    assert_eq!(
        report["data"]["packages"][0]["suggestions"]["npm"][0],
        "npm install 'react@^18.2.0'"
    );
    assert_eq!(
        fs::read(project.path().join("components/widget.tsx")).unwrap(),
        component_bytes
    );
    assert_eq!(
        fs::read(project.path().join("models/model.glb")).unwrap(),
        asset_bytes
    );
    assert!(!project.path().join("package.json").exists());
    let lock_path = project.path().join("friladd.lock.json");
    let first_lock = fs::read(&lock_path).unwrap();
    let lock = friladd::lockfile::Lock::parse(&first_lock).unwrap();
    assert_eq!(lock.items.len(), 2);
    let root_record = lock
        .items
        .iter()
        .find(|item| item.item_id == "widget")
        .unwrap();
    assert_eq!(root_record.dependencies.len(), 1);
    assert_eq!(root_record.dependencies[0].item_id, "model");
    assert_eq!(
        lock.items
            .iter()
            .find(|item| item.item_id == "model")
            .unwrap()
            .files[0]
            .installed
            .size,
        asset_bytes.len() as u64
    );

    let repeat = run_with_config(
        project.path(),
        &[
            "add",
            "widget",
            "--registry",
            "ui",
            "--project",
            &project_path,
            "--json",
        ],
        global_path,
    );
    assert_eq!(repeat.status.code(), Some(0), "{repeat:?}");
    let repeated: serde_json::Value = serde_json::from_slice(&repeat.stdout).unwrap();
    assert!(
        repeated["data"]["files"]
            .as_array()
            .unwrap()
            .iter()
            .all(|file| file["action"] == "unchanged")
    );
    assert_eq!(fs::read(&lock_path).unwrap(), first_lock);

    let cannot_remove_dependency = run_with_config(
        project.path(),
        &["remove", "assets/model", "--project", &project_path],
        global_path,
    );
    assert_eq!(cannot_remove_dependency.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&cannot_remove_dependency.stderr).contains("depends"));
    assert_eq!(
        fs::read(project.path().join("models/model.glb")).unwrap(),
        asset_bytes
    );

    fs::remove_file(project.path().join("components/widget.tsx")).unwrap();
    fs::remove_file(project.path().join("models/model.glb")).unwrap();
    let restored = run_with_config(
        project.path(),
        &["restore", "ui/widget", "--project", &project_path, "--json"],
        global_path,
    );
    assert_eq!(restored.status.code(), Some(0), "{restored:?}");
    assert_eq!(
        fs::read(project.path().join("components/widget.tsx")).unwrap(),
        component_bytes
    );
    assert_eq!(
        fs::read(project.path().join("models/model.glb")).unwrap(),
        asset_bytes
    );

    let removed_root = run_with_config(
        project.path(),
        &["remove", "ui/widget", "--project", &project_path],
        global_path,
    );
    assert_eq!(removed_root.status.code(), Some(0), "{removed_root:?}");
    assert!(!project.path().join("components/widget.tsx").exists());
    assert_eq!(
        fs::read(project.path().join("models/model.glb")).unwrap(),
        asset_bytes
    );
    let retained = friladd::lockfile::Lock::parse(&fs::read(&lock_path).unwrap()).unwrap();
    assert_eq!(retained.items.len(), 1);
    assert_eq!(retained.items[0].item_id, "model");
    let removed_dependency = run_with_config(
        project.path(),
        &["remove", "assets/model", "--project", &project_path],
        global_path,
    );
    assert_eq!(
        removed_dependency.status.code(),
        Some(0),
        "{removed_dependency:?}"
    );
    assert!(!project.path().join("models/model.glb").exists());
}

#[test]
fn dependency_add_rejects_missing_provider_and_cycles_without_project_writes() {
    use sha2::{Digest, Sha256};

    let provider = tempdir().unwrap();
    let project = tempdir().unwrap();
    let global = tempdir().unwrap();
    fs::write(provider.path().join("root.bin"), b"root").unwrap();
    let manifest = provider.path().join("registry.json");
    let write_manifest = |dependencies: serde_json::Value| {
        fs::write(
            &manifest,
            serde_json::to_vec(&serde_json::json!({
                "schemaVersion": 2,
                "id": "org.example.graph",
                "name": "graph",
                "items": [{
                    "id": "root", "version": "1.0.0", "kind": "component",
                    "files": [{
                        "source":"root.bin", "target":"root.bin",
                        "sha256":format!("{:x}", Sha256::digest(b"root")), "size":4
                    }],
                    "dependencies": dependencies
                }]
            }))
            .unwrap(),
        )
        .unwrap();
    };
    let source = manifest.display().to_string();
    let project_path = project.path().display().to_string();
    write_manifest(serde_json::json!([{
        "providerId":"org.example.absent", "itemId":"missing", "version":"1.0.0",
        "manifestSha256":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
    }]));
    let missing = run_with_config(
        project.path(),
        &[
            "add",
            "root",
            "--registry",
            &source,
            "--project",
            &project_path,
        ],
        global.path(),
    );
    assert_eq!(missing.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("Missing provider snapshot"));
    assert!(!project.path().join("root.bin").exists());
    assert!(!project.path().join("friladd.lock.json").exists());
    assert!(!project.path().join(".friladd").exists());

    write_manifest(serde_json::json!([{
        "providerId":"org.example.graph", "itemId":"root", "version":"1.0.0"
    }]));
    let cycle = run_with_config(
        project.path(),
        &[
            "add",
            "root",
            "--registry",
            &source,
            "--project",
            &project_path,
        ],
        global.path(),
    );
    assert_eq!(cycle.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&cycle.stderr).contains("Cycle in internal dependencies"));
    assert!(!project.path().join("root.bin").exists());
    assert!(!project.path().join("friladd.lock.json").exists());
}

#[test]
fn cross_registry_dependency_resolution_rejects_ambiguous_and_stale_snapshots() {
    use sha2::{Digest, Sha256};

    let root_registry = tempdir().unwrap();
    let first = tempdir().unwrap();
    let second = tempdir().unwrap();
    let project = tempdir().unwrap();
    let global = tempdir().unwrap();
    let asset = b"shared asset";
    fs::write(first.path().join("asset.bin"), asset).unwrap();
    fs::write(second.path().join("asset.bin"), asset).unwrap();
    let asset_manifest = serde_json::to_vec(&serde_json::json!({
        "schemaVersion":2,"id":"org.example.assets","name":"assets",
        "items":[{"id":"asset","version":"1.0.0","kind":"asset","files":[{"source":"asset.bin","target":"asset.bin","sha256":format!("{:x}",Sha256::digest(asset)),"size":asset.len()}]}]
    }))
    .unwrap();
    let first_manifest = first.path().join("registry.json");
    let second_manifest = second.path().join("registry.json");
    fs::write(&first_manifest, &asset_manifest).unwrap();
    fs::write(&second_manifest, &asset_manifest).unwrap();
    let root_file = b"root";
    fs::write(root_registry.path().join("root.bin"), root_file).unwrap();
    let root_manifest = root_registry.path().join("registry.json");
    let write_root = |digest: String| {
        fs::write(
            &root_manifest,
            serde_json::to_vec(&serde_json::json!({
                "schemaVersion":2,"id":"org.example.root","name":"root",
                "items":[{"id":"root","version":"1.0.0","kind":"component","files":[{"source":"root.bin","target":"root.bin","sha256":format!("{:x}",Sha256::digest(root_file)),"size":root_file.len()}],"dependencies":[{"providerId":"org.example.assets","itemId":"asset","version":"1.0.0","manifestSha256":digest}]}]
            }))
            .unwrap(),
        )
        .unwrap();
    };
    let exact_digest = format!("{:x}", Sha256::digest(&asset_manifest));
    write_root(exact_digest);
    fs::write(
        project.path().join(".friladd.json"),
        serde_json::to_vec(&serde_json::json!({
            "schemaVersion":1,
            "registries":{
                "first":{"source":first_manifest},
                "second":{"source":second_manifest}
            }
        }))
        .unwrap(),
    )
    .unwrap();
    let project_path = project.path().display().to_string();
    let root_source = root_manifest.display().to_string();
    let ambiguous = run_with_config(
        project.path(),
        &[
            "add",
            "root",
            "--registry",
            &root_source,
            "--project",
            &project_path,
        ],
        global.path(),
    );
    assert_eq!(ambiguous.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&ambiguous.stderr).contains("Ambiguous provider snapshot"));
    assert!(!project.path().join("root.bin").exists());
    assert!(!project.path().join("asset.bin").exists());
    assert!(!project.path().join("friladd.lock.json").exists());

    write_root("0".repeat(64));
    let stale = run_with_config(
        project.path(),
        &[
            "add",
            "root",
            "--registry",
            &root_source,
            "--project",
            &project_path,
        ],
        global.path(),
    );
    assert_eq!(stale.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&stale.stderr).contains("Stale provider snapshot"));
    assert!(!project.path().join("root.bin").exists());
    assert!(!project.path().join("friladd.lock.json").exists());
    assert!(!project.path().join(".friladd").exists());
}

#[test]
fn dependency_graph_preflights_target_collisions_and_aggregate_download_bounds() {
    let provider = tempdir().unwrap();
    let project = tempdir().unwrap();
    let global = tempdir().unwrap();
    let manifest = provider.path().join("registry.json");
    let root_file = serde_json::json!({
        "id":"root","version":"1.0.0","kind":"component",
        "files":[{"source":"root.bin","target":"root.bin","sha256":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef","size":4}],
        "dependencies":[
            {"providerId":"org.example.bounds","itemId":"first","version":"1.0.0"},
            {"providerId":"org.example.bounds","itemId":"second","version":"1.0.0"}
        ]
    });
    let make_item = |id: &str, target: &str, dependencies: serde_json::Value| {
        serde_json::json!({
            "id":id,"version":"1.0.0","kind":"asset",
            "files":[{"source":format!("{id}.bin"),"target":target,"sha256":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef","size":4}],
            "dependencies":dependencies
        })
    };
    fs::write(
        &manifest,
        serde_json::to_vec(&serde_json::json!({
            "schemaVersion":2,"id":"org.example.bounds","name":"bounds",
            "items":[
                root_file,
                make_item("first","shared.bin",serde_json::json!([])),
                make_item("second","shared.bin",serde_json::json!([]))
            ]
        }))
        .unwrap(),
    )
    .unwrap();
    let source = manifest.display().to_string();
    let project_path = project.path().display().to_string();
    let collision = run_with_config(
        project.path(),
        &[
            "add",
            "root",
            "--registry",
            &source,
            "--project",
            &project_path,
        ],
        global.path(),
    );
    assert_eq!(collision.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&collision.stderr).contains("collision"));
    assert_eq!(fs::read_dir(project.path()).unwrap().count(), 0);

    let huge_items = (0..6)
        .map(|index| {
            serde_json::json!({
                "id":format!("node-{index}"),"version":"1.0.0","kind":"asset",
                "files":[{"source":format!("large-{index}.bin"),"target":format!("large-{index}.bin"),"sha256":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef","size":100*1024*1024}],
                "dependencies":if index == 0 { (1..6).map(|dep| serde_json::json!({"providerId":"org.example.bounds","itemId":format!("node-{dep}"),"version":"1.0.0"})).collect::<Vec<_>>() } else { Vec::new() }
            })
        })
        .collect::<Vec<_>>();
    let huge_root = provider.path().join("huge.json");
    fs::write(
        &huge_root,
        serde_json::to_vec(&serde_json::json!({
            "schemaVersion":2,"id":"org.example.bounds","name":"bounds","items":huge_items
        }))
        .unwrap(),
    )
    .unwrap();
    let huge_source = huge_root.display().to_string();
    let bounded = run_with_config(
        project.path(),
        &[
            "add",
            "node-0",
            "--registry",
            &huge_source,
            "--project",
            &project_path,
        ],
        global.path(),
    );
    assert_eq!(bounded.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&bounded.stderr).contains("512 MiB acquisition limit"));
    assert_eq!(fs::read_dir(project.path()).unwrap().count(), 0);
}

#[test]
fn update_reconciles_the_dependency_graph_in_one_dry_run_and_transaction() {
    use sha2::{Digest, Sha256};

    let ui = tempdir().unwrap();
    let assets = tempdir().unwrap();
    let project = tempdir().unwrap();
    let global = tempdir().unwrap();
    let asset_bytes = b"pinned model";
    fs::write(assets.path().join("model.bin"), asset_bytes).unwrap();
    let asset_manifest_path = assets.path().join("registry.json");
    let asset_manifest_for = |version: &str, bytes: &[u8]| {
        serde_json::to_vec(&serde_json::json!({
            "schemaVersion": 2,
            "id": "org.example.assets",
            "name": "assets",
            "items": [{
                "id":"model", "version":version, "kind":"asset",
                "files":[{"source":"model.bin","target":"models/model.bin","sha256":format!("{:x}",Sha256::digest(bytes)),"size":bytes.len()}]
            }]
        }))
        .unwrap()
    };
    let asset_manifest = asset_manifest_for("1.0.0", asset_bytes);
    fs::write(&asset_manifest_path, &asset_manifest).unwrap();
    let ui_manifest_path = ui.path().join("registry.json");
    let component_file = ui.path().join("widget.tsx");
    let write_component = |version: &str,
                           dependency_version: &str,
                           bytes: &[u8],
                           dependency_manifest: &[u8]| {
        fs::write(&component_file, bytes).unwrap();
        fs::write(
            &ui_manifest_path,
            serde_json::to_vec(&serde_json::json!({
                "schemaVersion":2,
                "id":"org.example.ui",
                "name":"ui",
                "items":[{
                    "id":"widget", "version":version, "kind":"component",
                    "files":[{"source":"widget.tsx","target":"components/widget.tsx","sha256":format!("{:x}",Sha256::digest(bytes)),"size":bytes.len()}],
                    "dependencies":[{"providerId":"org.example.assets","itemId":"model","version":dependency_version,"manifestSha256":format!("{:x}",Sha256::digest(dependency_manifest))}]
                }]
            }))
            .unwrap(),
        )
        .unwrap();
    };
    write_component(
        "1.0.0",
        "1.0.0",
        b"export const Widget = 1;\n",
        &asset_manifest,
    );
    fs::write(
        project.path().join(".friladd.json"),
        serde_json::to_vec(&serde_json::json!({
            "schemaVersion":1,
            "registries":{
                "ui":{"source":ui_manifest_path},
                "assets":{"source":asset_manifest_path}
            }
        }))
        .unwrap(),
    )
    .unwrap();
    let project_path = project.path().display().to_string();
    let config = global.path();
    let source_v1 = run_with_config(
        project.path(),
        &[
            "add",
            "widget",
            "--registry",
            "ui",
            "--project",
            &project_path,
        ],
        config,
    );
    assert_eq!(source_v1.status.code(), Some(0), "{source_v1:?}");
    let lock_path = project.path().join("friladd.lock.json");
    let old_lock = fs::read(&lock_path).unwrap();
    let old_component = fs::read(project.path().join("components/widget.tsx")).unwrap();

    let asset_bytes_v2 = b"updated model";
    fs::write(assets.path().join("model.bin"), asset_bytes_v2).unwrap();
    let asset_manifest_v2 = asset_manifest_for("2.0.0", asset_bytes_v2);
    fs::write(&asset_manifest_path, &asset_manifest_v2).unwrap();
    write_component(
        "2.0.0",
        "2.0.0",
        b"export const Widget = 2;\n",
        &asset_manifest_v2,
    );
    let dry_run = run_with_config(
        project.path(),
        &[
            "update",
            "widget",
            "--registry",
            "ui",
            "--version",
            "2.0.0",
            "--project",
            &project_path,
            "--dry-run",
            "--json",
        ],
        config,
    );
    assert_eq!(dry_run.status.code(), Some(0), "{dry_run:?}");
    let dry_report: serde_json::Value = serde_json::from_slice(&dry_run.stdout).unwrap();
    assert_eq!(dry_report["command"], "update");
    assert_eq!(
        dry_report["data"]["dependencyGraph"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        dry_report["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["itemId"] == "widget")
            .unwrap()["actions"][0]["action"],
        "replace"
    );
    assert_eq!(
        dry_report["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["itemId"] == "model")
            .unwrap()["actions"][0]["action"],
        "replace"
    );
    assert_eq!(fs::read(&lock_path).unwrap(), old_lock);
    assert_eq!(
        fs::read(project.path().join("components/widget.tsx")).unwrap(),
        old_component
    );
    assert_eq!(
        fs::read(project.path().join("models/model.bin")).unwrap(),
        asset_bytes
    );

    let updated = run_with_config(
        project.path(),
        &[
            "update",
            "widget",
            "--registry",
            "ui",
            "--version",
            "2.0.0",
            "--project",
            &project_path,
            "--json",
        ],
        config,
    );
    assert_eq!(updated.status.code(), Some(0), "{updated:?}");
    assert_eq!(
        fs::read(project.path().join("components/widget.tsx")).unwrap(),
        b"export const Widget = 2;\n"
    );
    assert_eq!(
        fs::read(project.path().join("models/model.bin")).unwrap(),
        asset_bytes_v2
    );
    let updated_report: serde_json::Value = serde_json::from_slice(&updated.stdout).unwrap();
    assert_eq!(updated_report["command"], "update");
    let updated_lock = friladd::lockfile::Lock::parse(&fs::read(&lock_path).unwrap()).unwrap();
    let component = updated_lock
        .items
        .iter()
        .find(|item| item.item_id == "widget")
        .unwrap();
    assert_eq!(component.version.as_deref(), Some("2.0.0"));
    assert_eq!(component.dependencies.len(), 1);
    assert_eq!(component.dependencies[0].version, "2.0.0");
    assert_eq!(
        updated_lock
            .items
            .iter()
            .find(|item| item.item_id == "model")
            .unwrap()
            .version
            .as_deref(),
        Some("2.0.0")
    );

    let lock_after_update = fs::read(&lock_path).unwrap();
    let updated_asset_bytes = fs::read(project.path().join("models/model.bin")).unwrap();
    let asset_bytes_v3 = b"third model";
    fs::write(assets.path().join("model.bin"), asset_bytes_v3).unwrap();
    let asset_manifest_v3 = asset_manifest_for("3.0.0", asset_bytes_v3);
    fs::write(&asset_manifest_path, &asset_manifest_v3).unwrap();
    let blocked_dependency_update = run_with_config(
        project.path(),
        &[
            "update",
            "model",
            "--registry",
            "assets",
            "--version",
            "3.0.0",
            "--project",
            &project_path,
        ],
        config,
    );
    assert_eq!(blocked_dependency_update.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&blocked_dependency_update.stderr).contains("outside this graph")
    );
    assert_eq!(fs::read(&lock_path).unwrap(), lock_after_update);
    assert_eq!(
        fs::read(project.path().join("models/model.bin")).unwrap(),
        updated_asset_bytes
    );

    let asset_bytes_v1 = b"original model";
    fs::write(assets.path().join("model.bin"), asset_bytes_v1).unwrap();
    let asset_manifest_v1 = asset_manifest_for("1.0.0", asset_bytes_v1);
    fs::write(&asset_manifest_path, &asset_manifest_v1).unwrap();
    write_component(
        "3.0.0",
        "1.0.0",
        b"export const Widget = 3;\n",
        &asset_manifest_v1,
    );
    let rejected_downgrade = run_with_config(
        project.path(),
        &[
            "update",
            "widget",
            "--registry",
            "ui",
            "--version",
            "3.0.0",
            "--project",
            &project_path,
        ],
        config,
    );
    assert_eq!(
        rejected_downgrade.status.code(),
        Some(2),
        "{rejected_downgrade:?}"
    );
    assert!(String::from_utf8_lossy(&rejected_downgrade.stderr).contains("--allow-downgrade"));
    assert_eq!(fs::read(&lock_path).unwrap(), lock_after_update);
    assert_eq!(
        fs::read(project.path().join("models/model.bin")).unwrap(),
        updated_asset_bytes
    );

    let confirmed_downgrade = run_with_config(
        project.path(),
        &[
            "update",
            "widget",
            "--registry",
            "ui",
            "--version",
            "3.0.0",
            "--allow-downgrade",
            "--project",
            &project_path,
        ],
        config,
    );
    assert_eq!(
        confirmed_downgrade.status.code(),
        Some(0),
        "{confirmed_downgrade:?}"
    );
    assert_eq!(
        fs::read(project.path().join("models/model.bin")).unwrap(),
        asset_bytes_v1
    );
}

#[test]
fn package_requirement_conflicts_fail_before_any_project_write() {
    use sha2::{Digest, Sha256};

    let provider = tempdir().unwrap();
    let project = tempdir().unwrap();
    let global = tempdir().unwrap();
    fs::write(provider.path().join("component.bin"), b"component").unwrap();
    fs::write(provider.path().join("dependency.bin"), b"dependency").unwrap();
    let manifest = provider.path().join("registry.json");
    fs::write(
        &manifest,
        serde_json::to_vec(&serde_json::json!({
            "schemaVersion":2,
            "id":"org.example.packages",
            "name":"packages",
            "items":[
                {
                    "id":"component","version":"1.0.0","kind":"component",
                    "files":[{"source":"component.bin","target":"component.bin","sha256":format!("{:x}",Sha256::digest(b"component")),"size":9}],
                    "dependencies":[{"providerId":"org.example.packages","itemId":"shared","version":"1.0.0"}],
                    "packages":[{"name":"react","spec":"18.2.0","kind":"peer"}]
                },
                {
                    "id":"shared","version":"1.0.0","kind":"asset",
                    "files":[{"source":"dependency.bin","target":"dependency.bin","sha256":format!("{:x}",Sha256::digest(b"dependency")),"size":10}],
                    "packages":[{"name":"react","spec":"18.3.0","kind":"runtime"}]
                }
            ]
        }))
        .unwrap(),
    )
    .unwrap();
    let source = manifest.display().to_string();
    let project_path = project.path().display().to_string();
    let output = run_with_config(
        project.path(),
        &[
            "add",
            "component",
            "--registry",
            &source,
            "--project",
            &project_path,
        ],
        global.path(),
    );
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("Conflicting package requirements for react")
    );
    assert!(!project.path().join("component.bin").exists());
    assert!(!project.path().join("dependency.bin").exists());
    assert!(!project.path().join("friladd.lock.json").exists());
    assert!(!project.path().join(".friladd").exists());
    assert_eq!(fs::read_dir(project.path()).unwrap().count(), 0);
}

#[test]
fn graph_install_preserves_distinct_targets_for_a_repeated_source() {
    use sha2::{Digest, Sha256};

    let provider = tempdir().unwrap();
    let project = tempdir().unwrap();
    let global = tempdir().unwrap();
    fs::write(provider.path().join("shared.bin"), b"shared bytes").unwrap();
    let manifest = provider.path().join("registry.json");
    fs::write(
        &manifest,
        serde_json::to_vec(&serde_json::json!({
            "schemaVersion": 2,
            "id": "org.example.repeated",
            "name": "repeated",
            "items": [{
                "id": "item", "version": "1.0.0", "kind": "asset",
                "files": [
                    {"source":"shared.bin", "target":"first.bin", "sha256":format!("{:x}", Sha256::digest(b"shared bytes")), "size":12},
                    {"source":"shared.bin", "target":"second.bin", "sha256":format!("{:x}", Sha256::digest(b"shared bytes")), "size":12}
                ]
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    let project_path = project.path().display().to_string();
    let output = run_with_config(
        project.path(),
        &[
            "add",
            "item",
            "--registry",
            &manifest.display().to_string(),
            "--project",
            &project_path,
        ],
        global.path(),
    );
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(
        fs::read(project.path().join("first.bin")).unwrap(),
        b"shared bytes"
    );
    assert_eq!(
        fs::read(project.path().join("second.bin")).unwrap(),
        b"shared bytes"
    );
    let lock = friladd::lockfile::Lock::parse(
        &fs::read(project.path().join("friladd.lock.json")).unwrap(),
    )
    .unwrap();
    let item = lock
        .items
        .iter()
        .find(|item| item.item_id == "item")
        .unwrap();
    assert_eq!(item.files.len(), 2);
    assert_eq!(item.files[0].declared_target, "first.bin");
    assert_eq!(item.files[1].declared_target, "second.bin");
}

#[test]
fn diamond_dependencies_share_one_owned_item_and_remove_retains_orphans() {
    use sha2::{Digest, Sha256};

    let provider = tempdir().unwrap();
    let project = tempdir().unwrap();
    let global = tempdir().unwrap();
    for (name, bytes) in [
        ("root.bin", b"root".as_slice()),
        ("first.bin", b"first".as_slice()),
        ("second.bin", b"second".as_slice()),
        ("shared.bin", b"shared".as_slice()),
    ] {
        fs::write(provider.path().join(name), bytes).unwrap();
    }
    let manifest = provider.path().join("registry.json");
    let declarations = [
        ("root", "root.bin", "root.bin"),
        ("first", "first.bin", "first.bin"),
        ("second", "second.bin", "second.bin"),
        ("shared", "shared.bin", "shared.bin"),
    ];
    let items = declarations
        .iter()
        .map(|(id, source, target)| {
            let bytes = fs::read(provider.path().join(source)).unwrap();
            let dependencies = match *id {
                "root" => serde_json::json!([
                    {"providerId":"org.example.diamond","itemId":"first","version":"1.0.0"},
                    {"providerId":"org.example.diamond","itemId":"second","version":"1.0.0"}
                ]),
                "first" | "second" => serde_json::json!([
                    {"providerId":"org.example.diamond","itemId":"shared","version":"1.0.0"}
                ]),
                _ => serde_json::json!([]),
            };
            serde_json::json!({
                "id":id,"version":"1.0.0","kind":"other",
                "files":[{"source":source,"target":target,"sha256":format!("{:x}",Sha256::digest(&bytes)),"size":bytes.len()}],
                "dependencies":dependencies
            })
        })
        .collect::<Vec<_>>();
    fs::write(
        &manifest,
        serde_json::to_vec(&serde_json::json!({
            "schemaVersion":2,"id":"org.example.diamond","name":"diamond","items":items
        }))
        .unwrap(),
    )
    .unwrap();
    let source = manifest.display().to_string();
    let project_path = project.path().display().to_string();
    let add = run_with_config(
        project.path(),
        &[
            "add",
            "root",
            "--registry",
            &source,
            "--project",
            &project_path,
            "--json",
        ],
        global.path(),
    );
    assert_eq!(add.status.code(), Some(0), "{add:?}");
    let add_report: serde_json::Value = serde_json::from_slice(&add.stdout).unwrap();
    let order = add_report["data"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["itemId"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(order, ["shared", "first", "second", "root"]);
    let lock_path = project.path().join("friladd.lock.json");
    let lock = friladd::lockfile::Lock::parse(&fs::read(&lock_path).unwrap()).unwrap();
    assert_eq!(lock.items.len(), 4);
    assert_eq!(
        lock.items
            .iter()
            .filter(|item| item.item_id == "shared")
            .count(),
        1
    );

    let remove_root = run_with_config(
        project.path(),
        &["remove", "root", "--project", &project_path],
        global.path(),
    );
    assert_eq!(remove_root.status.code(), Some(0), "{remove_root:?}");
    assert!(project.path().join("shared.bin").exists());
    let cannot_remove_shared = run_with_config(
        project.path(),
        &["remove", "shared", "--project", &project_path],
        global.path(),
    );
    assert_eq!(cannot_remove_shared.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&cannot_remove_shared.stderr).contains("depends"));

    let remove_first = run_with_config(
        project.path(),
        &["remove", "first", "--project", &project_path],
        global.path(),
    );
    assert_eq!(remove_first.status.code(), Some(0), "{remove_first:?}");
    let still_shared = friladd::lockfile::Lock::parse(&fs::read(&lock_path).unwrap()).unwrap();
    assert!(
        still_shared
            .items
            .iter()
            .any(|item| item.item_id == "shared")
    );
    assert!(project.path().join("shared.bin").exists());
    let remove_second = run_with_config(
        project.path(),
        &["remove", "second", "--project", &project_path],
        global.path(),
    );
    assert_eq!(remove_second.status.code(), Some(0), "{remove_second:?}");
    assert!(project.path().join("shared.bin").exists());
    let remove_orphan = run_with_config(
        project.path(),
        &["remove", "shared", "--project", &project_path],
        global.path(),
    );
    assert_eq!(remove_orphan.status.code(), Some(0), "{remove_orphan:?}");
    assert!(!project.path().join("shared.bin").exists());
}

fn v1_manifest(files: &[(&str, &str, &[u8])]) -> String {
    use sha2::{Digest, Sha256};
    serde_json::json!({
        "schemaVersion": 1,
        "name": "test",
        "items": [{
            "name": "sample",
            "description": "test resource",
            "files": files.iter().map(|(source, target, bytes)| serde_json::json!({
                "source": source,
                "target": target,
                "sha256": format!("{:x}", Sha256::digest(bytes)),
            })).collect::<Vec<_>>(),
        }],
    })
    .to_string()
}
