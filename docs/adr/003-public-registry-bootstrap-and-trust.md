# ADR 003: Public registry bootstrap and trust boundary

Status: accepted contract; deployment pending maintainer operation.

## Context

The 0.2.x series needs zero-configuration discovery while retaining private/custom registries. A client-controlled arbitrary URL would turn a catalog into an SSRF primitive and allow registry namespace spoofing. A public list also must not imply that provider content is safe.

## Decision

The directory is a separately maintained static JSON document, not part of FrilAdd and not a registration API. It has the strict schema in `docs/schemas/public-directory-v1.schema.json`; a real production document must be hosted on one FrilLab-controlled HTTPS origin selected by maintainers. No endpoint is built into the implementation until ownership, deployed content and public reachability are verified. The client must use a strict public transport with approved origin and DNS/peer validation for the directory, manifests and artifacts. It must never accept production endpoint overrides.

The directory binds each registry ID to publisher, repository, approved URL origin, immutable reviewed revision and manifest digest. Official IDs are reserved. Community entries remain opt-in and require exact identity-scoped consent. Changed identity/revision, suspension, malformed data, transport failure and missing entries fail closed for new public resolution. No stale public cache is used. Explicit custom registries keep their current path and do not inherit public trust.

## Consequences

This document records the contract but does not claim that network enforcement or CLI support is already shipped. The endpoint, signing/rotation mechanism, real provider admission and deployment operations remain prerequisites for #40–#43. HTTPS to a controlled host does not defend against compromise of that host or its maintainers. Review is evidence and provenance, not a malware-free guarantee.
