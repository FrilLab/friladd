# Changelog

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
