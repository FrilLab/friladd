# Changelog

## Unreleased

- Install v1 and dependency-free v2 items as managed resources, recording ownership with the file changes through the recoverable transaction engine.
- Make identical managed adds no-ops and refuse to overwrite modified, missing, or unmanaged files; dry runs verify the transaction plan without writes.
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
