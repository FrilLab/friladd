# Contributing to FrilAdd

Thanks for helping improve FrilAdd. Contributions should keep FrilAdd independent of registry providers and preserve the existing installation safety checks.

## Development setup

Install stable Rust with Cargo (edition 2024) and Node.js 18 or newer for npm package checks.

```sh
git clone https://github.com/FrilLab/friladd.git
cd friladd
cargo build
cargo run -- --help
```

## Before opening a change

Run the local validation suite:

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked
npm run test:package --prefix npm
```

The npm package smoke test builds and packs the native executable for the current host. The CI workflow builds and checks macOS ARM64, macOS x64, Linux x64, and Windows x64 packages.

Tests that write files should use isolated temporary directories. Do not use the developer's real home configuration or mutate public registry contents in tests.

The `registry/` directory is a repository-only test fixture; it must not be staged into the published npm tarball. Packed npm consumer tests use the repository fixture from the CI checkout. Fixture files are byte-sensitive because the manifest contains SHA-256 digests. `.gitattributes` disables checkout-time newline conversion in `registry/fixtures/` so macOS, Linux, and Windows test identical bytes. Update the manifest hashes when fixture contents change.

## Architecture

The command flow resolves a manifest, loads and validates Registry Manifest v1, plans destinations, acquires and hash-verifies file contents, then installs them through the safe installer. Keep registry configuration (`.friladd.json`) separate from component manifests (`registry.json`). Registry sources are independent providers; do not add a provider-specific default, URL, or installation rule to the CLI.

Preserve these guarantees when changing installation behavior:

- Reject unsafe relative paths and symlinks in registry source or destination paths.
- Verify declared SHA-256 digests before writing project files.
- Never overwrite existing destination files.
- Keep `--dry-run` free of project file writes.
- Reuse the existing planner, downloader, and installer rather than duplicating them.

## Documentation and pull requests

Write repository documentation in English. Update the README and focused reference docs when user-facing behavior or manifest rules change. Include the motivation, behavior changes, and validation performed in the pull request description. Do not include unpublished release or account-operation notes in user documentation.
