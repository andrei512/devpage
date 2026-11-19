import assert from 'node:assert/strict';
import test from 'node:test';
import { makePlan, commandsFor } from './run.mjs';

test('one changed live unit test does not run Cargo or Chrome', () => {
  const plan = makePlan(['live/registry.test.ts']);
  assert.deepEqual(commandsFor(plan).map(c => c.argv), [
    ['bun', 'run', 'tsc', '--project', 'tsconfig.json'], ['bun', 'test', './registry.test.ts'],
  ]);
  assert.equal(plan.install, false);
});

test('docs checking never pulls ignored browser/agent suites into the normal docs command', () => {
  const plan = makePlan(['docs/boxes/engineering/tests/README.md']);
  const commands = commandsFor(plan, {scope:'docs'});
  assert.ok(commands.some(c => c.argv.includes('--docs')));
  assert.ok(commands.every(c => !c.argv.includes('--ignored')));
});

test('explicit build produces only affected Cargo artifacts; Windows does not duplicate Linux-only CI work', () => {
  const plan = makePlan(['bench/src/main.rs', 'live/registry.test.ts'], {specChanges:false});
  const [cmd] = commandsFor(plan, {buildOnly:true});
  assert.deepEqual(cmd.argv, ['cargo','build','--locked','-p','devpage-bench']);
  const win = commandsFor(plan, {ci:true, nonBrowser:true, platform:'win32'});
  assert.ok(win.some(c => c.argv.includes('clippy')));
  assert.ok(win.every(c => c.argv[0] === 'cargo' && !c.argv.includes('--ignored')));
});

test('unknown source widens all checks, while a root brief has no execution', () => {
  assert.deepEqual(commandsFor(makePlan(['eval_discussion_breif.md'])), []);
  const broad = makePlan(['future/newlang.code']);
  assert.ok(broad.full && broad.install && broad.rust.workspace && broad.live.typecheck && broad.docs.check && broad.browser.smoke);
});

test('an integration-test edit selects verification without building production artifacts', () => {
  const plan = makePlan(['extension-api/tests/abi.rs'], {specChanges:false});
  assert.deepEqual(commandsFor(plan, {buildOnly:true}), []);
  assert.ok(commandsFor(plan).some(c => c.argv.includes('devpage-extension-api') && c.argv.includes('test')));
});
