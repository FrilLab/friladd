# Public registry directory (0.2.x contract and deployment status)

This document defines the static directory contract for public discovery and records the current deployment state. FrilLab operates the separate public repository [FrilLab/frilregistry](https://github.com/FrilLab/frilregistry). The fixed production endpoint is `https://registry.frillab.com/registries.json`. Its DNS, Cloudflare deployment, TLS and JSON response have not been verified; therefore it is not live. The interim GitHub raw source URL is not an approved production fallback. The directory currently has no active provider, and the CLI fails closed until the canonical endpoint is deployed.

## Trust and scope

The directory is metadata that points to provider-owned Registry v1 or v2 manifests. It is not itself a registry, does not authorize arbitrary URLs, and never causes provider code to run. Listing approval means only that a maintainer reviewed the identity, provenance, declared files and hashes at a particular immutable revision. It is not a guarantee that the content is safe. Consumers must still inspect code and decide whether it fits their project.

The endpoint is intended to be a static Cloudflare Workers Asset built from reviewed `FrilLab/frilregistry` commits. GitHub remains the source, review history and CI; Cloudflare serves only static JSON. Clients do not accept endpoint overrides and do not follow redirects or fall back to GitHub raw. HTTPS does not independently authenticate directory contents against compromise of the domain, Cloudflare account or FrilLab account. Domain ownership, account recovery, reviewed history, deployment permissions, cache policy and rollback are part of the trust root. Existing repository protection currently requires one code-owner approval. A single-maintainer policy proposal is draft-only pending explicit adoption.

Private and custom registries remain configured explicitly and retain their existing behavior. Public directory policy is stricter and must never be used to reinterpret an explicitly selected custom source. Discovery and metadata commands do not register aliases or write project, global, lock, or transaction state.

## Schema version 1

The document is UTF-8 JSON, at most 1 MiB, with exactly `schemaVersion` and `registries`. Unknown fields are rejected. `schemaVersion` is the integer `1`. At most 256 entries are allowed. IDs are unique lower-case ASCII identifiers matching `[a-z][a-z0-9-]{0,62}`. The `fril` namespace is reserved for official FrilLab providers: an official ID must start with `fril`, match the lower-case repository name, and use the matching FrilLab repository and publisher. Community entries cannot use the `fril` namespace or a FrilLab-owned repository. This generic namespace rule lets FrilLab add future official providers through directory data without embedding provider catalog IDs in FrilAdd.

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

Each ID, registry URL, and repository is unique, compared after canonicalization. The publisher identity must match the GitHub repository owner; official entries follow the generic `fril` namespace rule. Community entries cannot claim a FrilLab repository or namespace, are opt-in, and are never implicitly trusted. Submitted values are data only; schema validation is not approval.

`deprecated` entries remain visible in discovery but cannot be selected for new install, update, or dependency resolution. Existing lock-based status, remove and exact restore remain independent of current directory status. `suspended` entries are hidden from normal listing and fail closed for any new resolution. Suspension does not delete consumer files or rewrite lockfiles. A corrected entry requires a reviewed directory change. Replacements require a new reviewed revision and digest plus explicit provenance review; an ID cannot silently move to another publisher or repository.

Directory bytes and entry strings are bounded as specified above. The client rejects duplicate JSON object keys, duplicate IDs, unknown fields, invalid enums, unsafe URLs, collisions, and malformed digests before using any entry. Fetch errors fail the public operation; there is no stale-cache fallback in the initial design.

## Selection and consent

`registry discover [ID] [--json]` reads directory metadata only. `list` and `search` keep their current configured-registry behavior. `--public` explicitly searches active official and community directory entries; it conflicts with explicit registry selectors and `--all-registries`. With no configured default, ordinary `list` and `search` may use active official entries only once the canonical endpoint is configured in a release. A directory outage is reported separately from a registry-manifest error; results are not presented as successful partial output.

`info` and `add` resolve `ID/ITEM` from an explicit configured alias first, then the public directory when the ID is not locally configured. A configured identity collision never falls through to public resolution. Unqualified installation never guesses a provider. Official active entries can be installed without a prompt. Community add, update, and lock-based restore require an explicit `--trust-community ID` every time the operation includes that provider, including transitive dependencies. A lockfile records provenance for reproducible restore but never grants consent. A generic `--yes` does not confer community trust. Every dependency is subject to the same checks; changed publisher, origin, reviewed revision or digest requires renewed authorization.

Locked restore uses exact recorded provenance and hashes, never current directory URLs. Existing explicit registry operations, configured defaults, `--all-registries`, offline status/remove/restore, Registry v1/v2, lock v1, configuration, output schema v1 and exit codes remain compatible. Public discovery failures do not affect offline operations or explicit custom registries.

## Operations and review

The directory repository documents the maintainer workflow in its [contribution policy](https://github.com/FrilLab/frilregistry/blob/main/CONTRIBUTING.md), security policy, CODEOWNERS and `AGENTS.md`. Admission requires evidence of repository control, publisher identity, immutable manifest digest, artifact hashes/size/path safety, licensing/provenance and security review. Review scope and limitations must be recorded. CI performs local bounded JSON/schema/policy checks only; it does not fetch contributor URLs or execute provider code and cannot approve or merge entries. Suspension preserves history and consumer files; correction and rollback use reviewed catalog commits. Account recovery and endpoint migration remain owner-controlled; schema v1 has no client-side key rotation mechanism.

Current operational blocker: the FrilLab organization currently has one member (`mors119`). The FrilCraft entry candidate is in [frilregistry PR #1](https://github.com/FrilLab/frilregistry/pull/1), pinned to a commit with successful provider consumer-install CI and matching manifest/file hashes, but its status is `suspended`. Branch protection requires a code-owner approval that is unavailable for a maintainer-authored PR. The draft single-maintainer policy has not been adopted; branch protection has not changed and the provider remains suspended. Do not merge or activate it under the proposed policy without explicit maintainer authorization.

## Example fixture (not a production endpoint)

`docs/schemas/public-directory-v1.example.json` is a non-live schema fixture. It intentionally uses an invalid example host and must never be shipped as bootstrap data.
