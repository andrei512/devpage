import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { comparison, workloads } from './ci.mjs';

const before = '1'.repeat(40);
const head = '2'.repeat(40);
const lastGreen = '3'.repeat(40);
const push = { before, after: head, ref: 'refs/heads/design/one' };
const env = { GITHUB_RUN_NUMBER: '120' };
const green = { head_branch: 'design/one', event: 'push', conclusion: 'success', head_sha: lastGreen, run_number: 110 };

test('a fork PR targeting a design branch uses the merge base, while the tests run on the merge checkout', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'devpage-range-'));
  const git = (...args) => execFileSync('git', ['-C', dir, ...args], { encoding: 'utf8' }).trim();
  try {
    git('init', '-q');
    git('config', 'user.name', 'test');
    git('config', 'user.email', 'test@localhost');
    git('config', 'commit.gpgsign', 'false');
    const record = msg => { git('commit', '--allow-empty', '-qm', msg); return git('rev-parse', 'HEAD'); };
    const forkPoint = record('shared');
    git('checkout', '-qb', 'design/one');
    const base = record('base moved');
    git('checkout', '-qb', 'local-copy-of-fork', forkPoint);
    const forkHead = record('fork head');
    const payload = { pull_request: { base: { sha: base }, head: { sha: forkHead } } };
    const range = await comparison('pull_request', payload, env, 'gates.yml', { mergeBase: (...args) => git('merge-base', ...args) });
    assert.deepEqual(range, { base: forkPoint, head: forkHead });
  } finally { rmSync(dir, { recursive: true, force: true }); }
});

test('a shallow PR checkout and an unsafe ref both request full coverage', async () => {
  const pr = { pull_request: { base: { sha: before }, head: { sha: head } } };
  assert.equal((await comparison('pull_request', pr, env, 'gates.yml', { mergeBase: () => null })).full, true);
  pr.pull_request.head.sha = '--help';
  assert.equal((await comparison('pull_request', pr, env, 'gates.yml', { mergeBase: () => assert.fail() })).full, true);
});

test('push range includes failed intermediate work, uses its own workflow and ignores reruns for the same SHA', async () => {
  const result = await comparison('push', push, env, 'deploy-server.yml', {
    mergeBase: (a, b) => b === head && a === before ? before : b === before && a === lastGreen ? lastGreen : null,
    successfulPushes: async (workflow, branch) => {
      assert.equal(workflow, 'deploy-server.yml');
      assert.equal(branch, 'design/one');
      return [{ ...green, head_sha: head }, { ...green, run_number: 124 }, { ...green, event: 'pull_request' },
        { ...green, conclusion: 'failure' }, { ...green, head_branch: 'main' }, green];
    },
  });
  assert.deepEqual(result, { base: lastGreen, head });
});

test('missing, inaccessible or unrelated push baselines request full coverage', async () => {
  const base = { mergeBase: (a, b) => b === head ? a : null };
  for (const lookup of [async () => [], async () => null, async () => [green], async () => { throw Error('history unavailable'); }]) {
    assert.equal((await comparison('push', push, env, 'gates.yml', { ...base, successfulPushes: lookup })).full, true);
  }
  for (const change of [{ created: true }, { forced: true }, { before: '0'.repeat(40) }]) {
    assert.equal((await comparison('push', { ...push, ...change }, env, 'gates.yml', {
      mergeBase: () => assert.fail(), successfulPushes: () => assert.fail(),
    })).full, true);
  }
});

test('manual and scheduled runs always request full coverage', async () => {
  for (const event of ['workflow_dispatch', 'schedule']) {
    assert.equal((await comparison(event, {}, env, 'gates.yml')).full, true);
  }
});

test('only the required platform tools are installed for each part of a project plan', () => {
  const empty = { rust: { workspace: false, format: false, packages: [] }, docs: { check: false, filters: [] },
    live: { typecheck: false, tests: null }, browser: { files: [], filters: [], full: false }, install: false, tooling: false };
  const none = workloads(empty);
  assert.deepEqual(Object.values(none), Array(Object.keys(none).length).fill(false));
  const bun = workloads({ ...empty, live: { typecheck: false, tests: ['service.test.ts'] } });
  assert.ok(bun.linux && bun.live);
  assert.ok(!bun.linuxRust && !bun.windows && !bun.install);
  const tooling = workloads({ ...empty, tooling: true });
  assert.ok(tooling.linux && tooling.linuxRust && !tooling.windows && !tooling.live);
  const docs = workloads({ ...empty, docs: { check: true, filters: [] } });
  assert.ok(docs.linux && docs.linuxRust);
  assert.ok(!docs.windows);
  const chrome = workloads({ ...empty, browser: { files: [], filters: ['browser_tests::video::'], full: false } });
  assert.ok(chrome.windows && chrome.windowsRust && chrome.browser);
  assert.ok(!chrome.linux && !chrome.inventory);
  const moved = workloads({ ...empty, browser: { files: ['src/drive/browser_tests/sidebar.rs'],
    filters: ['browser_tests::sidebar::'], full: false } });
  assert.ok(moved.inventory);
  const cargo = workloads({ ...empty, rust: { workspace: false, format: true, packages: ['devpage-core'] } });
  assert.ok(cargo.windowsRust && cargo.linuxRust);
});
