# FrilAdd

The npm package is named `friladd` and exposes both `fril` (primary) and `friladd` (backward-compatible alias) on macOS ARM64, macOS x64, Linux x64, and Windows x64. Both command shims use the same launcher and native executable. `fril --version` prints `fril VERSION`; `friladd --version` preserves `friladd VERSION`. Both commands share project configuration and lock state.

```sh
npm install --global friladd
fril --help
friladd --help
npx friladd --help
npm exec --package=friladd -- fril --help
pnpm dlx friladd --help
fril registry add team-ui https://example.com/registry.json --global --default
fril add button
fril add team-ui/button
```

The npm package includes one native executable for each supported platform (macOS ARM64/x64, Linux x64, and Windows x64), but does not bundle example components, test fixtures, or a default Registry. The package remains `friladd`; use `npx friladd`, not `npx fril`. After a global install, use `fril` or `friladd`; when invoking the package through npx, use `npx friladd`. Register a Registry over HTTPS or use a local manifest file. Component `source` paths in a Registry Manifest must be relative; their bytes are checked against the declared SHA-256 digests before installation. See the [full documentation](https://github.com/FrilLab/friladd#readme) for registry setup, the manifest format, and the safety model.
