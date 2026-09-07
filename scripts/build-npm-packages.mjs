/**
 * Assembles the per-platform npm packages that let the Rust binary ship through
 * the channel the users are already in.
 *
 * The README's headline install is `npx useful-system-monitor`, and the badge,
 * the sponsors and every existing instruction point at npm. A Rust rewrite that
 * moved to Homebrew would be a new product with the same name. So the binary
 * ships the way esbuild, swc and biome ship theirs: one small package per
 * platform, declared as `optionalDependencies`, and a launcher in the main
 * package that resolves whichever one npm actually installed.
 *
 * Why `optionalDependencies` and not a postinstall download:
 *
 *   - npm resolves them against `os` and `cpu`, so a Mac fetches ~3.5 MB and
 *     not the other three. A postinstall script would fetch at install time
 *     from somewhere that has to stay up forever, and would be blocked outright
 *     by `--ignore-scripts`, which security-conscious installs and most CI
 *     images set.
 *   - The tarballs are content-addressed by the registry, so the lockfile
 *     covers the binary too. A downloaded artefact is outside the lockfile and
 *     outside `npm audit signatures`.
 *   - "Optional" is doing real work: on an unsupported platform the install
 *     succeeds and the launcher explains itself, rather than the whole
 *     dependency tree failing to install.
 *
 * Run: npm run build:npm-packages
 */
import { chmodSync, copyFileSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const pkg = require('../package.json');
const root = fileURLToPath(new URL('..', import.meta.url));

/** The four platforms the binary supports, and where cargo puts each build. */
export const TARGETS = [
  { rust: 'aarch64-apple-darwin', os: 'darwin', cpu: 'arm64' },
  { rust: 'x86_64-apple-darwin', os: 'darwin', cpu: 'x64' },
  { rust: 'aarch64-unknown-linux-gnu', os: 'linux', cpu: 'arm64' },
  { rust: 'x86_64-unknown-linux-gnu', os: 'linux', cpu: 'x64' },
];

export const packageNameFor = (target) => `${pkg.name}-${target.os}-${target.cpu}`;

function buildOne(target, outDir) {
  const binary = `${root}target/${target.rust}/release/sysmon`;
  if (!existsSync(binary)) return { ...target, built: false };

  const dir = `${outDir}/${packageNameFor(target)}`;
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(`${dir}/bin`, { recursive: true });
  copyFileSync(binary, `${dir}/bin/sysmon`);
  // npm preserves the mode in the tarball, and a binary that is not executable
  // is a very confusing install failure.
  chmodSync(`${dir}/bin/sysmon`, 0o755);

  writeFileSync(
    `${dir}/package.json`,
    `${JSON.stringify(
      {
        name: packageNameFor(target),
        version: pkg.version,
        description: `${pkg.name} binary for ${target.os} ${target.cpu}`,
        license: pkg.license,
        repository: pkg.repository,
        // npm reads these to decide whether to install the package at all, so a
        // Mac never downloads the Linux binaries.
        os: [target.os],
        cpu: [target.cpu],
        files: ['bin'],
        // Deliberately no `bin` field: the binary is resolved and executed by
        // the launcher in the main package, not linked into the user's PATH
        // four times under four names.
      },
      null,
      2,
    )}\n`,
  );
  return { ...target, built: true, dir };
}

function main() {
  const outDir = `${root}npm`;
  mkdirSync(outDir, { recursive: true });
  const results = TARGETS.map((t) => buildOne(t, outDir));

  // The stanza the main package needs once the bin is switched over. Written
  // out rather than edited in, because flipping `bin` is the cutover and that
  // is a decision, not a build step.
  const optional = Object.fromEntries(
    TARGETS.map((t) => [packageNameFor(t), pkg.version]),
  );
  writeFileSync(
    `${outDir}/optional-dependencies.json`,
    `${JSON.stringify({ optionalDependencies: optional }, null, 2)}\n`,
  );

  for (const r of results) {
    process.stdout.write(
      `${r.built ? 'built  ' : 'skipped'} ${packageNameFor(r).padEnd(40)} ${r.rust}\n`,
    );
  }
  const missing = results.filter((r) => !r.built);
  if (missing.length) {
    process.stdout.write(
      `\n${missing.length} target(s) had no binary. Cross-compiling to Linux needs a\n` +
        'linker for that target, so CI builds those on a Linux runner:\n' +
        missing.map((r) => `  cargo build --release --target ${r.rust}`).join('\n') +
        '\n',
    );
  }
}

main();
