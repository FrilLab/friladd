# Distribution roadmap

FrilAdd is a provider-independent developer-side file installer, not a runtime renderer or asset authoring tool. Keep registry v1 working for existing providers. Do not hard-code FrilLab URLs or require consumers to install every provider.

## v0.1.2 baseline (historical)

- FrilCraft publishes a v1 manifest for Envelope Reveal, with a documented React/Tailwind setup.
- FrilAssets is an asset production repository; reviewed asset publication and an install registry are not yet available.
- FrilStudio is a consumer. FrilRemotion is an intended producer/consumer; its current repository implementation was not accessible during this review.
- v1 supports exact binary bytes, with a 10 MiB per-file limit. No installed-item records, package or registry dependencies, update/uninstall or target mapping are implemented.

## v0.2.0 implementation status

- Registry v1 keeps its 10 MiB file limit. Registry v2 streams up to 100 MiB per file and 512 MiB per operation into verified temporary staging, with exact size and SHA-256 checks before the shared transaction engine writes project files.
- Managed ownership, status, update/remove/restore and target mapping use the shared lock and recoverable transaction contract. Dependency execution and real-provider qualification remain tracked in Epic #9.

## Phase 1: real provider release gate

- [ ] Provider verification of every declared file using the same validation and hash logic as installation.
- [ ] CI tests local and HTTPS installation, dry run, tampered/missing files, binary byte identity, and existing-file protection.
- [ ] Build and test the packed npm CLI on all supported platforms; publish only after successful CI and release review.
- [ ] Publish one reviewed FrilAssets item containing a GLB and any required textures/license, with a provider-generated manifest. Install it into an empty consumer using a pinned registry URL, then render it in a real consumer.
- [ ] Install a real FrilCraft component from a pinned manifest, then typecheck and build a clean React consumer with documented dependencies.
- [ ] Install a real FrilRemotion composition and render a short clip once the provider is available.

## Phase 2: safe resource lifecycle (implemented in v0.2.0)

The v1 project lock records provider identity, resolved source, item version or manifest digest, installed paths and hashes. It preserves unversioned v1 provider identity without inventing versions, tracks ownership outside reserved metadata paths, and supports installed/status, exact-source restore, explicit update and remove through recoverable transactions. Lifecycle operations preserve local edits and unrelated files and expose dry-run and machine-readable output. Prefer pinned immutable release or commit URLs over mutable branch URLs.

## Phase 3: assets and composition dependencies

Registry v2 defines item type, license, version, file size, package requirements and explicit registry dependencies. Dependency execution and conflict resolution remain tracked work. Package installs stay explicit and opt-in; registry-provided scripts are never executed. Target roots are configurable without permitting traversal, symlinks or reserved paths. Large model, HDR environment and media transport uses bounded streaming, verified staging, per-file and aggregate limits and transactional rollback. Registry v1 retains its original per-file cap.

## Boundaries

FrilAssets owns source assets, licenses and optimized exports. FrilCraft owns components, rendering adapters and interactions. FrilRemotion owns compositions and timelines. FrilStudio owns editing, publication and end-user UX. FrilAdd owns registry resolution, verified acquisition, installation planning and developer tooling. It is not invoked on wedding guests' browsers or video-rendering frames. Runtime asset hosting and public application deployment remain the consumer's responsibility.

## Completion definition

A clean consumer can select and install a versioned component and binary asset from independent providers, verify them, build/render without cloning those repositories, and repeat the process reliably in CI. Locally changed files are never overwritten silently. Document the exact commands, dependency setup, supported platforms and known limitations. Do not claim an integration is finished based on a synthetic fixture alone.
