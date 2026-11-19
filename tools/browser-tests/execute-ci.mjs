// Execute the Windows browser portion of the plan shared by the gates workflow.
import { spawnSync } from 'node:child_process';
import { appendFileSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { cargoArgs } from './run.mjs';

const plan = JSON.parse(readFileSync(join(process.env.RUNNER_TEMP, 'devpage-check-plan.json'), 'utf8'));
const args = cargoArgs(plan.browser);
const started = performance.now();
if (args) {
  console.log('cargo', ...args);
  const result = spawnSync('cargo', args, { stdio: 'inherit' });
  if (result.error) throw result.error;
  process.exitCode = result.status ?? 1;
}
appendFileSync(process.env.GITHUB_STEP_SUMMARY,
  `Rust browser execution: ${args ? 'ran Cargo' : 'zero checks selected'} in ${((performance.now() - started) / 1000).toFixed(2)} s.\n`);
