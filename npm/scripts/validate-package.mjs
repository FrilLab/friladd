import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const packageDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const required = [
  ['friladd-aarch64-apple-darwin', false],
  ['friladd-x86_64-apple-darwin', false],
  ['friladd-x86_64-unknown-linux-gnu', false],
  ['friladd-x86_64-pc-windows-msvc.exe', true],
];
const missing = [];
for (const [name, windows] of required) {
  const filename = path.join(packageDir, 'native', name);
  try {
    const stat = fs.statSync(filename);
    if (!stat.isFile()) missing.push(name);
    if (!windows && (stat.mode & 0o111) === 0) missing.push(`${name} (not executable)`);
  } catch {
    missing.push(name);
  }
}
if (missing.length) {
  console.error(`npm package is missing required native binaries: ${missing.join(', ')}`);
  process.exit(1);
}
console.log(`Validated ${required.length} native executables in ${path.join(packageDir, 'native')}`);
