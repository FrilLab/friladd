# Distribution roadmap

FrilAdd is a provider-independent developer-side file installer, not a runtime renderer or asset authoring tool. Keep registry v1 working for existing providers. Do not hard-code FrilLab URLs or require consumers to install every provider.

## Current contract (v0.1.2 baseline)

- FrilCraft publishes a v1 manifest for Envelope Reveal, with a documented React/Tailwind setup.
- FrilAssets is an asset production repository; reviewed asset publication and an install registry are not yet available.
- FrilStudio is a consumer. FrilRemotion is an intended producer/consumer; its current repository implementation was not accessible during this review.
- v1 supports exact binary bytes, with a 10 MiB per-file limit. No installed-item records, package or registry dependencies, update/uninstall or target mapping are implemented.

## Phase 1: real provider release gate

- [ ] Provider verification of every declared file using the same validation and hash logic as installation.
- [ ] CI tests local and HTTPS installation, dry run, tampered/missing files, binary byte identity, and existing-file protection.
- [ ] Build and test the packed npm CLI on all supported platforms; publish only after successful CI and release review.
- [ ] Publish one reviewed FrilAssets item containing a GLB and any required textures/license, with a provider-generated manifest. Install it into an empty consumer using a pinned registry URL, then render it in a real consumer.
- [ ] Install a real FrilCraft component from a pinned manifest, then typecheck and build a clean React consumer with documented dependencies.
- [ ] Install a real FrilRemotion composition and render a short clip once the provider is available.

## Phase 2: safe resource lifecycle

Design a versioned project lock record with registry identity, resolved source, item version or manifest digest, installed paths and hashes. Preserve v1 providers; do not silently invent versions for unversioned items. Establish ownership and reserve FrilAdd and VCS metadata paths. Implement installed/status, reproducible restore and opt-in update/remove only after conflict and rollback tests. Never silently replace modified files or delete unrelated files. Explicitly distinguish unmanaged existing files, unchanged installed files and locally edited files. Include dry run and machine-readable output. Prefer pinned immutable release or commit URLs over mutable branch URLs.

## Phase 3: assets and composition dependencies

Design a backward-compatible extension or a negotiated v2 protocol for item type, license, version, file size, package requirements and explicit registry dependencies. Resolve and detect dependency cycles and conflicting target paths before writing. Keep package installs explicit and opt-in; never execute registry-provided scripts. Support configurable target roots without permitting traversal, symlinks or reserved paths. For larger models, HDR environments and media, use bounded streaming to verified staged files, explicit per-file and aggregate limits and transactional rollback. Do not merely increase the existing in-memory buffer limit.

## Boundaries

FrilAssets owns source assets, licenses and optimized exports. FrilCraft owns components, rendering adapters and interactions. FrilRemotion owns compositions and timelines. FrilStudio owns editing, publication and end-user UX. FrilAdd owns registry resolution, verified acquisition, installation planning and developer tooling. It is not invoked on wedding guests' browsers or video-rendering frames. Runtime asset hosting and public application deployment remain the consumer's responsibility.

## Completion definition

A clean consumer can select and install a versioned component and binary asset from independent providers, verify them, build/render without cloning those repositories, and repeat the process reliably in CI. Locally changed files are never overwritten silently. Document the exact commands, dependency setup, supported platforms and known limitations. Do not claim an integration is finished based on a synthetic fixture alone.
