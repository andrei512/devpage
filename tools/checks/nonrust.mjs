// The Rust package graph and browser targets have their own declared inputs.
// These independent checks cover the live program, documentation and developer tools.
import { existsSync, readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../../', import.meta.url));
const docLinks = 'devpage_docs_have_no_broken_links';
const docFolders = 'no_folder_of_devpage_docs_holds_more_than_ten_subfolders';
const fixtureLinks = 'the_fixture_doc_the_catalog_and_the_talk_have_no_broken_links';
const codeDocs = 'every_item_of_devpages_code_is_documented';

const checkTools = /^(tools\/(?:browser-tests|checks)\/|\.github\/(?:actions\/browser-selection\/|workflows\/(?:gates|deploy-server)\.yml$)|(?:project\.json|nx\.json|package(?:-lock)?\.json|\.node-version)$)/;
const knownDirectory = /^(?:src|core|assets|extension-api|bench|design|tests|examples|live|docs|deploy|\.cargo|\.claude|\.agents|\.github)\//;
const knownRootFile = /^(?:[^/]+\.md|Cargo\.(?:toml|lock)|build\.rs|rust-toolchain(?:\.toml)?|\.?rustfmt\.toml|\.?clippy\.toml|dist-workspace\.toml|install\.(?:sh|ps1)|LICENSE|\.gitattributes|\.gitignore|\.dockerignore)$/;

function liveSelection(files, full) {
  const live = files.filter(file => file.startsWith('live/'));
  if (!full && !live.length) return {typecheck: false, tests: null, evals: false};
  const onlyTestFiles = !full && live.every(file => /^live\/[^/]+\.test\.ts$/.test(file) && existsSync(root + file));
  const evals = full || live.some(file => file === 'live/eval.ts' || file.startsWith('live/evals/'));
  const allEvals = !full && live.every(file => file.startsWith('live/evals/'));
  const tests = onlyTestFiles ? live.map(file => './' + file.slice('live/'.length)) : allEvals ? null : [];
  return {typecheck: true, tests, evals};
}

// The requirements extension reads literal #[spec("…")] lines from Rust sources.
// Inspect changes across the comparison instead of linking the devpage binary
// for every unrelated Rust edit merely to check the same references.
export function changedSpecReferences(diff, local, cwd = root) {
  if (!diff.files.some(file => file.endsWith('.rs'))) return false;
  const ranges = [[diff.base, diff.head]];
  if (local) ranges.push(['--cached', diff.head], []);
  const spec = /#\[spec\("[^"]+"\)\]/;
  for (const range of ranges) {
    const result = spawnSync('git', ['diff', '--no-renames', '--no-ext-diff', '--no-color', '--unified=0',
      ...range, '--', '*.rs'], {cwd, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024});
    if (result.status !== 0) return true;
    if (result.stdout.split('\n').some(line => /^[-+]/.test(line) && spec.test(line))) return true;
  }
  if (local) {
    const result = spawnSync('git', ['ls-files', '--others', '--exclude-standard', '-z', '--', '*.rs'], {cwd,encoding:'utf8'});
    if (result.status !== 0) return true;
    for (const file of result.stdout.split('\0').filter(Boolean)) {
      try { if (spec.test(readFileSync(join(cwd, file), 'utf8'))) return true; }
      catch { return true; }
    }
  }
  return false;
}

export function nonRustPlan(files, {full = false, specChanges = files.some(file => file.endsWith('.rs'))} = {}) {
  const unknown = files.filter(file => !knownDirectory.test(file) && !knownRootFile.test(file) && !checkTools.test(file));
  const broad = full || unknown.length > 0;
  const any = pattern => broad || files.some(file => pattern.test(file));
  const linkedFiles = ['LICENSE', 'install.sh', 'install.ps1', 'core/src/code/rust.scm', 'live/service.ts'];
  const brokenTarget = files.some(file => linkedFiles.includes(file) && !existsSync(join(root, file)));
  const docs = brokenTarget || any(/^CHANGELOG\.md$|^docs\/|^core\/src\/(?:check\.rs|external\.rs|requirements\.rs|document\.rs|markup\.rs|code\/)|^src\/(?:main\.rs|adapters\/external\.rs)$/);
  const filters = new Set();
  if (docs) filters.add(docLinks);
  if (any(/^docs\/.*(?:README\.md|devpage\.yml)$/)) filters.add(docFolders);
  if (any(/^(?:tests\/fixtures\/sync-design\/|docs\/(?:talks\/devpage\/|features\/|devpage\.yml$))/)) filters.add(fixtureLinks);
  if (any(/^docs\/devpage\.yml$|^(?:src|(?:design|extension-api|core)\/src)\/.*\.rs$/)) filters.add(codeDocs);
  // A constraint may be any Markdown page in the document, including a deleted page.
  const requirements = specChanges || any(/^docs\/(?:devpage\.yml$|.*\.md$)/);
  // Packaging and toolchain edits can change a clean cargo install. Ordinary
  // implementation changes are compiled by their affected Rust package checks.
  const install = any(/^(?:install\.(?:sh|ps1)|Cargo\.(?:toml|lock)|(?:core|design|extension-api)\/Cargo\.toml|(?:core\/|design\/)?build\.rs|rust-toolchain(?:\.toml)?|\.cargo\/.*|\.github\/workflows\/gates\.yml)$/);
  return {unknown, live: liveSelection(files, broad), docs: {check: docs, filters: [...filters], requirements}, install, tooling: any(checkTools)};
}
