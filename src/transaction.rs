//! Project mutation and journal recovery. Callers plan from an ownership snapshot.

use crate::{
    lockfile::{self, Content, Lock, Snapshot},
    planner::validate_destination,
    registry,
};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{ErrorKind, Read, Write},
    path::{Path, PathBuf},
};

pub const MAX_OPERATION_BYTES: u64 = 512 * 1024 * 1024;
const MAX_EDITS: usize = 100_000;
const MAX_JOURNAL_BYTES: usize = 32 * 1024 * 1024;
const STATE: &str = ".friladd";
const PREPARING: &str = "preparing";
const ACTIVE: &str = "transaction";
const CLEANUP: &str = "cleanup";
const JOURNAL: &str = "journal.json";
const COMMITTED: &str = "committed";
const BEFORE_LOCK: &str = "before-lock";
const AFTER_LOCK: &str = "after-lock";
const SCRATCH: &str = "scratch";

#[derive(Debug)]
pub enum WriteSource {
    Bytes(Vec<u8>),
    Shared(std::sync::Arc<[u8]>),
    File(PathBuf),
}

#[derive(Debug)]
pub struct Edit {
    pub target: String,
    pub before: Option<Content>,
    pub write: Option<WriteSource>,
}

pub struct Plan {
    before: Snapshot,
    next: Lock,
    edits: BTreeMap<String, Edit>,
    expectations: Vec<Expectation>,
    next_bytes: Option<Vec<u8>>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Expectation {
    target: String,
    before: Option<Content>,
    after: Option<Content>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Journal {
    schema_version: u32,
    edits: Vec<Expectation>,
    directories: Vec<String>,
    before_lock: Option<Content>,
    after_lock: Option<Content>,
}

struct Project {
    root: PathBuf,
    state: PathBuf,
    _lock: File,
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self._lock);
    }
}

impl Plan {
    /// An edit that creates a path must explicitly expect it to be absent.
    /// Removals require ownership even if the path is already absent.
    pub fn new(before: Snapshot, next: Lock, edits: Vec<Edit>) -> Result<Self, String> {
        let next_bytes = if before.lock == next {
            before.bytes.clone()
        } else {
            Some(next.encode()?)
        };
        let edits = unique_edits(edits)?;
        let targets = next.owners();
        let expectations: Vec<_> = edits
            .values()
            .map(|edit| {
                let after = if edit.write.is_some() {
                    Some(
                        targets
                            .get(edit.target.as_str())
                            .ok_or("Write target is unowned in next lock")?
                            .1
                            .installed
                            .clone(),
                    )
                } else {
                    None
                };
                Ok(Expectation {
                    target: edit.target.clone(),
                    before: edit.before.clone(),
                    after,
                })
            })
            .collect::<Result<_, String>>()?;
        validate_ownership(&before.lock, &next, &expectations)?;
        Ok(Self {
            before,
            next,
            edits,
            expectations,
            next_bytes,
        })
    }

    /// Check the same file and lock preconditions used by apply without creating state.
    pub fn check(&self, project: &Path) -> Result<(), String> {
        let root = lockfile::canonical_project(project)?;
        if pending_at(&root)? {
            return Err(format!(
                "Project has an interrupted transaction at {}",
                root.join(STATE).display()
            ));
        }
        self.check_at(&root)?;
        for expect in &self.expectations {
            if let Some(input) = &self.edits[&expect.target].write {
                let content = match input {
                    WriteSource::Bytes(bytes) => Content::of(bytes),
                    WriteSource::Shared(bytes) => Content::of(bytes),
                    WriteSource::File(path) => fingerprint(path, lockfile::MAX_FILE_BYTES)?,
                };
                if expect.after.as_ref() != Some(&content) {
                    return Err(format!("Digest mismatch for {}", expect.target));
                }
            }
        }
        Ok(())
    }

    fn check_at(&self, root: &Path) -> Result<(), String> {
        if lockfile::load(root)?.bytes != self.before.bytes {
            return Err("Project lock changed since planning; resolve and plan again".into());
        }
        for edit in &self.expectations {
            if inspect_at(root, &edit.target)? != edit.before {
                return Err(format!(
                    "Destination changed or is unowned: {}",
                    edit.target
                ));
            }
        }
        Ok(())
    }
}

