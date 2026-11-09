import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const [tarball] = process.argv.slice(2);
if (!tarball) {
  console.error('Usage: node scripts/smoke-pack.mjs <npm-tarball>');
  process.exit(2);
}
const repoRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../..',
);
const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'friladd-npm-smoke-'));
const consumer = path.join(tempRoot, 'consumer');
fs.mkdirSync(consumer);
fs.writeFileSync(
  path.join(consumer, 'package.json'),
  JSON.stringify({
    name: 'friladd-smoke-consumer',
    private: true,
    version: '0.0.0',
  }),
);

function run(command, args, cwd) {
  const result = spawnSync(command, args, {
    cwd,
    encoding: 'utf8',
    stdio: 'pipe',
    shell: process.platform === 'win32',
  });
  if (result.error) throw result.error;
  return result;
}

try {
  const install = run(
    'npm',
    [
      'install',
      '--no-audit',
      '--no-fund',
      '--ignore-scripts',
      path.resolve(tarball),
    ],
    consumer,
  );
  assert.equal(install.status, 0, `${install.stdout}\n${install.stderr}`);
  const installedPackage = path.join(consumer, 'node_modules', 'friladd');
  const invoke = (args) =>
    run('npm', ['exec', '--yes', '--', 'friladd', ...args], consumer);

  const version = invoke(['--version']);
  assert.equal(version.status, 0, version.stderr);
  const packageVersion = JSON.parse(
    fs.readFileSync(path.join(installedPackage, 'package.json'), 'utf8'),
  ).version;
  assert.equal(version.stdout.trim(), `friladd ${packageVersion}`);

  const help = invoke(['--help']);
  assert.equal(help.status, 0, help.stderr);
  assert.match(help.stdout, /Install selected source files/);

  const bad = invoke([
    'add',
    'does-not-exist',
    '--registry',
    path.join(installedPackage, 'registry', 'registry.json'),
  ]);
  assert.notEqual(
    bad.status,
    0,
    'Rust CLI exit status must be forwarded by the npm wrapper',
  );
  assert.match(bad.stderr, /Component not found/);

  const project = path.join(consumer, 'project');
  fs.mkdirSync(project);
  fs.copyFileSync(
    path.join(repoRoot, 'fixtures', 'consumer-project', 'package.json'),
    path.join(project, 'package.json'),
  );
  const registry = path.join(installedPackage, 'registry', 'registry.json');
  const add = invoke([
    'add',
    'envelope-reveal',
    '--registry',
    registry,
    '--project',
    project,
  ]);
  assert.equal(add.status, 0, `${add.stdout}\n${add.stderr}`);
  for (const [source, target] of [
    ['EnvelopeReveal.tsx', 'src/components/envelope-reveal/EnvelopeReveal.tsx'],
    [
      'envelope-reveal.css',
      'src/components/envelope-reveal/envelope-reveal.css',
    ],
  ]) {
    assert.deepEqual(
      fs.readFileSync(
        path.join(
          installedPackage,
          'registry',
          'fixtures',
          'envelope-reveal',
          source,
        ),
      ),
      fs.readFileSync(path.join(project, target)),
    );
  }
  const remoteRegistry = process.env.FRILADD_REMOTE_REGISTRY_URL;
  if (remoteRegistry) {
    const remoteProject = path.join(consumer, 'remote-project');
    fs.mkdirSync(remoteProject);
    const remoteAdd = invoke([
      'add',
      'envelope-reveal',
      '--registry',
      remoteRegistry,
      '--project',
      remoteProject,
    ]);
    assert.equal(
      remoteAdd.status,
      0,
      `${remoteAdd.stdout}\n${remoteAdd.stderr}`,
    );
    for (const [source, target] of [
      [
        'EnvelopeReveal.tsx',
        'src/components/envelope-reveal/EnvelopeReveal.tsx',
      ],
      [
        'envelope-reveal.css',
        'src/components/envelope-reveal/envelope-reveal.css',
      ],
    ]) {
      assert.deepEqual(
        fs.readFileSync(
          path.join(
            installedPackage,
            'registry',
            'fixtures',
            'envelope-reveal',
            source,
          ),
        ),
        fs.readFileSync(path.join(remoteProject, target)),
      );
    }
  }
  const remoteResult = remoteRegistry ? ' and two-file HTTPS install' : '';
  console.log(
    `npm consumer smoke test passed: help/version, argument and exit-code forwarding, and two-file local install${remoteResult}`,
  );
} finally {
  fs.rmSync(tempRoot, { recursive: true, force: true });
}
