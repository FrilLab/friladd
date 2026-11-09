use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use crate::{
    downloader::MAX_FILE_BYTES,
    planner::{InstallationPlan, validate_file_plan},
};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub fn install(plan: &InstallationPlan, contents: &[Vec<u8>]) -> Result<(), String> {
    if contents.len() != plan.files.len() {
        return Err("Internal error: content count does not match installation plan".to_string());
    }
    if contents
        .iter()
        .any(|content| content.len() > MAX_FILE_BYTES)
    {
        return Err(format!(
            "Internal error: file content exceeds the {MAX_FILE_BYTES} byte limit"
        ));
    }
    validate_file_plan(plan)?;

    let mut created_directories = Vec::new();
    let mut staged_files: Vec<PathBuf> = Vec::new();
    let mut installed_files: Vec<PathBuf> = Vec::new();
    let result = (|| {
        for file in &plan.files {
            ensure_parent_directories(&plan.project_root, &file.target, &mut created_directories)?;
        }
        for (file, content) in plan.files.iter().zip(contents) {
            let temp = stage_file(&file.destination, content)?;
            staged_files.push(temp);
        }
        for (file, staged) in plan.files.iter().zip(&staged_files) {
            // Recheck the path immediately before the no-overwrite link operation.
            if crate::planner::validate_destination(
                &plan.project_root,
                &file.target,
                &file.destination,
            )? {
                return Err(format!(
                    "Destination file appeared during installation: {}",
                    file.target_display
                ));
            }
            fs::hard_link(staged, &file.destination)
                .map_err(|error| format!("Cannot install {}: {error}", file.target_display))?;
            installed_files.push(file.destination.clone());
        }
        Ok(())
    })();

    if let Err(error) = result {
        let cleanup_errors = cleanup(&installed_files, &staged_files, &created_directories);
        return if cleanup_errors.is_empty() {
            Err(error)
        } else {
            Err(format!(
                "{error}; cleanup also failed: {}",
                cleanup_errors.join("; ")
            ))
        };
    }

    let mut cleanup_errors = Vec::new();
    for temp in &staged_files {
        if let Err(error) = fs::remove_file(temp) {
            cleanup_errors.push(format!("{}: {error}", temp.display()));
        }
    }
    if !cleanup_errors.is_empty() {
        // Installed content is already complete and verified; temporary names are harmless,
        // but report them so a user can remove any left behind.
        return Err(format!(
            "Files were installed, but temporary cleanup failed: {}",
            cleanup_errors.join("; ")
        ));
    }
    Ok(())
}

fn ensure_parent_directories(
    root: &Path,
    target: &Path,
    created: &mut Vec<PathBuf>,
) -> Result<(), String> {
    let mut current = root.to_path_buf();
    let components: Vec<_> = target.components().collect();
    for component in components.iter().take(components.len().saturating_sub(1)) {
        let name = match component {
            Component::Normal(name) => name,
            _ => return Err(format!("Unsafe destination path: {}", target.display())),
        };
        current.push(name);
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(format!(
                        "Destination contains a symlink: {}",
                        current.display()
                    ));
                }
                if !metadata.is_dir() {
                    return Err(format!(
                        "Destination parent is not a directory: {}",
                        current.display()
                    ));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                match fs::create_dir(&current) {
                    Ok(()) => created.push(current.clone()),
                    Err(create_error)
                        if create_error.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(create_error) => {
                        return Err(format!(
                            "Cannot create destination directory {}: {create_error}",
                            current.display()
                        ));
                    }
                }
                let metadata = fs::symlink_metadata(&current).map_err(|inspect_error| {
                    format!(
                        "Cannot inspect destination directory {}: {inspect_error}",
                        current.display()
                    )
                })?;
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err(format!(
                        "Destination directory changed during installation: {}",
                        current.display()
                    ));
                }
            }
            Err(error) => {
                return Err(format!(
                    "Cannot inspect destination directory {}: {error}",
                    current.display()
                ));
            }
        }
        let canonical = fs::canonicalize(&current).map_err(|error| {
            format!(
                "Cannot resolve destination directory {}: {error}",
                current.display()
            )
        })?;
        if !canonical.starts_with(root) {
            return Err(format!(
                "Destination escapes project directory: {}",
                target.display()
            ));
        }
    }
    Ok(())
}

fn stage_file(destination: &Path, content: &[u8]) -> Result<PathBuf, String> {
    let parent = destination
        .parent()
        .ok_or_else(|| format!("Destination has no parent: {}", destination.display()))?;
    destination
        .file_name()
        .ok_or_else(|| format!("Destination has no filename: {}", destination.display()))?;
    for _ in 0..100 {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(".friladd-{}-{sequence}.tmp", std::process::id()));
        let mut file = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("Cannot stage {}: {error}", destination.display())),
        };
        if let Err(error) = write_and_sync(&mut file, content) {
            let _ = fs::remove_file(&temporary);
            return Err(format!("Cannot stage {}: {error}", destination.display()));
        }
        return Ok(temporary);
    }
    Err(format!(
        "Cannot allocate a temporary filename for {}",
        destination.display()
    ))
}

fn write_and_sync(file: &mut File, content: &[u8]) -> std::io::Result<()> {
    file.write_all(content)?;
    file.sync_all()
}

fn cleanup(installed: &[PathBuf], staged: &[PathBuf], directories: &[PathBuf]) -> Vec<String> {
    let mut errors = Vec::new();
    for path in installed.iter().rev().chain(staged.iter().rev()) {
        if let Err(error) = fs::remove_file(path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            errors.push(format!("{}: {error}", path.display()));
        }
    }
    for path in directories.iter().rev() {
        if let Err(error) = fs::remove_dir(path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            errors.push(format!("{}: {error}", path.display()));
        }
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        planner,
        registry::{RegistryFile, RegistryItem},
    };
    use tempfile::tempdir;

    fn component() -> RegistryItem {
        RegistryItem {
            name: "sample".into(),
            description: "A sample".into(),
            files: vec![
                RegistryFile {
                    source: "one.txt".into(),
                    target: "src/components/sample/one.txt".into(),
                    sha256: "0".repeat(64),
                },
                RegistryFile {
                    source: "two.txt".into(),
                    target: "src/components/sample/two.txt".into(),
                    sha256: "0".repeat(64),
                },
            ],
        }
    }

    #[test]
    fn installs_exact_bytes_and_creates_only_required_directories() {
        let project = tempdir().unwrap();
        fs::write(project.path().join("keep.txt"), "untouched").unwrap();
        let item = component();
        let plan = planner::build_plan(project.path(), &item).unwrap();
        install(
            &plan,
            &[b"first\0bytes".to_vec(), b"second\nbytes".to_vec()],
        )
        .unwrap();
        assert_eq!(
            fs::read(project.path().join(&item.files[0].target)).unwrap(),
            b"first\0bytes"
        );
        assert_eq!(
            fs::read(project.path().join(&item.files[1].target)).unwrap(),
            b"second\nbytes"
        );
        assert_eq!(
            fs::read(project.path().join("keep.txt")).unwrap(),
            b"untouched"
        );
    }

    #[test]
    fn refuses_overwrites_without_changing_existing_file() {
        let project = tempdir().unwrap();
        let item = component();
        let target = project.path().join(&item.files[0].target);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, b"keep me").unwrap();
        let plan = planner::build_plan(project.path(), &item).unwrap();
        assert!(
            install(&plan, &[b"new one".to_vec(), b"new two".to_vec()])
                .unwrap_err()
                .contains("already exist")
        );
        assert_eq!(fs::read(target).unwrap(), b"keep me");
    }
}
