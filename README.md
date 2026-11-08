# FrilAdd

**Install only what you need.**

FrilAdd is a planned, standalone command-line tool written in Rust for adding individual components, templates, and other reusable source files to existing projects.

The goal is to provide a small, predictable, and safe installation experience without requiring developers to install an entire UI library.

> **Status: Planning.** This repository documents the idea and learning roadmap. The CLI is not implemented, released, or published to npm yet. Commands below illustrate the intended experience and do not work yet.

## Planned usage

```bash
# Add a single component from a registry
npx friladd add frilcraft/envelope-reveal

# List available items
npx friladd list frilcraft

# Preview file changes without installing
npx friladd add frilcraft/envelope-reveal --dry-run
```

FrilAdd will be a **general-purpose installer**, not a FrilCraft-specific tool. FrilCraft is intended to be its first integration and test case; other registries should be able to use the same installer.

## How it is intended to work

```text
npx friladd add <registry/item>
             |
             v
       npm CLI entrypoint
             |
             v
      Rust native binary
             |
             v
    Read registry manifest
             |
             v
    Validate files and paths
             |
             v
   Preview or apply installation
             |
             v
        User project
```

The npm package will launch a platform-specific Rust binary. The file download and installation logic will be implemented in Rust, not in the JavaScript wrapper.

## Initial scope

The first version should focus on:

1. Parsing `list` and `add` commands.
2. Reading a small, versioned registry manifest.
3. Showing the installation plan with `--dry-run`.
4. Installing selected files into an existing project.
5. Rejecting unsafe paths and preventing accidental overwrites.
6. Reporting clear errors and testing install behavior in temporary project directories.

No authentication, UI framework, database, package publishing automation, or custom visual editor is planned for the initial implementation.

## Development roadmap

- **Phase 1 — Learn the fundamentals:** Build a minimal Rust CLI and support a local test registry.
- **Phase 2 — Install safely:** Add remote manifests, download validation, path safety, file collision handling, and tests.
- **Phase 3 — Distribute:** Compile binaries for supported operating systems and connect them to an npm CLI package for `npx` usage.
- **Later, if useful:** Support additional registry formats and dependency management based on real use cases.

## Design principles

- **Independent:** No runtime coupling to FrilCraft or any single registry.
- **Explicit:** Show which files will be added or changed.
- **Safe by default:** Do not overwrite existing files without explicit approval; reject path traversal and unexpected destinations.
- **Small:** Install only the requested item and its declared requirements.
- **Reproducible:** Prefer versioned manifests and verifiable file content.
- **Learn by building:** Keep the implementation understandable and document design decisions as it develops.

## Technology choices (planned)

- **Rust** for command parsing, registry handling, downloading, and file installation.
- **Node.js/npm** only as a distribution and execution entrypoint for `npx`.
- **GitHub Actions** for automated tests and platform-specific release builds when the CLI is ready.

Useful Rust crates will be selected as each phase is implemented, rather than adding dependencies in advance.

## Relationship to FrilCraft

[FrilCraft](https://github.com/FrilLab/frilcarft) builds reusable interactive web components. FrilAdd will eventually be able to install those components, but neither project should require the other to develop or run.

## License

A license has not yet been added to this repository. Choose one before publishing reusable source code; MIT is a candidate.
