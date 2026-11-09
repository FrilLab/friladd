# FrilAdd

**Install only the source files you select.** FrilAdd is a standalone Rust CLI that reads a small, versioned JSON registry and installs the declared files for one item into a project. It does not depend on FrilCraft and can be used with any registry that follows the FrilAdd manifest format.

## What it does

- Lists items in a selected local or HTTPS registry, including registered aliases.
- Registers independent project or global registries and resolves a configured default.
- Plans or installs only the selected item's declared files.
- Checks manifest structure and source/destination paths before installation.
- Verifies every downloaded file against its declared SHA-256 digest.
- Refuses existing files, symlinks in the destination path, path traversal, and unsupported file types.
- Keeps downloaded files as source code; it never executes them or invokes npm.

FrilAdd does not discover components across registries, install package dependencies, update or uninstall files, or provide general shadcn Registry compatibility.

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
friladd registry add <alias> <path-or-https-url> [--global] [--default]
friladd registry list [--global]
friladd registry default <alias> [--global]
friladd registry remove <alias> [--global]
friladd list [<registry-alias>] [--registry <path-or-https-url>]
friladd add <item-or-registry/item> [--registry <alias-or-path-or-https-url>] [--project <directory>] [--dry-run]
```

For legacy commands, `--registry` remains supported. Explicit local paths resolve from the current directory. A registry stored in project `.friladd.json` resolves local paths from that file's directory; a local source registered globally is normalized to an absolute path. Remote registries must use HTTPS; file sources are resolved against the manifest directory or final manifest URL and remain subject to the existing origin checks.

Examples using this repository's fixture:

```sh
friladd list --registry ./registry/registry.json
friladd add envelope-reveal --registry ./registry/registry.json --dry-run
friladd add envelope-reveal --registry ./registry/registry.json --project ./my-app
```

### Registering registries

Project registrations are stored in `.friladd.json` at the selected project root and can be committed with the project. `registry add` defaults to project scope; `--global` stores entries in the platform application configuration directory. Global settings are changed only by commands with `--global`.

```sh
friladd registry add team-ui https://example.com/registry.json --default
friladd add button
friladd add other-ui/envelope-reveal
friladd list team-ui
friladd registry list
friladd registry default other-ui
friladd registry remove team-ui
```

The configuration schema is separate from the Registry Manifest v1 format:

```json
{
  "schemaVersion": 1,
  "defaultRegistry": "team-ui",
  "registries": {
    "team-ui": { "source": "https://example.com/registry.json" },
    "local-dev": { "source": "./registry/registry.json" }
  }
}
```

Resolution order for an unqualified item is explicit `--registry`, then project default, then global default. A `registry/item` identifier selects that alias directly; project entries shadow global entries with the same alias. There is no search across providers. An explicit `--registry` can be a registered alias, local manifest path, or HTTPS manifest URL. Qualified identifiers cannot be combined with `--registry`. A broken configured project default is reported instead of falling through to global configuration.

For example, FrilCraft is one independent third-party provider and can register like any other provider:

```sh
friladd registry add frilcraft https://registry-provider.example/registry.json --global --default
friladd add envelope-reveal
friladd add frilcraft/envelope-reveal
```

The URL above is illustrative; FrilAdd contains no built-in FrilCraft URL or provider logic. Any provider can publish a compatible Registry Manifest v1 and be registered without changes to FrilAdd.

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

Install FrilAdd with npm, then use the same commands shown above:

```sh
npx friladd --help
npx friladd registry add sample https://example.com/registry.json --global --default
npx friladd add envelope-reveal
```

The package provides native executables for macOS ARM64, macOS x64, Linux x64, and Windows x64. The platform-specific npm package checks run in CI.

## Third-party registries

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

For any deployed third-party manifest, register its actual HTTPS URL and run:

```sh
npx friladd registry add frilcraft https://your-frilcraft-host.example/registry/registry.json --global --default
npx friladd add envelope-reveal --project ./my-app
```

The URL above is a placeholder. Each manifest can expose selected source files; FrilAdd does not require developers to clone a provider repository. FrilAdd does not install declared npm dependencies; component authors should document those requirements.

## Development and tests

For contributor setup, validation commands, and architecture notes, see [CONTRIBUTING.md](CONTRIBUTING.md).

The CI workflow validates Rust formatting, Clippy, tests, four native builds, and packed npm consumer tests.

## License

MIT. See [LICENSE](LICENSE).
