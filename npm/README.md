# FrilAdd

FrilAdd is a source distribution CLI with optional lifecycle management. From your project, use the v0.2.1 package to copy selected source files and their declared file dependencies:

```sh
npm install -g friladd
fril add frilcraft/envelope-reveal
```

Add `--managed` when you want lifecycle commands to track the installation:

```sh
fril add frilcraft/envelope-reveal --managed
fril status
```

Use a fresh project for the managed example if you already copied the same item. Existing destinations are protected. Default copies need no project configuration for official providers and leave no FrilAdd metadata in a clean project. Managed installs record ownership in `friladd.lock.json` and use `.friladd/` for transaction coordination. FrilAdd reports declared package requirements for you to install separately.

The package exposes `fril` and its `friladd` alias on macOS ARM64, macOS x64, Linux x64, and Windows x64. Use `npx friladd add frilcraft/envelope-reveal` for one-off installations. For a custom registry, use `fril registry add company-ui https://example.com/registry.json` or pass `--registry` explicitly.

Optional [community badges and brand assets](https://github.com/FrilLab/friladd/blob/main/docs/branding.md) are available for projects using FrilAdd and compatible registry authors.

See the [full documentation](https://github.com/FrilLab/friladd#readme) for safety, configuration, and compatibility with v0.2.0.
