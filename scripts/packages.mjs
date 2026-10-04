import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

// Exercise Cargo archives and their normalized manifests.
const root = resolve(import.meta.dirname, '..');
const work = realpathSync(mkdtempSync(join(tmpdir(), 'hypercmd-packages-')));
const bundle = join(root, 'target', 'package-acceptance');
const env = { ...process.env, CARGO_INCREMENTAL: '0', RUSTUP_TOOLCHAIN: '1.85' };
const hash = path => createHash('sha256').update(readFileSync(path)).digest('hex');
function run(command, args, cwd = root) {
  const result = spawnSync(command, args, { cwd, env, encoding: 'utf8', maxBuffer: 32 * 1024 * 1024 });
  if (result.error) throw result.error;
  assert.equal(result.status, 0, `${command} ${args.join(' ')}\n${result.stdout}\n${result.stderr}`);
  return result.stdout;
}
function write(path, text) {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, text);
}

try {
  const metadata = JSON.parse(run('cargo', ['metadata', '--format-version=1', '--all-features', '--offline']));
  const names = new Set(['hypercmd', 'hypercmd-build', 'hypercmd-cli', 'hypercmd-job-controls']);
  const packages = metadata.packages.filter(pkg => names.has(pkg.name));
  for (const name of names) assert(packages.some(pkg => pkg.name === name), `missing ${name}`);
  for (const pkg of metadata.packages.filter(pkg => pkg.source === null)) {
    assert(pkg.manifest_path.startsWith(root + '/'), `${pkg.name} must come from crates.io, not ${pkg.manifest_path}`);
  }

  const patches = packages.map(pkg => `${JSON.stringify(pkg.name)} = { path = ${JSON.stringify(dirname(pkg.manifest_path))} }`).join('\n');
  const developmentConfig = join(work, 'development.toml');
  write(developmentConfig, `[patch.crates-io]\n${patches}\n`);
  mkdirSync(bundle, { recursive: true });
  const extracted = join(work, 'packages');
  mkdirSync(extracted);
  const records = [];
  const directories = new Map();
  for (const pkg of packages) {
    run('cargo', ['package', '--manifest-path', pkg.manifest_path, '--target-dir', join(work, 'packaging'),
      '--no-verify', '--allow-dirty', '--offline', '--config', developmentConfig]);
    const file = `${pkg.name}-${pkg.version}.crate`;
    const archive = join(bundle, file);
    copyFileSync(join(work, 'packaging', 'package', file), archive);
    const sha256 = hash(archive);
    directories.set(pkg.name, join(extracted, `${pkg.name}-${pkg.version}`));
    records.push({ name: pkg.name, version: pkg.version, file, sha256 });
    run('tar', ['-xzf', archive, '-C', extracted], work);
  }
  write(join(bundle, 'manifest.json'), JSON.stringify({ packages: records }, null, 2) + '\n');
  write(join(bundle, 'SHA256SUMS'), records.map(pkg => `${pkg.sha256}  ${pkg.file}`).join('\n') + '\n');
  const packagePath = name => directories.get(name);
  const cleanConfig = join(work, '.cargo', 'config.toml');
  write(cleanConfig, `[patch.crates-io]\n${records.map(pkg => `${JSON.stringify(pkg.name)} = { path = ${JSON.stringify(packagePath(pkg.name))} }`).join('\n')}\n`);
  // No build below this point has a path to either repository's source tree.
  const app = join(work, 'consumer');
  write(join(app, 'Cargo.toml'), `[package]
name = "hypercmd-packaged-consumer"
version = "0.0.0"
edition = "2024"
rust-version = "1.85"
[workspace]
[dependencies]
hypercmd = "=0.1.1"
hypercmd-job-controls = { version = "=0.1.1", features = ["terminal"] }
fusor = { package = "fusor-core", version = "=0.1.4", default-features = false }
fusor-components = { version = "=0.1.4", default-features = false }
[build-dependencies]
hypercmd-build = "=0.1.1"
[package.metadata.hypercmd]
entry = "ui/app.html"
`);
  write(join(app, 'build.rs'), 'fn main() { hypercmd_build::compile_app().unwrap(); }\n');
  write(join(app, 'ui', 'app.html'), `<App state="{{ Demo::new() }}"><main>
<ForEach items="{{ state.jobs.get() }}" key="{{ |job| job.id }}"><JobRow job="{{ item.clone() }}" jobs="{{ state.jobs.clone() }}" inspect="{{ state.inspect.clone() }}"></JobRow></ForEach>
</main></App>\n`);
  write(join(app, 'src', 'main.rs'), `use fusor::{signal, Signal};
use hypercmd_job_controls::{Job, JobRow};
use std::rc::Rc;
struct Demo { jobs: Signal<Vec<Job>>, inspect: Rc<dyn Fn(u32)> }
impl Demo {
    fn new() -> Self {
        Self { jobs: signal(vec![Job { id: 7, name: "Archived 東京 job".into(), progress: 30, cancelled: false }]), inspect: Rc::new(|id| assert_eq!(id, 7)) }
    }
}
fusor::template!(backend = "hypercmd", "ui/app.html");
fn buttons(node: &hypercmd::Node) -> Vec<hypercmd::Node> {
    let mut found = Vec::new();
    if node.tag() == "button" { found.push(node.clone()); }
    for child in node.children() { found.extend(buttons(&child)); }
    found
}
fn main() {
    let app = hypercmd_app().unwrap();
    app.publish();
    let root = app.root();
    let controls = buttons(&root);
    controls[0].dispatch("click").unwrap();
    controls[0].dispatch("click").unwrap();
    assert_eq!(controls[0].text(), "Inspect (2)");
    let frame = hypercmd::layout::render(&root, (80, 10), None, &mut Default::default(), &Default::default()).unwrap();
    let rect = |node: &hypercmd::Node| frame.entries.iter().find(|entry| entry.node == *node).unwrap().rect;
    assert_eq!(rect(&controls[0]).y, rect(&controls[1]).y, "packaged component CSS lays out its actions in a row");
    assert!(rect(&controls[1]).x > rect(&controls[0]).x);
    controls[1].dispatch("click").unwrap();
    assert!(controls[1].attribute("disabled").is_some());
    controls[2].dispatch("click").unwrap();
    assert!(!controls[0].is_alive());
    controls[0].dispatch("click").unwrap();
    assert!(buttons(&root).is_empty());
    println!("Packaged stateful HTML, styles, callbacks and disposal passed.");
}
`);
  env.CARGO_TARGET_DIR = join(work, 'target');
  run('cargo', ['generate-lockfile', '--offline'], app);
  const cleanMetadata = JSON.parse(run('cargo', ['metadata', '--format-version=1', '--locked', '--offline'], app));
  for (const pkg of cleanMetadata.packages.filter(pkg => pkg.source === null)) {
    assert(pkg.manifest_path.startsWith(work + '/'), `checkout dependency leaked: ${pkg.manifest_path}`);
  }
  run('cargo', ['run', '--locked', '--offline'], app);
  for (const [cwd, args] of [[app, []], [root, ['-p', 'hypercmd']], [root, ['--manifest-path', 'tests/consumer/Cargo.toml']]]) {
    assert.doesNotMatch(run('cargo', ['tree', '--locked', '--offline', '-e', 'normal', ...args], cwd), /\b(?:web-sys|js-sys|wasm-bindgen) v/, 'native and portable graphs must stay browser-free');
  }
  run('cargo', ['doc', '--manifest-path', join(packagePath('hypercmd'), 'Cargo.toml'), '--no-default-features', '--no-deps', '--offline'], work);
  run('cargo', ['build', '--manifest-path', join(packagePath('hypercmd-cli'), 'Cargo.toml'), '--bin', 'hypercmd', '--offline'], work);

  const check = (authored, expression, location, template) => {
    const original = readFileSync(authored, 'utf8');
    writeFileSync(authored, original.replace(expression, 'state.no_such_packaged_method()'));
    try {
      const diagnostic = spawnSync(join(work, 'target', 'debug', 'hypercmd'), ['check', '--offline', '--locked'], { cwd: app, env, encoding: 'utf8', maxBuffer: 8 * 1024 * 1024 });
      if (diagnostic.error) throw diagnostic.error;
      const output = diagnostic.stdout + diagnostic.stderr;
      assert.notEqual(diagnostic.status, 0);
      assert.match(output, new RegExp(`(?:^|\\s)${location}:\\d+:\\d+: error: `, 'm'));
      assert.match(output, /no_such_packaged_method/);
      assert.match(output, new RegExp(`fusor_backends/hypercmd/ui/${template.replace('.', '\\.')}\\.rs`));
    } finally { writeFileSync(authored, original); }
  };
  check(join(app, 'ui', 'app.html'), 'state.jobs.get()', 'ui/app\\.html', 'app.html');
  check(join(packagePath('hypercmd-job-controls'), 'ui', 'job.html'), 'state.job.get().name', '\\S*hypercmd-job-controls-[\\d.]+/ui/job\\.html', 'job.html');
  console.log(`Package-only Rust 1.85 consumer passed with ${records.length} pinned archives, styles, rustdoc, isolation and authored library diagnostics.`);
} finally {
  rmSync(work, { recursive: true, force: true });
}
