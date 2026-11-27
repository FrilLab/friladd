# 002. Lock ownership, recoverable transactions, and registry v2 identity

Status: lock and transaction API in #11. Registry v2 protocol coordinated with #15.

## Context

Issue #11 introduces managed ownership and must support lifecycle commands without adopting existing files or losing user edits. Issue #15 introduces versioned provider identities and dependencies. Those interfaces must agree before either is frozen. Registry v1 remains unchanged.

## Lock v1

- The project state file is `friladd.lock.json`, independent of CLI, registry, and machine-output versions. Missing means no managed items. Unknown schema versions, unknown fields, duplicate JSON keys, malformed data, and symlink/non-regular lock paths fail closed. No command adopts existing files.
- Root fields are `schemaVersion: 1`, `providers`, and `items`. Records and their nested file and dependency arrays serialize in deterministic lexical order. Readers validate the complete structure and all cross-record ownership before use.
- A provider record has `providerId`, `declaredSource`, `resolvedSource`, and `manifestSha256`. For v1, `providerId` is a namespaced digest of the normalized, credential-free resolved manifest locator. For v2, the provider's immutable registry `id` is the canonical identity. The display alias is not identity. Two aliases for the same provider converge; different provider IDs remain distinct even when item IDs match.
- An item record has `providerId`, `itemId`, optional exact `version`, `manifestSha256`, normalized `dependencies`, and normalized owned `files`. V1 has no version; do not infer one. V2 records both exact version and manifest digest. Dependencies refer to canonical provider/item identity and exact version/digest. Record the declared source path and immutable resolved source locator for each file, plus expected SHA-256 and installed SHA-256/size.
- Every normalized target has at most one owner. Duplicate, case-folded, and file/directory-prefix collisions across all records invalidate the lock. Shared dependencies therefore have one owned installation record with reverse dependency references/refcounts; two items cannot independently claim the same target. Effective targets are persisted so a later mapping change cannot relocate installed files.
- Provenance excludes user information, query strings, fragments, and temporary authorization URLs. A lock is descriptive state, not a signature; validate every path and observed file before acting.

## Transaction API and recovery

- The shared transaction API accepts a complete validated plan of writes, removals, reads, and next-lock bytes. Planning and dry-run remain read-only. Under the project mutation lock, it recovers any prior journal, reloads and revalidates lock/files, then rechecks every planned precondition before staging.
- The process lock is acquired before mutation and held through cleanup. Its file lives in reserved `.friladd` state, which is a real non-symlink directory. Lock acquisition is OS-backed and cross-process; stale locks are not broken based on age or PID guesses.
- All replacement bytes are bounded, staged on the project filesystem, hash checked, flushed, and recorded in a durable journal before commit. Preserve old file bytes in same-filesystem backups. Commit file changes and the atomically replaced lock as explicit journal states; sync file and parent metadata where supported. The journal supports repeatable roll-forward or rollback after restart, including interruption at each commit boundary.
- Recovery only changes a path when its current bytes/type match a recorded pre-state or post-state. If the path is symlinked, unreadable, or has user-modified bytes that match neither state, stop and report the journal/recovery path without deleting or overwriting it. Cleanup failure reports the exact retained recovery location. Re-running recovery has the same outcome.
- On ordinary failure, rollback is attempted through the same journal. If rollback cannot prove a safe result, retain the journal/backups and report recovery required; never report success or erase unrelated paths. After successful commit, cleanup is idempotent.
- Require local filesystem support for same-filesystem staging, atomic rename, hard links, and OS process locking. Document that hostile external processes can still race filesystem checks; use no-overwrite creation and recheck immediately before each operation. State guarantees on filesystems/platforms where directory fsync or locking semantics differ.

## Registry v2 identity coordination

- V2 adds immutable registry `id`, explicit item `id`, exact item `version`, `kind`, metadata, and per-file source, target, digest, and declared size. The registry schema version remains independent from item versions. V1 parsing stays strict and unchanged.
- A v2 provider ID is immutable and globally scoped by its publisher; the registry name and configured alias are presentation and lookup values. A release snapshot is the exact manifest bytes identified by SHA-256. Version selection uses exact Semantic Versions in explicitly chosen snapshots. A provider manifest advertises one revision of each item.
- V2 dependencies use a qualified provider ID/item ID and exact version. External references include the exact manifest digest. Internal references derive that digest from the enclosing snapshot. Providers are explicitly configured; no implicit discovery or registration. Cycles, missing providers, and digest/version disagreement fail before writes. Package requirements are metadata only and never trigger an installer or script.
- Targets remain portable relative paths. Kind/root mapping is explicit project configuration with deterministic precedence; the effective path is written into the lock. No interpolation, path escape, silent relocation, or unsafe symlink traversal is allowed. GLB/glTF-relative resources retain their declared relative relationships.
- Artifact sources remain manifest-relative by default. Any future external transport must be explicitly approved and HTTPS constrained; arbitrary URLs, embedded credentials, secret query strings, and insecure redirects are rejected.

## Consequences and review gates

- #11 owns the lock schema, transaction journal format, OS lock implementation, and fault/recovery tests. #15 owns the compatible v2 reader/schema/fixtures. Both must agree on provider ID, item ID, version/digest, dependency reference, and effective target fields before implementation merges.
- Managed add/update/remove/restore use the shared transaction API. Registry v2 add records exact resolved dependency edges; removal refuses live dependents and retains dependency items by default. Dependency updates must preserve graph and ownership consistency through the same transaction API.
- Any deviation requires updating this ADR and the CLI/security docs before downstream implementation. No change to Registry v1, existing command forms, or the current installer is implied by this proposal.

See [Project ownership and recovery](../lock-and-recovery.md) for the implemented API behavior and limits.
