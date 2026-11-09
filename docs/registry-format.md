# FrilAdd Registry Manifest, Schema 1

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
- Item names are unique without regard to ASCII case. Descriptions contain 1–1024 UTF-8 bytes after trimming. Each item declares 1–256 files; a manifest contains at most 1,000 items.
- File paths are nonempty relative slash-separated paths. Every segment uses ASCII letters, digits, `.`, `_`, or `-`, and may not be empty, `.` or `..`, end in a dot, or use a Windows reserved device name such as `NUL`. Backslashes, colons, URL syntax, absolute paths, and platform-specific drive/UNC forms are invalid. These rules keep the same manifest portable on macOS, Linux, and Windows.
- `source` is resolved from the directory containing a local manifest, or relative to the remote manifest URL. Remote manifest source paths stay on the manifest's HTTPS origin. Sources are regular files; local symlinks are rejected.
- `target` is resolved from the selected project root. Repeated target paths are rejected case-insensitively for portability. Symlink path components and non-regular existing destinations are rejected. Existing regular files are conflicts and are never overwritten.
- `sha256` is exactly 64 hexadecimal characters. FrilAdd verifies the bytes after acquisition and before any destination directories are created.
- Remote manifests use HTTPS only. HTTPS redirects are limited to five and cannot downgrade to HTTP. Registry manifests are capped at 1 MiB; file content is capped at 10 MiB.

The manifest format is a simple source-file list, not a package format. It does not declare package-manager dependencies, run scripts, or offer version resolution. SHA-256 protects the declared content from accidental or intermediate alteration, but it does not authenticate who published the manifest.
