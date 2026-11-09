# FrilAdd

This package provides the Rust FrilAdd CLI for macOS ARM64, macOS x64, Linux x64, and Windows x64.

```sh
npx friladd --help
npx friladd registry add team-ui https://example.com/registry.json --global --default
npx friladd add button
npx friladd add team-ui/button
```

The npm package includes the native executable for each supported platform. Registry sources must be relative files and their SHA-256 digests are verified before installation. See the [full documentation](https://github.com/FrilLab/friladd#readme) for registry setup, the manifest format, and the safety model.
