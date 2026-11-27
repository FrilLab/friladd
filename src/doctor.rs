use crate::{config, lockfile, registry, resolver, transaction};
use std::{env, fs, path::PathBuf};

pub struct Report {
    pub data: serde_json::Value,
    pub failed: bool,
}

pub fn inspect(project: Option<PathBuf>, online: bool) -> Result<Report, String> {
    let project_root = match project {
        Some(path) => path,
        None => env::current_dir()
            .map_err(|error| format!("Cannot determine current directory: {error}"))?,
    };
    if project_root.as_os_str().is_empty() {
        return Err("Project directory cannot be empty".into());
    }
    let project_root: PathBuf = project_root.components().collect();
    let project_config = config::project_path(&project_root);
    let global_config = config::global_path()?;
    let mut checks = Vec::<serde_json::Value>::new();

    doctor_check(
        &mut checks,
        "cli.version",
        "ok",
        format!(
            "FrilAdd {} on {}-{}",
            env!("CARGO_PKG_VERSION"),
            env::consts::OS,
            env::consts::ARCH
        ),
        None,
    );

    let canonical_root = match lockfile::canonical_project(&project_root) {
        Ok(root) => {
            doctor_check(
                &mut checks,
                "project.path",
                "ok",
                "Project root is a regular non-symlink directory",
                None,
            );
            Some(root)
        }
        Err(error) => {
            doctor_check(
                &mut checks,
                "project.path",
                "fail",
                error,
                Some("Pass an existing non-symlink project directory with --project."),
            );
            None
        }
    };

    let project_config_result = config::load(&project_config);
    match &project_config_result {
        Ok(_) => doctor_check(
            &mut checks,
            "config.project",
            "ok",
            "Project configuration is valid or absent",
            None,
        ),
        Err(error) => doctor_check(
            &mut checks,
            "config.project",
            "fail",
            error,
            Some("Repair or remove .friladd.json, then rerun doctor."),
        ),
    }
    let global_config_result = config::load(&global_config);
    match &global_config_result {
        Ok(_) => doctor_check(
            &mut checks,
            "config.global",
            "ok",
            "Global configuration is valid or absent",
            None,
        ),
        Err(error) => doctor_check(
            &mut checks,
            "config.global",
            "fail",
            error,
            Some("Repair the global FrilAdd configuration, then rerun doctor."),
        ),
    }

    let snapshot = if canonical_root.is_some() {
        match lockfile::load(&project_root) {
            Ok(snapshot) => {
                doctor_check(
                    &mut checks,
                    "lock.schema",
                    "ok",
                    "Lockfile is valid or absent",
                    None,
                );
                Some(snapshot)
            }
            Err(error) => {
                doctor_check(
                    &mut checks,
                    "lock.schema",
                    "fail",
                    error,
                    Some("Back up and repair friladd.lock.json before managed operations."),
                );
                None
            }
        }
    } else {
        doctor_check(
            &mut checks,
            "lock.schema",
            "skip",
            "Project root is unavailable",
            None,
        );
        None
    };

    let mut pending = false;
    if canonical_root.is_some() {
        match transaction::pending(&project_root) {
            Ok(true) => {
                pending = true;
                doctor_check(
                    &mut checks,
                    "transaction.recovery",
                    "fail",
                    "An interrupted FrilAdd transaction is present",
                    Some(
                        "Preserve the project and transaction state, then run a managed command to attempt recovery.",
                    ),
                );
            }
            Ok(false) => doctor_check(
                &mut checks,
                "transaction.recovery",
                "ok",
                "No interrupted transaction is present",
                None,
            ),
            Err(error) => {
                pending = true;
                doctor_check(
                    &mut checks,
                    "transaction.recovery",
                    "fail",
                    error,
                    Some(
                        "Inspect the reserved .friladd transaction path without deleting its contents.",
                    ),
                );
            }
        }
    } else {
        doctor_check(
            &mut checks,
            "transaction.recovery",
            "skip",
            "Project root is unavailable",
            None,
        );
    }

    let mut file_states = std::collections::BTreeMap::<String, usize>::new();
    let mut item_count = 0usize;
    let mut file_count = 0usize;
    if let Some(snapshot) = &snapshot {
        item_count = snapshot.lock().items.len();
        for item in &snapshot.lock().items {
            for file in &item.files {
                file_count += 1;
                let state = match transaction::observe(&project_root, &file.target) {
                    Ok(transaction::Observation::Content(content)) if content == file.installed => {
                        "unchanged"
                    }
                    Ok(transaction::Observation::Content(_)) => "modified",
                    Ok(transaction::Observation::Missing) => "missing",
                    Ok(transaction::Observation::Unsafe) => "unsafe",
                    Ok(transaction::Observation::Unreadable) => "unreadable",
                    Err(_) => "unsafe",
                };
                *file_states.entry(state.to_string()).or_default() += 1;
            }
        }
        let unsafe_count = *file_states.get("unsafe").unwrap_or(&0);
        let unreadable_count = *file_states.get("unreadable").unwrap_or(&0);
        let drift_count = file_count - *file_states.get("unchanged").unwrap_or(&0);
        let state = if unsafe_count > 0 {
            "fail"
        } else if drift_count > 0 {
            "warn"
        } else {
            "ok"
        };
        let message = format!(
            "Inspected {file_count} owned file(s): {} unchanged, {} modified, {} missing, {} unsafe, {} unreadable",
            file_states.get("unchanged").unwrap_or(&0),
            file_states.get("modified").unwrap_or(&0),
            file_states.get("missing").unwrap_or(&0),
            unsafe_count,
            unreadable_count
        );
        let action = if unsafe_count > 0 {
            Some("Resolve unsafe paths before any managed operation.")
        } else if drift_count > 0 {
            Some(
                "Run fril status for per-file details; review local edits before restoring or updating.",
            )
        } else {
            None
        };
        doctor_check(&mut checks, "managed.files", state, message, action);
    } else {
        doctor_check(
            &mut checks,
            "managed.files",
            "skip",
            "Lockfile could not be inspected",
            None,
        );
    }

    let mut package_managers = Vec::new();
    if let Some(root) = &canonical_root {
        let package_manifest = root.join("package.json");
        match fs::symlink_metadata(&package_manifest) {
            Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => {
                for (name, path) in [
                    ("npm", "package-lock.json"),
                    ("pnpm", "pnpm-lock.yaml"),
                    ("yarn", "yarn.lock"),
                    ("bun", "bun.lock"),
                    ("bun", "bun.lockb"),
                ] {
                    match fs::symlink_metadata(root.join(path)) {
                        Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => {
                            package_managers.push(name)
                        }
                        Ok(_) => doctor_check(
                            &mut checks,
                            "package.manager",
                            "fail",
                            format!("Package manager lock path {path} is not a regular file"),
                            Some("Replace the unsafe lock path with a regular file."),
                        ),
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
                        Err(error) => doctor_check(
                            &mut checks,
                            "package.manager",
                            "fail",
                            format!("Cannot inspect {path}: {error}"),
                            Some("Check project permissions and filesystem health."),
                        ),
                    }
                }
                package_managers.sort_unstable();
                package_managers.dedup();
                if package_managers.is_empty() {
                    doctor_check(
                        &mut checks,
                        "package.manager",
                        "warn",
                        "package.json exists but no npm, pnpm, yarn, or bun lockfile was found",
                        Some(
                            "Choose a package manager and create its lockfile if the consumer project needs one.",
                        ),
                    );
                } else if package_managers.len() > 1 {
                    doctor_check(
                        &mut checks,
                        "package.manager",
                        "warn",
                        "Multiple package manager lockfiles are present",
                        Some(
                            "Keep only the lockfile for the package manager used by this project.",
                        ),
                    );
                } else {
                    doctor_check(
                        &mut checks,
                        "package.manager",
                        "ok",
                        format!("Detected {} package manager lockfile", package_managers[0]),
                        None,
                    );
                }
            }
            Ok(_) => doctor_check(
                &mut checks,
                "package.manager",
                "fail",
                "package.json is not a regular non-symlink file",
                Some("Replace the unsafe package.json path before using package tooling."),
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => doctor_check(
                &mut checks,
                "package.manager",
                "skip",
                "No package.json is present; the native CLI does not require Node.js",
                None,
            ),
            Err(error) => doctor_check(
                &mut checks,
                "package.manager",
                "fail",
                format!("Cannot inspect package.json: {error}"),
                Some("Check project permissions and filesystem health."),
            ),
        }
    } else {
        doctor_check(
            &mut checks,
            "package.manager",
            "skip",
            "Project root is unavailable",
            None,
        );
    }

    let config_valid = project_config_result.is_ok() && global_config_result.is_ok();
    let mut resolved_default = None;
    if config_valid {
        match resolver::resolve_list_selection(None, None, &project_config, &global_config) {
            Ok(selection) => {
                resolved_default = selection.alias.clone();
                doctor_check(
                    &mut checks,
                    "registry.default",
                    "ok",
                    format!(
                        "Resolved default registry alias '{}' without fetching its manifest",
                        selection.alias.as_deref().unwrap_or("(explicit source)")
                    ),
                    None,
                );
            }
            Err(error) if error.starts_with("No default registry") => doctor_check(
                &mut checks,
                "registry.default",
                "warn",
                "No default registry is configured",
                Some(
                    "Register a provider with fril registry add ALIAS SOURCE --default, or select --registry per command.",
                ),
            ),
            Err(error) => doctor_check(
                &mut checks,
                "registry.default",
                "fail",
                error,
                Some("Repair the configured default alias and rerun doctor."),
            ),
        }
    } else {
        doctor_check(
            &mut checks,
            "registry.default",
            "skip",
            "Configuration could not be validated",
            None,
        );
    }

    if online {
        if config_valid {
            match resolver::effective_registries(&project_config, &global_config) {
                Ok(registries) if registries.is_empty() => doctor_check(
                    &mut checks,
                    "registry.online",
                    "warn",
                    "No configured registries are available for online validation",
                    Some("Register a registry before running provider validation."),
                ),
                Ok(registries) => {
                    for (alias, location) in registries {
                        match registry::load_registry(&location) {
                            Ok(loaded) => doctor_check(
                                &mut checks,
                                &format!("registry.online.{alias}"),
                                "ok",
                                format!(
                                    "Registry manifest '{}' is reachable and valid ({})",
                                    loaded.registry.name, loaded.manifest_sha256
                                ),
                                None,
                            ),
                            Err(error) => doctor_check(
                                &mut checks,
                                &format!("registry.online.{alias}"),
                                "fail",
                                format!("Registry manifest validation failed: {error}"),
                                Some(
                                    "Check the registry URL, HTTPS availability, and manifest schema; then run fril registry verify.",
                                ),
                            ),
                        }
                    }
                }
                Err(error) => doctor_check(
                    &mut checks,
                    "registry.online",
                    "fail",
                    error,
                    Some(
                        "Reduce the effective registry set or repair configuration before online validation.",
                    ),
                ),
            }
        } else {
            doctor_check(
                &mut checks,
                "registry.online",
                "skip",
                "Configuration could not be validated",
                None,
            );
        }
    } else {
        doctor_check(
            &mut checks,
            "registry.online",
            "skip",
            "Online registry checks were not requested",
            None,
        );
    }

    if snapshot.is_some() {
        doctor_check(
            &mut checks,
            "package.requirements",
            "skip",
            "The current lock schema does not store package requirements",
            None,
        );
    } else {
        doctor_check(
            &mut checks,
            "package.requirements",
            "skip",
            "Lockfile could not be inspected",
            None,
        );
    }

    let mut summary = std::collections::BTreeMap::<String, usize>::new();
    for check in &checks {
        *summary
            .entry(check["state"].as_str().unwrap_or("fail").to_owned())
            .or_default() += 1;
    }
    for state in ["ok", "warn", "fail", "skip"] {
        summary.entry(state.to_owned()).or_default();
    }
    let failed = summary.get("fail").copied().unwrap_or_default() > 0;
    let data = serde_json::json!({
        "project": canonical_root.as_ref().map(|path| path.display().to_string()),
        "version": env!("CARGO_PKG_VERSION"),
        "platform": { "os": env::consts::OS, "arch": env::consts::ARCH },
        "defaultRegistry": resolved_default,
        "packageManagers": package_managers,
        "managed": { "itemCount": item_count, "fileCount": file_count, "fileStates": file_states },
        "summary": summary,
        "checks": checks,
        "transactionPending": pending,
    });
    Ok(Report { data, failed })
}

fn doctor_check(
    checks: &mut Vec<serde_json::Value>,
    code: &str,
    state: &str,
    message: impl Into<String>,
    action: Option<&str>,
) {
    checks.push(serde_json::json!({
        "code": code,
        "state": state,
        "message": terminal_safe(&message.into()),
        "action": action.map(terminal_safe),
    }));
}

pub fn print_human(data: &serde_json::Value) {
    println!(
        "FrilAdd doctor {} on {}-{}",
        data["version"].as_str().unwrap_or(""),
        data["platform"]["os"].as_str().unwrap_or(""),
        data["platform"]["arch"].as_str().unwrap_or("")
    );
    for check in data["checks"].as_array().into_iter().flatten() {
        println!(
            "[{}] {}: {}",
            check["state"].as_str().unwrap_or("fail").to_uppercase(),
            check["code"].as_str().unwrap_or("unknown"),
            check["message"].as_str().unwrap_or("")
        );
        if let Some(action) = check["action"].as_str() {
            println!("  Next: {action}");
        }
    }
}

fn terminal_safe(value: &str) -> String {
    value
        .chars()
        .flat_map(|character| {
            if character.is_control() {
                character.escape_default().collect::<Vec<_>>()
            } else {
                vec![character]
            }
        })
        .collect()
}
