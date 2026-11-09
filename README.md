# FrilAdd

**Install only the source files you select.** FrilAdd is a standalone Rust CLI that reads a small, versioned JSON registry and installs the declared files for one item into a project. It does not depend on FrilCraft and can be used with any registry that follows the FrilAdd manifest format.

## Status

The Rust CLI, local fixture, HTTPS downloader, installer, tests, npm package wrapper, and GitHub Actions release workflow are implemented for **v0.1.0**. npm publication is conditional on package ownership and Trusted Publishing being configured for the publishing account.

## What it does

- Lists the items in an explicitly selected local or HTTPS registry.
- Plans or installs only the selected item's declared files.
- Checks manifest structure and source/destination paths before installation.
- Verifies every downloaded file against its declared SHA-256 digest.
- Refuses existing files, symlinks in the destination path, path traversal, and unsupported file types.
- Keeps downloaded files as source code; it never executes them or invokes npm.

FrilAdd does not resolve registry aliases, discover components, install package dependencies, update or uninstall files, or provide general shadcn Registry compatibility.

## Quick start from this repository

Requirements: Rust stable (edition 2024) and Cargo. To build and try the local fixture:

```sh
cargo build --release
./target/release/friladd --help
./target/release/friladd --version
./target/release/friladd list --registry registry/registry.json

mkdir -p /tmp/friladd-consumer
./target/release/friladd add envelope-reveal \
  --registry registry/registry.json \
  --project /tmp/friladd-consumer \
  --dry-run
./target/release/friladd add envelope-reveal \
  --registry registry/registry.json \
  --project /tmp/friladd-consumer
```

The final command writes two files under `/tmp/friladd-consumer/src/components/envelope-reveal/`. Existing files are never overwritten. Run it in a disposable project directory or use `--dry-run` first.

## CLI

```text
friladd --help
friladd --version
friladd list --registry <path-or-https-url>
friladd add <item> --registry <path-or-https-url> [--project <directory>] [--dry-run]
```

`--registry` is required. A local path is resolved from the current directory. Registry file sources are resolved relative to the manifest's directory. Remote registries must use HTTPS; their relative file sources are resolved against the final manifest URL, and their initial source URLs must remain on that HTTPS origin.

Examples using this repository's fixture:

```sh
friladd list --registry ./registry/registry.json
friladd add envelope-reveal --registry ./registry/registry.json --dry-run
friladd add envelope-reveal --registry ./registry/registry.json --project ./my-app
```

After the v0.1.0 tag is published, the repository fixture can also be used remotely:

```sh
friladd add envelope-reveal \
  --registry https://raw.githubusercontent.com/FrilLab/friladd/v0.1.0/registry/registry.json \
  --dry-run
```

This remote example becomes available when that tagged registry is present on GitHub. No currently deployed FrilCraft registry is assumed.

### Dry run

`--dry-run` uses the same manifest parsing, destination planning, and content acquisition and hash checks as installation. It prints each planned destination and any existing-file conflict. It creates no project directories and changes no project files. For remote registries, it may download the manifest and declared source files to check their hashes.

## Registry manifest v1

The registry format is independent of FrilCraft. See [docs/registry-format.md](docs/registry-format.md) for the complete rules. A minimal working manifest is:

```json
{
  "schemaVersion": 1,
  "name": "local-dev",
  "items": [
    {
      "name": "envelope-reveal",
      "description": "A small interactive envelope sample for local development.",
      "files": [
        {
          "source": "fixtures/envelope-reveal/EnvelopeReveal.tsx",
          "target": "src/components/envelope-reveal/EnvelopeReveal.tsx",
          "sha256": "aef017b275d8420fe22161db1e0db57edc916c0bd9a812dba09ffd573b003398"
        },
        {
          "source": "fixtures/envelope-reveal/envelope-reveal.css",
          "target": "src/components/envelope-reveal/envelope-reveal.css",
          "sha256": "513830c41af1ed42684d795b06565699f3c9ac711bb21b9460e976337581493a"
        }
      ]
    }
  ]
}
```

The sample files live under `registry/fixtures/` and are not represented as published FrilCraft components. To generate a digest for a file, use `shasum -a 256 path/to/file` on macOS/Linux or `Get-FileHash -Algorithm SHA256 path\to\file` in PowerShell.

## Installation safety

- Manifest schema version 1 is required. Unknown fields, invalid names, duplicate item names, duplicate target paths, missing fields, and malformed SHA-256 digests are rejected.
- `source` and `target` are relative slash-separated paths. Absolute paths, empty segments, `.`/`..`, backslashes, drive letters, UNC paths, Windows device names such as `NUL`, trailing dots, URL syntax, and non-portable path characters are rejected.
- A local source must be a regular file inside the manifest directory. Symlinks in source paths are rejected.
- Target ancestors are checked component by component. Any symlink is rejected, including one that currently resolves inside the project. Existing target files are reported as conflicts and block installation.
- Registry manifests are limited to 1 MiB. Each source file is limited to 10 MiB. HTTPS uses a 5-second connect timeout, a 30-second request timeout, and at most five redirects; redirects must stay on HTTPS. Hash verification applies after redirects.
- All selected contents are acquired and hash-verified before the installer creates project directories. Files are staged beside their destinations, then added with a no-overwrite hard link. On a later failure, FrilAdd removes its installed files, temporary files, and newly created empty directories where possible.
- A manifest's digest detects content that differs from that manifest; it is not a signature or publisher identity. Review the registry and source code you choose to trust.

