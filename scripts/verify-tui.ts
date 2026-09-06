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
/** Lines of the child's output to quote when a step fails. */
const FAILURE_CONTEXT_LINES = 25;
/** Bytes of the last frame to quote when a keypress goes unanswered. */
const FAILURE_CONTEXT_BYTES = 2_000;
/** How often to re-check a condition while waiting for the child to settle. */
const POLL_MS = 100;

function plain(text: string): string {
  return text.replace(ANSI, '');
}

function wait(milliseconds: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}

if (!existsSync(CLI)) {
  process.stderr.write(`verify:tui: ${CLI} does not exist — run \`npm run build\` first\n`);
  process.exit(1);
}

/** The spawned dashboard, plus the few things every step below asks of it. */
interface Session {
  /** Everything written so far, ANSI stripped. */
  drawn: () => string;
  /** Raw bytes written so far — the growth check counts these, not glyphs. */
  bytes: () => number;
  /** Everything drawn since a byte offset, ANSI stripped. */
  drawnSince: (offset: number) => string;
  exitCode: () => number | null;
  press: (key: string) => void;
  fail: (message: string, extra?: string) => never;
  /** Polls `until` to a deadline; returns whether it ever came true. */
  settled: (until: () => boolean, deadlineMs: number) => Promise<boolean>;
}

const spawnDashboard = () =>
  spawn(
    process.execPath,
    ['--import', SHIM, CLI, '--mock', '--interval', '1'],
    /* stdin stays open for the whole run: a closed stdin makes ink unmount at
       once, which looks exactly like the failure this exists to detect. */
    { stdio: ['pipe', 'pipe', 'inherit'], env: { ...process.env, TUI_COLS: '100', TUI_ROWS: '36' } },
  );

const startSession = (): Session => {
  const child = spawnDashboard();

  let out = '';
  child.stdout.setEncoding('utf8');
  child.stdout.on('data', (chunk: string) => {
    out += chunk;
  });

  let exited: number | null = null;
  child.on('exit', (code) => {
    exited = code ?? 0;
  });

  return {
    drawn: () => plain(out),
    bytes: () => out.length,
    drawnSince: (offset) => plain(out.slice(offset)),
    exitCode: () => exited,
    press: (key) => child.stdin.write(key),
    fail: (message, extra = ''): never => {
      if (exited === null) child.kill('SIGKILL');
      process.stderr.write(`\nverify:tui: ${message}\n`);
      if (extra) {
        process.stderr.write(extra.split('\n').slice(-FAILURE_CONTEXT_LINES).join('\n') + '\n');
      }
      process.exit(1);
    },
    settled: async (until, deadlineMs) => {
      const deadline = Date.now() + deadlineMs;
      while (Date.now() < deadline && !until()) await wait(POLL_MS);
      return until();
    },
  };
};

/** 1. It mounts and draws a recognisable dashboard. */
const checkMounts = async (session: Session) => {
  const mounted = () => {
    const screen = session.drawn();
    return (
      screen.includes('useful-system-monitor') && screen.includes('PID') && screen.includes('CPU')
    );
  };
  await session.settled(() => mounted() || session.exitCode() !== null, MOUNT_TIMEOUT_MS);

  if (session.exitCode() !== null && !mounted()) {
    session.fail(
      `it exited (code ${session.exitCode()}) after drawing ${session.bytes()} bytes, without ever mounting.\n` +
        '        A React core and JSX runtime from different builds fail exactly this way.',
      session.drawn(),
    );
  }
  if (!mounted()) {
    session.fail(
      `no dashboard within ${MOUNT_TIMEOUT_MS}ms (${session.bytes()} bytes)`,
      session.drawn(),
    );
  }
  console.log(`tui: mounted — dashboard drawn in ${session.bytes()} bytes`);
};

/** 2 and 3. Still alive, and still drawing — after the launch frames drain. */
const checkStillDrawing = async (session: Session) => {
  await wait(SETTLE_MS);
  if (session.exitCode() !== null) {
    session.fail(`it exited (code ${session.exitCode()}) ${SETTLE_MS}ms after mounting`);
  }

  const before = session.bytes();
  await wait(DRAW_WINDOW_MS);
  if (session.exitCode() !== null) session.fail(`it exited (code ${session.exitCode()}) while running`);
  if (session.bytes() === before) {
    session.fail(
      `not one frame in ${DRAW_WINDOW_MS}ms at --interval 1, ${SETTLE_MS}ms after mounting.\n` +
        '        It mounted and then stopped committing — which no heap check can see,\n' +
        '        because an app that never redraws never allocates.',
    );
  }
  console.log(`tui: alive — ${session.bytes() - before} more bytes over ${DRAW_WINDOW_MS}ms`);
};

/*
 * 4. It answers a keypress.
 *
 * The assertion is the *active* tab marker, `[3 MEMORY]`, not the word MEMORY:
 * every tab label is on screen at all times, so matching the bare word passed
 * instantly against the frame already drawn, before the key had been read at
 * all — a check that could not fail. Only frames written after the keystroke
 * are searched, for the same reason. See I-23 for the brackets.
 */
const checkRespondsToKey = async (session: Session) => {
  const mark = session.bytes();
  session.press('3');
  const switched = () => session.drawnSince(mark).includes('[3 MEMORY]');
  if (!(await session.settled(switched, KEY_TIMEOUT_MS))) {
    session.fail(
      'it did not respond to `3` — raw mode or useInput is broken',
      session.drawn().slice(-FAILURE_CONTEXT_BYTES),
    );
  }
  console.log('tui: responsive — `3` switched to the memory screen');
};

/** 5. It quits, rather than hanging on a terminal it cannot restore. */
const checkQuits = async (session: Session) => {
  session.press('q');
  if (!(await session.settled(() => session.exitCode() !== null, QUIT_TIMEOUT_MS))) {
    session.fail(`it did not quit within ${QUIT_TIMEOUT_MS}ms of \`q\``);
  }
  if (session.exitCode() !== 0) session.fail(`it quit with code ${session.exitCode()}, expected 0`);
  console.log('tui: quit cleanly on `q`');
};

const main = async () => {
  const session = startSession();
  await checkMounts(session);
  await checkStillDrawing(session);
  await checkRespondsToKey(session);
  await checkQuits(session);

  console.log('\ntui: PASS — the built binary mounts, draws, responds and exits');
  process.exit(0);
};

void main();
