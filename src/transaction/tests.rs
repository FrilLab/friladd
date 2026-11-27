use super::*;
use crate::lockfile::{Item, OwnedFile, Provider};
use std::{env, process::Command};
use tempfile::tempdir;

const MANIFEST: &str = "https://example.invalid/published/registry.json";
const TARGET: &str = "nested/assets/file.bin";
const OLD: &[u8] = b"old\0\xff";
const NEW: &[u8] = b"new\0\x80";

fn lock(target: &str, contents: &[u8]) -> Lock {
    let provider_id = lockfile::v1_provider_id(MANIFEST).unwrap();
    let installed = Content::of(contents);
    Lock {
        providers: vec![Provider {
            provider_id: provider_id.clone(),
            declared_source: MANIFEST.into(),
            resolved_source: MANIFEST.into(),
            manifest_sha256: "ab".repeat(32),
            public_identity: None,
        }],
        items: vec![Item {
            provider_id,
            item_id: "model".into(),
            version: None,
            manifest_source: MANIFEST.into(),
            manifest_sha256: "ab".repeat(32),
            target_root: None,
            dependencies: Vec::new(),
            files: vec![OwnedFile {
                source: "source.bin".into(),
                resolved_source: "https://example.invalid/published/source.bin".into(),
                declared_target: target.into(),
                target: target.into(),
                expected_sha256: installed.sha256.clone(),
                installed,
            }],
        }],
        ..Lock::default()
    }
}

fn plan(project: &Path, before: Option<&[u8]>, after: &[u8]) -> Plan {
    Plan::new(
        lockfile::load(project).unwrap(),
        lock(TARGET, after),
        vec![Edit {
            target: TARGET.into(),
            before: before.map(Content::of),
            write: Some(WriteSource::Bytes(after.to_vec())),
        }],
    )
    .unwrap()
}

#[test]
fn lock_is_deterministic_and_validates_provenance_and_portable_ownership() {
    let mut next = lock("Dir/File", NEW);
    let mut second = next.items[0].clone();
    second.item_id = "another".into();
    second.files[0].declared_target = "Dir/Other".into();
    second.files[0].target = "Dir/Other".into();
    next.items.push(second);
    let encoded = next.encode().unwrap();
    next.items.reverse();
    assert_eq!(next.encode().unwrap(), encoded);
    assert_eq!(Lock::parse(&encoded).unwrap(), next);

    for target in [
        "Dir/File",
        "dir/file",
        "Dir/File/name",
        "Dir/Other/.Git/file",
        "../outside",
    ] {
        next.items[0].files[0].declared_target = target.into();
        next.items[0].files[0].target = target.into();
        assert!(next.encode().is_err(), "{target}");
    }
    let mut invalid = lock(TARGET, NEW);
    invalid.providers[0].declared_source =
        "https://example.invalid/registry.json?auth=value".into();
    assert!(
        invalid
            .encode()
            .unwrap_err()
            .contains("without authorization")
    );
    invalid = lock(TARGET, NEW);
    invalid.items[0].files[0].resolved_source = "https://elsewhere.invalid/source.bin".into();
    assert!(invalid.encode().unwrap_err().contains("escaped"));
    invalid = lock(TARGET, NEW);
    invalid.items[0].files[0].expected_sha256 = "broken".into();
    assert!(invalid.encode().unwrap_err().contains("digest"));
    invalid = lock(TARGET, NEW);
    invalid.items.push(invalid.items[0].clone());
    assert!(
        invalid
            .encode()
            .unwrap_err()
            .contains("duplicate lock item")
    );
}

#[test]
fn loader_distinguishes_missing_empty_and_invalid_lock_without_writes() {
    let root = tempdir().unwrap();
    let path = root.path().join(lockfile::NAME);
    let missing = lockfile::load(root.path()).unwrap();
    assert_eq!(missing.lock(), &Lock::default());
    assert!(missing.bytes.is_none());
    assert!(!path.exists());
    assert!(!pending(root.path()).unwrap());

    let bytes = Lock::default().encode().unwrap();
    fs::write(&path, &bytes).unwrap();
    assert_eq!(lockfile::load(root.path()).unwrap().bytes, Some(bytes));
    for contents in [
        "",
        "{",
        r#"{"schemaVersion":99,"items":[],"providers":[]}"#,
        r#"{"schemaVersion":1,"items":[],"providers":[],"items":[]}"#,
        r#"{"schemaVersion":1,"items":[],"providers":[],"unknown":true}"#,
    ] {
        fs::write(&path, contents.as_bytes()).unwrap();
        assert!(lockfile::load(root.path()).is_err(), "{contents}");
        assert_eq!(fs::read(&path).unwrap(), contents.as_bytes());
    }
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(
        lockfile::load(root.path())
            .unwrap_err()
            .contains("non-symlink file")
    );
    assert!(!root.path().join(STATE).exists());
}