The filesystem checks protect against accidental traversal and existing symlinks. Like a normal cross-platform CLI using standard filesystem APIs, FrilAdd cannot lock a project against another process that maliciously swaps directories between checks and writes. Install into a project that is not being concurrently modified by an untrusted process. Rollback is best effort if the filesystem itself refuses cleanup; successful file creation is atomic per file, not as one all-or-nothing multi-file transaction.

## npm and npx

The package name configured for distribution is `friladd`; its `bin` entry exposes the command `friladd`. The package includes the Rust executables for macOS ARM64, macOS x64, Linux x64, and Windows x64. The small Node wrapper only selects that executable, forwards arguments and terminal streams, and returns its exit status.

Once `friladd@0.1.0` is published, valid invocations are:

```sh
npx friladd --help
npx friladd add envelope-reveal \
  --registry https://raw.githubusercontent.com/FrilLab/friladd/v0.1.0/registry/registry.json \
  --dry-run
```

Before publication, `npm run test:package --prefix npm` stages the current host's release binary, packs the npm directory, installs that tarball into a temporary consumer project, checks argument and exit-code forwarding, performs the two-file fixture install, and verifies byte-for-byte output. The cross-platform CI/release package job checks that all four executables are present before packing, then runs the packed package on each supported runner.

## FrilCraft integration

FrilAdd does not import FrilCraft code. FrilCraft can publish a versioned manifest beside its component files:

```text
FrilCraft source files
        |
        v
Versioned FrilAdd registry manifest
        |
        v
FrilAdd CLI
        |
        v
Developer's React project
```

For a deployed FrilCraft manifest, set its actual HTTPS URL and run:

```sh
FRILCRAFT_REGISTRY_URL="https://your-frilcraft-host.example/registry/registry.json"
npx friladd add envelope-reveal --registry "$FRILCRAFT_REGISTRY_URL" --project ./my-app
```

The URL above is a placeholder: FrilCraft has not been confirmed to host a compatible registry. Each manifest can expose selected source files without requiring developers to clone the full FrilCraft repository. FrilAdd does not install any declared npm dependencies; component authors should document those requirements for users.

## Development and tests

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --locked
npm run test:package --prefix npm
```

Rust filesystem tests use temporary directories. The consumer fixture at `fixtures/consumer-project/` is copied into temporary locations; tests do not write to real projects. Network limits, interrupted reads, checksum mismatches, rejected HTTP, registry validation, planner safety, conflicts, dry runs, and installation behavior have deterministic tests.

## Contributor flow

```text
CLI
  -> Registry Loader
  -> Manifest Validation
  -> Installation Planner
  -> File Acquisition + Hash Verification
  -> Safe Installer
  -> Result
```

The modules are `cli.rs`, `registry.rs`, `planner.rs`, `downloader.rs`, and `installer.rs`. Keep the manifest and installer independent of any one component library.

## CI and release

`.github/workflows/ci.yml` runs formatting, Clippy, tests, native builds, npm package validation, and packed consumer smoke tests, using Cargo caches for development runs. `.github/workflows/release.yml` runs on version tags, verifies the tag against Cargo and npm versions, performs clean uncached builds, waits for all supported platform package smoke tests, and attaches the four binaries plus the npm tarball to a GitHub Release.

For the initial v0.1.0 release, merge the implementation and push the `v0.1.0` tag with `NPM_PUBLISH_ENABLED` unset. The workflow will build and test the complete package and attach it to the GitHub Release. An authorized npm maintainer can then publish that verified tarball and complete the publisher setup. For later versions, set up Trusted Publishing first and set `NPM_PUBLISH_ENABLED=true` before pushing the version tag.

```sh
git tag v0.1.0
git push origin v0.1.0
```

An authorized npm maintainer must confirm that FrilLab can publish the unscoped name `friladd`. For the initial package, leave `NPM_PUBLISH_ENABLED` unset, then manually publish the verified `friladd-0.1.0.tgz` attached to the GitHub Release if npm needs an initial authenticated publish to create package settings. Configure the npm Trusted Publisher with organization `FrilLab`, repository `friladd`, workflow filename `release.yml`, and permission for direct `npm publish`. Set the repository variable `NPM_PUBLISH_ENABLED=true` only for a later version that has not already been published. If `friladd` cannot be claimed, verify control of the `@frillab` scope before changing the package name and release configuration.

Without that variable the release workflow builds and validates artifacts but skips npm publishing. The release workflow uses OIDC and short-lived npm credentials; it stores no npm token. npm Trusted Publishing requires Node 22.14+ and npm CLI 11.5.1+; the workflow sets up Node 24 and the required npm CLI. Package settings, ownership, and a successful publisher configuration cannot be validated from this workspace.

## License

MIT. See [LICENSE](LICENSE).
