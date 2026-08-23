/*
 * The only check that mounts the real dashboard from the built binary.
 *
 * Everything else stops short of it. The test suite imports `App` and renders
 * it with ink-testing-library. `verify:smoke` runs `dist/cli.js --json`, which
 * returns from `oneShot` before `render()` is ever reached. `verify:longrun`
 * measures a harness that imports `App` from source. So the built program's
 * mount path — the launcher, prod-env, React, ink's reconciler, every
 * component, in one process — was covered by nothing at all.
 *
 * That gap is not hypothetical. A build that paired React's *production* core
 * with the development JSX runtime (`jsxDEV`, which the production build
 * exports as `undefined`) mounted nothing, drew nothing, and exited 0 after
 * half a second having written six bytes. `verify:smoke` passed it, because
 * --json never renders. `verify:longrun` would have passed it too, and reported
 * a beautifully flat heap, because an app that never draws never allocates.
 * See I-10b.
 *
 * So this asserts the things that failure broke, hardest-to-fake first:
 *
 *   1. it mounts and draws a recognisable dashboard;
 *   2. it is still alive several seconds later;
 *   3. it is still *drawing* — which is what separates a live app from one
 *      whose scheduler stopped after the first commit, and is invisible to
 *      every other check here;
 *   4. it answers a keypress, the only exercise raw mode and `useInput` get
 *      outside a human's hands;
 *   5. it quits on `q` instead of hanging.
 *
 * A SHIM, NOT A PTY, and deliberately. `/usr/bin/script` refuses the socketpair
 * Node's `spawn` gives it ("tcgetattr/ioctl: Operation not supported"), and
 * feeding it a real pipe instead made the run hang rather than exit. A pty
 * would add fidelity in termios handling, which is ink's code and not this
 * project's, at the cost of a check that is flaky on the one machine that
 * matters — CI. `scripts/tty-shim.mjs` fakes exactly what I-22 tests and what
 * ink needs for raw mode; everything above that line is the real built code.
 * `--mock`, so the result does not depend on what the machine is doing.
 */
import { spawn } from 'node:child_process';
import { existsSync } from 'node:fs';

const CLI = process.argv[2] ?? 'dist/cli.js';
const SHIM = new URL('./tty-shim.mjs', import.meta.url).pathname;

/* Generous, because these wait on a real process on a possibly loaded CI box.
   Each deadline means "it never happened", never "it was slow". */
const MOUNT_TIMEOUT_MS = 30_000;
/* Startup keeps drawing for a moment after the first dashboard appears — five
   collectors resolve at their own pace, then the priming sample lands. Measuring
   through that tail counted a *dead* app as alive, because its launch frames
   were still arriving. Settle first, then measure. */
const SETTLE_MS = 3_000;
const DRAW_WINDOW_MS = 4_000;
const KEY_TIMEOUT_MS = 10_000;
const QUIT_TIMEOUT_MS = 10_000;

const ESC = String.fromCharCode(27);
const ANSI = new RegExp(`${ESC}\\[[0-9;?]*[a-zA-Z]`, 'g');
const plain = (s: string) => s.replace(ANSI, '');
const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

if (!existsSync(CLI)) {
  process.stderr.write(`verify:tui: ${CLI} does not exist — run \`npm run build\` first\n`);
  process.exit(1);
}

const main = async () => {
  const child = spawn(
    process.execPath,
    ['--import', SHIM, CLI, '--mock', '--interval', '1'],
    /* stdin stays open for the whole run: a closed stdin makes ink unmount at
       once, which looks exactly like the failure this exists to detect. */
    { stdio: ['pipe', 'pipe', 'inherit'], env: { ...process.env, TUI_COLS: '100', TUI_ROWS: '36' } },
  );

  let out = '';
  child.stdout.setEncoding('utf8');
  child.stdout.on('data', (c: string) => {
    out += c;
  });

  let exited: number | null = null;
  child.on('exit', (code) => {
    exited = code ?? 0;
  });

  const fail = (msg: string, extra = ''): never => {
    if (exited === null) child.kill('SIGKILL');
    process.stderr.write(`\nverify:tui: ${msg}\n`);
    if (extra) process.stderr.write(extra.split('\n').slice(-25).join('\n') + '\n');
    process.exit(1);
  };

  const settled = async (until: () => boolean, deadlineMs: number) => {
    const deadline = Date.now() + deadlineMs;
    while (Date.now() < deadline && !until()) await wait(100);
    return until();
  };

  // 1. It mounts and draws a recognisable dashboard.
  const drawn = () => plain(out);
  const mounted = () =>
    drawn().includes('useful-system-monitor') && drawn().includes('PID') && drawn().includes('CPU');
  await settled(() => mounted() || exited !== null, MOUNT_TIMEOUT_MS);
  if (exited !== null && !mounted()) {
    fail(
      `it exited (code ${exited}) after drawing ${out.length} bytes, without ever mounting.\n` +
        '        A React core and JSX runtime from different builds fail exactly this way.',
      drawn(),
    );
  }
  if (!mounted()) fail(`no dashboard within ${MOUNT_TIMEOUT_MS}ms (${out.length} bytes)`, drawn());
  console.log(`tui: mounted — dashboard drawn in ${out.length} bytes`);

  // 2 and 3. Still alive, and still drawing — after the launch frames drain.
  await wait(SETTLE_MS);
  if (exited !== null) fail(`it exited (code ${exited}) ${SETTLE_MS}ms after mounting`);
  const before = out.length;
  await wait(DRAW_WINDOW_MS);
  if (exited !== null) fail(`it exited (code ${exited}) while running`);
  if (out.length === before) {
    fail(
      `not one frame in ${DRAW_WINDOW_MS}ms at --interval 1, ${SETTLE_MS}ms after mounting.\n` +
        '        It mounted and then stopped committing — which no heap check can see,\n' +
        '        because an app that never redraws never allocates.',
    );
  }
  console.log(`tui: alive — ${out.length - before} more bytes over ${DRAW_WINDOW_MS}ms`);

  /*
   * 4. It answers a keypress.
   *
   * The assertion is the *active* tab marker, `[3 MEMORY]`, not the word
   * MEMORY: every tab label is on screen at all times, so matching the bare
   * word passed instantly against the frame already drawn, before the key had
   * been read at all — a check that could not fail. Only frames written after
   * the keystroke are searched, for the same reason. See I-23 for the brackets.
   */
  const mark = out.length;
  child.stdin.write('3');
  const switched = () => plain(out.slice(mark)).includes('[3 MEMORY]');
  if (!(await settled(switched, KEY_TIMEOUT_MS))) {
    fail('it did not respond to `3` — raw mode or useInput is broken', drawn().slice(-2_000));
  }
  console.log('tui: responsive — `3` switched to the memory screen');

  // 5. It quits, rather than hanging on a terminal it cannot restore.
  child.stdin.write('q');
  if (!(await settled(() => exited !== null, QUIT_TIMEOUT_MS))) {
    fail(`it did not quit within ${QUIT_TIMEOUT_MS}ms of \`q\``);
  }
  if (exited !== 0) fail(`it quit with code ${exited}, expected 0`);
  console.log('tui: quit cleanly on `q`');

  console.log('\ntui: PASS — the built binary mounts, draws, responds and exits');
  process.exit(0);
};

void main();
