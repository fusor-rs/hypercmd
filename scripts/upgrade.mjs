import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { once } from 'node:events';
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, realpathSync,
  rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { basename, dirname, join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const root = resolve(import.meta.dirname, '..');
const work = realpathSync(mkdtempSync(join(tmpdir(), 'hypercmd-upgrade-')));
const installer = join(root, 'install.sh');
const binary = join(root, 'target/debug/hypercmd');
const previousVersion = '0.0.0';
const installed = join(work, 'custom location/bin/hypercmd');
const environment = { ...process.env, XDG_CACHE_HOME: join(work, 'cache'),
  HYPERCMD_INSTALL: join(work, 'default'),
  HYPERCMD_DOWNLOAD_BASE: pathToFileURL(join(work, 'releases')).href };
delete environment.CI;
delete environment.HYPERCMD_BIN;
let release;
let requests = 0;
let responseStatus = 200;
const server = createServer((request, response) => {
  requests += 1;
  response.writeHead(responseStatus, { 'Content-Type': 'application/json' });
  response.end(JSON.stringify(release));
});

function run(command, args, env = environment, expectedStatus = 0) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, { cwd: root, env, stdio: ['ignore', 'pipe', 'pipe'] });
    let stdout = '';
    let stderr = '';
    child.stdout.on('data', bytes => { stdout += bytes; });
    child.stderr.on('data', bytes => { stderr += bytes; });
    child.on('error', reject);
    child.on('close', status => {
      try {
        assert.equal(status, expectedStatus, `${command} ${args.join(' ')}\n${stdout}\n${stderr}`);
        resolve({ stdout, stderr });
      } catch (error) { reject(error); }
    });
  });
}

async function previousBinary(packageMetadata) {
  const dependencies = packageMetadata.dependencies
    .map(dependency => {
      const fields = { version: dependency.req, package: dependency.name,
        'default-features': dependency.uses_default_features, features: dependency.features };
      if (dependency.path) fields.path = dependency.path;
      const entries = Object.entries(fields)
        .map(([name, value]) => `${name} = ${JSON.stringify(value)}`);
      return `${dependency.rename ?? dependency.name} = { ${entries.join(', ')} }`;
    }).join('\n');
  const manifest = join(work, 'Cargo.toml');
  writeFileSync(manifest, `[package]
name = "${packageMetadata.name}"
version = "${previousVersion}"
edition = "${packageMetadata.edition}"
repository = "${packageMetadata.repository}"
[workspace]
[[bin]]
name = "hypercmd-previous"
path = ${JSON.stringify(join(root, 'crates/hypercmd-cli/src/main.rs'))}
[[bin]]
name = "hypercmd-terminal"
path = ${JSON.stringify(join(root, 'tests/support/terminal-command.rs'))}
[dependencies]
${dependencies}
`);
  await run('cargo', ['build', '--offline', '--manifest-path', manifest,
    '--target-dir', join(root, 'target')]);
  return join(root, 'target/debug/hypercmd-previous');
}

async function archive(version) {
  const name = (await run('sh', [installer, '--archive', version])).stdout.trim();
  const directory = join(work, 'releases', `v${version}`);
  const member = name.slice(0, -'.tar.gz'.length);
  mkdirSync(join(directory, member), { recursive: true });
  copyFileSync(binary, join(directory, member, 'hypercmd'));
  const path = join(directory, name);
  await run('tar', ['-czf', path, '-C', directory, member]);
  const checksum = createHash('sha256').update(readFileSync(path)).digest('hex');
  writeFileSync(`${path}.sha256`, `${checksum}  ${name}\n`);
  return { path, release: { tag_name: `v${version}`, draft: false, prerelease: false,
    assets: [basename(installer), name, `${name}.sha256`].map(name => ({ name })) } };
}