fn unique_edits(edits: Vec<Edit>) -> Result<BTreeMap<String, Edit>, String> {
    if edits.len() > MAX_EDITS {
        return Err("Transaction has too many edits".into());
    }
    let mut sorted = BTreeMap::new();
    for edit in edits {
        if sorted.insert(edit.target.clone(), edit).is_some() {
            return Err("Transaction repeats a target".into());
        }
    }
    Ok(sorted)
}

fn validate_ownership(before: &Lock, after: &Lock, edits: &[Expectation]) -> Result<(), String> {
    if edits.len() > MAX_EDITS {
        return Err("Transaction has too many edits".into());
    }
    let old = before.owners();
    let new = after.owners();
    let by_target: BTreeMap<_, _> = edits
        .iter()
        .map(|edit| (edit.target.as_str(), edit))
        .collect();
    registry::validate_targets(edits.iter().map(|edit| edit.target.as_str()))?;
    let mut backup_bytes = 0;
    let mut new_bytes = 0;
    for edit in edits {
        let old_owner = old.get(edit.target.as_str());
        let new_owner = new.get(edit.target.as_str());
        let old_content = old_owner.map(|(_, file)| &file.installed);
        let new_content = new_owner.map(|(_, file)| &file.installed);
        if edit
            .before
            .as_ref()
            .is_some_and(|state| Some(state) != old_content)
            || edit.after.as_ref() != new_content
            || (edit.after.is_none() && old_content.is_none())
            || (old_content.is_some()
                && new_content.is_some()
                && old_owner.map(|(i, _)| (&i.provider_id, &i.item_id))
                    != new_owner.map(|(i, _)| (&i.provider_id, &i.item_id)))
            || (edit.before.is_none()
                && old_content.is_some()
                && new_content.is_some()
                && old_content != new_content)
        {
            return Err(format!(
                "Edit disagrees with lock ownership: {}",
                edit.target
            ));
        }
        if let Some(content) = &edit.before {
            content.validate()?;
            backup_bytes += content.size;
        }
        if let Some(content) = &edit.after {
            content.validate()?;
            new_bytes += content.size;
        }
        if backup_bytes > MAX_OPERATION_BYTES || new_bytes > MAX_OPERATION_BYTES {
            return Err(
                "Transaction exceeds the 512 MiB byte limit for either staged files or backups"
                    .into(),
            );
        }
    }
    for (&target, (item, file)) in &new {
        let previous = old.get(target);
        if let Some((old_item, _)) = previous
            && (&old_item.provider_id, &old_item.item_id) != (&item.provider_id, &item.item_id)
        {
            return Err(format!("Cannot transfer ownership implicitly: {target}"));
        }
        if previous.is_none_or(|(_, old_file)| old_file.installed != file.installed)
            && !by_target.contains_key(target)
        {
            return Err(format!("Missing transaction write: {target}"));
        }
    }
    for &target in old.keys() {
        if !new.contains_key(target) && !by_target.contains_key(target) {
            return Err(format!("Missing transaction removal: {target}"));
        }
    }
    Ok(())
}

pub fn inspect(project: &Path, target: &str) -> Result<Option<Content>, String> {
    registry::validate_targets([target])?;
    inspect_at(&lockfile::canonical_project(project)?, target)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Observation {
    Missing,
    Content(Content),
    Unsafe,
    Unreadable,
}

/// Inspect one owned target without following symlinks or changing project state.
pub fn observe(project: &Path, target: &str) -> Result<Observation, String> {
    registry::validate_targets([target])?;
    let root = lockfile::canonical_project(project)?;
    let mut current = root.clone();
    let segments: Vec<_> = target.split('/').collect();
    for (index, segment) in segments.iter().enumerate() {
        current.push(segment);
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                return Ok(Observation::Missing);
            }
            Err(_) => return Ok(Observation::Unreadable),
        };
        if metadata.file_type().is_symlink()
            || (index + 1 < segments.len() && !metadata.is_dir())
            || (index + 1 == segments.len() && !metadata.is_file())
        {
            return Ok(Observation::Unsafe);
        }
        let canonical = match fs::canonicalize(&current) {
            Ok(path) => path,
            Err(_) => return Ok(Observation::Unreadable),
        };
        if !canonical.starts_with(&root) {
            return Ok(Observation::Unsafe);
        }
    }
    match fingerprint(&current, lockfile::MAX_FILE_BYTES) {
        Ok(content) => Ok(Observation::Content(content)),
        Err(_) => Ok(Observation::Unreadable),
    }
}

