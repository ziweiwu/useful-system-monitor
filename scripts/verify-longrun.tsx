/*
 * I-10b: the heap stays flat while the dashboard runs forever.
 *
 * I-10 covers the structures this app owns — the rings, the per-process history
 * map, the metadata cache — and every one of them was already bounded when the
 * shipped binary died of "Ineffective mark-compacts near heap limit" after ~82
 * hours. The growth was underneath them: React's development build emits
 * Performance Tracks, ~40 `performance.measure()` calls per commit, into a User
 * Timing buffer that Node never caps and nothing here ever reads. A dashboard
 * commits forever, so that buffer grew with uptime and with nothing else.
 *
 * Which is why this one has to be a measurement over time rather than an
 * assertion about a data structure. No unit test on any object this repo
 * defines could have caught it, because no object this repo defines was wrong.
 *
 * Measured per collector tick, mock provider, colour on, on an M1 Pro:
 *   React development build   ~223 KB/tick -> ~4 GB in about 3.5 days
 *   React production build      ~3.9 KB/tick -> ~4 GB in about 4-6 months
 *
 * A tick is one cycle of every collector, which is what drives a render, so
 * those are per-render figures in all but name. Renders are not counted
 * directly: ink skips the write when a frame is byte-identical to the last, so
 * counting writes undercounts commits, and React's `Profiler` — the honest
 * counter — is inert in the production build this exists to check.
 *
 * The residual is real but is not ours: ink 7.1.1 keeps two module-level caches
 * keyed by the text it has laid out, neither of which evicts —
 * `build/measure-text.js` (every string measured) and `build/wrap-text.js`
 * (every string that overflowed its box). A monitor redraws changing numbers by
 * definition, so both are unbounded in principle — just far too slow to matter
 * against a restart. The budget below sits between the two figures above, so it
 * catches the regression and not the residual.
 *
 * COLOUR IS PART OF THE MEASUREMENT. Those caches are keyed on the *decorated*
 * string, so a run without ANSI codes has shorter keys and fewer of them: the
 * same harness measured 1.03 KB/tick bare and 3.94 KB/tick with FORCE_COLOR=3.
 * A user gets the coloured one, so that is what is forced below — measuring the
 * bare case reported a quarter of the real figure and called it a year's
 * headroom.
 *
 * Known under-measurement, accepted: the mock's process count is a constant, so
 * the status line's `top N of TOTAL` never drifts and `wrap-text` saturates
 * here where against a real machine it does not. That term is ~5% of the
 * residual; pinning it down would mean making this check depend on the host's
 * process churn, which is worse than the error it removes.
 *
 * SCOPE. This measures the app under React's production build. That the
 * *shipped* binary actually reaches that build is a separate claim, checked
 * where it can be checked exactly rather than statistically:
 *   - `scripts/smoke.sh` runs the built CLI with NODE_ENV cleared, the way a
 *     user's shell starts it, and asks which React modules it actually
 *     evaluated. That is the check that caught a version of the fix which was
 *     correct in the source and wrong in the binary.
 *   - `test/prod-env.test.ts` asserts the launcher stays free of static
 *     imports, and that an explicit NODE_ENV still wins.
 */
import { spawnSync } from 'node:child_process';
import { EventEmitter } from 'node:events';
import React from 'react';
import { render } from 'ink';
import { App } from '../src/app.js';
import { MockProvider } from '../src/providers/mock/provider.js';

const gc = (globalThis as { gc?: () => void }).gc;

if (!gc || process.env['NODE_ENV'] !== 'production' || !process.env['FORCE_COLOR']) {
  /*
   * Re-exec with --expose-gc and NODE_ENV=production, both of which have to be
   * true before the process starts.
   *
   * NODE_ENV cannot be set from inside: it selects React's build at runtime but
   * also the JSX transform at compile time, and the two have to agree. `tsx`
   * emits `jsxDEV` unless NODE_ENV is already production when it transforms,
   * and `react/jsx-dev-runtime` exports `jsxDEV: undefined` in the production
   * build — so a run that set it late rendered nothing at all and reported a
   * beautifully flat heap. That is the one way this check could lie, and it did
   * before the frame counter below was added.
   */
  const r = spawnSync(process.execPath, ['--expose-gc', '--import', 'tsx', import.meta.filename], {
    stdio: 'inherit',
    /* FORCE_COLOR because ink's caches are keyed on the decorated string, and
       chalk reads the *real* stdout, not the fake one below — under a pipe or
       CI that is level 0. See the header. */
    env: { ...process.env, NODE_ENV: 'production', FORCE_COLOR: '3' },
  });
  process.exit(r.status ?? 1);
}

