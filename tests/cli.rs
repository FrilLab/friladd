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
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("SHA-256 mismatch"));
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
