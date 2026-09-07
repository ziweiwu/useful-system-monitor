import { execFileSync, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, rmSync, writeFileSync, cpSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';

/**
 * The launcher that will become the npm `bin` once the Rust build takes over.
 *
 * It is worth testing before it is wired up, because every one of its failure
 * modes appears *at install time on someone else's machine* — an unsupported
 * platform, an install run with `--omit=optional`, a hoisted or pnpm-linked
 * `node_modules`. None of those are reproducible after the fact from a bug
 * report saying "it doesn't work".
 */
const root = fileURLToPath(new URL('..', import.meta.url));
const launcher = join(root, 'scripts/npm-launcher.mjs');
const platformPkg = join(root, 'npm/useful-system-monitor-darwin-arm64');

/** A throwaway tree shaped the way npm would leave one. */
let sandbox: string;

/**
 * Only runs where the binary for this machine has actually been built —
 * `npm run build:npm-packages` after a release build. Skipping is the honest
 * outcome: asserting against a binary that is not there would test nothing.
 */
const built = process.platform === 'darwin' && process.arch === 'arm64' && existsSync(platformPkg);

beforeAll(() => {
  sandbox = join(tmpdir(), `usm-launcher-${process.pid}`);
  rmSync(sandbox, { recursive: true, force: true });
  mkdirSync(join(sandbox, 'node_modules'), { recursive: true });
  // The launcher reads `../package.json` for the name and version, so the
  // sandbox has to be shaped like the real package.
  writeFileSync(
    join(sandbox, 'package.json'),
    JSON.stringify({ name: 'useful-system-monitor', version: '0.9.0' }),
  );
  mkdirSync(join(sandbox, 'scripts'), { recursive: true });
  cpSync(launcher, join(sandbox, 'scripts/npm-launcher.mjs'));
});

afterAll(() => rmSync(sandbox, { recursive: true, force: true }));

const run = (args: string[]) =>
  spawnSync(process.execPath, [join(sandbox, 'scripts/npm-launcher.mjs'), ...args], {
    encoding: 'utf8',
    cwd: sandbox,
  });

describe('I-24: the npm launcher explains itself rather than throwing', () => {
  it('names the remedy when the optional binary package is absent', () => {
    // The common real cause: `npm install --omit=optional`, or a CI image that
    // sets it globally. The message has to name the package and the fix.
    const r = run(['--version']);
    expect(r.status).toBe(1);
    expect(r.stderr).toContain('useful-system-monitor-darwin-arm64');
    expect(r.stderr).toMatch(/optional/i);
    expect(r.stderr).toContain('npm install');
    // A stack trace would be the wrong answer here.
    expect(r.stderr).not.toContain('Error:');
    expect(r.stderr).not.toContain('at ');
  });
});

describe.skipIf(!built)('the launcher hands over to the platform binary', () => {
  beforeAll(() => {
    cpSync(platformPkg, join(sandbox, 'node_modules/useful-system-monitor-darwin-arm64'), {
      recursive: true,
    });
  });

  it('resolves the package npm installed and runs it', () => {
    const r = run(['--version']);
    expect(r.status).toBe(0);
    expect(r.stdout.trim()).toBe('0.9.0');
  });

  it('passes arguments through and forwards the exit status', () => {
    expect(run(['--help']).status).toBe(0);
    expect(run(['--help']).stdout).toContain('Usage');
    // I-24: exit 2 is "bad usage", and it has to survive the hand-over —
    // a wrapper that collapses every failure to 1 loses the distinction a
    // script depends on.
    const bad = run(['--nonsense']);
    expect(bad.status).toBe(2);
    expect(bad.stderr).toContain('unknown option');
  });

  it('produces the same JSON shape as the Node build', () => {
    const viaLauncher = JSON.parse(run(['--json']).stdout);
    const viaNode = JSON.parse(
      execFileSync(process.execPath, [join(root, 'dist/cli.js'), '--json'], {
        encoding: 'utf8',
      }),
    );
    expect(Object.keys(viaLauncher).toSorted()).toEqual(Object.keys(viaNode).toSorted());
    expect(Object.keys(viaLauncher.memory).toSorted()).toEqual(
      Object.keys(viaNode.memory).toSorted(),
    );
    expect(Object.keys(viaLauncher.processes[0]).toSorted()).toEqual(
      Object.keys(viaNode.processes[0]).toSorted(),
    );
  });
});