fn inspect_at(root: &Path, target: &str) -> Result<Option<Content>, String> {
    let path = root.join(target);
    if !validate_destination(root, Path::new(target), &path)? {
        return Ok(None);
    }
    fingerprint(&path, lockfile::MAX_FILE_BYTES)
        .map(Some)
        .map_err(|error| format!("{target}: {error}"))
}

pub fn pending(project: &Path) -> Result<bool, String> {
    pending_at(&lockfile::canonical_project(project)?)
}

fn pending_at(root: &Path) -> Result<bool, String> {
    let state = root.join(STATE);
    if !directory_exists(&state)? {
        return Ok(false);
    }
    let mut pending = false;
    for name in [PREPARING, ACTIVE, CLEANUP] {
        pending |= directory_exists(&state.join(name))?;
    }
    Ok(pending)
}

pub fn recover(project: &Path) -> Result<(), String> {
    let root = lockfile::canonical_project(project)?;
    if pending_at(&root)? {
        Project::open(root)?.recover()?;
    }
    Ok(())
}

pub fn apply(project: &Path, plan: Plan) -> Result<(), String> {
    apply_with(project, plan, |_| Ok(()))
}

fn apply_with(
    project: &Path,
    plan: Plan,
    mut checkpoint: impl FnMut(&str) -> Result<(), String>,
) -> Result<(), String> {
    let root = lockfile::canonical_project(project)?;
    if plan.edits.is_empty() && plan.before.bytes == plan.next_bytes && !pending_at(&root)? {
        return plan.check_at(&root);
    }
    let project = Project::open(root)?;
    project.recover()?;
    plan.check_at(&project.root)?;
    if plan.edits.is_empty() && plan.before.bytes == plan.next_bytes {
        return Ok(());
    }

    let preparing = project.state.join(PREPARING);
    let active = project.state.join(ACTIVE);
    let result = (|| {
        fs::create_dir(&preparing).map_err(|e| fs_error(&preparing, e))?;
        sync_dir(&project.state)?;
        checkpoint("preparing")?;
        let journal = project.stage(plan, &preparing)?;
        checkpoint("staged")?;
        fs::rename(&preparing, &active).map_err(|e| fs_error(&active, e))?;
        sync_dir(&project.state)?;
        checkpoint("journal")?;
        for directory in &journal.directories {
            project.ensure_directory(directory)?;
            checkpoint("directory")?;
        }
        for (index, edit) in journal.edits.iter().enumerate() {
            project.change_file(&active, edit, index, true)?;
            checkpoint("file")?;
        }
        project.change_lock(&active, &journal, true)?;
        checkpoint("lock")?;
        write_small(&active.join(COMMITTED), b"committed\n")?;
        sync_dir(&active)?;
        checkpoint("committed")?;
        project.retire(&active)?;
        checkpoint("cleanup")?;
        Ok(())
    })();
    if let Err(error) = result {
        return match project.recover() {
            Ok(()) => Err(error),
            Err(recovery) => Err(format!("{error}; {recovery}")),
        };
    }
    Ok(())
}

