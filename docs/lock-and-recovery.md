# Project ownership and recovery

FrilAdd owns only the paths recorded through a successful managed transaction. `installed` and `status` read lock state and owned paths offline without writing project state. Lifecycle mutations use the shared lock and transaction implementation.

## Lock v1

The project-root file `friladd.lock.json` uses independent `schemaVersion: 1`. A missing file represents empty state. A zero-byte, non-regular, symlinked, malformed, unsupported, or internally inconsistent file fails closed. Keep the lock with the project when you need to reproduce ownership and file provenance. [Lock JSON Schema](schemas/lock-v1.schema.json)

- A v1 provider ID is `v1:` followed by the lowercase SHA-256 of its canonical resolved manifest locator. A v2 provider ID is `v2:` followed by the provider's registry ID. Aliases may change independently of ownership. Two providers can both publish the same item ID.
- The provider record contains the declared and resolved locators and the manifest digest. Each item retains its own manifest locator and digest, so older snapshots remain identifiable after a newer item from that provider is added.
- Each file records the declared relative source, the exact resolved locator, the declared target, the effective project-relative target, and the expected and installed SHA-256 plus installed byte count. The optional item target root records the mapping used at installation.
- Locators are canonical `https:` or `file:` URLs. They exclude user information, queries, and fragments. A local file locator can become unavailable on another machine. A mutable remote locator can also lose the exact historical bytes. `restore` fetches only the recorded locator and verifies the locked digest and size before staging; it never substitutes a current manifest or newer artifact.
- One item owns each target. Changes cannot adopt unmanaged files or implicitly transfer a path between owners. Dependency edges point to exact recorded v2 versions and manifest digests. Referenced installations are single records. Reverse references are derived from the graph.
- Serialization sorts providers and items by their canonical identifiers, dependencies by identity, and files by effective target. The reader enforces structural, byte, mapping, path, and graph invariants. It validates target collisions across the whole managed project. A lock is descriptive state and needs the same scrutiny as other source-controlled input.

A future lock format must introduce a new schema version and an explicit migration procedure. The v1 reader never converts or discards unknown versions. It preserves original bytes upon a no-op. The lock reader bounds input to 16 MiB and checks up to 10,000 providers or items and 100,000 files or dependency edges.

## Commit and recovery

1. Construct a complete plan from a lock snapshot. Check path types, uniqueness, explicit before-states and ownership, and planned lock changes. Read-only checks can also verify staged inputs without creating project state.
2. Acquire the operating system's exclusive lock on `.friladd/mutation.lock`. Keep this coordination file in place. Removing it while a command runs can let two processes acquire different locks. If it is busy, retry after the other process exits.
3. Recover an interrupted transaction under the lock. Re-read the current lock and target bytes. A changed snapshot requires replanning.
4. Copy the before-state and replacement data into `.friladd/preparing`, capping each file at 100 MiB and the staged data and backups at 512 MiB each. Hash during copying. Flush the files and place a bounded, versioned journal in the staging directory. Rename it to `.friladd/transaction` before changing a destination.
5. Create required directories and apply the edits and lock replacement. New files are published using no-overwrite hard links. Replacements use atomic same-filesystem rename with a fresh staged copy. The journal keeps recovery copies and proves the identity of created paths. Recheck recorded expectations immediately before use.
6. Mark the journal committed after the lock and files are in their final state. Rename the journal directory to `.friladd/cleanup` and retire only recognized regular state files.

An interrupted transaction without the commit marker rolls back, including restoring the original lock bytes or its original absence and removing newly created empty directories. Recovery of a committed journal validates its after-state and retries cleanup. Recovery checks the full set before making changes. An unrecognized file, symlink, modified destination, or failed cleanup retains the recovery location and returns an error. Preserve the files and journal for inspection in that case. The transaction engine can safely retry recovery after the ambiguity has been resolved.

A local filesystem must support process locking, hard links, and atomic rename. The engine syncs directories on Unix and file data on all supported hosts. It cannot promise directory-entry durability on Windows or preserve guarantees on network mounts and filesystems that disregard sync or locks. A hostile external process can race path checks. Backups preserve file bytes and attempt to preserve basic permissions. Extended attributes, timestamps, and filesystem ACLs are outside the transaction contract.
