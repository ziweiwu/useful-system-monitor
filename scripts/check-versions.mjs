/*
 * package.json, the sysmon crate and any optionalDependencies must all carry
 * the same version.
 *
 * `--version` is the shape marker a consumer reads (I-25), and after the
 * cutover it comes from the *binary* — `env!("CARGO_PKG_VERSION")` — while the
 * registry, the tag check and the launcher's error messages all read
 * package.json. Nothing else compares them, so a release that bumped one and
 * not the other would publish a package whose `--version` disagreed with its
 * own metadata, and every jq pipeline keyed on that string would be reading a
 * number the registry never published.
 *
 * The optional dependencies are pinned to the exact version rather than a
 * range, so they need the same treatment: a stale pin resolves to the previous
 * release's binary and the launcher runs the wrong build without a word. They
 * are absent from the committed package.json and injected at publish time (see
 * scripts/apply-optional-deps.mjs), so this checks them when they are there and
 * says so when they are not — running before and after the injection are both
 * useful, and neither should be silently vacuous.
 */
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('..', import.meta.url));
const pkg = JSON.parse(readFileSync(`${root}package.json`, 'utf8'));

const cargo = readFileSync(`${root}crates/sysmon/Cargo.toml`, 'utf8');
/* The first `version = "…"` under [package]. Matching anywhere would find the
   dependency versions further down and compare against one of those. */
const crateVersion = /\[package\][\s\S]*?^version\s*=\s*"([^"]+)"/m.exec(cargo)?.[1];

const problems = [];
if (!crateVersion) {
  problems.push('could not read version from crates/sysmon/Cargo.toml');
} else if (crateVersion !== pkg.version) {
  problems.push(`crates/sysmon/Cargo.toml is ${crateVersion}, package.json is ${pkg.version}`);
}

for (const [name, range] of Object.entries(pkg.optionalDependencies ?? {})) {
  if (range !== pkg.version) {
    problems.push(`optionalDependencies["${name}"] is ${range}, package.json is ${pkg.version}`);
  }
}

if (problems.length) {
  process.stderr.write(`versions disagree:\n${problems.map((p) => `  ${p}\n`).join('')}`);
  process.exit(1);
}
const pins = Object.keys(pkg.optionalDependencies ?? {}).length;
const where = pins ? `the crate and all ${pins} binary pins` : 'the crate (no binary pins yet)';
process.stdout.write(`versions ok — ${pkg.version} in package.json and ${where}\n`);