impl Project {
    fn open(root: PathBuf) -> Result<Self, String> {
        let state = root.join(STATE);
        ensure_directory(&state)?;
        let path = state.join("mutation.lock");
        if let Ok(meta) = fs::symlink_metadata(&path)
            && (!meta.is_file() || meta.file_type().is_symlink())
        {
            return Err(format!("Unsafe mutation lock: {}", path.display()));
        }
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|e| fs_error(&path, e))?;
        FileExt::try_lock_exclusive(&lock).map_err(|error| {
            format!(
                "Cannot acquire project mutation lock {}: {error}",
                path.display()
            )
        })?;
        Ok(Self {
            root,
            state,
            _lock: lock,
        })
    }

    fn stage(&self, plan: Plan, work: &Path) -> Result<Journal, String> {
        let mut directories = BTreeSet::new();
        let mut edits = plan.edits;
        for (index, expect) in plan.expectations.iter().enumerate() {
            let path = self.root.join(&expect.target);
            if let Some(before) = &expect.before {
                validate_destination(&self.root, Path::new(&expect.target), &path)?;
                copy_verified(&path, &work.join(format!("before-{index}")), before, true)?;
            }
            if let Some(after) = &expect.after {
                for (end, _) in expect.target.match_indices('/') {
                    let ancestor = &expect.target[..end];
                    if !directory_exists(&self.root.join(ancestor))? {
                        directories.insert(ancestor.to_string());
                    }
                }
                let output = work.join(format!("after-{index}"));
                let write = edits
                    .remove(&expect.target)
                    .and_then(|edit| edit.write)
                    .ok_or("Missing staged bytes")?;
                match write {
                    WriteSource::Bytes(bytes) => {
                        if Content::of(&bytes) != *after {
                            return Err(format!("Verified bytes changed for {}", expect.target));
                        }
                        write_small(&output, &bytes)?;
                    }
                    WriteSource::Shared(bytes) => {
                        if Content::of(&bytes) != *after {
                            return Err(format!("Verified bytes changed for {}", expect.target));
                        }
                        write_small(&output, &bytes)?;
                    }
                    WriteSource::File(path) => copy_verified(&path, &output, after, false)?,
                }
            }
        }
        let before_lock = stage_lock(work, BEFORE_LOCK, plan.before.bytes.as_deref())?;
        let after_lock = stage_lock(work, AFTER_LOCK, plan.next_bytes.as_deref())?;
        let journal = Journal {
            schema_version: 1,
            edits: plan.expectations,
            directories: directories.into_iter().collect(),
            before_lock,
            after_lock,
        };
        validate_journal(&journal, &plan.before.lock, &plan.next)?;
        let bytes = serde_json::to_vec(&journal).map_err(|error| error.to_string())?;
        if bytes.len() > MAX_JOURNAL_BYTES {
            return Err("Transaction journal exceeds the byte limit".into());
        }
        write_small(&work.join(JOURNAL), &bytes)?;
        sync_dir(work)?;
        Ok(journal)
    }

    fn ensure_directory(&self, relative: &str) -> Result<(), String> {
        let path = self.root.join(relative);
        let parent = path.parent().expect("validated relative directory");
        check_directories(&self.root, parent)?;
        ensure_directory(&path)
    }

    fn change_file(
        &self,
        work: &Path,
        edit: &Expectation,
        index: usize,
        forward: bool,
    ) -> Result<(), String> {
        let (expected, desired, backup) = if forward {
            (&edit.before, &edit.after, format!("after-{index}"))
        } else {
            (&edit.after, &edit.before, format!("before-{index}"))
        };
        let target = self.root.join(&edit.target);
        self.change(work, &target, &backup, (expected, desired), forward, || {
            inspect_at(&self.root, &edit.target)
        })
    }

    fn change_lock(&self, work: &Path, journal: &Journal, forward: bool) -> Result<(), String> {
        let (expected, desired, backup) = if forward {
            (&journal.before_lock, &journal.after_lock, AFTER_LOCK)
        } else {
            (&journal.after_lock, &journal.before_lock, BEFORE_LOCK)
        };
        self.change(
            work,
            &self.root.join(lockfile::NAME),
            backup,
            (expected, desired),
            forward,
            || {
                Ok(lockfile::read_optional(&self.root.join(lockfile::NAME))?
                    .as_deref()
                    .map(Content::of))
            },
        )
    }

    fn change(
        &self,
        work: &Path,
        target: &Path,
        backup: &str,
        (expected, desired): (&Option<Content>, &Option<Content>),
        forward: bool,
        inspect: impl Fn() -> Result<Option<Content>, String>,
    ) -> Result<(), String> {
        let current = inspect()?;
        if !forward && &current == desired {
            return Ok(());
        }
        if &current != expected {
            return Err(format!("Unexpected bytes at {}", target.display()));
        }

        if &current == desired {
            return Ok(());
        }
        let parent = target.parent().expect("project-relative target");
        check_directories(&self.root, parent)?;
        if let Some(desired) = desired {
            let saved = work.join(backup);
            if forward && current.is_none() {
                // Retain the original hard link until retirement. Recovery can
                // distinguish this creation from a same-byte external file.
                if fingerprint(&saved, desired.size)? != *desired || inspect()?.is_some() {
                    return Err(format!(
                        "Staged data or destination changed: {}",
                        target.display()
                    ));
                }
                fs::hard_link(&saved, target).map_err(|e| fs_error(target, e))?;
                return sync_dir(parent);
            }
            let scratch = work.join(SCRATCH);
            remove_regular_if_exists(&scratch)?;
            copy_verified(&saved, &scratch, desired, true)?;
            if inspect()? != current {
                return Err(format!(
                    "Destination changed while committing: {}",
                    target.display()
                ));
            }
            if current.is_none() {
                let scratch = tempfile::TempPath::try_from_path(scratch.clone())
                    .map_err(|e| fs_error(&scratch, e))?;
                scratch
                    .persist_noclobber(target)
                    .map_err(|e| fs_error(target, e.error))?;
            } else {
                fs::rename(&scratch, target).map_err(|e| fs_error(target, e))?;
            }
            sync_dir(parent)?;
            remove_regular_if_exists(&scratch)?;
        } else {
            if inspect()? != current {
                return Err(format!(
                    "Destination changed while committing: {}",
                    target.display()
                ));
            }
            fs::remove_file(target).map_err(|e| fs_error(target, e))?;
            sync_dir(parent)?;
        }
        Ok(())
    }

    fn recover(&self) -> Result<(), String> {
        let result = (|| {
            let cleanup = self.state.join(CLEANUP);
            clean_directory(&cleanup)?;
            let preparing = self.state.join(PREPARING);
            clean_directory(&preparing)?;
            let active = self.state.join(ACTIVE);
            if !directory_exists(&active)? {
                return Ok(());
            }
            let journal = read_journal(&active)?;
            let committed = exists_regular(&active.join(COMMITTED))?;
            self.check_recovery(&active, &journal, committed)?;
            if !committed {
                for (index, edit) in journal.edits.iter().enumerate().rev() {
                    self.change_file(&active, edit, index, false)?;
                }
                self.change_lock(&active, &journal, false)?;
                for relative in journal.directories.iter().rev() {
                    let path = self.root.join(relative);
                    if directory_exists(&path)? {
                        let parent = path.parent().expect("validated directory");
                        check_directories(&self.root, parent)?;
                        match fs::remove_dir(&path) {
                            Ok(()) => sync_dir(parent)?,
                            Err(e)
                                if matches!(
                                    e.kind(),
                                    ErrorKind::DirectoryNotEmpty | ErrorKind::NotFound
                                ) => {}
                            Err(e) => return Err(fs_error(&path, e)),
                        }
                    }
                }
            }
            self.retire(&active)
        })();
        result.map_err(|error| format!("Recovery required at {}: {error}", self.state.display()))
    }

    fn check_recovery(
        &self,
        work: &Path,
        journal: &Journal,
        committed: bool,
    ) -> Result<(), String> {
        let matches =
            |current: &Option<Content>, before: &Option<Content>, after: &Option<Content>| {
                current == after || (!committed && current == before)
            };
        let current_lock = lockfile::load(&self.root)?
            .bytes
            .as_deref()
            .map(Content::of);
        if !matches(&current_lock, &journal.before_lock, &journal.after_lock) {
            return Err("Current project lock matches neither expected recovery state".into());
        }
        check_created_identity(
            &current_lock,
            &journal.before_lock,
            &self.root.join(lockfile::NAME),
            &work.join(AFTER_LOCK),
        )?;
        for (index, edit) in journal.edits.iter().enumerate() {
            let content = inspect_at(&self.root, &edit.target)?;
            if !matches(&content, &edit.before, &edit.after) {
                return Err(format!(
                    "Local edits or unexpected bytes at {}",
                    edit.target
                ));
            }
            check_created_identity(
                &content,
                &edit.before,
                &self.root.join(&edit.target),
                &work.join(format!("after-{index}")),
            )?;
        }
        Ok(())
    }

    fn retire(&self, active: &Path) -> Result<(), String> {
        let cleanup = self.state.join(CLEANUP);
        fs::rename(active, &cleanup).map_err(|e| fs_error(&cleanup, e))?;
        sync_dir(&self.state)?;
        clean_directory(&cleanup)
    }
}

