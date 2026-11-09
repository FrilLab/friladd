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
    Command::new(binary())
        .args(args)
        .output()
        .expect("run friladd")
}

fn run_in(directory: &Path, args: &[&str]) -> Output {
    Command::new(binary())
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

#[test]
fn help_version_and_invalid_arguments_have_expected_exit_codes() {
    assert!(run(&["--help"]).status.success());
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
