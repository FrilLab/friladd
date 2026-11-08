import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const npmDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const repoRoot = path.resolve(npmDir, '..');
const targets = {
  'darwin-arm64': ['friladd-aarch64-apple-darwin', 'friladd'],
  'darwin-x64': ['friladd-x86_64-apple-darwin', 'friladd'],
  'linux-x64': ['friladd-x86_64-unknown-linux-gnu', 'friladd'],
  'win32-x64': ['friladd-x86_64-pc-windows-msvc.exe', 'friladd.exe'],
};
const mapping = targets[`${process.platform}-${process.arch}`];
if (!mapping) {
  console.error(`Package smoke tests require one supported platform; got ${process.platform}-${process.arch}`);
  process.exit(1);
}
const [packageBinary, buildBinary] = mapping;
const nativeDir = path.join(npmDir, 'native');
const destination = path.join(nativeDir, packageBinary);
const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'friladd-package-build-'));
const backup = path.join(tempRoot, 'previous-native');
const hadPrevious = fs.existsSync(destination);
if (hadPrevious) fs.copyFileSync(destination, backup);

function run(command, args, cwd) {
  const result = spawnSync(command, args, { cwd, encoding: 'utf8', stdio: 'inherit', shell: process.platform === 'win32' });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${command} exited with status ${result.status}`);
}

try {
  run('cargo', ['build', '--locked', '--release', '--manifest-path', path.join(repoRoot, 'Cargo.toml')], repoRoot);
  fs.mkdirSync(nativeDir, { recursive: true });
  fs.copyFileSync(path.join(repoRoot, 'target', 'release', buildBinary), destination);
  if (!packageBinary.endsWith('.exe')) fs.chmodSync(destination, 0o755);
  fs.cpSync(path.join(repoRoot, 'registry'), path.join(npmDir, 'registry'), { recursive: true, force: true });
  const packedDir = path.join(tempRoot, 'packed');
  fs.mkdirSync(packedDir);
  run('npm', ['pack', npmDir, '--pack-destination', packedDir], repoRoot);
  const packageInfo = JSON.parse(fs.readFileSync(path.join(npmDir, 'package.json'), 'utf8'));
  const tarball = path.join(packedDir, `${packageInfo.name}-${packageInfo.version}.tgz`);
  run(process.execPath, [path.join(npmDir, 'scripts', 'smoke-pack.mjs'), tarball], repoRoot);
} finally {
  if (hadPrevious) fs.copyFileSync(backup, destination);
  else fs.rmSync(destination, { force: true });
  fs.rmSync(path.join(npmDir, 'registry'), { recursive: true, force: true });
  fs.rmSync(tempRoot, { recursive: true, force: true });
}
