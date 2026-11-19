// Resolve the diff tested by a GitHub workflow and report the selector's decision.
import { spawnSync } from 'node:child_process';
import { appendFileSync, readFileSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const sha = value => typeof value === 'string' && /^[0-9a-f]{40}$/.test(value) && !/^0+$/.test(value);
const broad = reason => ({ full: true, reason });

function mergeBase(left, right) {
  if (!sha(left) || !sha(right)) return null;
  const result = spawnSync('git', ['merge-base', left, right], { encoding: 'utf8' });
  if (result.error) throw result.error;
  return result.status === 0 ? result.stdout.trim() : null;
}

// The workflow file and push event both constrain history. A green pull request
// or a green Windows gate cannot establish the deployed PDF smoke's baseline.
async function successfulPushes(workflow, branch, env) {
  const url = new URL(`${env.GITHUB_API_URL}/repos/${env.GITHUB_REPOSITORY}/actions/workflows/${encodeURIComponent(workflow)}/runs`);
  url.search = new URLSearchParams({ branch, event: 'push', status: 'success', per_page: '100' });
  const response = await fetch(url, {
    headers: { Authorization: `Bearer ${env.GITHUB_TOKEN}`, Accept: 'application/vnd.github+json' },
    signal: AbortSignal.timeout(10000),
  });
  if (!response.ok) throw new Error(`GitHub returned HTTP ${response.status}`);
  return (await response.json()).workflow_runs;
}

export async function comparison(event, payload, env, workflow, options = {}) {
  const common = options.mergeBase ?? mergeBase;
  const history = options.successfulPushes ?? successfulPushes;
  if (event === 'pull_request') {
    const { base, head } = payload.pull_request;
    const ancestor = sha(base.sha) && sha(head.sha) && common(base.sha, head.sha);
    return ancestor ? { base: ancestor, head: head.sha } : broad('PR merge base or head unavailable.');
  }
  if (event !== 'push') return broad(`Full verification for ${event}.`);
  if (payload.created || payload.forced || payload.deleted || !sha(payload.before) || !sha(payload.after)) {
    return broad('Branch created, rewritten or deleted, or commit unavailable.');
  }
  if (common(payload.before, payload.after) !== payload.before) return broad('Previous push is outside the available history.');
  const branch = payload.ref.replace(/^refs\/heads\//, '');
  let runs;
  try { runs = await history(workflow, branch, env); }
  catch (error) { return broad(`Successful push history unavailable: ${error.message}`); }
  if (!Array.isArray(runs)) return broad('Successful push history could not be read.');
  const prior = runs.find(run => run.head_branch === branch && run.event === 'push' && run.conclusion === 'success'
    && run.run_number < Number(env.GITHUB_RUN_NUMBER) && sha(run.head_sha) && run.head_sha !== payload.after);
  if (!prior || common(prior.head_sha, payload.before) !== prior.head_sha) {
    return broad('No previous successful push on the available branch history.');
  }
  return { base: prior.head_sha, head: payload.after };
}

function escapeHtml(text) {
  return text.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;');
}

// Both matrix runners consume this same plan. Tool setup follows the work on
// each platform; the clean install job gets just its boolean decision.
export function workloads(checks) {
  const rust = checks.rust.workspace || checks.rust.format || checks.rust.packages.length > 0;
  const docs = checks.docs.check || checks.docs.requirements || checks.docs.filters.length > 0;
  const live = checks.live.typecheck || Array.isArray(checks.live.tests);
  const browser = checks.browser.full || checks.browser.filters.length > 0;
  return {
    linux: Boolean(rust || docs || live || checks.tooling),
    windows: Boolean(rust || browser),
    // The Linux tooling tests compare Nx discovery with real Cargo metadata.
    linuxRust: Boolean(rust || docs || checks.tooling), windowsRust: Boolean(rust || browser),
    live, install: checks.install, browser,
    inventory: browser && (checks.browser.full || checks.tooling || checks.browser.files.some(file =>
      file.startsWith('src/drive/browser_tests/') || file === 'src/adapters/chrome.rs')),
  };
}

async function report(env) {
  const started = performance.now();
  const payload = JSON.parse(readFileSync(env.GITHUB_EVENT_PATH, 'utf8'));
  const mode = env.DEVPAGE_CHECKS_MODE || 'affected';
  if (!['affected', 'full', 'shadow'].includes(mode)) throw new Error('Unknown DEVPAGE_CHECKS_MODE: ' + mode);
  const range = env.BROWSER_FORCE_FULL === 'true' || mode !== 'affected' ? broad('Full verification explicitly requested.')
    : await comparison(env.GITHUB_EVENT_NAME, payload, env, env.BROWSER_WORKFLOW);
  const args = range.full ? ['--full'] : ['--base', range.base, '--head', range.head];
  const project = env.BROWSER_WORKFLOW === 'gates.yml';
  const cli = project ? ['tools/checks/run.mjs', '--ci', '--platform', 'linux'] : ['tools/browser-tests/run.mjs'];
  const childEnv = { ...env };
  delete childEnv.GITHUB_TOKEN;
  const child = spawnSync(process.execPath, [...cli, '--json', ...args],
    { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024, env: childEnv });
  if (child.error) throw child.error;
  if (child.status !== 0) throw new Error(`Check selector failed: ${child.stderr}`);
  const selected = JSON.parse(child.stdout);
  const plan = project ? selected.browser : selected;
  const elapsed = Math.round(performance.now() - started);
  const flags = project ? workloads(selected) : {};
  if (project) writeFileSync(join(env.RUNNER_TEMP, 'devpage-check-plan.json'), JSON.stringify(selected));
  const outputs = { ...flags, smoke: plan.smoke };
  if (project) outputs.matrix = JSON.stringify({ include: [
    ...(flags.linux ? [{ os: 'ubuntu-latest', rust: flags.linuxRust }] : []),
    ...(flags.windows ? [{ os: 'windows-latest', rust: flags.windowsRust }] : []),
  ] });
  appendFileSync(env.GITHUB_OUTPUT, Object.entries(outputs)
    .map(([key, value]) => `${key}=${value}\n`).join(''));
  const detail = [
    range.reason ?? `Comparing ${range.base} to ${range.head}`,
    ...(project ? [
      ...(selected.reason ? [selected.reason] : []),
      `Changed paths (${selected.files.length}): ${selected.files.slice(0, 30).join(', ') || 'none supplied'}`,
      `Rust: ${selected.rust.workspace ? 'workspace' : selected.rust.packages.join(', ') || 'none'}; format: ${selected.rust.format}`,
      `Live: ${flags.live ? 'selected' : 'none'}; docs: ${selected.docs.check || selected.docs.requirements || selected.docs.filters.length ? 'selected' : 'none'}`,
      `Selector tests: ${selected.tooling}; clean install: ${flags.install}`,
    ] : []),
    `Rust browser suites: ${plan.full ? 'full' : plan.suites.join(', ') || 'none'}`,
    `Deploy-server PDF smoke: ${plan.smoke ? 'selected' : 'unaffected'}`,
    `Selection including baseline lookup: ${elapsed} ms`,
    ...plan.reasons.slice(0, 80),
  ];
  appendFileSync(env.GITHUB_STEP_SUMMARY, `### Affected checks\n\n<pre>${escapeHtml(detail.join('\n').slice(0, 40000))}</pre>\n`);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  await report(process.env);
}
