// Replay real PR-sized changes against today's test ownership. Run from a checkout with history.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { changedFiles, selectFiles } from './run.mjs';

const project = JSON.parse(readFileSync(new URL('../../project.json', import.meta.url)));
const all = Object.keys(project.targets).filter(name => project.targets[name].metadata.browserFilters).sort();
const examples = [
  ['221fdfa92b5781498a0b581a587a08903718ff21', 'release archive format', [], false],
  ['06994906e1ec6cb4f01bcaddd5bd62451f23e243', 'deployed PDF rendering and server config', [
    'browser-canvas', 'browser-pdf', 'browser-server-mode', 'browser-server-mode-comments', 'browser-server-mode-pages',
  ], true],
  ['8dbc165571dce82b6d3e517ad42aa951b89ed6f4', 'shared Chrome adapter', all, true],
];
for (const [head, description, expected, smoke] of examples) {
  const started = performance.now();
  const { files, fallback } = changedFiles(head + '^', head);
  assert.ok(!fallback, `${description}: fetch the sample's history before replaying it`);
  const plan = selectFiles(files);
  assert.deepEqual([...plan.suites].sort(), expected, description);
  assert.equal(plan.smoke, smoke, description);
  console.log(`${head.slice(0, 8)} • ${description} • ${plan.suites.length} Rust suites • Docker ${smoke ? 'selected' : 'skipped'} • ${Math.round(performance.now() - started)} ms`);
}
