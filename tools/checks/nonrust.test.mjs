import assert from 'node:assert/strict';
import test from 'node:test';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { nonRustPlan, changedSpecReferences } from './nonrust.mjs';

test('a changed Bun test runs itself; shared helpers exercise their consumers', () => {
  assert.deepEqual(nonRustPlan(['live/registry.test.ts']).live.tests, ['./registry.test.ts']);
  assert.equal(nonRustPlan(['live/service.test.harness.ts']).live.tests.length, 0);
  assert.equal(nonRustPlan(['live/removed.test.ts']).live.tests.length, 0);
  assert.equal(nonRustPlan(['live/service.ts']).live.typecheck, true);
});

test('planning distinguishes docs, toolchain, install, Rust coverage and unknown code', () => {
  assert.ok(nonRustPlan(['docs/features/README.md']).docs.check);
  assert.ok(nonRustPlan(['docs/features/new-constraint.md']).docs.requirements);
  assert.ok(nonRustPlan(['CHANGELOG.md']).docs.check);
  assert.equal(nonRustPlan(['review-brief.md']).docs.check, false);
  assert.equal(nonRustPlan(['tools/checks/run.mjs']).tooling, true);
  assert.ok(nonRustPlan(['extension-api/src/lib.rs']).docs.filters.includes('every_item_of_devpages_code_is_documented'));
  assert.equal(nonRustPlan(['live/registry.test.ts']).install, false);
  assert.equal(nonRustPlan(['install.sh']).install, true);
  assert.equal(nonRustPlan(['unowned/future.go']).unknown.length, 1);
});

test('only changed spec annotations select static mapping for a Rust edit', t => {
  const cwd = mkdtempSync(join(tmpdir(), 'devpage-spec-'));
  t.after(() => rmSync(cwd, {recursive:true, force:true}));
  const git = (...args) => execFileSync('git', args, {cwd, encoding:'utf8'}).trim();
  const name = join(cwd, 'sample.rs');
  git('init', '-q');
  git('config', 'user.name', 'Test');
  git('config', 'user.email', 'test@example.org');
  writeFileSync(name, '#[spec("a-rule")]\nfn checked() {}\n');
  git('add', '.'); git('commit', '-qm', 'original');
  const head = git('rev-parse', 'HEAD');
  const local = {base: head, head, files: ['sample.rs']};
  writeFileSync(name, '#[spec("a-rule")]\nfn renamed() {}\n');
  assert.equal(changedSpecReferences(local, true, cwd), false);
  writeFileSync(name, 'fn renamed() {}\n');
  git('add', 'sample.rs');
  assert.equal(changedSpecReferences(local, true, cwd), true);
  assert.equal(changedSpecReferences(local, false, cwd), false);
  git('commit', '-qm', 'remove spec');
  const removed = git('rev-parse', 'HEAD');
  assert.equal(changedSpecReferences({base:head,head:removed,files:['sample.rs']}, false, cwd), true);
  writeFileSync(join(cwd, 'new.rs'), '#[spec("added")]\nfn checked() {}\n');
  assert.equal(changedSpecReferences({base:removed,head:removed,files:['new.rs']}, true, cwd), true);
});
