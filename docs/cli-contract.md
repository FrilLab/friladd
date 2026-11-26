# FrilAdd CLI contract

## Implemented commands

The commands in this section are implemented in the source tree. Published npm releases have their own versioned behavior. Native usage is `friladd`. For the npm package, append the same arguments to `npm exec -- friladd`.

| Command | Scope, defaults and output |
| --- | --- |
| `friladd --help`, `friladd help [COMMAND …]`, `friladd COMMAND … --help` | Print usage on stdout. Registry subcommands accept help independently. Short form is `-h`. |
| `friladd --version` | Print `friladd VERSION` on stdout. Short form is `-V`. |
| `friladd registry add ALIAS SOURCE [--default] [--project DIRECTORY \| --global]` | Validate the manifest, then register the alias in the chosen scope and print confirmation. Duplicate aliases fail in that scope. `--default` sets the same scope's default. Content files are neither downloaded nor installed. |
| `friladd registry list [--project DIRECTORY \| --global]` | Print tab-separated alias, source, scope, and an optional `default` field. The default view prints project aliases followed by unshadowed global aliases, sorted by alias within each group. `--global` lists only global entries. Both views are offline. An empty view succeeds with empty stdout. |
| `friladd registry default ALIAS [--project DIRECTORY \| --global]` | Set an existing same-scope alias as default and print confirmation. Unknown aliases fail. No network request. |
| `friladd registry remove ALIAS [--project DIRECTORY \| --global]` | Remove an existing same-scope alias and clear the default if it pointed to that alias. Print confirmation. Unknown aliases fail. No network request and no installed files are removed. |
| `friladd registry verify [ALIAS] [--registry ALIAS_OR_SOURCE] [--project DIRECTORY]` | Resolve a registry and check all declared source bytes. Print a summary with registry name, item count and declared file count. Empty manifests succeed. Alias and `--registry` are mutually exclusive. |
| `friladd list [ALIAS] [--registry ALIAS_OR_SOURCE] [--project DIRECTORY]` | Read one manifest and print registry name and item names/descriptions in ascending item-name order. Alias and `--registry` are mutually exclusive. No artifact fetch. |
| `friladd add ITEM [--registry ALIAS_OR_SOURCE] [--project DIRECTORY] [--dry-run]` | Select a case-sensitive item name or `ALIAS/ITEM`, print the installation plan in manifest file order, and install verified bytes. Qualified names cannot be combined with `--registry`. Dry run validates and acquires bytes without writes, then prints a dry-run confirmation. A destination conflict produces a plan and an error before acquisition. Successful installation prints the installed paths. |

Exit status **0** means success, including help and version. Invalid usage and operational failures use **2**. Operational errors are written to stderr with an `Error:` prefix. A human-mode command may have printed progress or its plan before an error. CLI parsing errors include usage and go to stderr. Scripts should branch on exit status and use machine mode where available.

### Paths and precedence

- The project root defaults to the invocation directory. An explicit `--project` is relative to that directory unless absolute. FrilAdd reads exactly that root's `.friladd.json` and never walks upward to find one. Installation requires an existing root directory. Configuration commands can inspect a missing final project directory with an existing parent, and registration can create that directory.
- Registration, default selection, and removal default to project scope. `--global` selects the platform configuration directory, overridable by `FRILADD_CONFIG_DIR`. Global-only commands work without reading project configuration. `--global` and `--project` cannot be combined.
- With `--registry`, a bare alias-shaped value names an alias unless a filesystem entry with that exact name exists in the invocation directory. Use an explicit path such as `./registry.json` to remove ambiguity even when the file is absent. An explicit local path is relative to the invocation directory, independent of `--project`. An explicit path or URL bypasses configuration loading.
- A positional alias or qualified item resolves the project alias first, then global. Without a selector, use the project default, then the global default. Each default refers to an alias in its own file. A global default still uses its global source if the same alias exists in the project without a project default.
- Relative stored sources resolve from the directory containing their configuration. The CLI stores local project registrations relative to that directory when possible and stores local global registrations as absolute paths.
- Missing configuration means an empty scope. Corrupt or unsupported configuration and stale defaults fail when used. Configured selection reads both configurations. A missing default or missing item gives an error.

```sh
friladd --version
friladd registry verify --help
friladd registry add local ./registry/registry.json --project ./fixtures/consumer-project --default
friladd registry list --project ./fixtures/consumer-project
friladd registry default local --project ./fixtures/consumer-project
friladd registry verify local --project ./fixtures/consumer-project
friladd list local --project ./fixtures/consumer-project
friladd add local/envelope-reveal --project ./fixtures/consumer-project --dry-run
friladd add envelope-reveal --registry ./registry/registry.json --project ./fixtures/consumer-project
friladd registry remove local --project ./fixtures/consumer-project
friladd registry list --global
```

## Planned 0.2.0 commands and options

The examples below define the intended interface. Availability is gated by the corresponding implementation issues. Options shown in this section are planned.

