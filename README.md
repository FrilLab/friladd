# FrilAdd

FrilAdd is a **source distribution CLI with optional lifecycle management**. It copies selected components, assets, animations, and their declared file dependencies into your project. Your project can build and run independently of FrilAdd.

## Quick start

With v0.2.1, install an official resource from inside your project:

```sh
npm install -g friladd
fril add frilcraft/envelope-reveal
```

Default installation copies only the declared files. It needs no registry configuration and leaves no FrilAdd metadata in a clean project. FrilAdd reports declared external package requirements so you can install them yourself.

Use `--managed` to track ownership for status, update, remove, and exact restore:

```sh
fril add frilcraft/envelope-reveal --managed
fril status
```

Without a global install, use `npx friladd add frilcraft/envelope-reveal` with the same options. The `friladd` command remains an alias for `fril`. See the [v0.2.1 release notes](docs/releases/v0.2.1.md) for availability and the change from v0.2.0. While v0.2.1 is a candidate, use the source build below to try its behavior.

## What it does

- Lists items in a selected local or HTTPS registry, including registered aliases.
- Registers independent project or global registries and resolves a configured default.
- Plans or installs only the selected item's declared files.
- Verifies every declared file in a registry before publication, without installing it.
- Checks manifest structure and source/destination paths before installation.
- Verifies every downloaded file against its declared SHA-256 digest.
- Refuses existing files, symlinks in the destination path, path traversal, and unsupported file types.
- Keeps downloaded files as source code; it never executes them or invokes npm.

FrilAdd resolves declared file dependencies from the selected provider and eligible public or configured registries. It reports package requirements and leaves package manifests and package manager lockfiles untouched. FrilAdd does not provide general shadcn Registry compatibility.

## Quick start from this repository

Requirements: Rust stable (edition 2024) and Cargo. To build and try the local fixture:

```sh
cargo build --release
./target/release/fril --help
./target/release/fril --version
./target/release/fril list --registry registry/registry.json

mkdir -p /tmp/friladd-consumer
./target/release/fril add envelope-reveal \
  --registry registry/registry.json \
  --project /tmp/friladd-consumer \
  --dry-run
./target/release/fril add envelope-reveal \
  --registry registry/registry.json \
  --project /tmp/friladd-consumer
```

The final command writes two files under `/tmp/friladd-consumer/src/components/envelope-reveal/`. Repeating a copy reports an existing-file conflict, even for identical bytes. To use the managed example, start with a separate clean project and add `--managed`. An unchanged managed add is a no-op.

## CLI

```text
fril --help
fril --version
fril registry add <alias> <path-or-https-url> [--global | --project <directory>] [--default]
fril registry list [--global | --project <directory>] [--json]
fril registry default <alias> [--global | --project <directory>]
fril registry remove <alias> [--global | --project <directory>]
fril registry discover [<registry-id>] [--json]
fril registry verify [<registry-alias> | --registry <alias-or-source>] [--project <directory>] [--json]
fril list [<registry-alias> | --registry <alias-or-source> | --all-registries | --public] [--project <directory>] [--json]
fril search <query> [--registry <alias-or-source> | --all-registries | --public] [--project <directory>] [--json]
fril info <item-or-registry/item> [--registry <alias-or-source>] [--project <directory>] [--json]
fril add <item-or-registry/item> [--registry <alias-or-path-or-https-url>] [--managed] [--project <directory>] [--dry-run] [--json]
fril installed [--project <directory>] [--json]
fril status [<item-or-registry/item>] [--project <directory>] [--json] [--check]
fril update <item-or-registry/item> --registry <alias-or-source> [--version <exact-v2-version>] [--allow-downgrade] [--project <directory>] [--dry-run] [--json]
fril remove <item-or-registry/item> [--project <directory>] [--dry-run] [--json]
fril restore [<item-or-registry/item>] [--project <directory>] [--dry-run] [--json]
fril doctor [--project <directory>] [--json] [--online]
```

For legacy commands, `--registry` remains supported. Explicit local paths resolve from the current directory. A registry stored in project `.friladd.json` resolves local paths from that file's directory; a local source registered globally is normalized to an absolute path. Remote registries must use HTTPS; file sources are resolved against the manifest directory or final manifest URL and stay on the same HTTPS origin. See the [CLI contract](docs/cli-contract.md), [security model](docs/security-model.md), and [lock and recovery reference](docs/lock-and-recovery.md) for managed ownership and recovery behavior.

Registry authors can use [Registry v2](docs/registry-v2.md) for versioned metadata and asset declarations. Registry v1 remains supported.

Examples using this repository's fixture:

```sh
fril list --registry ./registry/registry.json --json
fril search model --all-registries --json
fril info triangle --registry ./registry/examples/v2/assets/registry.json --json
fril add envelope-reveal --registry ./registry/registry.json --dry-run
fril add envelope-reveal --managed --registry ./registry/registry.json --project ./my-app
fril installed --project ./my-app
fril status --project ./my-app --check
```

### Registering custom registries

Project registrations are stored in `.friladd.json` at the selected project root and can be committed with the project. `registry add` defaults to project scope; `--global` stores entries in the platform application configuration directory. Global settings are changed only by commands with `--global`.