#[test]
fn apply_keeps_user_files_and_preexisting_lock_bytes() {
    let root = tempdir().unwrap();
    let root = root.path();
    assert!(plan(root, None, NEW).check(root).is_ok());
    assert_eq!(fs::read_dir(root).unwrap().count(), 0);

    let install = plan(root, None, NEW);
    fs::create_dir_all(root.join("nested/assets")).unwrap();
    fs::write(root.join(TARGET), NEW).unwrap();
    assert!(apply(root, install).unwrap_err().contains("unowned"));
    assert_eq!(fs::read(root.join(TARGET)).unwrap(), NEW);
    assert!(!root.join(lockfile::NAME).exists());

    fs::remove_file(root.join(TARGET)).unwrap();
    apply(root, plan(root, None, NEW)).unwrap();
    let bytes = fs::read(root.join(lockfile::NAME)).unwrap();
    assert_eq!(Lock::parse(&bytes).unwrap(), lock(TARGET, NEW));
    assert_eq!(inspect(root, TARGET).unwrap(), Some(Content::of(NEW)));

    let update = plan(root, Some(NEW), OLD);
    fs::write(root.join(TARGET), b"user").unwrap();
    assert!(apply(root, update).unwrap_err().contains("changed"));
    assert_eq!(fs::read(root.join(TARGET)).unwrap(), b"user");
    assert_eq!(fs::read(root.join(lockfile::NAME)).unwrap(), bytes);
    assert!(!pending(root).unwrap());
}

#[test]
fn rollback_restores_files_and_lock_for_every_commit_checkpoint() {
    for checkpoint in [
        "preparing",
        "staged",
        "journal",
        "file",
        "lock",
        "committed",
        "cleanup",
    ] {
        let root = tempdir().unwrap();
        let root = root.path();
        apply(root, plan(root, None, OLD)).unwrap();
        let old_lock = fs::read(root.join(lockfile::NAME)).unwrap();
        let mut hit = false;
        let error = apply_with(root, plan(root, Some(OLD), NEW), |name| {
            if name == checkpoint {
                hit = true;
                return Err("injected failure".into());
            }
            Ok(())
        })
        .unwrap_err();
        assert!(
            hit && error.contains("injected failure"),
            "{checkpoint}: {error}"
        );
        assert!(!pending(root).unwrap());
        if matches!(checkpoint, "committed" | "cleanup") {
            assert_eq!(fs::read(root.join(TARGET)).unwrap(), NEW, "{checkpoint}");
            assert_eq!(lockfile::load(root).unwrap().lock(), &lock(TARGET, NEW));
        } else {
            assert_eq!(fs::read(root.join(TARGET)).unwrap(), OLD, "{checkpoint}");
            assert_eq!(fs::read(root.join(lockfile::NAME)).unwrap(), old_lock);
        }
    }
}

#[test]
fn rolls_back_partial_multi_file_commit_and_preserves_ambiguous_paths() {
    let root = tempdir().unwrap();
    let root = root.path();
    let another = "nested/assets/second.bin";
    let mut next = lock(TARGET, NEW);
    let mut file = next.items[0].files[0].clone();
    file.target = another.into();
    file.declared_target = another.into();
    next.items[0].files.push(file);
    let install = |next: Lock| {
        Plan::new(
            lockfile::load(root).unwrap(),
            next,
            [TARGET, another]
                .into_iter()
                .map(|target| Edit {
                    target: target.into(),
                    before: None,
                    write: Some(WriteSource::Bytes(NEW.to_vec())),
                })
                .collect(),
        )
        .unwrap()
    };
    let error = apply_with(root, install(next.clone()), |name| {
        if name == "file" {
            assert_eq!(fs::read(root.join(TARGET)).unwrap(), NEW);
            assert!(!root.join(another).exists());
            return Err("fail after first file".into());
        }
        Ok(())
    })
    .unwrap_err();
    assert_eq!(error, "fail after first file");
    assert!(!root.join("nested").exists());
    assert!(!root.join(lockfile::NAME).exists());

    let error = apply_with(root, install(next), |name| {
        if name == "file" {
            // A different process created this path with matching bytes.
            fs::write(root.join(another), NEW).unwrap();
        }
        Ok(())
    })
    .unwrap_err();
    assert!(error.contains("Cannot prove"), "{error}");
    assert_eq!(fs::read(root.join(another)).unwrap(), NEW);
    assert!(pending(root).unwrap());
    let first = recover(root).unwrap_err();
    assert_eq!(recover(root).unwrap_err(), first);
    assert_eq!(fs::read(root.join(another)).unwrap(), NEW);
}

