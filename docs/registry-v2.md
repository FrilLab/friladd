# Registry Manifest v2

Registry v2 is an opt-in protocol. Registry v1 manifests, including the original FrilCraft format, keep their existing syntax and reader. Register either schema with the same `registry add` command. Use `list` to inspect names and `registry verify` to validate the manifest's declared files. Managed add, cross-registry execution, and large-file streaming are delivered in the corresponding lifecycle issues. [Registry v2 JSON Schema](schemas/registry-v2.schema.json)

## Identity and selection

- A registry has `schemaVersion: 2`, an immutable publisher-chosen `id`, a display `name`, and an `items` array. Choose a globally scoped identifier, for example `org.example.assets`. Names and identifiers use the lowercase ASCII grammar from v1. An installed v2 identity is recorded as `v2:<id>`. Registration aliases remain local lookup keys.
- Each item has a unique `id`, an exact Semantic Version 2.0 `version`, a `kind`, and at least one file. Supported kinds are `component`, `asset`, `material`, `texture`, `image`, `environment`, `animation`, `composition`, and `other`. New kinds need an explicit compatibility change. Use `other` for a resource that does not fit the current set.
- Pick a published immutable manifest URL to select a snapshot. A snapshot contains one version per item ID and is identified by the SHA-256 of its literal bytes. An exact version requirement must match the selected item. The CLI does not enumerate releases or pick a semantic version range for registry items.

## Files and metadata

- Each file has portable relative `source` and `target`, a lowercase 64-hex `sha256`, and an integer `size` in bytes. The format allows up to 100 MiB per file. Larger fixtures require the streaming transport in #17. Select the provider manifest itself with a local path or an HTTPS URL. V2 manifest URLs omit queries, fragments, and credentials. External artifact URLs are outside this schema. HTTPS artifact requests and redirects stay on the original origin. Artifact redirect destinations omit queries and fragments.
- Optional fields are `description` (at most 1,024 UTF-8 bytes), `tags` (at most 32 unique identifier-like values of at most 64 bytes), and `license` (`id`, optionally a `notice` matching a listed source file). License declarations describe the publisher's material. FrilAdd does not infer a license or check rights.
- The manifest is UTF-8 JSON of up to 1 MiB and 1,000 items. Each item has at most 256 files, 256 registry dependencies, and 256 package requirements. Unknown fields and versions fail. Repeated sources must agree on the digest and size. Each item's targets must be portable, unique, and free of case and prefix collisions.

## Dependencies

- A registry edge names `providerId`, `itemId`, and exact `version`. A different provider also requires `manifestSha256`. An edge within the same provider omits `manifestSha256` because it selects from the enclosing snapshot. A manifest cannot contain its own digest literally. The lock records the resolved digest for either form.
- Cross-provider execution requires explicit registrations. The dependency resolver rejects ambiguous providers, unsatisfied versions or digests, missing items, cycles, and target conflicts before mutation. Providers are never discovered or registered from dependency declarations.
- Package requirements have npm `name`, `spec`, and `kind` (`runtime`, `peer`, or `dev`). A spec is an exact Semantic Version, or a single caret or tilde range around that version. Package aliases, URL or Git specs, tags, arbitrary scripts, and package-manager flags are outside this grammar. Package requirements are reported as metadata.

## Target mapping

An optional project-root `.friladd-targets.json` controls v2 file placement. Registry configuration and v1 installs are unaffected. Effective precedence is an explicit install root when the managed CLI supports that option, the item's `kindRoots` entry, `defaultRoot`, then the published target. Prefix the full declared target with the selected root. This preserves relative paths between models and textures, and between files in the same item. Use consistent roots for items whose declared paths intentionally refer to one another. Relative roots must satisfy the same target validation as files. The lock records the effective mapping. Editing mapping rules alone cannot move an installed item. [Target map JSON Schema](schemas/target-map-v1.schema.json)

```json
{"schemaVersion":1,"kindRoots":{"asset":"static"},"defaultRoot":"vendor"}
```

## Authoring fixtures

The existing v1 [component manifest](../registry/registry.json) remains compatible. These v2 examples are local authoring fixtures.

- A [component with an exact binary dependency](../registry/examples/v2/components/registry.json).
- An [asset with a generated GLB, relative texture, and license notice](../registry/examples/v2/assets/registry.json).
- A [composition with documented package requirements](../registry/examples/v2/compositions/registry.json).

```sh
friladd registry verify --registry ./registry/examples/v2/components/registry.json
friladd registry verify --registry ./registry/examples/v2/assets/registry.json
friladd registry verify --registry ./registry/examples/v2/compositions/registry.json
```
