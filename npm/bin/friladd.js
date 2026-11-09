#!/usr/bin/env node
'use strict';

const fs = require('node:fs');
const path = require('node:path');
const { spawnSync } = require('node:child_process');

const targets = {
  'darwin-arm64': 'friladd-aarch64-apple-darwin',
  'darwin-x64': 'friladd-x86_64-apple-darwin',
  'linux-x64': 'friladd-x86_64-unknown-linux-gnu',
  'win32-x64': 'friladd-x86_64-pc-windows-msvc.exe',
};

const platform = `${process.platform}-${process.arch}`;
const binaryName = targets[platform];
if (!binaryName) {
  console.error(
    `FrilAdd does not provide a native binary for ${platform}. Supported targets: macOS ARM64, macOS x64, Linux x64, and Windows x64.`,
  );
  process.exit(1);
}

const binaryPath = path.join(__dirname, '..', 'native', binaryName);
if (!fs.existsSync(binaryPath)) {
  console.error(
    `FrilAdd's ${platform} executable is missing from this npm package. Reinstall friladd@${require('../package.json').version} or report the incomplete package at https://github.com/FrilLab/friladd/issues.`,
  );
  process.exit(1);
}

const result = spawnSync(binaryPath, process.argv.slice(2), {
  stdio: 'inherit',
  windowsHide: true,
});
if (result.error) {
  console.error(
    `Could not start FrilAdd's native executable: ${result.error.message}`,
  );
  process.exit(1);
}
process.exit(result.status === null ? 1 : result.status);