#[test]
fn refuses_symlink_state_lock_destination_and_backup() {
    #[cfg(unix)]
    fn symlink(original: &Path, link: &Path, _directory: bool) {
        std::os::unix::fs::symlink(original, link).unwrap();
    }
    #[cfg(not(unix))]
    fn symlink(_original: &Path, _link: &Path, _directory: bool) {}
    if !cfg!(unix) {
        // Windows symlink permissions vary. A non-directory state is still rejected.
        let root = tempdir().unwrap();
        fs::write(root.path().join(STATE), b"keep").unwrap();
        assert!(apply(root.path(), plan(root.path(), None, NEW)).is_err());
        assert_eq!(fs::read(root.path().join(STATE)).unwrap(), b"keep");
        return;
    }

    let root = tempdir().unwrap();
    let outside = tempdir().unwrap();
    let root = root.path();
    let outside = outside.path();
    let keep = outside.join("keep");
    fs::write(&keep, b"keep").unwrap();
    symlink(&keep, &root.join(lockfile::NAME), false);
    assert!(lockfile::load(root).is_err());
    fs::remove_file(root.join(lockfile::NAME)).unwrap();

    symlink(outside, &root.join(STATE), true);
    assert!(plan(root, None, NEW).check(root).is_err());
    assert!(apply(root, plan(root, None, NEW)).is_err());
    fs::remove_file(root.join(STATE)).unwrap();

    symlink(outside, &root.join("nested"), true);
    assert!(
        plan(root, None, NEW)
            .check(root)
            .unwrap_err()
            .contains("symlink")
    );
    assert!(apply(root, plan(root, None, NEW)).is_err());
    fs::remove_file(root.join("nested")).unwrap();

    assert!(
        apply_with(root, plan(root, None, NEW), |name| {
            if name == "journal" {
                let stage = root.join(STATE).join(ACTIVE).join("after-0");
                fs::remove_file(&stage).unwrap();
                symlink(&keep, &stage, false);
                return Err("interrupted staging".into());
            }
            Ok(())
        })
        .unwrap_err()
        .contains("Recovery required")
    );
    assert!(pending(root).unwrap());
    assert_eq!(fs::read(keep).unwrap(), b"keep");
    assert!(!root.join(TARGET).exists());
}

#[test]
fn interrupted_process_releases_os_lock_and_is_recovered_repeatably() {
    for checkpoint in [
        "staged",
        "journal",
        "directory",
        "file",
        "lock",
        "committed",
    ] {
        let root = tempdir().unwrap();
        let result = child(root.path(), checkpoint).status().unwrap();
        assert_eq!(result.code(), Some(73), "{checkpoint}");
        assert!(pending(root.path()).unwrap());
        recover(root.path()).unwrap();
        recover(root.path()).unwrap();
        assert!(!pending(root.path()).unwrap());
        if checkpoint == "committed" {
            assert_eq!(fs::read(root.path().join(TARGET)).unwrap(), NEW);
            assert_eq!(
                lockfile::load(root.path()).unwrap().lock(),
                &lock(TARGET, NEW)
            );
        } else {
            assert!(!root.path().join("nested").exists(), "{checkpoint}");
            assert!(!root.path().join(lockfile::NAME).exists(), "{checkpoint}");
        }
    }
}

#[test]
fn another_process_cannot_mutate_during_a_lock() {
    let root = tempdir().unwrap();
    let project = Project::open(lockfile::canonical_project(root.path()).unwrap()).unwrap();
    let output = child(root.path(), "contended").output().unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(!root.path().join(TARGET).exists());
    assert!(!root.path().join(lockfile::NAME).exists());
    drop(project);
    apply(root.path(), plan(root.path(), None, NEW)).unwrap();
}

fn child(project: &Path, checkpoint: &str) -> Command {
    let mut command = Command::new(env::current_exe().unwrap());
    command
        .args(["--exact", "transaction::tests::mutation_worker", "--quiet"])
        .env("FRILADD_TEST_MUTATION_ROOT", project)
        .env("FRILADD_TEST_MUTATION_CHECKPOINT", checkpoint);
    command
}

