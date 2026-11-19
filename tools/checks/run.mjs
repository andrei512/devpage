import { spawnSync } from 'node:child_process';
import { readFileSync, existsSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';
import { changedFiles, selectFiles, fullPlan, cargoArgs } from '../browser-tests/run.mjs';
import { nonRustPlan, changedSpecReferences } from './nonrust.mjs';
import { rustPlan } from './rust.mjs';

const root = fileURLToPath(new URL('../../', import.meta.url));
const scopes = ['rust', 'live', 'docs', 'browser', 'tooling'];

export function makePlan(files, options = {}) {
  const {unknown, ...checks} = nonRustPlan(files, options);
  const reason = options.reason || (options.full ? 'Full verification requested.' :
    unknown.length ? 'Unclassified paths: ' + unknown.join(', ') : null);
  const full = Boolean(reason);
  const rust = rustPlan(files, {full});
  const browser = options.nonBrowser || options.buildOnly || (options.scope && options.scope !== 'browser')
    ? null : full ? fullPlan(files, reason) : selectFiles(files);
  return {files, full, reason, rust, ...checks, browser};
}

// Reconstruct argv from decisions for the executing host. No shell evaluates
// paths, and CI never executes a serialized command string from a planning job.
export function commandsFor(plan, options = {}) {
  const platform = options.platform ?? process.platform;
  const selected = scope => !options.scope || options.scope === scope;
  const {rust, live, docs} = plan;
  const commands = [];
  const add = (argv, cwd = '.') => commands.push({argv, cwd});
  const owners = options.buildOnly ? rust.buildPackages : rust.packages;
  const packages = rust.workspace ? ['--workspace'] : owners.flatMap(name => {
    if (!/^[A-Za-z0-9][\w-]*$/.test(name)) throw new Error('Invalid Cargo package: ' + name);
    return ['-p', name];
  });
  if (options.buildOnly) {
    if (packages.length) add(['cargo', 'build', '--locked', ...packages]);
    return commands;
  }
  if (selected('rust') && packages.length) {
    if (rust.format) add(['cargo', 'fmt', ...(rust.workspace ? ['--all'] : packages), '--check']);
    add(['cargo', 'clippy', '--locked', ...packages, '--all-targets', '--', '-D', 'warnings']);
    add(['cargo', 'test', '--locked', ...packages]);
  }

  // Both hosted platforms run the affected Rust tests. Platform-neutral
  // docs, live and Node tooling checks run once on Linux.
  if (!options.ci || platform === 'linux') {
    if (selected('live')) {
      if (live.typecheck) add(['bun', 'run', 'tsc', '--project', 'tsconfig.json'], 'live');
      if (live.tests !== null) {
        if (!live.tests.every(name => /^\.\/[^/]+\.test\.ts$/.test(name))) throw new Error('Invalid live test path');
        add(['bun', 'test', ...live.tests], 'live');
      }
      if (live.evals) {
        // Construct each case with its recorded session and sandbox scripts
        // using the eval runner's offline API. No model call or grading occurs.
        add(['bun', '-e', 'import { CASES } from "./evals/cases.ts"; import { moment } from "./eval.ts"; for (const c of CASES) await moment(c); console.log(CASES.length, "eval cases validated");'], 'live');
      }
    }
    if (selected('docs')) {
      const coreAlreadyTested = selected('rust') && (rust.workspace || rust.packages.includes('devpage-core'));
      if (docs.filters.length && !coreAlreadyTested) {
        if (!docs.filters.every(name => /^\w+$/.test(name))) throw new Error('Invalid docs test filter');
        add(['cargo', 'test', '--locked', '-p', 'devpage-core', '--', ...docs.filters]);
      }
      if (docs.check) add(['cargo', 'run', '--locked', '-p', 'devpage', '--', 'check', 'docs', '--docs']);
      if (docs.requirements) add(['cargo', 'run', '--locked', '-p', 'devpage', '--', 'check', 'docs', '--requirements-map']);
    }
    if (selected('tooling') && plan.tooling) {
      add(['node', '--test', 'tools/browser-tests/*.test.mjs', 'tools/checks/*.test.mjs']);
    }
  }
  if (selected('browser') && !options.nonBrowser && plan.browser) {
    const args = cargoArgs(plan.browser, platform);
    if (args) add(['cargo', ...args]);
  }
  return commands;
}

function execute({argv, cwd}) {
  const [name, ...args] = argv;
  if (cwd === 'live' && !existsSync(join(root, 'live/node_modules/typescript/package.json'))) {
    throw new Error('Live checks need their locked dependencies. Run bun install --frozen-lockfile in live/.');
  }
  const result = spawnSync(name === 'node' ? process.execPath : name, args, {cwd: join(root, cwd), stdio: 'inherit'});
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${name} exited with ${result.status ?? result.signal}`);
}

function main() {
  const {values} = parseArgs({options: {
    help: {type:'boolean', short:'h'}, base: {type:'string'}, head: {type:'string'}, explain: {type:'boolean'}, json: {type:'boolean'},
    full: {type:'boolean'}, ci: {type:'boolean'}, scope: {type:'string'}, platform: {type:'string'},
    'build-only': {type:'boolean'}, 'non-browser': {type:'boolean'}, 'plan-file': {type:'string'},
  }});
  if (values.help) {
    return console.log(`Usage:
  npm run check [-- OPTIONS]   Run affected Rust, live, docs, tooling and browser checks
  npm run build [-- OPTIONS]   Build the affected Cargo packages

  --explain                   Print the selected commands without running them
  --json                      Emit the plan as JSON
  --base REF                  Compare from the merge base; include local edits
  --base BASE --head HEAD     Compare the specified committed range
  --full                      Select full coverage
  --scope NAME                Restrict checks to rust, live, docs, tooling or browser

The default checks staged, unstaged and untracked changes since HEAD.
Use npm run check -- --base origin/main before a pull request to main.`);
  }
  const options = {...values, buildOnly:values['build-only'], nonBrowser:values['non-browser']};
  if (options.buildOnly && options.scope && options.scope !== 'rust') throw new Error('The artifact build accepts Rust packages.');
  if (options.scope === 'browser' && options.nonBrowser) throw new Error('--scope browser conflicts with --non-browser');
  if (options.head && !options.base) throw new Error('--head requires --base');
  if (options.base && options.full) throw new Error('Choose --full or a comparison range');
  if (options.scope && !scopes.includes(options.scope)) throw new Error('--scope accepts ' + scopes.join(', '));
  if (options.platform && !['linux','darwin','win32'].includes(options.platform)) throw new Error('Unsupported platform');
  let plan;
  if (values['plan-file']) {
    if (!options.ci || !options.nonBrowser || options.full || options.base) {
      throw new Error('--plan-file requires --ci --non-browser and a saved CI plan');
    }
    plan = JSON.parse(readFileSync(values['plan-file'], 'utf8'));
  } else {
    const diff = options.full ? {files:[]} : changedFiles(options.base, options.head);
    plan = makePlan(diff.files, {...options, reason:diff.fallback, full:options.full || Boolean(diff.fallback),
      specChanges:changedSpecReferences(diff, !options.head)});
  }
  const commands = commandsFor(plan, options);
  if (options.json) return console.log(JSON.stringify({...plan, commands}));
  if (plan.reason) console.log(plan.reason);
  if (options.explain && plan.files.length) console.log('Changed files:', plan.files.join(', '));
  for (const command of commands) console.log((command.cwd === '.' ? '' : '[' + command.cwd + '] ') + command.argv.join(' '));
  if (!commands.length) console.log('No affected commands.');
  if (options.explain) {
    if (plan.browser) for (const line of plan.browser.reasons) console.log(line);
    console.log('CI clean install:', plan.install ? 'selected' : 'unaffected');
    if (plan.browser) console.log('Deploy-server PDF smoke:', plan.browser.smoke ? 'selected' : 'unaffected');
    return;
  }
  for (const command of commands) execute(command);
}

if (import.meta.main) {
  try { main(); } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
