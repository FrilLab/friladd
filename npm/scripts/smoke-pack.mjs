import assert from 'node:assert/strict';
import crypto from 'node:crypto';
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

function run(command, args, cwd, env = {}) {
  const result = spawnSync(command, args, {
    cwd,
    env: { ...process.env, ...env },
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
  const configDir = path.join(tempRoot, 'global-config');
  const invoke = (args, cwd = consumer) =>
    run('npm', ['exec', '--yes', '--', 'friladd', ...args], cwd, {
      FRILADD_CONFIG_DIR: configDir,
    });

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

  const makeRegistry = (directory, entries) => {
    fs.mkdirSync(directory, { recursive: true });
    const items = entries.map(({ name, content, target }) => {
      const source = `${name}.txt`;
      const bytes = Buffer.from(content);
      fs.writeFileSync(path.join(directory, source), bytes);
      return {
        name,
        description: `Smoke fixture for ${name}`,
        files: [{
          source,
          target,
          sha256: crypto.createHash('sha256').update(bytes).digest('hex'),
        }],
      };
    });
    fs.writeFileSync(
      path.join(directory, 'registry.json'),
      JSON.stringify({ schemaVersion: 1, name: path.basename(directory), items }),
    );
    return path.join(directory, 'registry.json');
  };
  const registryA = makeRegistry(path.join(consumer, 'registry-a'), [
    { name: 'button', content: 'button-from-a', target: 'button.txt' },
    { name: 'after-switch', content: 'default-from-a', target: 'after-switch.txt' },
    { name: 'choice', content: 'choice-from-global', target: 'choice.txt' },
  ]);
  const registryB = makeRegistry(path.join(consumer, 'registry-b'), [
    { name: 'envelope-reveal', content: 'envelope-from-b', target: 'envelope.txt' },
    { name: 'after-switch', content: 'default-from-b', target: 'after-switch.txt' },
    { name: 'choice', content: 'choice-from-project', target: 'choice.txt' },
  ]);
  for (const [alias, manifest, isDefault] of [
    ['registry-a', registryA, true],
    ['registry-b', registryB, false],
    ['shadow', registryA, false],
  ]) {
    const result = invoke([
      'registry', 'add', alias, manifest, '--global', ...(isDefault ? ['--default'] : []),
    ]);
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
  }
  const globalList = invoke(['registry', 'list', '--global']);
  assert.equal(globalList.status, 0, globalList.stderr);
  assert.match(globalList.stdout, /registry-a.*global.*default/s);

  const defaultProject = path.join(consumer, 'default-project');
  fs.mkdirSync(defaultProject);
  const defaultInstall = invoke(['add', 'button', '--project', defaultProject]);
  assert.equal(defaultInstall.status, 0, `${defaultInstall.stdout}\n${defaultInstall.stderr}`);
  assert.equal(fs.readFileSync(path.join(defaultProject, 'button.txt'), 'utf8'), 'button-from-a');

  const qualifiedProject = path.join(consumer, 'qualified-project');
  fs.mkdirSync(qualifiedProject);
  const qualifiedInstall = invoke(['add', 'registry-b/envelope-reveal', '--project', qualifiedProject]);
  assert.equal(qualifiedInstall.status, 0, `${qualifiedInstall.stdout}\n${qualifiedInstall.stderr}`);
  assert.equal(fs.readFileSync(path.join(qualifiedProject, 'envelope.txt'), 'utf8'), 'envelope-from-b');

  const shadowProject = path.join(consumer, 'shadow-project');
  fs.mkdirSync(shadowProject);
  const projectRegister = invoke(['registry', 'add', 'shadow', registryB, '--default'], shadowProject);
  assert.equal(projectRegister.status, 0, `${projectRegister.stdout}\n${projectRegister.stderr}`);
  const shadowInstall = invoke(['add', 'shadow/choice'], shadowProject);
  assert.equal(shadowInstall.status, 0, `${shadowInstall.stdout}\n${shadowInstall.stderr}`);
  assert.equal(fs.readFileSync(path.join(shadowProject, 'choice.txt'), 'utf8'), 'choice-from-project');
  const projectDefaultInstall = invoke(['add', 'after-switch'], shadowProject);
  assert.equal(projectDefaultInstall.status, 0, `${projectDefaultInstall.stdout}\n${projectDefaultInstall.stderr}`);
  assert.equal(fs.readFileSync(path.join(shadowProject, 'after-switch.txt'), 'utf8'), 'default-from-b');

  const switched = invoke(['registry', 'default', 'registry-b', '--global']);
  assert.equal(switched.status, 0, `${switched.stdout}\n${switched.stderr}`);
  const switchedProject = path.join(consumer, 'switched-project');
  fs.mkdirSync(switchedProject);
  const switchedInstall = invoke(['add', 'after-switch', '--project', switchedProject]);
  assert.equal(switchedInstall.status, 0, `${switchedInstall.stdout}\n${switchedInstall.stderr}`);
  assert.equal(fs.readFileSync(path.join(switchedProject, 'after-switch.txt'), 'utf8'), 'default-from-b');

  const removed = invoke(['registry', 'remove', 'registry-b', '--global']);
  assert.equal(removed.status, 0, `${removed.stdout}\n${removed.stderr}`);
  assert.match(removed.stdout, /cleared its default/);
  const removedAlias = invoke(['add', 'registry-b/envelope-reveal', '--project', path.join(consumer, 'removed-project')]);
  assert.notEqual(removedAlias.status, 0);
  assert.match(removedAlias.stderr, /not registered/);

  const legacyDir = path.join(consumer, 'legacy');
  fs.mkdirSync(legacyDir);
  const legacyBytes = Buffer.from('legacy path content');
  const legacyHash = crypto.createHash('sha256').update(legacyBytes).digest('hex');
  fs.writeFileSync(path.join(legacyDir, 'source.txt'), legacyBytes);
  fs.writeFileSync(path.join(legacyDir, 'registry.json'), JSON.stringify({
    schemaVersion: 1,
    name: 'legacy-local',
    items: [{ name: 'sample', description: 'Legacy explicit path', files: [{ source: 'source.txt', target: 'legacy.txt', sha256: legacyHash }] }],
  }));
  const legacyList = invoke(['list', '--registry', 'registry.json'], legacyDir);
  assert.equal(legacyList.status, 0, `${legacyList.stdout}\n${legacyList.stderr}`);
  const legacyProject = path.join(legacyDir, 'project');
  fs.mkdirSync(legacyProject);
  const legacyInstall = invoke(['add', 'sample', '--registry', 'registry.json', '--project', legacyProject], legacyDir);
  assert.equal(legacyInstall.status, 0, `${legacyInstall.stdout}\n${legacyInstall.stderr}`);
  assert.deepEqual(fs.readFileSync(path.join(legacyProject, 'legacy.txt')), legacyBytes);

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
