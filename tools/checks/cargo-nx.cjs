// Nx discovers Cargo members and their local dependency edges from manifests.
// Project-graph discovery also runs in jobs that have Node but no Rust toolchain.
const { globSync, readFileSync } = require('node:fs');
const { join, posix } = require('node:path');
const { parse } = require('smol-toml');

function workspaceMembers(root) {
  const manifest = file => parse(readFileSync(join(root, file), 'utf8'));
  const top = manifest('Cargo.toml');
  const members = new Map();
  const folders = new Set(top.package ? ['.'] : []);
  for (const pattern of top.workspace.members) {
    for (const file of globSync(posix.join(pattern, 'Cargo.toml'), {cwd: root})) {
      folders.add(posix.dirname(file.replaceAll('\\', '/')));
    }
  }
  for (const dir of folders) {
    // Cargo adds in-tree path dependencies to a workspace, even when they
    // aren't listed explicitly. Excluded paths can still be external deps.
    if (dir.startsWith('../') || dir === '..' || top.workspace.exclude?.some(pattern => posix.matchesGlob(dir, pattern))) continue;
    const data = dir === '.' ? top : manifest(posix.join(dir, 'Cargo.toml'));
    const dependencyDirectories = new Set();
    for (const table of [data, ...Object.values(data.target ?? {})]) {
      for (const kind of ['dependencies', 'build-dependencies', 'dev-dependencies']) {
        for (const [name, spec] of Object.entries(table[kind] ?? {})) {
          const dependency = spec.workspace ? top.workspace.dependencies[name] : spec;
          if (!dependency.path) continue;
          const path = posix.normalize(posix.join(spec.workspace ? '.' : dir, dependency.path));
          dependencyDirectories.add(path);
          folders.add(path);
        }
      }
    }
    if (!/^[a-zA-Z0-9_][a-zA-Z0-9_-]*$/.test(data.package?.name ?? '')) {
      throw new Error(`Invalid Cargo package name at ${dir}/Cargo.toml`);
    }
    members.set(dir, { name: data.package.name, dir: dir === '.' ? '' : dir, dependencyDirectories });
  }
  return [...members.values()].map(({name, dir, dependencyDirectories}) => ({
    name, dir, dependencies: [...dependencyDirectories].filter(d => members.has(d)).map(d => members.get(d).name).sort(),
  })).sort((a, b) => b.dir.length - a.dir.length);
}

const shared = ['Cargo.toml', 'Cargo.lock', 'rust-toolchain*', '.cargo/**/*', '.clippy.toml', 'clippy.toml'];
// Cargo cannot express all build-script and test inputs as package edges.
const productionInputs = {
  devpage: ['assets/**/*', 'live/**/*', '!live/evals/**/*', '!live/*.test.*', '!live/smoke.ts', '!live/eval.ts'],
  'devpage-core': ['src/**/*', 'assets/**/*', 'design/src/**/*', 'design/proto/**/*', 'design/fonts/**/*'],
};
const testInputs = {
  devpage: ['docs/designs/checkout/**/*', 'docs/talks/devpage/**/*', 'docs/widgets/**/*', 'docs/logo.svg',
    'docs/devpage.yml', 'docs/boxes/viewer/on-the-page/designs/writing-a-design/README.md',
    'docs/boxes/viewer/share/making-a-video/README.md', 'examples/world/**/*', 'deploy/server/**/*'],
  'devpage-core': ['tests/fixtures/**/*', 'docs/**/*', 'extension-api/src/**/*'],
  'devpage-design': ['docs/designs/checkout/**/*'],
  'devpage-design-wasm': ['docs/designs/checkout/**/*'],
  'devpage-design-module': ['design/src/**/*', 'design/proto/**/*', 'design/fonts/**/*', 'design/build.rs',
    'design/Cargo.toml', 'design/wasm/**/*', 'core/assets/vendor/design-engine.*'],
};
const workspaceInputs = paths => paths.map(p => p.startsWith('!') ?
  '!{workspaceRoot}/' + p.slice(1) : '{workspaceRoot}/' + p);

function cargoProject({name, dir, dependencies}) {
  // Cargo's integration tests, benches and examples belong only to their
  // package. A dependent compile reads its dependencies' production sources.
  const targetFiles = ['tests', 'benches', 'examples', 'testdata'].map(t => `{projectRoot}/${t}/**/*`);
  const sources = dir ? ['{projectRoot}/**/*', ...targetFiles.map(f => '!' + f)] :
    workspaceInputs(['src/**/*', 'build.rs']);
  const compile = ['cargoCrate', '^cargoCrate'];
  const commonInputs = workspaceInputs([...shared, ...(productionInputs[name] ?? [])]);
  // Nx exclusions in a named input also apply to the other filesets of the
  // task. Use the full owner input for tests instead of re-adding exclusions.
  const allFiles = dir ? ['{projectRoot}/**/*'] : [...sources, ...targetFiles];
  const verify = [...allFiles, ...commonInputs, '^cargoCrate', ...workspaceInputs(testInputs[name] ?? [])];
  const format = dir ? ['{projectRoot}/**/*.rs'] : workspaceInputs(['src/**/*.rs', 'tests/**/*.rs', 'build.rs']);
  const target = (command, inputs) => ({
    command, inputs, cache: false, options: {cwd: '{workspaceRoot}'},
  });
  return {
    name, implicitDependencies: dependencies,
    namedInputs: {cargoCrate: [...sources, ...commonInputs]},
    targets: {
      build: target(`cargo build --locked -p ${name}`, compile),
      test: target(`cargo test --locked -p ${name}`, verify),
      clippy: target(`cargo clippy --locked --all-targets -p ${name} -- -D warnings`, verify),
      fmt: target(`cargo fmt -p ${name} --check`, [...format, ...workspaceInputs([
        'rust-toolchain*', 'Cargo.toml', 'rustfmt.toml', '.rustfmt.toml',
      ])]),
    },
  };
}

exports.name = 'devpage/cargo';
exports.workspaceMembers = workspaceMembers;
exports.createNodes = ['{Cargo.toml,**/Cargo.toml}', (_files, _options, context) =>
  workspaceMembers(context.workspaceRoot).map(member => [
    posix.join(member.dir, 'Cargo.toml'),
    { projects: { [member.dir || '.']: cargoProject(member) } },
  ])];