try {
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  environment.HYPERCMD_RELEASE_URL = `http://127.0.0.1:${server.address().port}/latest`;
  const metadata = JSON.parse((await run('cargo',
    ['metadata', '--no-deps', '--format-version=1'])).stdout);
  const packageMetadata = metadata.packages
    .find(packageMetadata => packageMetadata.name === 'hypercmd-cli');
  const previous = await previousBinary(packageMetadata);
  const current = await archive(packageMetadata.version);
  const bootstrap = join(work, 'bootstrap');
  await run('sh', [installer, packageMetadata.version], {
    ...environment, HYPERCMD_INSTALL: bootstrap,
  });
  assert.deepEqual(readFileSync(join(bootstrap, 'bin/hypercmd')), readFileSync(binary));
  release = current.release;
  mkdirSync(dirname(installed), { recursive: true });
  mkdirSync(join(environment.HYPERCMD_INSTALL, 'bin'), { recursive: true });
  const otherInstallation = join(environment.HYPERCMD_INSTALL, 'bin/hypercmd');
  writeFileSync(otherInstallation, 'another installation');
  copyFileSync(previous, installed);
  const link = join(work, 'hypercmd');
  symlinkSync(installed, link);
  await run(link, ['upgrade']);
  assert.deepEqual(readFileSync(installed), readFileSync(binary));
  assert.equal(readFileSync(otherInstallation, 'utf8'), 'another installation');
  assert.deepEqual(readdirSync(dirname(installed)), ['hypercmd']);

  for (const version of [packageMetadata.version, previousVersion]) {
    release = { ...current.release, tag_name: `v${version}`, assets: [] };
    const output = await run(installed, ['upgrade']);
    assert.equal(output.stdout, `hypercmd ${packageMetadata.version} is already up to date.\n`);
    assert.deepEqual(readFileSync(installed), readFileSync(binary));
  }

  const wrongVersion = await archive('99.0.0');
  const checksum = readFileSync(`${current.path}.sha256`);
  const failures = [
    { release: { ...current.release, assets: [] }, message: 'still being published' },
    { release: { ...current.release, prerelease: true }, message: 'not stable' },
    { release: wrongVersion.release, message: 'wrong version' },
    { release: current.release, checksum: 'invalid\n', message: 'checksum mismatch' },
  ];
  for (const failure of failures) {
    copyFileSync(previous, installed);
    release = failure.release;
    writeFileSync(`${current.path}.sha256`, failure.checksum ?? checksum);
    const output = await run(installed, ['upgrade'], environment, 1);
    assert(output.stderr.includes(failure.message), output.stderr);
    assert.deepEqual(readFileSync(installed), readFileSync(previous));
    assert.deepEqual(readdirSync(dirname(installed)), ['hypercmd']);
  }

  const cargoRoot = join(work, 'cargo-installation');
  await run('cargo', ['install', '--path', work, '--bin', 'hypercmd-previous',
    '--root', cargoRoot, '--offline', '--debug', '--target-dir', join(root, 'target')]);
  const cargoBinary = join(cargoRoot, 'bin/hypercmd-previous');
  release = wrongVersion.release;
  const cargoFailure = await run(cargoBinary, ['upgrade'], {
    ...environment, CARGO_HOME: join(work, 'empty-cargo-home'), CARGO_NET_OFFLINE: 'true',
  }, 1);
  assert.match(cargoFailure.stderr, /upgrade failed \(exit status: 101\)/);
  assert.deepEqual(readFileSync(cargoBinary), readFileSync(previous));

  release = current.release;
  requests = 0;
  const terminal = join(root, 'target/debug/hypercmd-terminal');
  const warning = `⚠️ New version available: ${previousVersion} → ${packageMetadata.version}. `
    + 'Run hypercmd upgrade';
  const checks = ['--offline', '--frozen'].map(mode =>
    ['check', mode, '--manifest-path', join(root, 'crates/hypercmd-cli/Cargo.toml')]);
  // Compile before entering the PTY helper's short exit deadline.
  await run(installed, checks[0]);
  for (const args of [['--help'], ['--version'], ...checks]) {
    assert.equal((await run(terminal, [installed, ...args])).stdout.includes(warning), false);
  }
  const continuousIntegration = await run(terminal,
    [installed, 'profile'], { ...environment, CI: '1' });
  assert.equal(continuousIntegration.stdout.includes(warning), false);
  assert.equal((await run(installed, ['profile'])).stderr, '');
  assert.equal(requests, 0);
  for (let invocation = 0; invocation < 2; invocation += 1) {
    const output = await run(terminal, [installed, 'profile']);
    assert(output.stdout.replaceAll('\r', '').startsWith(`${warning}\n`), output.stdout);
    assert.equal(requests, 1);
  }
  copyFileSync(binary, installed);
  assert.equal((await run(terminal, [installed, 'profile'])).stdout.includes(warning), false);
  assert.equal(requests, 1);

  copyFileSync(previous, installed);
  responseStatus = 503;
  const offline = { ...environment, XDG_CACHE_HOME: join(work, 'offline-cache') };
  for (let invocation = 0; invocation < 2; invocation += 1) {
    const output = await run(terminal, [installed, 'profile'], offline);
    assert.equal(output.stdout.includes(warning), false);
    assert.equal(requests, 2);
  }
  console.log('CLI replacement, failed upgrades, ordering and cached terminal warnings passed.');
} finally {
  server.close();
  rmSync(work, { recursive: true, force: true });
}
