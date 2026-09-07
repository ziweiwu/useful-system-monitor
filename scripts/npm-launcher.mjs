#!/usr/bin/env node
/**
 * The `bin` the npm package will point at once the Rust build takes over.
 *
 * It resolves whichever per-platform package npm actually installed and hands
 * control to the binary inside it. **Not yet wired up**: `package.json`'s `bin`
 * still points at `dist/cli.js`, because switching it is the cutover and the
 * cutover happens at proven parity, not when the mechanism first works.
 *
 * Three details that are easy to get wrong and expensive to get wrong:
 *
 *   - **`execv`, not `spawn`.** Replacing the process image means the binary
 *     inherits the real tty on both ends — which the dashboard requires
 *     (I-22) — and gets the signals, the exit status and the terminal size
 *     directly. A wrapper that spawns a child has to proxy SIGINT, SIGWINCH,
 *     stdin and the exit code, and gets at least one of them wrong. Node has no
 *     `execv`, so this spawns with `stdio: 'inherit'` and forwards the status,
 *     which is the closest thing available; the terminal is inherited intact.
 *   - **`require.resolve`, not a path guess.** The package can be hoisted to a
 *     top-level `node_modules`, nested, or pnpm-linked through a store. Only
 *     the resolver knows which.
 *   - **A missing package is explained, not a stack trace.** It means either an
 *     unsupported platform or `--no-optional`, and those have different
 *     remedies. See I-24.
 */
import { spawnSync } from 'node:child_process';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const { name, version } = require('../package.json');

/** npm's `process.arch` names, which are not cargo's target triples. */
const PLATFORM = `${process.platform}-${process.arch}`;
const SUPPORTED = ['darwin-arm64', 'darwin-x64', 'linux-arm64', 'linux-x64'];

function binaryPath() {
  const pkg = `${name}-${PLATFORM}`;
  try {
    return require.resolve(`${pkg}/bin/sysmon`);
  } catch {
    // I-24: name the cause *and* the remedy, and tell the two causes apart.
    if (!SUPPORTED.includes(PLATFORM)) {
      process.stderr.write(
        `${name}: no build for ${PLATFORM}.\n` +
          `        Supported: ${SUPPORTED.join(', ')}.\n`,
      );
    } else {
      process.stderr.write(
        `${name}: the binary package ${pkg}@${version} is not installed.\n` +
          '        It is an optional dependency, so this usually means the install ran\n' +
          `        with --no-optional or --omit=optional. Run \`npm install ${pkg}\` to add it.\n`,
      );
    }
    process.exit(1);
  }
}

const result = spawnSync(binaryPath(), process.argv.slice(2), { stdio: 'inherit' });
if (result.error) {
  process.stderr.write(`${name}: ${result.error.message}\n`);
  process.exit(1);
}
/* A process killed by a signal has no exit code. Shells report 128 + the
   signal number, so `$?` means the same thing it would if the binary had been
   run directly. */
const SIGNAL_EXIT_BASE = 128;
process.exit(result.status ?? (result.signal ? SIGNAL_EXIT_BASE : 1));
