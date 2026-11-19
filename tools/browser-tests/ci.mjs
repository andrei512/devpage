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

async function report(env) {
  const started = performance.now();
  const payload = JSON.parse(readFileSync(env.GITHUB_EVENT_PATH, 'utf8'));
  const range = env.BROWSER_FORCE_FULL === 'true' ? broad('Full verification explicitly requested.')
    : await comparison(env.GITHUB_EVENT_NAME, payload, env, env.BROWSER_WORKFLOW);
  const args = range.full ? ['--full'] : ['--base', range.base, '--head', range.head];
  const childEnv = { ...env };
  delete childEnv.GITHUB_TOKEN;
  const child = spawnSync(process.execPath, ['tools/browser-tests/run.mjs', '--json', ...args],
    { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024, env: childEnv });
  if (child.error) throw child.error;
  if (child.status !== 0) throw new Error(`Browser selector failed: ${child.stderr}`);
  const plan = JSON.parse(child.stdout);
  const full = plan.full, rust = full || plan.filters.length > 0;
  const elapsed = Math.round(performance.now() - started);
  const result = { range, plan, selectionMs: elapsed };
  writeFileSync(join(env.RUNNER_TEMP, 'devpage-browser-selection.json'), JSON.stringify(result, null, 2));
  appendFileSync(env.GITHUB_OUTPUT, Object.entries({
    full, rust, smoke: plan.smoke, base: range.base ?? '', head: range.head ?? '',
  }).map(([key, value]) => `${key}=${value}\n`).join(''));
  const detail = [
    range.reason ?? `Comparing ${range.base} to ${range.head}`,
    `Rust browser suites: ${full ? 'full' : plan.suites.join(', ') || 'none'}`,
    `Deploy-server PDF smoke: ${plan.smoke ? 'selected' : 'unaffected'}`,
    `Selection including baseline lookup: ${elapsed} ms`,
    ...plan.reasons.slice(0, 80),
  ];
  if (plan.reasons.length > 80) detail.push(`… ${plan.reasons.length - 80} more reasons`);
  appendFileSync(env.GITHUB_STEP_SUMMARY, `### Browser check selection\n\n<pre>${escapeHtml(detail.join('\n').slice(0, 40000))}</pre>\n`);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  await report(process.env);
}
