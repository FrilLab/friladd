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
const testRegistry = path.join(repoRoot, 'registry', 'registry.json');
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
  assert.equal(
    fs.existsSync(path.join(installedPackage, 'registry')),
    false,
    'The published npm package must not contain the repository test registry',
  );
  const packedPackage = JSON.parse(
    fs.readFileSync(path.join(installedPackage, 'package.json'), 'utf8'),
  );
  assert.deepEqual(packedPackage.bin, {
    fril: 'bin/fril.js',
    friladd: 'bin/friladd.js',
  });
  const nativeExecutables = fs.readdirSync(path.join(installedPackage, 'native')).sort();
  const supportedExecutables = [
    'friladd-aarch64-apple-darwin',
    'friladd-x86_64-apple-darwin',
    'friladd-x86_64-pc-windows-msvc.exe',
    'friladd-x86_64-unknown-linux-gnu',
  ];
  assert.ok(nativeExecutables.length >= 1 && nativeExecutables.length <= supportedExecutables.length);
  assert.ok(nativeExecutables.every((name) => supportedExecutables.includes(name)));
  assert.ok(nativeExecutables.every((name) => !name.startsWith('fril-')));
  if (nativeExecutables.length === supportedExecutables.length) {
    assert.deepEqual(nativeExecutables, supportedExecutables);
  }
  const configDir = path.join(tempRoot, 'global-config');
  const invoke = (args, cwd = consumer, command = 'friladd') =>
    run('npm', ['exec', '--yes', '--', command, ...args], cwd, {
      FRILADD_CONFIG_DIR: configDir,
    });

  const packageVersion = packedPackage.version;
  const version = invoke(['--version'], consumer, 'fril');
  const legacyVersion = invoke(['--version'], consumer, 'friladd');
  assert.equal(version.status, 0, version.stderr);
  assert.equal(legacyVersion.status, 0, legacyVersion.stderr);
  assert.equal(version.stdout.trim(), `fril ${packageVersion}`);
  assert.equal(legacyVersion.stdout.trim(), `friladd ${packageVersion}`);

  const help = invoke(['--help'], consumer, 'fril');
  const legacyHelp = invoke(['--help'], consumer, 'friladd');
  assert.equal(help.status, 0, help.stderr);
  assert.equal(legacyHelp.status, 0, legacyHelp.stderr);
  assert.match(help.stdout, /Usage: fril <COMMAND>/);
  assert.match(legacyHelp.stdout, /Usage: friladd <COMMAND>/);
  assert.match(help.stdout, /Install selected source files/);
  for (const name of ['fril', 'friladd']) {
    const unknown = invoke(['not-a-command'], consumer, name);
    assert.equal(unknown.status, 2, `${name}: ${unknown.stderr}`);
    assert.equal(unknown.stdout, '');
    assert.match(unknown.stderr, /unrecognized subcommand/i);
  }

  const commands = [
    ['registry', 'add'], ['registry', 'list'], ['registry', 'default'],
    ['registry', 'remove'], ['registry', 'verify'], ['list'], ['search'],
    ['info'], ['add'], ['installed'], ['status'], ['update'], ['remove'],
    ['restore'], ['doctor'],
  ];
  for (const commandArgs of commands) {
    for (const name of ['fril', 'friladd']) {
      const result = invoke([...commandArgs, '--help'], consumer, name);
      assert.equal(result.status, 0, `${name} ${commandArgs.join(' ')}: ${result.stderr}`);
      assert.match(result.stdout, new RegExp(`Usage: ${name}\\b`));
    }
  }

  const npxHelp = run('npx', ['friladd', '--help'], consumer, {
    FRILADD_CONFIG_DIR: configDir,
  });
  assert.equal(npxHelp.status, 0, `${npxHelp.stdout}\n${npxHelp.stderr}`);
  assert.match(npxHelp.stdout, /Usage: friladd <COMMAND>/);
  const npmExecHelp = run(
    'npm', ['exec', '--yes', '--package=friladd', '--', 'fril', '--help'],
    consumer,
    { FRILADD_CONFIG_DIR: configDir },
  );
  assert.equal(npmExecHelp.status, 0, `${npmExecHelp.stdout}\n${npmExecHelp.stderr}`);
  assert.match(npmExecHelp.stdout, /Usage: fril <COMMAND>/);
  const pnpmTarball = path.join(os.tmpdir(), `f-${process.pid}.tgz`);
  fs.copyFileSync(path.resolve(tarball), pnpmTarball);
  try {
    const pnpmDlxHelp = run(
      'pnpm', ['dlx', `--package=${pnpmTarball}`, 'fril', '--help'],
      consumer,
      { FRILADD_CONFIG_DIR: configDir },
    );
    assert.equal(pnpmDlxHelp.status, 0, `${pnpmDlxHelp.stdout}\n${pnpmDlxHelp.stderr}`);
    assert.match(pnpmDlxHelp.stdout, /Usage: fril <COMMAND>/);
  } finally {
    fs.rmSync(pnpmTarball, { force: true });
  }

  const globalPrefix = path.join(tempRoot, 'global-install');
  const globalInstall = run(
    'npm', ['install', '--global', '--prefix', globalPrefix, '--no-audit', '--no-fund', '--ignore-scripts', path.resolve(tarball)],
    consumer,
  );
  assert.equal(globalInstall.status, 0, `${globalInstall.stdout}\n${globalInstall.stderr}`);
  const globalBin = path.join(globalPrefix, process.platform === 'win32' ? '' : 'bin');
  const globalFril = path.join(globalBin, process.platform === 'win32' ? 'fril.cmd' : 'fril');
  const globalFriladd = path.join(globalBin, process.platform === 'win32' ? 'friladd.cmd' : 'friladd');
  for (const [executable, expected] of [[globalFril, `fril ${packageVersion}`], [globalFriladd, `friladd ${packageVersion}`]]) {
    const result = run(executable, ['--version'], consumer, { FRILADD_CONFIG_DIR: configDir });
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    assert.equal(result.stdout.trim(), expected);
  }

  const bad = invoke([
    'add',
    'does-not-exist',
    '--registry',
    testRegistry,
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
  const add = invoke([
    'add',
    'envelope-reveal',
    '--registry',
    testRegistry,
    '--project',
    project,
  ], consumer, 'fril');
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
          repoRoot,
          'registry',
          'fixtures',
          'envelope-reveal',
          source,
        ),
      ),
      fs.readFileSync(path.join(project, target)),
    );
  }
  const installedByLegacy = invoke(['installed', '--project', project, '--json'], consumer, 'friladd');
  assert.equal(installedByLegacy.status, 0, installedByLegacy.stderr);
  assert.equal(JSON.parse(installedByLegacy.stdout).data.items.length, 1);
  const statusByLegacy = invoke(['status', '--project', project, '--json'], consumer, 'friladd');
  assert.equal(statusByLegacy.status, 0, statusByLegacy.stderr);
  assert.equal(JSON.parse(statusByLegacy.stdout).data.counts.unchanged, 2);
  const removeByLegacy = invoke(['remove', 'envelope-reveal', '--project', project], consumer, 'friladd');
  assert.equal(removeByLegacy.status, 0, `${removeByLegacy.stdout}\n${removeByLegacy.stderr}`);
  const addByLegacy = invoke([
    'add', 'envelope-reveal', '--registry', testRegistry, '--project', project,
  ], consumer, 'friladd');
  assert.equal(addByLegacy.status, 0, `${addByLegacy.stdout}\n${addByLegacy.stderr}`);
  const firstManagedFile = path.join(project, 'src/components/envelope-reveal/EnvelopeReveal.tsx');
  fs.unlinkSync(firstManagedFile);
  const restoreByPrimary = invoke(['restore', 'envelope-reveal', '--project', project], consumer, 'fril');
  assert.equal(restoreByPrimary.status, 0, `${restoreByPrimary.stdout}\n${restoreByPrimary.stderr}`);
  assert.equal(fs.readFileSync(firstManagedFile, 'utf8'), fs.readFileSync(path.join(repoRoot, 'registry/fixtures/envelope-reveal/EnvelopeReveal.tsx'), 'utf8'));
  const finalStatus = invoke(['status', '--project', project, '--json'], consumer, 'friladd');
  assert.equal(finalStatus.status, 0, finalStatus.stderr);
  assert.equal(JSON.parse(finalStatus.stdout).data.counts.unchanged, 2);
  const removeByPrimary = invoke(['remove', 'envelope-reveal', '--project', project], consumer, 'fril');
  assert.equal(removeByPrimary.status, 0, `${removeByPrimary.stdout}\n${removeByPrimary.stderr}`);

  const makeV2Registry = (directory, version, content) => {
    fs.mkdirSync(directory, { recursive: true });
    const bytes = Buffer.from(content);
    fs.writeFileSync(path.join(directory, 'source.txt'), bytes);
    const manifest = {
      schemaVersion: 2,
      id: 'example.alias-compatibility',
      name: 'alias-compatibility',
      items: [{
        id: 'sample',
        version,
        kind: 'component',
        description: `Alias update fixture ${version}`,
        files: [{
          source: 'source.txt',
          target: 'alias-update.txt',
          sha256: crypto.createHash('sha256').update(bytes).digest('hex'),
          size: bytes.length,
        }],
      }],
    };
    const filename = path.join(directory, 'registry.json');
    fs.writeFileSync(filename, JSON.stringify(manifest));
    return filename;
  };
  const v2RegistryOne = makeV2Registry(path.join(consumer, 'v2-one'), '1.0.0', 'one');
  const v2RegistryTwo = makeV2Registry(path.join(consumer, 'v2-two'), '2.0.0', 'two');
  const v2Project = path.join(consumer, 'v2-alias-project');
  fs.mkdirSync(v2Project);
  const v2Add = invoke(['add', 'sample', '--registry', v2RegistryOne, '--project', v2Project], consumer, 'fril');
  assert.equal(v2Add.status, 0, `${v2Add.stdout}\n${v2Add.stderr}`);
  const v2Update = invoke(['update', 'sample', '--registry', v2RegistryTwo, '--project', v2Project], consumer, 'friladd');
  assert.equal(v2Update.status, 0, `${v2Update.stdout}\n${v2Update.stderr}`);
  assert.equal(fs.readFileSync(path.join(v2Project, 'alias-update.txt'), 'utf8'), 'two');
  const v2Status = invoke(['status', 'sample', '--project', v2Project, '--json'], consumer, 'fril');
  assert.equal(v2Status.status, 0, v2Status.stderr);
  assert.equal(JSON.parse(v2Status.stdout).data.counts.unchanged, 1);
  const v2Remove = invoke(['remove', 'sample', '--project', v2Project], consumer, 'friladd');
  assert.equal(v2Remove.status, 0, `${v2Remove.stdout}\n${v2Remove.stderr}`);

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
  const binaryBytes = Buffer.from([0x67, 0x6c, 0x54, 0x46, 0x00, 0xff, 0x80, 0x0d, 0x0a]);
  const binaryRegistry = makeRegistry(path.join(tempRoot, 'binary-provider'), [
    { name: 'binary', content: binaryBytes, target: 'assets/binary.glb' },
  ]);
  const binaryProject = path.join(tempRoot, 'binary-consumer');
  fs.mkdirSync(binaryProject);
  const verifyBinary = () =>
    invoke(['registry', 'verify', '--registry', binaryRegistry, '--project', binaryProject]);
  const verified = verifyBinary();
  assert.equal(verified.status, 0, verified.stderr);
  assert.match(verified.stdout, /Verified registry binary-provider: 1 item\(s\), 1 file\(s\)\./);
  assert.deepEqual(fs.readdirSync(binaryProject), []);

  const binaryAdd = invoke(['add', 'binary', '--registry', binaryRegistry, '--project', binaryProject]);
  assert.equal(binaryAdd.status, 0, binaryAdd.stderr);
  const binaryTarget = path.join(binaryProject, 'assets', 'binary.glb');
  assert.deepEqual(fs.readFileSync(binaryTarget), binaryBytes);

  const binarySource = path.join(path.dirname(binaryRegistry), 'binary.txt');
  fs.writeFileSync(binarySource, Buffer.alloc(binaryBytes.length));
  const changed = verifyBinary();
  assert.equal(changed.status, 2, changed.stderr);
  assert.match(changed.stderr, /SHA-256 mismatch/);
  fs.unlinkSync(binarySource);
  const missing = verifyBinary();
  assert.equal(missing.status, 2, missing.stderr);
  assert.match(missing.stderr, /binary\.txt/);
  assert.deepEqual(fs.readFileSync(binaryTarget), binaryBytes);

  for (const [alias, manifest, isDefault] of [
    ['registry-a', registryA, true],
    ['registry-b', registryB, false],
    ['shadow', registryA, false],
  ]) {
    const result = invoke([
      'registry', 'add', alias, manifest, '--global', ...(isDefault ? ['--default'] : []),
    ], consumer, alias === 'registry-a' ? 'fril' : 'friladd');
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
  }
  const globalList = invoke(['registry', 'list', '--global']);
  assert.equal(globalList.status, 0, globalList.stderr);
  assert.match(globalList.stdout, /registry-a.*global.*default/s);
  const registryJsonPrimary = invoke(['registry', 'list', '--global', '--json'], consumer, 'fril');
  const registryJsonLegacy = invoke(['registry', 'list', '--global', '--json'], consumer, 'friladd');
  assert.equal(registryJsonPrimary.status, 0, registryJsonPrimary.stderr);
  assert.equal(registryJsonLegacy.status, 0, registryJsonLegacy.stderr);
  assert.equal(registryJsonPrimary.stdout, registryJsonLegacy.stdout);

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
    const remoteVerify = invoke([
      'registry', 'verify', '--registry', remoteRegistry, '--project', remoteProject,
    ]);
    assert.equal(remoteVerify.status, 0, remoteVerify.stderr);
    assert.match(remoteVerify.stdout, /Verified registry /);
    assert.deepEqual(fs.readdirSync(remoteProject), []);
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
            repoRoot,
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
  const remoteResult = remoteRegistry ? ', HTTPS verify and install' : '';
  console.log(
    `npm consumer smoke test passed: help/version, argument and exit-code forwarding, local install, binary verification and byte identity${remoteResult}`,
  );
} finally {
  fs.rmSync(tempRoot, { recursive: true, force: true });
}
