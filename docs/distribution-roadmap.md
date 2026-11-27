# Distribution roadmap

FrilAdd is a provider-independent developer-side file installer, not a runtime renderer or asset authoring tool. Keep Registry v1 working for existing providers. Providers publish manifests and files that follow the documented contracts; FrilAdd contains no provider-specific implementation.

## v0.2.0 qualification status

FrilAdd v0.2.0 is accepted on the strength of FrilAdd-owned security, compatibility, test, package, and cross-platform release checks. Provider-specific integration status below is reported separately and does not gate this provider-agnostic CLI release. Unavailable provider resources are not treated as passed tests; remaining work is tracked in issue #33.

### Verified FrilAdd behavior

- Registry v1 retains its 10 MiB per-file cap; Registry v2 streams files up to 100 MiB and operations up to 512 MiB into verified temporary staging. Hash, size, and byte limits are checked before project destinations are committed.
- Managed ownership, status, update/remove/restore and portable target mapping use the shared lock and recoverable transaction contract. Dependency DAGs resolve across explicitly registered providers and install atomically with ownership edges. Package requirements are reported as advisory commands and are never executed.
- The CI workflow tests formatting, clippy, unit/CLI/Registry tests, Rust 1.88 MSRV, native build/tests, packed npm installation, and platform smoke tests on Linux x64, macOS ARM64/x64, and Windows x64.

### Provider qualification matrix

| Provider / consumer | Evidence | Status |
| --- | --- | --- |
| FrilCraft | Pinned provider commit `9fe26c6b9ccba34df027793d5e2e5f5ed45c69d3`; the published Envelope Reveal component and license were installed byte-for-byte into an isolated React consumer. Dry-run/no-write, repeat add, lock stability, behavior test, typecheck, and production build passed. The provider's own pinned-commit HTTPS workflow also passed. FrilStudio consumer typecheck/build and focused component interaction test passed after adding the provider-documented `cn` dependency in the isolated checkout. | Qualified for the available v1 component. FrilCraft currently has no reviewed binary asset for the cross-provider asset case. |
| FrilAssets | Repository inspected. No released registry or reviewed, licensed asset was available during qualification. | **Not qualified:** a reviewed real GLB, license/provenance, byte verification, and consumer render remain unperformed. Follow-up #33. |
| FrilRemotion | Pinned scaffold `a4b5b2992c81f01f36206cdba0d11a7ca65fc458`; provider registry validation, typecheck, and provider SmokeTest render passed. FrilAdd installed its published FadeIn primitive and license byte-for-byte; a clean Remotion consumer typechecked and rendered a 60-frame H.264 clip using the installed primitive. Status, exact-source restore, and remove were checked. | **Partially qualified:** a provider-published composition and dependency metadata, plus composition install/render, remain unperformed. Primitive rendering does not satisfy that test. Follow-up #33. |
| FrilStudio | Private consumer at `5acfa7cf742b9850d13ec7ce8d9ff17dbf9ab40e`; installed FrilCraft component was integrated and interaction-tested in an isolated checkout. Typecheck and production build passed with Node 24 and the provider-documented `cn` dependency. | Qualified for the available FrilCraft component. |

FrilCraft's source repository is accessible locally, but the developer-network FrilAdd HTTPS request to raw.githubusercontent.com timed out during this run. The pinned provider CI HTTPS integration has passed, and local integration used the byte-identical manifest/source checkout at the pinned commit. Record this as an environment limitation, not as local HTTPS qualification evidence.

## Safe resource lifecycle

The v1 project lock records provider identity, resolved source, item version or manifest digest, dependency edges, installed paths, and hashes. It preserves unversioned v1 provider identity without inventing versions, tracks ownership outside reserved metadata paths, and supports offline installed/status inspection, exact-source restore, explicit update, and remove through recoverable transactions. Lifecycle operations preserve local edits and unrelated files and expose dry-run and machine-readable output. Prefer pinned immutable release or commit URLs over mutable branch URLs.

Registry v2 defines item type, license, version, file size, package requirements, explicit registry dependencies, and portable target mapping. Dependency execution is limited to safe acquisition and installation of declared resources. Package installs remain explicit and opt-in; registry-provided scripts are never executed. Targets cannot traverse, follow symlinks, or enter reserved paths. Large model, HDR environment, and media transport uses bounded streaming and verified staging with per-file and aggregate limits. Registry v1 retains its original per-file cap.

## Provider and consumer boundaries

FrilAssets owns source assets, licenses, and optimized exports. FrilCraft owns components, rendering adapters, and interactions. FrilRemotion owns compositions and timelines. FrilStudio owns editing, publication, and end-user UX. FrilAdd owns registry resolution, verified acquisition, installation planning, and developer tooling. It is not invoked on wedding guests' browsers or video-rendering frames. Runtime asset hosting and public application deployment remain the consumer's responsibility.

## Completion definition

A clean consumer can select and install versioned components and binary assets from independent providers, verify them, build/render without cloning provider repositories, and repeat the process reliably in CI. Locally changed files are never overwritten silently. FrilAdd v0.2.0 establishes the provider-neutral distribution CLI contract; qualification of every provider and resource type is ongoing follow-up work. Do not claim integration based on synthetic fixtures or a primitive when the required published resource is absent.