fn check_created_identity(
    current: &Option<Content>,
    before: &Option<Content>,
    target: &Path,
    staged: &Path,
) -> Result<(), String> {
    if current.is_some()
        && before.is_none()
        && !same_file::is_same_file(target, staged).map_err(|e| fs_error(target, e))?
    {
        return Err(format!(
            "Cannot prove that FrilAdd created {}",
            target.display()
        ));
    }
    Ok(())
}

fn validate_journal(journal: &Journal, before: &Lock, after: &Lock) -> Result<(), String> {
    if journal.schema_version != 1 {
        return Err("Unsupported transaction journal version".into());
    }
    validate_ownership(before, after, &journal.edits)?;
    let mut writable_parents = BTreeSet::new();
    for edit in &journal.edits {
        if edit.after.is_some() {
            writable_parents.extend(
                edit.target
                    .match_indices('/')
                    .map(|(end, _)| &edit.target[..end]),
            );
        }
    }
    if journal
        .directories
        .windows(2)
        .any(|pair| pair[0] >= pair[1])
        || journal
            .directories
            .iter()
            .any(|directory| !writable_parents.contains(directory.as_str()))
    {
        return Err("Invalid journal directory list".into());
    }
    Ok(())
}

fn read_journal(active: &Path) -> Result<Journal, String> {
    let path = active.join(JOURNAL);
    let file = open_regular(&path)?;
    let bytes = crate::downloader::read_bounded(file, MAX_JOURNAL_BYTES, "transaction journal")?;
    let journal: Journal =
        serde_json::from_slice(&bytes).map_err(|_| "Invalid transaction journal")?;
    let before = read_saved_lock(active, BEFORE_LOCK, &journal.before_lock)?;
    let after = read_saved_lock(active, AFTER_LOCK, &journal.after_lock)?;
    validate_journal(&journal, &before, &after)?;
    // Validate backup files before changing any path during recovery.
    for (index, edit) in journal.edits.iter().enumerate() {
        for (prefix, expected) in [("before", &edit.before), ("after", &edit.after)] {
            if let Some(expected) = expected {
                let path = active.join(format!("{prefix}-{index}"));
                if fingerprint(&path, lockfile::MAX_FILE_BYTES)? != *expected {
                    return Err(format!(
                        "Digest mismatch in journal data at {}",
                        path.display()
                    ));
                }
            }
        }
    }
    Ok(journal)
}

