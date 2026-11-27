# Provider validation in CI

Run FrilAdd's verifier after generating and reviewing a registry manifest and its declared files. The command validates the manifest, downloads each distinct declared source through FrilAdd's bounded acquisition path, verifies SHA-256 and Registry v2 sizes, and exits nonzero on any mismatch. It writes no files into the provider or consumer project. It does not render assets, execute provider code, or install npm dependencies.

Use a pinned FrilAdd version in provider CI. Pass the manifest path explicitly so the job does not depend on user configuration or defaults.

## Native CLI

Check out FrilAdd at the version being qualified, install its binary in the runner's temporary directory, then validate the provider's manifest:

```sh
FRILADD_SOURCE="${RUNNER_TEMP}/friladd"
FRILADD_ROOT="${RUNNER_TEMP}/friladd-native"
cargo install --path "${FRILADD_SOURCE}" --locked --root "${FRILADD_ROOT}"
"${FRILADD_ROOT}/bin/fril" registry verify \
  --registry "${GITHUB_WORKSPACE}/registry.json" \
  --json
```

On Windows runners, invoke the installed `bin/fril.exe` path. The Cargo install also exposes `bin/friladd` (or `bin/friladd.exe`) as a backward-compatible alias. Keep the provider checkout as the current workflow workspace; only the CLI checkout and binary are placed under the runner's temporary directory.

## Packed npm CLI

Set `FRILADD_VERSION` to the exact published FrilAdd version that the provider release supports. `npm exec` downloads the package tarball into npm's cache and runs the packaged native binary:

```sh
npm exec --yes --package="friladd@${FRILADD_VERSION}" -- \
  fril registry verify \
  --registry "${GITHUB_WORKSPACE}/registry.json" \
  --json
```

Use the same immutable registry revision and expected CLI version in repeated release jobs. Do not substitute a mocked verifier or fixture-only command for this provider manifest check. Keep provider-specific build/render qualification as a separate job because `registry verify` checks transport integrity, not whether a component compiles or a model renders.

## Interpreting results

Exit status 0 means the manifest and every declared file passed integrity verification. Exit status 2 means verification failed; inspect stderr for the failing item/source. With `--json`, success writes the versioned `registry verify` envelope to stdout with registry name, manifest digest, item count, and declared file count. Failures before a complete verification report leave stdout empty. Credentials, query strings, and fragments are not printed.
