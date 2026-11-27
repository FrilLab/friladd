# ADR 003: Public registry bootstrap and trust boundary

Status: accepted contract; static host deployed, catalog bootstrap admission pending independent review.

## Context

The 0.2.x series needs zero-configuration discovery while retaining private/custom registries. A client-controlled arbitrary URL would turn a catalog into an SSRF primitive and allow registry namespace spoofing. A public list also must not imply that provider content is safe.

## Decision

The directory is a separately maintained static JSON document in [FrilLab/frilregistry](https://github.com/FrilLab/frilregistry), not part of FrilAdd and not a registration API. It has the strict schema in `docs/schemas/public-directory-v1.schema.json`. The canonical static endpoint is `https://raw.githubusercontent.com/FrilLab/frilregistry/main/registries.json`; it was verified reachable with HTTP 200 and currently contains no entries. FrilAdd must call it only through the strict public transport with approved origin and DNS/peer validation for the directory, manifests and artifacts. Production endpoint overrides are forbidden. CLI integration remains pending issue #41.

The directory binds each registry ID to publisher, repository, approved URL origin, immutable reviewed revision and manifest digest. Official IDs are reserved. Community entries remain opt-in and require exact identity-scoped consent. Changed identity/revision, suspension, malformed data, transport failure and missing entries fail closed for new public resolution. No stale public cache is used. Explicit custom registries keep their current path and do not inherit public trust.

## Consequences

The strict parser and provider transport are implemented in source; integration remains pending. The directory repository has static local-only validation and protected `main` with one required code-owner review and enforced checks. A FrilCraft candidate is pinned and its provider consumer-install CI and hashes have been checked, but it remains suspended pending independent maintainer approval. Only one FrilLab organization member is currently available, so activation is blocked. HTTPS to a controlled host does not defend against compromise of that host or its maintainers. Review is evidence and provenance, not a malware-free guarantee.