fn stage_lock(work: &Path, name: &str, bytes: Option<&[u8]>) -> Result<Option<Content>, String> {
    if let Some(bytes) = bytes {
        write_small(&work.join(name), bytes)?;
    }
    Ok(bytes.map(Content::of))
}

fn read_saved_lock(work: &Path, name: &str, expected: &Option<Content>) -> Result<Lock, String> {
    let bytes = lockfile::read_optional(&work.join(name))?;
    if bytes.as_deref().map(Content::of) != *expected {
        return Err(format!("Transaction {name} is missing or has changed"));
    }
    bytes
        .as_deref()
        .map(Lock::parse)
        .transpose()
        .map(Option::unwrap_or_default)
}

fn open_regular(path: &Path) -> Result<File, String> {
    if !exists_regular(path)? {
        return Err(format!("Expected a regular file at {}", path.display()));
    }
    File::open(path).map_err(|e| fs_error(path, e))
}

fn exists_regular(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => Ok(true),
        Ok(_) => Err(format!("Unsafe non-regular path: {}", path.display())),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(false),
        Err(e) => Err(fs_error(path, e)),
    }
}

fn directory_exists(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => Ok(true),
        Ok(_) => Err(format!("Unsafe non-directory path: {}", path.display())),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(false),
        Err(e) => Err(fs_error(path, e)),
    }
}

fn check_directories(root: &Path, path: &Path) -> Result<(), String> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| "Directory escapes project root")?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component);
        if !directory_exists(&current)? {
            return Err(format!("Missing directory {}", current.display()));
        }
    }
    Ok(())
}

fn ensure_directory(path: &Path) -> Result<(), String> {
    if !directory_exists(path)? {
        match fs::create_dir(path) {
            Ok(()) => sync_dir(path.parent().expect("directory parent"))?,
            Err(e) if e.kind() == ErrorKind::AlreadyExists && directory_exists(path)? => (),
            Err(e) => return Err(fs_error(path, e)),
        }
    }
    Ok(())
}

