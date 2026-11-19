import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, writeFileSync, renameSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { changedFiles, cargoArgs, selectFiles } from './run.mjs';

test('the committed range and local changes include both sides of renames and every worktree layer', (t) => {
  const cwd = mkdtempSync(join(tmpdir(), 'devpage-diff-'));
  t.after(() => rmSync(cwd, { recursive: true, force: true }));
  const git = (...args) => execFileSync('git', args, {cwd});
  git('init', '-q');
  git('config', 'user.email', 'test@example.org');
  git('config', 'user.name', 'Test');
  const write = (name, text) => writeFileSync(join(cwd, name), text);
  for (const name of ['old name', 'working', 'staged']) write(name, name);
  git('add', '.'); git('commit', '-qm', 'base');
  renameSync(join(cwd, 'old name'), join(cwd, 'new name'));
  git('add', '-A'); git('commit', '-qm', 'rename');
  write('working', 'unstaged');
  write('staged', 'staged change'); git('add', 'staged');
  write('untracked', 'new');
  assert.deepEqual(changedFiles('HEAD~1', 'HEAD', cwd).files, ['new name', 'old name']);
  assert.deepEqual(changedFiles('HEAD~1', undefined, cwd).files, ['new name','old name','staged','untracked','working']);
  assert.ok(changedFiles('invalid-base', 'HEAD', cwd).fallback);
  assert.ok(changedFiles('HEAD', 'HEAD~1', cwd).fallback);
});

test('the Nx selector batch-runs selected filters and skips Cargo on an empty selection', () => {
  const video = selectFiles(['assets/video.js']);
  assert.equal(video.full, false);
  assert.ok(video.filters.includes('browser_tests::video::'));
  const args = cargoArgs(video, 'win32');
  assert.ok(args.includes('--ignored') && args.includes('--skip') && args.includes('gpu'));
  assert.deepEqual(cargoArgs(selectFiles([])), null);
  const unknown = selectFiles(['new-product-code/mystery.rs']);
  assert.equal(unknown.full, true);
  assert.equal(unknown.smoke, true);
  assert.equal(cargoArgs(unknown, 'linux').at(-1), '--ignored');
});