```sh
fril registry add team-ui https://example.com/registry.json --default
fril add button
fril add other-ui/envelope-reveal
fril list team-ui
fril registry list
fril registry default other-ui
fril registry remove team-ui
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

Resolution order for an unqualified item is explicit `--registry`, then project default, then global default. A `registry/item` identifier tries configured aliases first, then the public directory. Project entries shadow global entries with the same alias. `search --all-registries` and `list --all-registries` inspect the configured effective aliases only; they do not discover providers. An explicit `--registry` can be a registered alias, local manifest path, or HTTPS manifest URL. Qualified identifiers cannot be combined with `--registry`. A broken configured project default is reported instead of falling through to global configuration.

`search` matches case-insensitively across item ID, name, description, tags, and kind. Empty or whitespace-only queries are errors; no matches is a successful empty result. Multi-registry operations inspect up to 64 effective aliases and fail as a whole if any selected registry is unavailable or invalid, so failures are never hidden. `list`, `search`, and `info` accept `--json`; their versioned output is documented in the CLI contract and output schema. Metadata commands read manifests only and never fetch asset files.

### Provider verification

`fril registry verify` checks the manifest and the SHA-256 digest of every declared source file using the same acquisition logic as installation. It exits nonzero for a missing, invalid, oversized, or changed source file. It does not create or change consumer files; unlike `add --dry-run`, it does not check consumer destination conflicts. A registry may be empty.

```sh
fril registry verify --registry ./registry.json
fril registry verify frilcraft
```

Providers should run this command in their publication CI after reviewing the manifest and asset bytes. See the [provider validation workflow](docs/provider-validation.md) for native and packed npm examples.

`fril doctor` diagnoses project configuration and managed-file state without network access by default. `--online` checks configured registry manifests only. It never repairs files or runs package managers. Missing package manager lockfiles are warnings; the native CLI itself does not require Node.js.

The installer and verifier preserve exact bytes, including binary data such as GLB models, PNG textures, and audio. Registry v1 retains its 10 MiB per-file cap; Registry v2 streams files up to 100 MiB and caps each operation at 512 MiB. Acquisition hashes and counts bytes incrementally into temporary staging files before the transaction can write project destinations. Verification uses the same streaming path and removes temporary files. These limits do not validate model, image, or audio formats. Publish only reviewed assets with appropriate license notices.

### Dry run

`--dry-run` uses the same manifest parsing, destination planning, content acquisition, hash checks, and transaction preconditions as installation. It prints each planned destination and ownership effect. It creates no project directories, lock, transaction state, or cache and changes no project files. For remote registries, it may download the manifest and declared source files to check their hashes.

### Managed state

An add with `--managed` records ownership in `friladd.lock.json`. Managed transactions retain `.friladd/mutation.lock` for coordination with existing managed projects. v0.2.0 lockfiles continue to work. A new item added without `--managed` remains a copy even in a managed project. Repeating an add for an already tracked item preserves ownership and fails on local edits or missing files. Either mode refuses to overwrite existing unmanaged destinations. See the [lock and recovery reference](docs/lock-and-recovery.md) for details.

`fril installed` and `fril status` read managed records offline. Use `status --check` to return exit status 2 for modified, missing, unsafe, or unreadable managed files.

`update` requires an explicit candidate registry for custom sources and resolves the active directory entry for public sources. It never selects a newer version automatically, refuses provider changes and local edits, and requires `--allow-downgrade` for a Registry v2 downgrade. `remove` deletes only unchanged managed files. `restore` recreates only missing files from the exact digest and locator in the lock; it never substitutes a newer source. Resolve local edits manually before retrying lifecycle operations, and keep normal project backups for recovery from user mistakes.

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
- Registry manifests are limited to 1 MiB. Registry v1 files are limited to 10 MiB; Registry v2 files are limited to 100 MiB and each staged operation to 512 MiB. HTTPS uses a 5-second connect timeout, a 30-second manifest timeout, and a 5-minute artifact request timeout. Artifact redirects are limited to four and remain on the original HTTPS origin. Hash and v2 size verification apply while streaming after redirects.
- All selected file bytes are acquired and verified before installation. Both modes use a recoverable project transaction and publish new files with no-overwrite hard links. Recovery preserves unexpected bytes and reports anything it cannot safely clean up.
- A manifest's digest detects content that differs from that manifest; it is not a signature or publisher identity. Review the registry and source code you choose to trust.

The filesystem checks protect against accidental traversal and existing symlinks. Like a normal cross-platform CLI using standard filesystem APIs, FrilAdd cannot lock a project against another process that maliciously swaps directories between checks and writes. Install into a project that is not being concurrently modified by an untrusted process. File creation is atomic per file. A journal enables rollback or cleanup after interruption.

## npm and npx

The package name configured for distribution is `friladd`; its `bin` entries expose `fril` and `friladd` through the same launcher implementation. It includes one Rust executable for each supported platform (macOS ARM64, macOS x64, Linux x64, and Windows x64). Both command shims select the matching executable, forward arguments and terminal streams, and return its exit status. `fril` prints `fril VERSION`; the compatibility alias preserves `friladd VERSION`. Both operate on the same project configuration and lockfile. Use `npx friladd` or `npm exec --package=friladd -- fril`; `npx fril` is not an equivalent package invocation.

The repository's `registry/` fixtures are excluded from the npm tarball. The public directory is served at `https://registry.frillab.com/registries.json`. Community add, update, and restore require `--trust-community <registry-id>` on each operation, including transitive providers. See the [directory operations runbook](docs/operations/public-directory.md) for the current production status.

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

## Community

Using FrilAdd in your project? See our [community badges and brand assets](docs/branding.md).

## Development and tests

For contributor setup, validation commands, and architecture notes, see [CONTRIBUTING.md](CONTRIBUTING.md).

The CI workflow validates Rust formatting, Clippy, tests, four native builds, and packed npm consumer tests.

## License

MIT. See [LICENSE](LICENSE).
