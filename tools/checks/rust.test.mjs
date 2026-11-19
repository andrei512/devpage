import assert from 'node:assert/strict';
import { test } from 'node:test';
import { spawnSync } from 'node:child_process';
import { rustPlan } from './rust.mjs';

const app = 'devpage';
const core = 'devpage-core';
const design = 'devpage-design';
const wasm = 'devpage-design-wasm';
const generator = 'devpage-design-module';
const expectPlan = (files, names, format, builds = names) => assert.deepEqual(rustPlan(files), {
  packages: [...names].sort(), buildPackages: [...builds].sort(), workspace: false, format,
});

test('regular source follows local Cargo dependencies across every platform', () => {
  expectPlan(['extension-api/src/lib.rs'], [app, 'devpage-extension-api'], true);
  expectPlan(['bench/src/main.rs'], ['devpage-bench'], true);
  expectPlan(['src/adapters/watch/macos.rs'], [app, core], true);
  expectPlan(['design/src/lib.rs'], [app, core, design, wasm, generator], true, [app, core, design, wasm]);
});

test('integration-test-only targets stay with their owner; shared fixtures reach their readers', () => {
  expectPlan(['extension-api/tests/abi.rs'], ['devpage-extension-api'], true, []);
  expectPlan(['tests/exit_codes.rs'], [app], true, []);
  expectPlan(['bench/src/main.rs', 'tests/exit_codes.rs'], ['devpage-bench', app], true, ['devpage-bench']);
  expectPlan(['tests/fixtures/search-docs/docs/README.md'], [app, core], false, []);
  expectPlan(['docs/designs/checkout/fonts/Figtree/Figtree-Variable.ttf'], [app, core, design, wasm], false, []);
});

test('embedded program and external WASM fingerprint edges are accounted for', () => {
  expectPlan(['live/service.ts'], [app], false);
  expectPlan(['core/assets/vendor/design-engine.wasm'], [app, core, generator], false, [app, core]);
  expectPlan(['design/wasm/src/lib.rs'], [wasm, generator], true, [wasm]);
  expectPlan(['core/assets/widgets/DevpageSidebar.tsx'], [app, core], false);
  expectPlan(['assets/video.js'], [app, core], false);
});

test('known documentation and Bun eval changes need no Rust package build', () => {
  expectPlan(['docs/boxes/engineering/tests/README.md', 'eval_discussion_breif.md', 'live/evals/case.json'], [], false);
  expectPlan([], [], false);
  expectPlan(['docs/boxes/viewer/share/making-a-video/README.md'], [app], false, []);
});

test('global changes and unknown files select workspace; separable package edits union', () => {
  for (const file of ['Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', '.cargo/config.toml', 'new-tool/new.rs']) {
    assert.equal(rustPlan([file]).workspace, true, file);
  }
  assert.equal(rustPlan([], {full: true}).format, true);
  expectPlan(['bench/src/main.rs', 'design/module/src/lib.rs'], ['devpage-bench', generator], true);
});

test('a planner without Cargo can skip a brief and select an individual crate', () => {
  const module = new URL('./rust.mjs', import.meta.url).href;
  const result = spawnSync(process.execPath, ['--input-type=module', '-e',
    `import { rustPlan } from ${JSON.stringify(module)}; console.log(JSON.stringify([
      rustPlan(['brief.md']), rustPlan(['bench/src/main.rs'])]));`],
  { encoding: 'utf8', env: { ...process.env, PATH: '' } });
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(JSON.parse(result.stdout), [
    { packages: [], buildPackages: [], format: false, workspace: false },
    { packages: ['devpage-bench'], buildPackages: ['devpage-bench'], format: true, workspace: false },
  ]);
});
