import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

const root = fileURLToPath(new URL('../../', import.meta.url));
const readJson = (path) => JSON.parse(readFileSync(path, 'utf8'));
const project = readJson(join(root, 'project.json'));
const nxConfig = readJson(join(root, 'nx.json'));

function run(command, args, options = {}) {
  const result = spawnSync(command, args, { cwd: root, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024, ...options });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(`${command} exited with ${result.status ?? result.signal}: ${result.stderr || result.stdout || ''}`);
  }
  return result.stdout;
}

function gitPaths(args, cwd) {
  return run('git', args, {cwd}).split('\0').filter(Boolean);
}

function commit(ref, cwd) {
  const result = spawnSync('git', ['rev-parse', '--verify', '--quiet', '--end-of-options', ref + '^{commit}'], {cwd, encoding: 'utf8'});
  if (result.error) throw result.error;
  return result.status === 0 ? result.stdout.trim() : null;
}

// --no-renames reports both the removed and added path. -z keeps whitespace and
// newlines intact until the Nx protocol boundary.
export function changedFiles(base = nxConfig.defaultBase, head, cwd = root) {
  const from = commit(base, cwd);
  const to = commit(head ?? 'HEAD', cwd);
  if (!from || !to) return {files: [], fallback: `Comparison history unavailable: ${base} to ${head ?? 'HEAD'}`};
  const merge = spawnSync('git', ['merge-base', from, to], {cwd, encoding: 'utf8'});
  if (merge.error) throw merge.error;
  if (merge.status !== 0 || (head && merge.stdout.trim() !== from)) {
    return {files: [], fallback: `No valid comparison ancestry: ${base} to ${head ?? 'HEAD'}`};
  }
  const diff = ['diff', '--name-only', '--no-renames', '-z'];
  const files = gitPaths([...diff, merge.stdout.trim(), to, '--'], cwd);
  if (!head) {
    files.push(...gitPaths([...diff, '--cached', to, '--'], cwd));
    files.push(...gitPaths([...diff, '--'], cwd));
    files.push(...gitPaths(['ls-files', '--others', '--exclude-standard', '-z'], cwd));
  }
  return {files: [...new Set(files)].sort()};
}

const targets = project.targets;
const names = Object.keys(targets);
for (const name of names) {
  const {inputs, metadata = {}} = targets[name];
  const { browserFilters, noBrowser, deploySmoke } = metadata;
  const rust = Array.isArray(browserFilters) && browserFilters.length > 0 &&
    browserFilters.every(f => typeof f === 'string' && f.length > 0 && !f.startsWith('-'));
  if (!inputs?.length || [rust, noBrowser === true, deploySmoke === true].filter(Boolean).length !== 1) {
    throw new Error(`Invalid inputs or coverage metadata for ${name}`);
  }
}
const rustSuites = names.filter(name => targets[name].metadata.browserFilters);
if (!rustSuites.length) throw new Error('Missing browser test suites in project.json');

function plan(files, suites = [], smoke = false, reasons = [], full = false) {
  const filters = [...new Set(suites.flatMap(name => targets[name].metadata.browserFilters))];
  return {files, suites, filters, smoke, full, reasons};
}

function fullPlan(files, reason) {
  return plan(files, rustSuites, true, [reason], true);
}

export function selectFiles(files) {
  if (!files.length) return plan(files);
  if (files.some(file => /[\r\n]/.test(file))) {
    return fullPlan(files, 'A path contains a newline and cannot be passed to Nx on a line of stdin.');
  }

  const require = createRequire(import.meta.url);
  const nxPackage = require.resolve('nx/package.json');
  const nxCli = join(dirname(nxPackage), readJson(nxPackage).bin.nx);
  const explanation = JSON.parse(run(process.execPath,
    [nxCli, 'affected', '-t', names.join(','), '--stdin', '--explain=stdout'],
    {input: files.join('\n') + '\n', env: {...process.env, NX_DAEMON: 'false', NX_NO_CLOUD: 'true', FORCE_COLOR: '0'}}));
  if (explanation.requested?.total !== names.length) throw new Error('Nx did not evaluate every configured target');
  const accounted = new Set();
  const suites = [];
  const reasons = [];
  let smoke = false;
  for (const [task, matches] of Object.entries(explanation.affected)) {
    const name = task.slice(project.name.length + 1);
    if (!task.startsWith(project.name + ':') || !names.includes(name)) throw new Error(`Unexpected Nx task: ${task}`);
    const metadata = targets[name].metadata;
    if (metadata.browserFilters) suites.push(name);
    if (metadata.deploySmoke) smoke = true;
    for (const match of matches) {
      if (match.file) accounted.add(match.file);
      reasons.push(`${match.file ?? match.kind} → ${name}`);
    }
  }
  const unknown = files.filter(file => !accounted.has(file));
  if (unknown.length) return fullPlan(files, `Unclassified changes require full coverage: ${unknown.join(', ')}`);
  return plan(files, suites.sort(), smoke, reasons);
}

export function cargoArgs(selection, platform = process.platform) {
  if (!selection.full && !selection.filters.length) return null;
  const args = ['test', '--locked', '--bin', 'devpage', '--', '--ignored'];
  if (platform === 'win32') args.push('--skip', 'gpu');
  if (!selection.full) args.push(...selection.filters);
  return args;
}

function main() {
  const { values } = parseArgs({options: {
    base: {type: 'string'}, head: {type: 'string'}, suite: {type: 'string'},
    full: {type: 'boolean'}, explain: {type: 'boolean'}, json: {type: 'boolean'},
  }});
  if (values.head && !values.base) throw new Error('--head requires --base');
  if ((values.full || values.suite) && (values.base || values.head) || values.full && values.suite) {
    throw new Error('Choose a diff, --full, or --suite');
  }
  let selection;
  if (values.full) selection = fullPlan([], 'Full coverage requested.');
  else if (values.suite) {
    if (!rustSuites.includes(values.suite)) throw new Error(`Unknown Rust suite: ${values.suite}`);
    selection = plan([], [values.suite], false, ['Suite explicitly requested.']);
  } else {
    const diff = changedFiles(values.base, values.head);
    selection = diff.fallback ? fullPlan(diff.files, diff.fallback) : selectFiles(diff.files);
  }

  if (values.json) return console.log(JSON.stringify(selection));
  for (const reason of selection.reasons) console.log(reason);
  console.log('Rust browser coverage:', selection.full ? 'full' : selection.suites.join(', ') || 'none');
  console.log('Deploy-server PDF smoke:', selection.smoke ? 'selected' : 'unaffected');
  if (values.explain) return;
  const args = cargoArgs(selection);
  if (args) run('cargo', args, {stdio: 'inherit'});
}

if (import.meta.main) {
  try { main(); } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
