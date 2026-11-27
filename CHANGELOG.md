# Changelog

## 0.2.0

- Expose `fril` as the primary Cargo, native release, and npm command while preserving the `friladd` executable alias and version output. Both aliases share the same CLI and project state; the npm tarball continues to contain one platform binary.
- Use `fril` in current help, errors, examples, and documentation while retaining `friladd` as a supported, non-deprecated alias.
- Update the locked `url` crate to 2.5.9 to address RUSTSEC-2026-0592 and test non-HTTPS URL rejection.
- Resolve Registry v2 dependency DAGs across explicitly registered providers and install missing graph items with ownership edges in one transaction.
- Report aggregated package requirements and advisory npm/pnpm/yarn commands without executing package managers or modifying project manifests.
- Add read-only `doctor` diagnostics with offline-by-default checks for configuration, lockfiles, transaction recovery, managed-file safety/drift, and package manager lockfiles; optional bounded registry manifest validation is available with `--online`.
- Add versioned JSON output to `registry list` and `registry verify`, with registry source redaction and verification manifest digests.
- Qualify FrilCraft's published component in isolated React and FrilStudio consumers. FrilAssets reviewed-asset and FrilRemotion composition integrations remain unperformed and are tracked separately in issue #33; they are not FrilAdd v0.2.0 release blockers.
- Add deterministic `list`, `search`, and `info` commands with provider-neutral Registry v1/v2 metadata and versioned JSON output.
- Add explicit configured-registry search/list selection with project-over-global alias shadowing and fail-all behavior for unavailable registries.
- Install v1 and dependency-free v2 items as managed resources, recording ownership with the file changes through the recoverable transaction engine.
- Make identical managed adds no-ops and refuse to overwrite modified, missing, or unmanaged files; dry runs verify the transaction plan without writes.
- Emit versioned JSON plans and results for managed adds.
- Add offline `installed` and `status` commands with deterministic JSON and bounded integrity inspection for recorded file ownership.
- Add transactional update, remove, and exact-source restore for managed items.
- Stream verified artifacts to bounded temporary staging, supporting Registry v2 files up to 100 MiB while preserving the Registry v1 10 MiB limit.
- Add the validated lock v1 and crash-recoverable transaction foundation for upcoming managed lifecycle commands.
- Read and verify versioned Registry v2 manifests, including file sizes, resource metadata, and exact dependency declarations.

- Add registry verification for source and binary bytes without installing files.
- Support explicit project scope across registry management and reject ambiguous selector or scope flags.
- Reject reserved metadata targets and portable path collisions, and constrain HTTPS fetches to same-origin redirects with redacted errors.
- Document the CLI, configuration and registry v1 contracts and test Rust 1.88 as the MSRV.

## 0.1.2

- Exclude development and test Registry fixtures from the published npm tarball. The CLI remains provider-independent and ships no built-in components.
- Restrict npm packaging to platform-specific `friladd-*` binaries and the minimal wrapper, metadata, README, and license.
- Keep independent packed-consumer tests using repository fixtures without embedding those fixtures in the package.
- Preserve exact fixture bytes across Git checkouts on Windows, macOS, and Linux to keep SHA-256 verification reproducible.
- Preserve the v0.1.1 CLI, Registry Manifest v1 format, and installation safety behavior without breaking changes.

## 0.1.1

- Add project and global registry registration, listing, default selection, and removal.
- Resolve items through explicit aliases, qualified `registry/item` identifiers, and configured defaults.
- Preserve explicit manifest paths and URLs and reuse the v0.1.0 manifest validation and safe installer.
- Document `.friladd.json` configuration and independent third-party registries.
