# Public directory operations runbook

The public catalog is maintained in [FrilLab/frilregistry](https://github.com/FrilLab/frilregistry). The canonical static URL is:

```text
https://raw.githubusercontent.com/FrilLab/frilregistry/main/registries.json
```

There is no service, database, account system, or registration API. Directory changes are reviewed Git changes. `registries.json` is the consumer document; `schema.json`, `scripts/validate-directory.mjs`, and `scripts/check-json-duplicates.py` define and check its static format and policy.

## Ownership and controls

- Repository owner: FrilLab organization. Current code owner: `@mors119`.
- `main` is protected. Required checks are `Validate static directory / validate`; pull requests need one code-owner approval, dismiss stale reviews, require approval after the latest push, enforce rules for administrators, reject force pushes/deletions, and require linear history and resolved conversations.
- CI has `contents: read`, does not persist checkout credentials, and performs local JSON/schema/policy validation only. It does not fetch contributor URLs, execute provider code, auto-approve, or auto-merge.
- The public content endpoint is HTTPS on `raw.githubusercontent.com`. FrilAdd's client must use its compiled canonical URL and strict public transport. Never follow endpoint redirects or accept user-provided public-directory overrides.

## Admission procedure

1. Triage a GitHub issue or pull request. Confirm a stable ID, tier, publisher, repository, full immutable commit, manifest URL at that commit, exact manifest digest, file hashes/sizes/paths, license/provenance, dependencies and security review scope.
2. Confirm source ownership with GitHub organization/repository permissions or independently verifiable publisher evidence. Reserved IDs are `frilcraft`, `frilassets`, `frilremotion`, and `frilstudio`; only FrilLab-owned corresponding repositories may use them as official entries.
3. Inspect exact manifest and all declared files at the pinned commit. Recalculate every digest; verify path and size limits; read licenses; inspect scripts, network behavior and dependency requirements. Do not execute submitted provider code.
4. Record the FrilLab account identity, UTC review time, immutable revision, manifest SHA-256, review findings and limits. Listing approval is not a guarantee of safety.
5. Run static validation, require an independent code-owner approval, then merge under branch protection. Community entries require explicit user consent in FrilAdd and are not trusted by being listed.

No submission is approved by automation. Never bypass required review to activate an entry. An entry with unresolved provenance or review evidence remains absent or suspended.

## Suspension, correction and rollback

- For a credible security report or ownership compromise, submit a reviewed catalog change setting `status` to `suspended`. This blocks new public resolution without deleting consumer files or rewriting locks.
- Preserve the immutable Git history and incident evidence. Notify affected maintainers through GitHub's private vulnerability reporting where available.
- A corrected source requires a new immutable revision and manifest digest, fresh hashes, review and consumer re-consent if identity binding changes.
- Roll back a mistaken catalog update with a new reviewed commit reverting the data. Never force-push or move a pinned provider commit.
- Check public availability and parse validity after each merge. On endpoint outage, report discovery failure and do not use stale cache; explicit custom registries and offline lifecycle commands continue independently.

## Current deployment record (2026-11-27)

- Repository created: [FrilLab/frilregistry](https://github.com/FrilLab/frilregistry).
- Initial static document commit: `75650f4`; endpoint returned HTTP 200 and an empty schema-v1 directory.
- Branch protection was verified through GitHub's branch protection API with the requirements listed above.
- Only one FrilLab organization member (`mors119`) is currently available. Candidate admission [PR #1](https://github.com/FrilLab/frilregistry/pull/1) proposes the pinned FrilCraft provider but keeps it suspended; it cannot merge until a second authorized code owner reviews it.
- FrilCraft evidence: immutable source commit `a5e57a4cda0a12c4dcce906ea98d20a94f1346a5`, manifest SHA-256 `963bbf8bdb7078921bc24bec1523e90d77a50e1b6f4489e44c841be0f581cc22`, declared source hashes matched, and `source-install-consumer` CI passed at that revision. This is provenance and integration evidence, not proof that code is harmless.
- FrilAssets and FrilRemotion qualification remain tracked separately in issue #33. No evidence for those integrations is claimed here.

## Maintenance cadence

Review pending submissions regularly, check endpoint availability and schema validity after catalog changes, and periodically verify organization/repository ownership and branch rules. After FrilAdd 0.2.x, routine CLI work is limited to bug fixes, security updates and necessary compatibility changes. Provider expansion occurs as reviewed directory data changes, not provider-specific FrilAdd code.
