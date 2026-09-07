/*
 * Writes the per-platform binary packages into package.json as
 * `optionalDependencies`, immediately before publishing.
 *
 * They are not committed, and the reason is a bootstrap the pattern cannot
 * avoid: the pins are exact, so a lockfile can only contain them once those
 * versions exist on the registry — and they do not exist until this release
 * publishes them. Committing the stanza therefore breaks `npm ci` for every
 * contributor and every CI run, on a version that by definition is not out yet.
 * `npm ci` is not optional here; it is what keeps the lockfile honest.
 *
 * So the repository holds a package.json a contributor can install, and the
 * published one gains the four dependencies a user needs. The release workflow
 * runs this after `build:npm-packages` (which writes the stanza) and before
 * `npm publish`. `npm run verify:versions` runs after it, so the pins are still
 * checked against the version actually being published.
 *
 * Run: node scripts/apply-optional-deps.mjs
 */
import { readFileSync, writeFileSync, existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('..', import.meta.url));
const stanzaPath = `${root}npm/optional-dependencies.json`;

if (!existsSync(stanzaPath)) {
  process.stderr.write(
    'apply-optional-deps: npm/optional-dependencies.json is missing.\n' +
      '        Run `npm run build:npm-packages` first — it writes the stanza\n' +
      '        alongside the per-platform packages it assembles.\n',
  );
  process.exit(1);
}

const { optionalDependencies } = JSON.parse(readFileSync(stanzaPath, 'utf8'));
const pkgPath = `${root}package.json`;
const pkg = JSON.parse(readFileSync(pkgPath, 'utf8'));

/* Every pin must be the version being published. `build:npm-packages` derives
   them from this same package.json, so a mismatch means the file changed under
   us — worth failing on rather than publishing a package that depends on a
   binary from some other release. */
const wrong = Object.entries(optionalDependencies).filter(([, v]) => v !== pkg.version);
if (wrong.length) {
  process.stderr.write(
    `apply-optional-deps: the stanza pins ${wrong.map(([k, v]) => `${k}@${v}`).join(', ')}, ` +
      `but this package is ${pkg.version}.\n`,
  );
  process.exit(1);
}

pkg.optionalDependencies = optionalDependencies;
writeFileSync(pkgPath, `${JSON.stringify(pkg, null, 2)}\n`);
process.stdout.write(
  `applied ${Object.keys(optionalDependencies).length} optional dependencies at ${pkg.version}\n`,
);