fn write_small(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = create_file(path)?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| fs_error(path, e))
}

fn create_file(path: &Path) -> Result<File, String> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| fs_error(path, e))
}

fn fingerprint(path: &Path, max_bytes: u64) -> Result<Content, String> {
    let mut input = open_regular(path)?;
    hash_copy(&mut input, &mut std::io::sink(), max_bytes)
        .map_err(|error| format!("{}: {error}", path.display()))
}

fn copy_verified(
    source: &Path,
    target: &Path,
    expected: &Content,
    permissions: bool,
) -> Result<(), String> {
    let mut input = open_regular(source)?;
    let mut output = create_file(target)?;
    if hash_copy(&mut input, &mut output, expected.size)? != *expected {
        return Err(format!(
            "Bytes changed or digest mismatch at {}",
            source.display()
        ));
    }
    if permissions {
        let metadata = input.metadata().map_err(|e| fs_error(source, e))?;
        output
            .set_permissions(metadata.permissions())
            .map_err(|e| fs_error(target, e))?;
    }
    output.sync_all().map_err(|e| fs_error(target, e))
}

fn hash_copy(
    input: &mut impl Read,
    output: &mut impl Write,
    limit: u64,
) -> Result<Content, String> {
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut size = 0;
    loop {
        let count = input
            .read(&mut buffer)
            .map_err(|e| format!("Cannot read file bytes: {e}"))?;
        if count == 0 {
            break;
        }
        size += count as u64;
        if size > limit {
            return Err("File exceeds the expected byte limit".into());
        }
        hasher.update(&buffer[..count]);
        output
            .write_all(&buffer[..count])
            .map_err(|e| format!("Cannot stage file bytes: {e}"))?;
    }
    Ok(Content {
        sha256: format!("{:x}", hasher.finalize()),
        size,
    })
}

fn clean_directory(path: &Path) -> Result<(), String> {
    if !directory_exists(path)? {
        return Ok(());
    }
    let mut files = Vec::new();
    for entry in fs::read_dir(path).map_err(|e| fs_error(path, e))? {
        let entry = entry.map_err(|e| fs_error(path, e))?;
        let name = entry.file_name();
        let recognized = name.to_str().is_some_and(|name| {
            matches!(
                name,
                JOURNAL | COMMITTED | BEFORE_LOCK | AFTER_LOCK | SCRATCH
            ) || name
                .strip_prefix("before-")
                .or_else(|| name.strip_prefix("after-"))
                .is_some_and(|index| {
                    !index.is_empty()
                        && index.bytes().all(|b| b.is_ascii_digit())
                        && index.parse::<usize>().is_ok_and(|index| index < MAX_EDITS)
                })
        });
        if !recognized || !exists_regular(&entry.path())? {
            return Err(format!(
                "Unexpected recovery entry at {}",
                entry.path().display()
            ));
        }
        files.push(entry.path());
        if files.len() > MAX_EDITS * 2 + 5 {
            return Err("Unexpected number of recovery files".into());
        }
    }
    for file in files {
        remove_regular_if_exists(&file)?;
    }
    fs::remove_dir(path).map_err(|e| fs_error(path, e))?;
    sync_dir(path.parent().expect("state directory parent"))
}

fn remove_regular_if_exists(path: &Path) -> Result<(), String> {
    if exists_regular(path)? {
        #[cfg(windows)]
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("before-") || name == SCRATCH)
        {
            let mut permissions = fs::symlink_metadata(path)
                .map_err(|e| fs_error(path, e))?
                .permissions();
            if permissions.readonly() {
                permissions.set_readonly(false);
                fs::set_permissions(path, permissions).map_err(|e| fs_error(path, e))?;
            }
        }
        fs::remove_file(path).map_err(|e| fs_error(path, e))?;
    }
    Ok(())
}

#[cfg(unix)]
fn sync_dir(path: &Path) -> Result<(), String> {
    File::open(path)
        .and_then(|dir| dir.sync_all())
        .map_err(|e| fs_error(path, e))
}

#[cfg(not(unix))]
fn sync_dir(_path: &Path) -> Result<(), String> {
    Ok(())
}

fn fs_error(path: &Path, error: std::io::Error) -> String {
    format!("{}: {error}", path.display())
}

#[cfg(test)]
mod tests;
