import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../../', import.meta.url));
const require = createRequire(import.meta.url);
const nx = join(dirname(require.resolve('nx/package.json')), require('nx/package.json').bin.nx);
function command(program, args) {
  return JSON.parse(execFileSync(program, args, {
    cwd: root, encoding: 'utf8', env: { ...process.env, NX_DAEMON: 'false', NX_NO_CLOUD: 'true',
      RUSTUP_AUTO_INSTALL: '0', FORCE_COLOR: '0' }, maxBuffer: 24 * 1024 * 1024,
  }));
}

test('Nx exposes the Cargo workspace, every local dependency, and executable per-crate targets', () => {
  const metadata = command('cargo', ['metadata', '--locked', '--offline', '--no-deps', '--format-version=1']);
  const members = metadata.packages.filter(p => metadata.workspace_members.includes(p.id));
  const names = new Map(members.map(p => [dirname(p.manifest_path), p.name]));
  const { graph } = command(process.execPath, [nx, 'graph', '--file=stdout']);
  for (const {name, dependencies} of members) {
    const node = graph.nodes[name];
    assert.ok(node, name);
    const expected = new Set(dependencies.map(d => names.get(d.path)).filter(Boolean));
    const actual = graph.dependencies[name].map(d => d.target).filter(target => graph.nodes[target]);
    assert.deepEqual([...new Set(actual)].sort(), [...expected].sort(), name);
    for (const target of ['build', 'test', 'clippy', 'fmt']) {
      assert.equal(node.data.targets[target].cache, false);
      assert.ok(node.data.targets[target].options.command.includes(`-p ${name}`));
    }
  }
});

test('Nx native build selection isolates bench and includes downstream consumers of extension API', () => {
  function affected(file, target = 'build') {
    const data = command(process.execPath, [nx, 'affected', '-t', target, '--files', file, '--explain=stdout']);
    return Object.keys(data.affected).sort();
  }
  assert.deepEqual(affected('extension-api/tests/abi.rs', 'test'), ['devpage-extension-api:test']);
  assert.deepEqual(affected('extension-api/tests/abi.rs'), []);
  assert.deepEqual(affected('docs/boxes/engineering/tests/README.md'), []);
  assert.deepEqual(affected('core/assets/vendor/design-engine.wasm', 'test'), [
    'devpage-core:test', 'devpage-design-module:test', 'devpage:test',
  ]);
  assert.deepEqual(affected('bench/src/main.rs'), ['devpage-bench:build']);
  assert.deepEqual(affected('extension-api/src/lib.rs'), ['devpage-extension-api:build', 'devpage:build']);
});
