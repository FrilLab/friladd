# Public directory operations runbook

## Service and ownership

The public directory is the static repository [FrilLab/frilregistry](https://github.com/FrilLab/frilregistry), served at `https://raw.githubusercontent.com/FrilLab/frilregistry/main/registries.json`. It has no API, database, authentication service, runtime backend, or telemetry dependency. Repository owner/reviewer and security-reporting instructions are in that repository's `CONTRIBUTING.md`, `CODEOWNERS`, and `SECURITY.md`. Report suspected vulnerabilities privately through GitHub Security Advisories for the affected repository; do not publish exploit details in a directory submission.

Admission is a human review. Validate repository control and publisher identity, manifest and artifact provenance, license, immutable source revision, manifest digest, file hashes, safe paths, and declared dependencies. CI validates static data only. It cannot approve, merge, execute provider code, or guarantee provider safety. A listed provider still requires consumer code review.

## Submission and review

1. Open a pull request against `FrilLab/frilregistry`; include evidence for each admission criterion and identify the exact repository owner, immutable revision, manifest digest, and review scope.
2. Run the repository's local-only schema/policy validator. Never let the validator fetch contributor URLs or run provider scripts.
3. Require the configured validation status and an independent code-owner review. Do not self-approve, bypass branch protection, activate a candidate, or approve automatically.
4. Merge only a reviewed, passing PR under the directory repository's branch protection. Verify the resulting static file at the canonical endpoint.
5. For suspension, change the entry status to `suspended` in a reviewed PR. Keep history and consumer files intact. For correction or rollback, submit another reviewed catalog commit; do not force-push or move an immutable provider revision.

A public listing means reviewed provenance and metadata, not a safety guarantee. Consumers must inspect source and decide whether it fits their project.

## Outage and incident response

FrilAdd fails public operations closed when the endpoint cannot be fetched or validated; there is no stale cache. Explicit custom/private registries continue through their independent configured path. Offline `status`, `remove`, and exact lock-based `restore` remain directory-independent. During an outage, announce the observed endpoint and time in the repository issue tracker and restore the static repository/origin; do not add an endpoint override or redirect fallback.

For suspected malicious, compromised, or misattributed content, suspend the entry through a reviewed change as soon as an authorized maintainer can do so. Record incident scope and affected immutable revisions. Suspension blocks new resolution but cannot revoke files already installed. Publish remediation and recovery instructions without claiming automated consumer cleanup.

## Maintenance policy

After the 0.2 feature series, routine work is limited to bug fixes, security updates, and necessary compatibility changes. New provider admissions are directory-only and must not require provider-specific FrilAdd code. No mandatory usage telemetry is collected. Review directory access, branch protection, endpoint availability, issue queue, and security contact during each maintainer handoff; no on-call SLA or third-party monitoring is currently configured.

## Current operational state (2026-11-27)

- Endpoint returns the static v1 document, but the catalog is empty.
- FrilCraft candidate PR #1 is suspended and requires an independent code-owner review.
- The FrilLab organization has one member (`mors119`), so that required independent review is currently unavailable.
- Therefore public installation and production launch are NO-GO. Do not represent a provider workflow as qualified until it succeeds from a clean environment with the packed npm CLI.
