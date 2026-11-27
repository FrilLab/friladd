# Public registry directory (proposed for 0.2.x)

This document defines the static directory contract for public discovery. It does not claim that a public directory is currently deployed. The canonical endpoint is intentionally unset until FrilLab operates an HTTPS endpoint and reviews its branch protections and bootstrap content.

## Trust and scope

The directory is metadata that points to provider-owned Registry v1 or v2 manifests. It is not itself a registry, does not authorize arbitrary URLs, and never causes provider code to run. Listing approval means only that a maintainer reviewed the identity, provenance, declared files and hashes at a particular immutable revision. It is not a guarantee that the content is safe. Consumers must still inspect code and decide whether it fits their project.

The first production deployment must be served from one FrilLab-controlled HTTPS origin selected by maintainers. Clients must not accept endpoint overrides from configuration, environment variables, or command-line options in production. A future endpoint migration requires a CLI release or an explicit authenticated bootstrap design; clients must not follow redirects to discover a new trust root. HTTPS protects transport to the configured host but does not independently authenticate directory contents against a compromised host or FrilLab account. Endpoint ownership, account recovery, reviewed history and release rollback are therefore part of the trust root.

Private and custom registries remain configured explicitly and retain their existing behavior. Public directory policy is stricter and must never be used to reinterpret an explicitly selected custom source. Discovery and metadata commands do not register aliases or write project, global, lock, or transaction state.

## Schema version 1

The document is UTF-8 JSON, at most 1 MiB, with exactly `schemaVersion` and `registries`. Unknown fields are rejected. `schemaVersion` is the integer `1`. At most 256 entries are allowed. IDs are unique lower-case ASCII identifiers matching `[a-z][a-z0-9-]{0,62}`. Reserved IDs are retained in the directory even when their entries are unpublished; `frilcraft`, `frilassets`, `frilremotion`, and `frilstudio` are reserved for their corresponding official publishers.

Each entry has exactly these fields:

| Field | Rule |
| --- | --- |
| `id` | Stable ID described above. |
| `displayName` | 1–128 UTF-8 bytes; no control characters. |
| `description` | 1–512 UTF-8 bytes; no control characters. |
| `registryUrl` | Absolute HTTPS URL on an origin approved by the client allowlist; no user info, query, fragment, IP literal, non-default port, or ambiguous host spelling. |
| `repository` | Canonical HTTPS source repository URL, with no credentials, query or fragment. |
| `publisher` | Stable publisher identity, 1–128 bytes, no control characters. |
| `tier` | `official` or `community`. |
| `status` | `active`, `deprecated`, or `suspended`. |
| `reviewedRevision` | Immutable revision identifier: 40 or 64 lowercase hexadecimal characters. |
| `manifestSha256` | SHA-256 of the exact reviewed manifest bytes, lowercase 64-character hexadecimal. |
| `review` | Object with exactly `reviewedAt` (RFC 3339 UTC timestamp), `reviewer` (stable FrilLab account identity), and `notes` (1–1024 bytes, no control characters). |

Each ID and registry URL is unique, compared after canonicalization. Entries may not collide by repository, publisher identity, or reserved namespace. Official entries require an official reserved ID and repository/publisher ownership proof. Community entries are opt-in and are never implicitly trusted. Submitted values are data only; schema validation is not approval.

`deprecated` entries remain visible in discovery but cannot be selected for new install, update, or dependency resolution. Existing lock-based status, remove and exact restore remain independent of current directory status. `suspended` entries are hidden from normal listing and fail closed for any new resolution. Suspension does not delete consumer files or rewrite lockfiles. A corrected entry requires a reviewed directory change. Replacements require a new reviewed revision and digest plus explicit provenance review; an ID cannot silently move to another publisher or repository.

Directory bytes and entry strings are bounded as specified above. The client rejects duplicate JSON object keys, duplicate IDs, unknown fields, invalid enums, unsafe URLs, collisions, and malformed digests before using any entry. Fetch errors fail the public operation; there is no stale-cache fallback in the initial design.

## Selection and consent

`registry discover [ID] [--json]` reads directory metadata only. `list` and `search` keep their current configured-registry behavior. `--public` explicitly searches active official and community directory entries; it conflicts with explicit registry selectors and `--all-registries`. With no configured default, ordinary `list` and `search` may use active official entries only once the canonical endpoint is configured in a release. A directory outage is reported separately from a registry-manifest error; results are not presented as successful partial output.

`info` and `add` resolve `ID/ITEM` from an explicit configured alias first, then the public directory when the ID is not locally configured. A configured identity collision never falls through to public resolution. Unqualified installation never guesses a provider. Official active entries can be installed without a prompt. Community installation requires an explicit first-use authorization bound to the provider ID, publisher, approved origin, reviewed revision and manifest digest. Authorization must be scoped to that exact identity, require an affirmative interactive response (or a dedicated exact-provider approval option in noninteractive use), and be reconsidered when any bound value changes or the entry is suspended. A generic `--yes` does not confer community trust. Dry runs do not persist trust. Every dependency is subject to the same checks.

Locked restore uses exact recorded provenance and hashes, never current directory URLs. Existing explicit registry operations, configured defaults, `--all-registries`, offline status/remove/restore, Registry v1/v2, lock v1, configuration, output schema v1 and exit codes remain compatible. Public discovery failures do not affect offline operations or explicit custom registries.

## Operations and review

Maintainers must establish protected directory source history, independent review for submissions, immutable reviewed source revisions, suspension and rollback procedures, an abuse/security contact and periodic endpoint checks before deployment. Submission CI may validate static JSON and schemas only. It must not run contributor code, fetch unreviewed URLs on privileged runners, or auto-approve/merge entries. Public provider admission requires evidence of repository/domain control, publisher identity, manifest digest, artifact hashes/size/path safety, licensing/provenance and security review. Review scope and limitations must be recorded. Keys/accounts and endpoint migration require owner-controlled recovery; no client-side key rotation mechanism is defined in schema v1.

## Example fixture (not a production endpoint)

`docs/schemas/public-directory-v1.example.json` is a non-live schema fixture. It intentionally uses an invalid example host and must never be shipped as bootstrap data.
