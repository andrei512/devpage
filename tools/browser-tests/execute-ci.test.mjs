import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';

test('an empty shared browser plan succeeds without spawning Cargo', () => {
  const temp = mkdtempSync(join(tmpdir(), 'affected-checks-'));
  try {
    const summary = join(temp, 'summary');
    writeFileSync(join(temp, 'devpage-check-plan.json'), JSON.stringify({ browser: { full: false, filters: [] } }));
    const result = spawnSync(process.execPath, ['tools/browser-tests/execute-ci.mjs'], {
      encoding: 'utf8', env: { ...process.env, RUNNER_TEMP: temp, GITHUB_STEP_SUMMARY: summary, PATH: '' },
    });
    assert.equal(result.status, 0, result.stderr);
    assert.match(readFileSync(summary, 'utf8'), /zero checks selected/);
  } finally { rmSync(temp, { recursive: true, force: true }); }
});
