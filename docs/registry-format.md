# FrilAdd Registry Manifest, Schema 1

This document describes `registry.json`, the component manifest. Registry aliases and defaults live in the separate `.friladd.json` client configuration documented in the repository README; that configuration does not change this manifest schema.

A registry is a UTF-8 JSON file. `schemaVersion` must be the integer `1`. The root object contains exactly `schemaVersion`, `name`, and `items`. Each item contains exactly `name`, `description`, and `files`; each file contains exactly `source`, `target`, and `sha256`.

```json
{
  "schemaVersion": 1,
  "name": "local-dev",
  "items": [
    {
      "name": "envelope-reveal",
      "description": "A small interactive envelope sample for local development.",
      "files": [
        {
          "source": "fixtures/envelope-reveal/EnvelopeReveal.tsx",
          "target": "src/components/envelope-reveal/EnvelopeReveal.tsx",
          "sha256": "aef017b275d8420fe22161db1e0db57edc916c0bd9a812dba09ffd573b003398"
        },
        {
          "source": "fixtures/envelope-reveal/envelope-reveal.css",
          "target": "src/components/envelope-reveal/envelope-reveal.css",
          "sha256": "513830c41af1ed42684d795b06565699f3c9ac711bb21b9460e976337581493a"
        }
      ]
    }
  ]
}
```

## Rules

- Registry and item names are 1–128 ASCII characters, start with a lowercase letter or digit, and otherwise use lowercase letters, digits, `.`, `_`, or `-`.
- Item names are unique without regard to ASCII case. Descriptions contain at most 1,024 UTF-8 bytes and must contain a non-whitespace character. Each item declares 1–256 files; a manifest contains at most 1,000 items.
- File paths are nonempty relative slash-separated paths. Every segment uses ASCII letters, digits, `.`, `_`, or `-`, and may not be empty, `.` or `..`, end in a dot, or use a Windows reserved device name such as `NUL`. Backslashes, colons, URL syntax, absolute paths, and platform-specific drive/UNC forms are invalid. These rules keep the same manifest portable on macOS, Linux, and Windows.
- `source` is resolved from the directory containing a local manifest, or relative to the remote manifest URL. Remote manifest source paths stay on the manifest's HTTPS origin. Sources are regular files; local symlinks are rejected.
- `target` is resolved from the selected project root. Within each item, target paths must be unique case-insensitively, share consistent directory spelling, and have no file/directory prefix collisions. VCS and tool metadata segments are reserved at any depth. Symlink path components and non-regular existing destinations are rejected. Existing regular files are conflicts and are never overwritten.
- `sha256` is exactly 64 hexadecimal characters. FrilAdd verifies the bytes after acquisition and before any destination directories are created.
- Remote manifests use HTTPS only. A request follows at most four redirects and requires the original HTTPS origin throughout. Registry manifests are capped at 1 MiB; Registry v1 file content retains the 10 MiB cap. Registry v2 has a 100 MiB per-file cap and a 512 MiB operation staging cap.

The manifest format is a simple source-file list, not a package format. It does not declare package-manager dependencies, run scripts, or offer version resolution. SHA-256 protects the declared content from accidental or intermediate alteration, but it does not authenticate who published the manifest.

See the [Registry v1 JSON Schema](schemas/registry-v1.schema.json) and the [security model](security-model.md) for runtime checks beyond the structural schema.

Registry v2 is a separate opt-in protocol with provider IDs, exact item versions, and asset metadata. [Registry v2 authoring guide](registry-v2.md)
