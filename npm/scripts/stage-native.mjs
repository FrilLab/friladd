import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const packageDir = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..',
);
const repoRoot = path.resolve(packageDir, '..');
const [mode, inputDir] = process.argv.slice(2);
if (mode !== '--artifacts' || !inputDir) {
  console.error('Usage: node scripts/stage-native.mjs --artifacts <directory>');
  process.exit(2);
}
const mappings = [
  ['aarch64-apple-darwin', 'friladd-aarch64-apple-darwin'],
  ['x86_64-apple-darwin', 'friladd-x86_64-apple-darwin'],
  ['x86_64-unknown-linux-gnu', 'friladd-x86_64-unknown-linux-gnu'],
  ['x86_64-pc-windows-msvc', 'friladd-x86_64-pc-windows-msvc.exe'],
];
const artifactDir = path.resolve(inputDir);
const nativeDir = path.join(packageDir, 'native');
fs.mkdirSync(nativeDir, { recursive: true });
for (const [target, outputName] of mappings) {
  const inputName = target.endsWith('windows-msvc')
    ? `friladd-${target}.exe`
    : `friladd-${target}`;
  const source = path.join(artifactDir, inputName);
  if (!fs.existsSync(source)) {
    console.error(`Missing build artifact: ${source}`);
    process.exit(1);
  }
  const destination = path.join(nativeDir, outputName);
  fs.copyFileSync(source, destination);
  if (!outputName.endsWith('.exe')) fs.chmodSync(destination, 0o755);
  console.log(`Staged ${target}`);
}
fs.cpSync(path.join(repoRoot, 'registry'), path.join(packageDir, 'registry'), {
  recursive: true,
  force: true,
});
console.log('Staged local registry consumer fixture');
