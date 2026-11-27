# Public directory architecture review — 2026-11-27

## Scope and release decision

Reviewed the merged implementation for #39, #40, #41 and #42 (PRs #44, #46, #47 and #48), plus release candidate PR #49; source paths include `src/public_transport.rs`, `src/public_directory.rs`, public resolution in `src/cli.rs`, dependency resolution, lock validation, and restore acquisition. Also reviewed the current `FrilLab/frilregistry` source, validation workflow, candidate provider PR #1, and directory admission/operations controls.

**Decision: NO-GO for v0.2.1.** The target canonical endpoint currently returns DNS `NXDOMAIN` for both A and AAAA. No Cloudflare credentials or deployed Worker are available in the inspected environment, the directory has no active provider, and branch protection still blocks the suspended FrilCraft candidate. This review does not claim successful canonical discovery, provider installation, or production qualification.

## Architecture and security decisions

- **Bootstrap root.** The production URL is fixed to `https://registry.frillab.com/registries.json`; there is no environment/config/CLI override or URL fallback. The endpoint has not been deployed. The source-of-truth directory remains the separate `FrilLab/frilregistry` repository.
- **Separate transports.** Directory requests are accepted only for the exact fixed FrilLab URL and use DNS validation/pinning for `registry.frillab.com`. Provider resources remain on `raw.githubusercontent.com` and must match a reviewed GitHub repository and immutable revision. The directory host is rejected by provider URL validation. Both paths disable proxies and redirects, retain HTTPS certificate/hostname checks, pin checked DNS answers in reqwest, and bound time and response bytes.
- **Directory parsing and provenance.** The Rust parser rejects unknown fields, duplicate struct keys, duplicate IDs/URLs/repositories, malformed identities, invalid timestamps, unsafe provider origins and invalid digests. A manifest is fetched only from the reviewed immutable repository path, then checked against the directory's SHA-256 and declared provider ID. Provider artifacts must remain beneath the reviewed manifest directory and are hash/size checked before transactional publication.
- **Namespace and future providers.** The previous hardcoded list of four official provider IDs prevented new official entries from being accepted without a FrilAdd change. The contract now reserves the generic `fril` namespace for official entries whose ID matches their FrilLab repository name. Community entries cannot use that namespace or FrilLab ownership. This keeps individual catalog contents out of FrilAdd while preventing community namespace claims.
- **Community authorization.** A project lock is untrusted and cannot grant community consent. Each add, update, and restore that includes a community provider requires `--trust-community ID`, including transitive providers. Restore remains directory-independent and uses exact locked locators, digest and size. Lock validation rejects official-tier records inconsistent with the generic FrilLab namespace/repository rule.
- **Directory repository validation.** The original JavaScript `JSON.parse` validation accepted duplicate JSON object keys because parsing silently retains the last value. CI now runs a standard-library duplicate-key checker with root/nested negative self-tests before schema/policy validation and before deployment.
- **Installation and compatibility.** The installer, ownership, lock/transaction formats and configured custom/private registry path remain the existing shared implementations. Public discovery does not write config or project state; the non-hermetic live-directory assertion was removed from the regular test suite. Production HTTPS and packed npm qualification remain separate required gates.

## Hosting and operations

The frilregistry change prepares a static-only Cloudflare Workers Assets deployment. It copies only reviewed `registries.json` and `_headers` into the deploy directory; no Worker code, backend, database, R2, proxy, or URL fetch exists. The prepared response sets JSON content type and 60-second browser/CDN freshness. The documented emergency procedure purges only the canonical document with a separate narrowly scoped operator token. GitHub remains reviewed source and CI; Cloudflare serves the asset.

The deployment config passed `wrangler 4.168.0 deploy --dry-run`; this is configuration validation, not deployment evidence. Initial provisioning needs the zone owner to create the Worker/custom domain, then a Worker-scoped deployment token and GitHub environment secret/variable. Exact scopes and rollback steps are in frilregistry `DEPLOYMENT.md`.

The single-maintainer admission proposal is draft-only. It does not edit branch protections or activate any provider. The existing one-code-owner rule remains in force; FrilCraft PR #1 remains suspended. The proposed policy requires explicit owner approval, successful automation, recorded manual source/security/provenance/licensing review, immutable resource identifiers, and visible disclosure that official single-maintainer review lacks independence. Community admissions remain closed unless sufficient stricter review capacity exists.

## Residual risks and required gates

- DNS/Worker/custom-domain provisioning and external TLS/headers/response verification.
- Maintainer adoption of the single-maintainer policy and separately authorized branch-protection adjustment before any official activation.
- Reviewed activation of at least one official provider; candidate CI/hash evidence alone is not admission or proof of safety.
- Clean packed npm end-to-end discovery/search/info/add with exact manifest/artifact byte verification, repeat install, status/update/remove/restore, outage/custom registry isolation, community-consent and transitive-trust checks, and suspension/cache behavior.
- Full release-commit artifact rebuild, checksums, packed npm verification and supported native/npm matrix on the exact release commit. Publication still needs exact-version explicit approval.

No endpoint override, arbitrary fetch, automatic submission approval, provider code execution, or release publication was introduced. Provider qualification issue #33 remains separate and incomplete.
