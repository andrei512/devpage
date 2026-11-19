// Cargo owns package boundaries and dependency declarations. The extra paths
// below are files read across those boundaries by build scripts or Rust tests.
import cargo from './cargo-nx.cjs';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../../', import.meta.url));
const workspace = () => ({ packages: [], buildPackages: [], format: true, workspace: true });
const under = (path, directory) => path.startsWith(directory + '/');

function liveProgram(file) {
  // The root build script skips these entire subtrees/files when embedding live.
  return file.slice('live/'.length).split('/').every(name => !name.startsWith('.') &&
    !['node_modules', 'evals', 'eval.ts', 'smoke.ts'].includes(name) && !name.includes('.test.'));
}

function ownedElsewhere(file) {
  // docs checks handle ordinary site text and the repository's docs-level Rust
  // assertions. The individual package-test fixtures below take precedence.
  return under(file, 'docs') || /^[^/]+\.md$/i.test(file) ||
    /^(?:\.github|\.agents|\.claude|deploy|tools\/checks|tools\/browser-tests)\//.test(file) ||
    ['.dockerignore', '.gitignore', '.gitattributes', '.node-version', 'LICENSE', 'dist-workspace.toml',
      'install.sh', 'install.ps1', 'package.json', 'package-lock.json', 'project.json', 'nx.json'].includes(file);
}

// An empty packages array means no package checks, except when workspace=true.
// Full plans and unrelated diffs require no Cargo command on the planner host.
export function rustPlan(files, {full = false} = {}) {
  if (full) return workspace();
  const sources = new Set();
  const tests = new Set();
  let graph;
  let format = false;
  for (const file of files) {
    if (['Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', 'rust-toolchain', 'rustfmt.toml',
      '.rustfmt.toml', 'clippy.toml', '.clippy.toml'].includes(file) || under(file, '.cargo')) return workspace();
    if (under(file, 'docs/designs/checkout')) {
      for (const name of ['devpage', 'devpage-core', 'devpage-design', 'devpage-design-wasm']) tests.add(name);
      continue;
    }
    if (['docs/logo.svg', 'docs/boxes/viewer/share/making-a-video/README.md'].includes(file)) {
      tests.add('devpage');
      continue;
    }
    if (under(file, 'deploy/server')) {
      // The ignored deploy restore integration target reads this configuration.
      tests.add('devpage');
      continue;
    }
    if (under(file, 'examples/world')) {
      tests.add('devpage');
      continue;
    }
    if (ownedElsewhere(file)) continue;
    if (under(file, 'live')) {
      if (liveProgram(file)) sources.add('devpage');
      continue;
    }

    graph ??= cargo.workspaceMembers(root);
    if (!graph.length) return workspace();
    const member = graph.find(p => p.dir && under(file, p.dir));
    // A workspace package lives at the root. Assign only its known inputs to
    // it; an unrecognized production file may feed any package.
    const dir = member?.dir ?? '';
    if (!dir && !/^(?:src\/|tests\/|assets\/|build\.rs$)/.test(file)) return workspace();
    const name = member?.name ?? graph.find(p => !p.dir)?.name;
    if (!name) return workspace();
    const local = dir ? file.slice(dir.length + 1) : file;
    format ||= file.endsWith('.rs');
    // Integration tests, benchmarks and examples are executable targets.
    // Other packages can't link to them as they can to a library.
    const testOnly = /^(tests|benches|examples)\//.test(local) || under(local, 'testdata');
    (testOnly ? tests : sources).add(name);

    // core/build.rs includes both of these root directories in core's build id.
    if (under(file, 'src') || under(file, 'assets')) sources.add('devpage-core');
    // The WASM generator verifies a fingerprint of the vendored bindings and
    // generator/engine inputs. This edge isn't present in Cargo's manifests.
    if (/^design\/(?:src\/|proto\/|fonts\/|build\.rs$|Cargo\.toml$|wasm\/(?:src\/|Cargo\.toml$))/.test(file) ||
        /^core\/assets\/vendor\/design-engine\.(?:js|wasm|fingerprint)$/.test(file)) tests.add('devpage-design-module');
    if (under(file, 'tests/fixtures/search-docs') || under(file, 'tests/fixtures/sync-design')) tests.add('devpage-core');
  }

  if (sources.size || tests.size) {
    graph ??= cargo.workspaceMembers(root);
    // A renamed cross-crate owner requires a full run until its mapping is updated.
    if ([...sources, ...tests].some(name => !graph.some(p => p.name === name))) return workspace();
    for (const changed of sources) {
      for (const member of graph) {
        if (member.dependencies.includes(changed)) sources.add(member.name);
      }
    }
  }
  return {
    packages: [...new Set([...sources, ...tests])].sort(),
    buildPackages: [...sources].sort(),
    format, workspace: false,
  };
}
