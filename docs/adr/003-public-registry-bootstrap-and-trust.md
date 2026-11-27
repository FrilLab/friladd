# ADR 003: Public registry bootstrap and trust boundary

Status: accepted architecture; canonical hosting and single-maintainer policy adoption are pending. The endpoint and static deployment are prepared but not verified live.

## Context

The 0.2.x series needs zero-configuration discovery while retaining private/custom registries. A client-controlled arbitrary URL would turn a catalog into an SSRF primitive and allow registry namespace spoofing. A public list also must not imply that provider content is safe.

## Decision

The directory is a separately maintained static JSON document in [FrilLab/frilregistry](https://github.com/FrilLab/frilregistry), not part of FrilAdd and not a registration API. It has the strict schema in `docs/schemas/public-directory-v1.schema.json`. Its fixed production endpoint is `https://registry.frillab.com/registries.json`; DNS, Cloudflare hosting, TLS and the JSON response at that host have not been verified, so the endpoint is not a live production service. No production endpoint overrides or fallback URLs are accepted. GitHub stores reviewed directory commits and runs CI; Cloudflare serves only static JSON and headers. Adding providers is a directory data change and does not require a FrilAdd rebuild.

Directory and provider transport are separate policies. The directory client accepts only the single compiled FrilLab URL and validates DNS answers for `registry.frillab.com`. Provider manifests and artifacts remain restricted to `raw.githubusercontent.com` paths under the reviewed GitHub repository and immutable commit. The directory host must never be added to the provider host allowlist. Each policy disables proxies and redirects, pins validated addresses in the HTTP resolver, retains TLS host verification, and bounds duration and bytes. Provider host expansion requires a separate security review.

The directory binds each registry ID to publisher, repository, immutable reviewed revision and manifest digest. The `fril` namespace is generically reserved for FrilLab providers whose IDs match their repository names; individual provider catalog IDs are not embedded in the CLI. Community add/update/restore requires `--trust-community ID` every time the operation includes that registry, including transitive dependencies. A lockfile records provenance for reproducible restore but never grants consent. Changed publisher, origin, revision or digest therefore requires renewed consent. Changed identity, suspension, malformed data, transport failure and missing entries fail closed for new public resolution. Clients do not retain stale directory data. Explicit custom/private registries keep their existing behavior and do not inherit public trust.

## Consequences

CLI discovery/install is merged, but production hosting and end-to-end qualification are pending. The directory repository has protected `main` with one required code-owner review and enforced checks. A single-maintainer policy proposal is draft-only and has no effect until explicitly adopted; no branch protection has changed and the FrilCraft candidate remains suspended. Only one FrilLab organization member is currently available. HTTPS to a controlled host does not defend against compromise of that host or its maintainers. Review is evidence and provenance, not a malware-free guarantee. See `docs/operations/public-directory.md` and frilregistry's deployment guide for operations.
