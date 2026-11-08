use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::registry::{RegistryItem, validate_relative_path};

#[derive(Debug)]
pub struct PlannedFile {
    pub source: String,
    pub target: PathBuf,
    pub target_display: String,
    pub destination: PathBuf,
    pub expected_sha256: String,
    pub exists: bool,
}

#[derive(Debug)]
pub struct InstallationPlan {
    pub project_root: PathBuf,
    pub files: Vec<PlannedFile>,
    pub conflicts: Vec<String>,
}

pub fn build_plan(project: &Path, item: &RegistryItem) -> Result<InstallationPlan, String> {
    let project_root = fs::canonicalize(project).map_err(|error| {
        format!(
            "Cannot resolve project directory {}: {error}",
            project.display()
        )
    })?;
    let root_metadata = fs::metadata(&project_root).map_err(|error| {
        format!(
            "Cannot inspect project directory {}: {error}",
            project_root.display()
        )
    })?;
    if !root_metadata.is_dir() {
        return Err(format!(
            "Project path is not a directory: {}",
            project_root.display()
        ));
    }

    let mut files = Vec::with_capacity(item.files.len());
    let mut conflicts = Vec::new();
    for file in &item.files {
        let target_display = validate_relative_path(&file.target, "target")?;
        let target = PathBuf::from(&target_display);
        let destination = project_root.join(&target);
        let exists = validate_destination(&project_root, &target, &destination)?;
        if exists {
            conflicts.push(target_display.clone());
        }
        files.push(PlannedFile {
            source: file.source.clone(),
            target,
            target_display,
            destination,
            expected_sha256: file.sha256.clone(),
            exists,
        });
    }
    Ok(InstallationPlan {
        project_root,
        files,
        conflicts,
    })
}

/// Validates every existing path component with filesystem metadata. Symlinks are
/// rejected (even when they currently point inside the project) to avoid redirects.
pub fn validate_destination(
    root: &Path,
    relative: &Path,
    destination: &Path,
) -> Result<bool, String> {
    let mut current = root.to_path_buf();
    let components: Vec<_> = relative.components().collect();
    for (index, component) in components.iter().enumerate() {
        let name = match component {
            std::path::Component::Normal(name) => name,
            _ => return Err(format!("Unsafe destination path: {}", relative.display())),
        };
        current.push(name);
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => {
                return Err(format!(
                    "Cannot inspect destination {}: {error}",
                    current.display()
                ));
            }
        };
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "Destination contains a symlink: {}",
                current.display()
            ));
        }
        let final_component = index + 1 == components.len();
        if !final_component && !metadata.is_dir() {
            return Err(format!(
                "Destination parent is not a directory: {}",
                current.display()
            ));
        }
        if final_component && !metadata.is_file() {
            return Err(format!(
                "Destination is not a regular file: {}",
                current.display()
            ));
        }
        let canonical = fs::canonicalize(&current).map_err(|error| {
            format!("Cannot resolve destination {}: {error}", current.display())
        })?;
        if !canonical.starts_with(root) {
            return Err(format!(
                "Destination escapes project directory: {}",
                relative.display()
            ));
        }
    }
    if current != destination {
        return Err(format!("Destination path mismatch: {}", relative.display()));
    }
    Ok(true)
}

pub fn validate_file_plan(plan: &InstallationPlan) -> Result<(), String> {
    let mut conflicts = Vec::new();
    for file in &plan.files {
        if validate_destination(&plan.project_root, &file.target, &file.destination)? {
            conflicts.push(file.target_display.clone());
        }
    }
    if !conflicts.is_empty() {
        return Err(format!(
            "Destination file(s) already exist: {}",
            conflicts.join(", ")
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::RegistryFile;
    use tempfile::tempdir;

    fn item(target: &str) -> RegistryItem {
        RegistryItem {
            name: "sample".into(),
            description: "A sample".into(),
            files: vec![RegistryFile {
                source: "source.txt".into(),
                target: target.into(),
                sha256: "0".repeat(64),
            }],
        }
    }

    #[test]
    fn detects_existing_file_as_conflict() {
        let project = tempdir().unwrap();
        fs::create_dir_all(project.path().join("src")).unwrap();
        fs::write(project.path().join("src/file.tsx"), "existing").unwrap();
        let plan = build_plan(project.path(), &item("src/file.tsx")).unwrap();
        assert!(plan.files[0].exists);
        assert_eq!(plan.conflicts, ["src/file.tsx"]);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_destination_escape() {
        use std::os::unix::fs::symlink;
        let project = tempdir().unwrap();
        let outside = tempdir().unwrap();
        symlink(outside.path(), project.path().join("src")).unwrap();
        assert!(
            build_plan(project.path(), &item("src/file.tsx"))
                .unwrap_err()
                .contains("symlink")
        );
    }

    #[test]
    fn accepts_missing_nested_destination_without_creating_it() {
        let project = tempdir().unwrap();
        let plan = build_plan(project.path(), &item("src/nested/file.tsx")).unwrap();
        assert!(!plan.files[0].exists);
        assert!(!project.path().join("src").exists());
    }

    #[test]
    fn rejects_destination_parents_that_are_regular_files() {
        let project = tempdir().unwrap();
        fs::write(project.path().join("src"), "not a directory").unwrap();
        assert!(
            build_plan(project.path(), &item("src/file.tsx"))
                .unwrap_err()
                .contains("parent is not a directory")
        );
    }
}