/** Bytes per tick this is allowed to retain. See the table above. */
const BUDGET_BYTES_PER_TICK = 8 * 1024;
/* Far below the 10s default so the run finishes in seconds. The leak is per
   render, not per second, so compressing the tier compresses the clock without
   changing what is being measured. */
const TIER_MS = 5;
const WARMUP_TICKS = 400;
const MEASURE_TICKS = 3000;
/* Renders are not the denominator — see the header — but a run that drew almost
   nothing measured almost nothing, so the count is kept as a floor. */
const MIN_FRAMES = 100;

/** Frames go nowhere; only the count is kept, and only as a sanity gate. */
class Stdout extends EventEmitter {
  columns = 100;
  rows = 40;
  isTTY = true;
  frames = 0;
  write(): boolean {
    this.frames++;
    return true;
  }
}

/* Raw mode has to be answerable or `useInput` throws (I-22); nothing is ever
   typed here, so answering is all this has to do. */
class Stdin extends EventEmitter {
  isTTY = true;
  setRawMode() {
    return this;
  }
  setEncoding() {
    return this;
  }
  resume() {
    return this;
  }
  pause() {
    return this;
  }
  read() {
    return null;
  }
  ref() {
    return this;
  }
  unref() {
    return this;
  }
}

const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));
const mb = (b: number) => (b / 1024 / 1024).toFixed(1);

const main = async () => {
  const stdout = new Stdout();
  const tiers = {
    cpu: TIER_MS,
    memory: TIER_MS,
    disk: TIER_MS,
    battery: TIER_MS,
    processes: TIER_MS,
  };
  const instance = render(<App provider={new MockProvider()} tiers={tiers} demo />, {
    stdout: stdout as never,
    stdin: new Stdin() as never,
    stderr: stdout as never,
    exitOnCtrlC: false,
    patchConsole: false,
  });

  const tick = async (n: number) => {
    for (let i = 0; i < n; i++) await wait(TIER_MS);
  };

  /* The first ticks allocate what a steady state then keeps — module code, the
     mock's process list, ink's caches for every label on screen. Measuring
     through them would report one-time cost as a leak. */
  await tick(WARMUP_TICKS);
  gc();
  gc();
  const startFrames = stdout.frames;
  const startHeap = process.memoryUsage().heapUsed;

  await tick(MEASURE_TICKS);
  gc();
  gc();
  const grew = process.memoryUsage().heapUsed - startHeap;
  const perTick = grew / MEASURE_TICKS;
  const frames = stdout.frames - startFrames;

  instance.unmount();

  /* Days to the ~4 GB V8 ceiling at the 10s default. The crash report is the
     only reason this number is interesting, so it is the number printed. */
  const ticksPerDay = (24 * 60 * 60) / 10;
  const days = perTick > 0 ? (4 * 1024 ** 3) / (perTick * ticksPerDay) : Infinity;
  const entries = performance.getEntriesByType('measure').length;

  console.log(`react ${React.version} (${process.env['NODE_ENV']} build) · ink 7`);
  console.log(`${MEASURE_TICKS} ticks at ${TIER_MS}ms after ${WARMUP_TICKS} warm-up, ${frames} frames drawn`);
  console.log(
    `heap ${mb(startHeap)} MB -> ${mb(startHeap + grew)} MB   ${(perTick / 1024).toFixed(2)} KB/tick`,
  );
  console.log(`User Timing entries retained: ${entries}`);
  console.log(
    `projected: ${days > 3650 ? '>10 years' : `${days.toFixed(1)} days`} to a 4 GB heap at the 10s default`,
  );

  if (frames < MIN_FRAMES) {
    console.error(
      `\nI-10b: FAIL — only ${frames} frames drawn, so nothing was really measured.` +
        ' The app rendered but stopped committing; a flat heap here means nothing.',
    );
    process.exit(1);
  }

  /* Both conditions, because either alone can pass while the bug is back: the
     filling buffer is the mechanism, and bytes/tick is the effect. */
  const ok = perTick < BUDGET_BYTES_PER_TICK && entries === 0;
  console.log(
    `\nI-10b: ${ok ? 'PASS' : 'FAIL'} — ${(perTick / 1024).toFixed(2)} KB/tick ` +
      `(budget ${BUDGET_BYTES_PER_TICK / 1024} KB), ${entries} User Timing entries (budget 0)`,
  );
  process.exit(ok ? 0 : 1);
};

void main();
