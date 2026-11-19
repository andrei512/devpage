import { spawnSync } from 'node:child_process';
import { appendFileSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { cargoArgs } from './run.mjs';

// Before cutover, keep the deploy workflow's former push and PR path coverage.
// A newly affected smoke also runs during comparison.
function previousSmoke(files, event) {
  if (event !== 'pull_request') return true;
  return files.some(file => /^(deploy\/server\/|src\/|core\/|design\/|assets\/)/.test(file) ||
    ['Cargo.toml', 'Cargo.lock', '.github/workflows/deploy-server.yml'].includes(file));
}

export function execution(plan, mode, event, platform = process.platform) {
  if (mode !== 'affected' && mode !== 'shadow' && mode !== '') {
    throw new Error('DEVPAGE_BROWSER_MODE must be affected or shadow');
  }
  const shadow = mode !== 'affected';
  const rust = cargoArgs(shadow ? { ...plan, full: true } : plan, platform);
  const smoke = plan.smoke || (shadow && previousSmoke(plan.files, event));
  return { rust, smoke, mode: shadow ? 'shadow' : 'affected' };
}

function main() {
  const [check] = process.argv.slice(2);
  if (!['rust', 'smoke'].includes(check)) throw new Error('Choose rust or smoke.');
  const { plan } = JSON.parse(readFileSync(join(process.env.RUNNER_TEMP, 'devpage-browser-selection.json'), 'utf8'));
  const decision = execution(plan, process.env.DEVPAGE_BROWSER_MODE ?? '', process.env.GITHUB_EVENT_NAME);
  if (check === 'smoke') {
    appendFileSync(process.env.GITHUB_OUTPUT, `run=${decision.smoke}\n`);
    appendFileSync(process.env.GITHUB_STEP_SUMMARY,
      `Deploy-server PDF smoke (${decision.mode}): ${decision.smoke ? 'run' : 'zero checks selected'}.\n`);
    console.log(decision.smoke ? 'Run deploy-server PDF smoke.' : 'Zero deploy-server PDF checks selected.');
    return;
  }

  const started = performance.now();
  if (decision.rust) {
    console.log('cargo', ...decision.rust);
    const result = spawnSync('cargo', decision.rust, { stdio: 'inherit' });
    if (result.error) throw result.error;
    process.exitCode = result.status ?? 1;
  } else {
    console.log('Zero Rust browser tests selected.');
  }
  const duration = ((performance.now() - started) / 1000).toFixed(2);
  appendFileSync(process.env.GITHUB_STEP_SUMMARY,
    `Rust browser execution (${decision.mode}): ${decision.rust ? 'ran Cargo' : 'zero checks selected'} in ${duration} s.\n`);
}

if (import.meta.main) main();
