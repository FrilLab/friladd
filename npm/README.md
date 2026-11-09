# FrilAdd

This package provides the Rust FrilAdd CLI for macOS ARM64, macOS x64, Linux x64, and Windows x64.

```sh
npx friladd --help
npx friladd list --registry ./registry/registry.json
npx friladd add envelope-reveal --registry ./registry/registry.json --dry-run
```

The npm package includes the native executable for each supported platform. Registry sources must be relative files and their SHA-256 digests are verified before installation. See the [full documentation](https://github.com/FrilLab/friladd#readme) for the manifest format and safety model.
