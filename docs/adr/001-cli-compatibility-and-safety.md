# 001. CLI compatibility and safety rules for 0.2.0

Status: proposed for review in issue #10. This decision applies after the issue merges.

## Decision

- Keep the v1 manifest and configuration wire formats. Keep existing unambiguous commands and exit codes. Reject a positional alias combined with `--registry`, and reject `--global` combined with `--project`. These combinations previously either failed or silently ignored one input.
- A source argument on the command line is relative to the invocation directory. Project registrations store a path relative to the project configuration when possible. Validate it from its original absolute location so a different working directory has no effect. Preserve same-scope defaults, including a global default whose alias also exists in a project.
- Apply portable metadata reservations and case-folded prefix checks to targets. Use consistent ancestor spelling inside a plan. Allow targets shared by alternate v1 items because v1 installs one item per operation. The ownership foundation will validate collisions across managed items.
- Require same-origin HTTPS redirects, including the port. Resolve artifacts from the final manifest path. Keep query support for existing registry sources and redact sensitive URL components. Changing publication host now requires explicitly registering that host.
- Check the final local root, manifest and config path types, and every manifest-controlled source and destination component. Support OS-level aliases in explicitly chosen ancestors. The transaction foundation must account for application-level concurrency and interruption. See the security model for external process and filesystem limits.
- Start machine output at version 1 with an envelope separate from the CLI, registry, configuration, and future lock versions. Add output to the commands in their implementation issues using the shared contract.
- Declare Rust 1.88 as the minimum because the lockfile includes dependencies with that requirement and the code uses edition 2024 let chains. Run a dedicated MSRV test job, then run native tests, builds, and packed smoke on the supported platform matrix.