#[test]
fn mutation_worker() {
    let Some(project) = env::var_os("FRILADD_TEST_MUTATION_ROOT") else {
        return;
    };
    let project = PathBuf::from(project);
    let checkpoint = env::var("FRILADD_TEST_MUTATION_CHECKPOINT").unwrap();
    if checkpoint == "contended" {
        let result = apply(&project, plan(&project, None, NEW));
        assert!(
            result
                .unwrap_err()
                .contains("Cannot acquire project mutation lock")
        );
        return;
    }
    apply_with(&project, plan(&project, None, NEW), |current| {
        if current == checkpoint {
            std::process::exit(73);
        }
        Ok(())
    })
    .unwrap();
    panic!("missed crash checkpoint {checkpoint}");
}

#[cfg(unix)]
#[test]
fn permission_failure_preserves_project_contents() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempdir().unwrap();
    let root = root.path();
    let parent = root.join("nested/assets");
    fs::create_dir_all(&parent).unwrap();
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o500)).unwrap();
    // The test can be run by root locally. Normal CI runners cannot bypass this permission.
    let probe = parent.join("permission-probe");
    if fs::File::create(&probe).is_ok() {
        fs::remove_file(&probe).unwrap();
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).unwrap();
        return;
    }
    let result = apply(root, plan(root, None, NEW));
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(result.is_err());
    assert!(!root.join(TARGET).exists());
    assert!(!root.join(lockfile::NAME).exists());
    assert!(!pending(root).unwrap());
}

#[test]
fn protects_unrecognized_recovery_files_and_retries_cleanup() {
    let root = tempdir().unwrap();
    let root = root.path();
    let keep = root.join(STATE).join(CLEANUP).join("user-file");
    let result = apply_with(root, plan(root, None, NEW), |checkpoint| {
        if checkpoint == "committed" {
            let active = root.join(STATE).join(ACTIVE);
            fs::write(active.join("user-file"), b"untouched").unwrap();
        }
        Ok(())
    });
    let error = result.unwrap_err();
    assert!(error.contains("Recovery required"), "{error}");
    assert_eq!(fs::read(&keep).unwrap(), b"untouched");
    assert_eq!(fs::read(root.join(TARGET)).unwrap(), NEW);
    let relocated = root.join("keep.txt");
    fs::rename(keep, &relocated).unwrap();
    recover(root).unwrap();
    assert_eq!(fs::read(relocated).unwrap(), b"untouched");
    assert!(!pending(root).unwrap());
}

#[test]
fn verifies_streamed_staging_before_any_destination_write() {
    let source = tempdir().unwrap();
    let source = source.path().join("artifact");
    let root = tempdir().unwrap();
    let root = root.path();
    let from_file = || {
        Plan::new(
            lockfile::load(root).unwrap(),
            lock(TARGET, NEW),
            vec![Edit {
                target: TARGET.into(),
                before: None,
                write: Some(WriteSource::File(source.clone())),
            }],
        )
        .unwrap()
    };
    fs::write(&source, OLD).unwrap();
    assert!(
        from_file()
            .check(root)
            .unwrap_err()
            .contains("Digest mismatch")
    );
    assert_eq!(fs::read_dir(root).unwrap().count(), 0);
    assert!(
        apply(root, from_file())
            .unwrap_err()
            .contains("digest mismatch")
    );
    assert!(!root.join(TARGET).exists());
    assert!(!root.join(lockfile::NAME).exists());
    assert!(!pending(root).unwrap());
    fs::write(&source, NEW).unwrap();
    apply(root, from_file()).unwrap();
    assert_eq!(fs::read(root.join(TARGET)).unwrap(), NEW);
}

#[test]
fn validates_exact_dependencies_and_rejects_cycles() {
    use crate::lockfile::Dependency;
    let mut lock = lock(TARGET, NEW);
    lock.providers[0].provider_id = "v2:example.assets".into();
    let item = &mut lock.items[0];
    item.provider_id = "v2:example.assets".into();
    item.version = Some("1.0.0".into());

    let mut dependency = item.clone();
    dependency.item_id = "material".into();
    dependency.files[0].target = "material".into();
    dependency.files[0].declared_target = "material".into();
    let edge = |item: &Item| Dependency {
        provider_id: item.provider_id.clone(),
        item_id: item.item_id.clone(),
        version: item.version.clone().unwrap(),
        manifest_sha256: item.manifest_sha256.clone(),
    };
    item.dependencies.push(edge(&dependency));
    assert!(lock.encode().unwrap_err().contains("Unresolved"));
    lock.items.push(dependency);
    assert!(lock.encode().is_ok());
    lock.items[0].dependencies[0].version = "2.0.0".into();
    assert!(lock.encode().unwrap_err().contains("conflicting"));
    lock.items[0].dependencies[0].version = "1.0.0".into();
    let back = edge(&lock.items[0]);
    lock.items[1].dependencies.push(back);
    assert!(lock.encode().unwrap_err().contains("Cycle"));
}
