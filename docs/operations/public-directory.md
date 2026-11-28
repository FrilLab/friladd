# Public directory operations runbook

## Service and ownership

The source of truth is the static repository [FrilLab/frilregistry](https://github.com/FrilLab/frilregistry). The fixed production endpoint is `https://registry.frillab.com/registries.json`. Public DNS, TLS, and the Cloudflare JSON response were verified on November 28, 2026. GitHub stores the catalog and runs CI. The deployment uses Workers Static Assets, with no Worker application code, database, R2 bucket, API, authentication service, or arbitrary URL fetching. See frilregistry `DEPLOYMENT.md` for provisioning, credentials, release checks, cache behavior, rollback, and incident handling.

Directory and provider transport are separate trust policies. The CLI's directory request is fixed to that exact FrilLab URL. Provider manifests and artifacts remain restricted to reviewed GitHub repository paths on `raw.githubusercontent.com`. Never widen one policy by changing the other, accept an endpoint override, follow redirects, or add a GitHub raw fallback.

## Admission and suspension

Admission requires a human review of source ownership, publisher identity, immutable source revision, exact manifest bytes/digest, every declared resource and hash/size/path, licensing/provenance, dependencies, and security scope. Record reviewer identity, UTC time, evidence, findings, and limitations. Official IDs use the generic `fril` namespace and matching FrilLab repository name; no provider catalog is embedded in FrilAdd. Do not execute provider code. CI validates bounded local data and does not approve, merge, fetch provider URLs, or guarantee safety.

The active FrilRegistry ruleset requires a PR, linear history, and the validation check. It currently requires zero approving reviews. The separate single-maintainer admission policy proposal remains draft. FrilCraft's activation changed the status field while its review record still requires an independent approval. Static validation and deployment do not establish that approval. See the [activation PR and review](https://github.com/FrilLab/frilregistry/pull/3). External community admission remains closed pending review capacity. FrilAdd requires `--trust-community ID` on every community add, update, and lock-based restore operation.

For a credible security or ownership issue, suspend the provider in a reviewed catalog change as soon as an authorized maintainer can do so. Preserve history, record affected immutable revisions, and publish a reviewed correction or rollback. Suspension blocks new public resolution; it cannot revoke previously installed files. Tracked installations remain available to normal lifecycle commands.

## Cache, outage, and incident response

The prepared static response sets `Cache-Control: public, max-age=60, must-revalidate` and `CDN-Cache-Control: public, max-age=60`. The intended maximum ordinary revocation delay is 60 seconds plus edge propagation. For urgent suspension, use a separate narrowly scoped operator token to purge only the canonical JSON URL; do not grant cache-purge rights to deployment CI. Verify the new response bytes and status externally and record the time. The CLI does not retain stale directory data.

On directory outage, discovery and public installs fail closed. Explicit private/custom registry operations and offline status, remove, and exact locked restore remain independent. Do not add overrides or redirect/fallback logic. For domain, GitHub, or Cloudflare account compromise, restrict credentials, suspend affected entries, preserve evidence, involve the account owner, and follow the relevant repository `SECURITY.md` process.

## Deployment verification

After each deployment, confirm HTTPS certificate validation, HTTP 200 without redirects, `Content-Type: application/json`, the 60-second cache headers, exact response bytes matching the reviewed source commit, valid schema-v1 JSON, and 404 for unknown paths. Then verify `fril registry discover --json` with the packed npm CLI from a clean environment. Record endpoint response headers, source commit, SHA-256, CLI version, and verification time. A successful deploy command alone is not proof of a live or correct endpoint.

## Maintenance and status

After successful v0.2.1 publication and public verification, FrilAdd follows the maintenance policy in [AGENTS.md](../../AGENTS.md). New providers are directory-only and must not require FrilAdd republication. Review branch protections, domain ownership, token scope, TLS, endpoint availability, deployment history, and security contact during maintainer handoffs. No on-call SLA or third-party monitoring is configured.

The [production deployment](https://github.com/FrilLab/frilregistry/actions/runs/46123431201) serves directory commit `09048c5fc84ff528882b48b71392b399cc4f988e` byte-for-byte. The JSON SHA-256 is `7ff8e8254b07b8fcc4315aa9bd41c165d311a9439699280746b0490fd5109e01`. The canonical request returned HTTP 200 with `application/json; charset=utf-8`, the configured 60-second cache headers, and `nosniff`. An unknown path returned 404. The response labels FrilCraft active. Admission evidence and exact candidate qualification are tracked in [FrilAdd #43](https://github.com/FrilLab/friladd/issues/43).
