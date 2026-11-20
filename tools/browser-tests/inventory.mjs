// Enumerate the same ignored binary tests as the full gate, without launching
// Chrome. Compare before applying runtime capability filters such as --skip gpu.
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../../', import.meta.url));
const project = JSON.parse(readFileSync(new URL('../../project.json', import.meta.url)));
const filters = Object.values(project.targets).flatMap(target => target.metadata?.browserFilters ?? []);
const listing = execFileSync('cargo', ['test', '--locked', '--bin', 'devpage', '--', '--list', '--ignored'], {
  cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'inherit'],
});
const names = listing.split(/\r?\n/).filter(line => line.endsWith(': test')).map(line => line.slice(0, -6));
if (!names.length) throw new Error('Cargo listed no ignored binary tests.');
const unowned = names.filter(name => !filters.some(filter => name.includes(filter)));
const stale = filters.filter(filter => !names.some(name => name.includes(filter)));
if (unowned.length || stale.length) {
  for (const name of unowned) console.error('No suite owns ignored test:', name);
  for (const filter of stale) console.error('No ignored test matches suite filter:', filter);
  process.exitCode = 1;
} else {
  console.log(`All ${names.length} ignored binary tests match a registered suite; all ${filters.length} filters select tests.`);
}