| Example | Contract |
| --- | --- |
| `friladd add local/envelope-reveal --project ./app --dry-run --json` | Resolve with existing precedence, plan verified bytes and ownership together, refuse unmanaged or edited destinations, and treat an unchanged managed item as a no-op. Dry run leaves project state untouched. |
| `friladd installed --project ./app --json` | List managed item records offline, including provider, version or manifest digest, and file count. Empty state succeeds. |
| `friladd status local/envelope-reveal --project ./app --check --json` | Inspect local integrity offline. Omit the item to inspect all recorded items. Report unchanged, modified, missing, unsafe, unreadable, and explicitly queried unmanaged state. Drift succeeds by default. `--check` makes observed drift exit 2 with the report. |
| `friladd update local/envelope-reveal --project ./app --dry-run --json` | Plan a candidate from the installed provider with exact provenance. Refuse edited files, unresolved graph changes, and unmanaged conflicts. Refuse missing owned files and direct the user to restore. |
| `friladd remove local/envelope-reveal --project ./app --dry-run --json` | Plan removal of unchanged owned files offline. Refuse edits and required dependencies. Report already-missing files and allow safe ownership cleanup. |
| `friladd restore --project ./app --dry-run --json` | Plan recreation of missing managed files from the locked provenance. Omit the selector to restore all. Refuse modified files and unavailable or changed locked bytes. Unchanged files need no fetch. |
| `friladd search envelope --registry local --project ./app --json` | Read manifest metadata and match a nonempty query case-insensitively. Empty results succeed. `--all-registries` searches all effective configured aliases and conflicts with `--registry`. A registry failure fails the whole operation. |
| `friladd info local/envelope-reveal --project ./app --json` | Resolve an item with add precedence and report manifest metadata only. V1 has no invented version, kind, dependencies, or file size. |
| `friladd doctor --project ./app --json` | Report local diagnostics and drift without network or writes. Add `--online` to check configured registry reachability and manifest validity. Required failures exit 2 with the report. Warnings alone exit 0. |
| `friladd list local --json`, `friladd registry list --json`, `friladd registry verify local --json` | Use the same selection and scope as human mode. Report manifest items, configured aliases, or verified item and file counts, respectively. |

Planned commands default to the current project root and human output. Mutation commands apply their plan when `--dry-run` is absent, without requiring interaction. Managed commands resolve qualified selectors against lock ownership and report ambiguous unqualified names. Missing managed items are errors except when explicitly queried for unmanaged status. Dry runs have the validation and acquisition scope of their corresponding operation. Read-only commands do not run package managers, scripts, or repair steps. On-demand sync is outside 0.2.0.

### Machine output v1

Where supported, `--json` writes one UTF-8 JSON object with a trailing newline to stdout. Human diagnostics go to stderr. Machine output contains no terminal color, progress lines, timestamps, or URL credentials. Use stable key names, normalized project-relative file paths, and deterministic ordering. Sort registries by scope and alias, item results by provider identity and item identifier, and file rows by normalized target. A dependency execution sequence, where exposed, uses deterministic topological order.

```json
{"schemaVersion":1,"command":"registry verify","data":{"registry":"local-dev","itemCount":2,"fileCount":4}}
```

`schemaVersion` and `command` are required. The command string uses canonical subcommand names separated by one space. `data` is a command-specific object. Use `items` arrays for list, search and installed, `registries` for registry list, an `item` object for info, and `items` plus integrity summaries for status. Use `checks` for doctor, with stable codes and states `ok`, `warn`, `fail` and `skip`. Mutation data includes `dryRun` and a plan of per-file actions with ownership effects. Individual implementation issues specify fields and assert the actual payloads in subprocess tests. For a failure before a report can be generated, stdout is empty. Status with `--check` and doctor can emit a complete report and exit 2. Exit status remains authoritative.

## Version and platform policy

- The CLI and npm package use the same release version and keep compatible command behavior across 0.2.x. CLI help and documentation advertise only shipped subcommands.
- Registry v1 and [configuration v1](schemas/config-v1.schema.json) use independent `schemaVersion` numbers. Readers reject unknown schema versions and unknown structural fields. An older CLI needs no implicit migration. A new registry version must coexist with v1. See the [registry schema](schemas/registry-v1.schema.json).
- The project lock has its own version and migration policy, specified in #11. A CLI or registry upgrade alone must not rewrite lock identity. Machine output also has an independent schema version. Compatible additions are optional fields. Incompatible changes require a new version.
- The CI platform matrix tests Linux x64 (GNU, Ubuntu 22.04), macOS arm64 and x64, and Windows x64 (MSVC). Node.js 22 and its bundled npm are CI-tested for the npm wrapper. The wrapper declares Node.js 18 or newer. The native CLI uses no Node runtime. Stable Rust runs the primary matrix. Rust 1.88 is the minimum supported version and has a locked test job.
