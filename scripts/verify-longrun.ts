/**
 * I-10b: the heap must stay flat across a long run.
 *
 * This app's premise is that it sits in a background pane for days, so a
 * per-render allocation that is never released is not a slow leak — it is a
 * crash with a delay on it. One shipped: React's *development* reconciler
 * narrates every render to the Performance Timeline for React DevTools
 * (`performance.measure()` per component, per commit, on the "Components ⚛"
 * track). Node buffers user-timing entries for the life of the process and
 * never evicts them, and a globally installed CLI runs with `NODE_ENV` unset,
 * which is exactly how React picks that build. Measured at 54 KB per render;
 * the dashboard reached V8's 4 GB ceiling and died on `FATAL ERROR: Ineffective
 * mark-compacts near heap limit`. `dist/cli.js` now defaults `NODE_ENV` to
 * `production` before React loads. This is the check that says so.
 *
 * ## Why this file contains no JSX
 *
 * It is not a style choice, and removing this constraint silently breaks the
 * check. TypeScript's automatic runtime turns JSX into a *static*
 * `import ... from 'react/jsx-runtime'`, and static imports are hoisted above
 * every statement in the module — including the call that sets `NODE_ENV`. So
 * a JSX version of this file loaded **development** React first and then
 * **production** react-reconciler, and the two do not fit: the dev `react`
 * calls `dispatcher.getOwner`, which the production reconciler does not
 * provide. React caught that `TypeError` in the empty `onUncaughtError`
 * callback ink passes to `createContainer`, discarded the tree, and committed
 * an empty root.
 *
 * The failure mode is the dangerous one: the app rendered *nothing*, allocated
 * nothing, and the heap was perfectly flat. The check reported PASS at
 * "-87.7 KB/render" against zero renders. Hence `renders > 0` is a pass
 * condition below, not a detail — see also `verify:tui` (I-22b), which is the
 * same lesson applied to the built binary.
 *
 * ## Measuring
 *
 * Per *render*, not per second: a render is the unit that allocates, so a rate
 * would change meaning the next time a tier default moves. `FORCE_COLOR=3` is
 * required because ink keys its layout and wrap caches on the *decorated*
 * string — without colour the run walks a smaller set of keys and under-reports
 * by ~4x. And frames go to a sink that discards them, because
 * `ink-testing-library` appends every frame it has ever rendered to an array
 * and never drops one: a heap probe pointed at it measures the probe, which it
 * did, reporting a ~9 MB/5s "leak" that was entirely its own frame log.
 */
import { EventEmitter } from 'node:events';

if (process.env['FORCE_COLOR'] !== '3') {
  console.error(
    'verify:longrun needs FORCE_COLOR=3 in its environment, because ink keys its\n' +
      'layout cache on the decorated string — measuring without colour walks a\n' +
      'smaller set of keys and under-reports by ~4x. Use `npm run verify:longrun`.',
  );
  process.exit(1);
}

/* The same decision `dist/cli.js` makes, made the same way and in the same
   place: before the first React import. Overridable on purpose —
   `NODE_ENV=development npm run verify:longrun` reproduces the original leak
   and must fail. */
const { preferProductionReact } = await import('../src/core/reactEnv.js');
preferProductionReact();

const React = (await import('react')).default;
const { render } = await import('ink');
const { App } = await import('../src/app.js');
const { DarwinProvider } = await import('../src/providers/darwin/provider.js');

/**
 * A TTY-shaped stdout that throws frames away.
 *
 * The size is fixed so that a resize of the real terminal cannot change the
 * layout mid-measurement and show up as a step in the heap.
 */
class Sink extends EventEmitter {
  isTTY = true;
  columns = 104;
  rows = 32;
  write(): void {}
}

class Stdin extends EventEmitter {
  isTTY = true;
  setEncoding(): void {}
  setRawMode(): void {}
  resume(): void {}
  pause(): void {}
  ref(): void {}
  unref(): void {}
  read(): null {
    return null;
  }
}

/* Fast tiers buy renders rather than wall-clock: the leak is per-render, so
   200ms compresses days of the 10s default into a couple of minutes. */
const TIERS = { cpu: 200, memory: 200, disk: 200, battery: 200, processes: 200 };

const RUN_MS = Number(process.env['RUN_MS'] ?? 120_000);
/** Long enough for the module graph, the first `ps` and yoga's warm-up. */
const WARMUP_MS = 20_000;
/** An order of magnitude under the 54 KB/render the development build cost. */
const BUDGET_BYTES_PER_RENDER = 5 * 1024;
/** Below this the run proves nothing, however flat the heap looks. */
const MIN_RENDERS = 100;
/** The ceiling V8 dies at. */
const HEAP_LIMIT_BYTES = 4 * 1024 ** 3;
/** A render every 10s is 8,640 a day. */
const RENDERS_PER_DAY_AT_DEFAULT_TIER = 8_640;

const wait = (delayMs: number) => new Promise((r) => setTimeout(r, delayMs));
const megabytes = (n: number) => `${(n / 1024 / 1024).toFixed(1)} MB`;

if (!global.gc) {
  console.error('verify:longrun must run under --expose-gc. Use `npm run verify:longrun`.');
  process.exit(1);
}

let renders = 0;
const app = render(React.createElement(App, { provider: new DarwinProvider(), tiers: TIERS }), {
  stdout: new Sink() as never,
  stderr: new Sink() as never,
  stdin: new Stdin() as never,
  exitOnCtrlC: false,
  patchConsole: false,
  /* ink's own commit callback, so the denominator counts renders rather than
     writes — ink emits several writes per frame. */
  onRender: () => {
    renders++;
  },
} as never);

console.log(
  `NODE_ENV=${process.env['NODE_ENV'] ?? '(unset)'}  react=${React.version}  ` +
    `tiers=${TIERS.cpu}ms  warmup=${WARMUP_MS / 1000}s  run=${RUN_MS / 1000}s`,
);

await wait(WARMUP_MS);
global.gc();
const heap0 = process.memoryUsage().heapUsed;
const renders0 = renders;

const t0 = Date.now();
while (Date.now() - t0 < RUN_MS) {
  await wait(15_000);
  global.gc();
  const m = process.memoryUsage();
  const n = renders - renders0;
  console.log(
    `  ${String(Math.round((Date.now() - t0) / 1000)).padStart(3)}s  ` +
      `heap ${megabytes(m.heapUsed).padStart(8)}  renders ${String(n).padStart(5)}  ` +
      `${n > 0 ? ((m.heapUsed - heap0) / n / 1024).toFixed(1) : '—'} KB/render`,
  );
}

global.gc();
const heap1 = process.memoryUsage().heapUsed;
const total = renders - renders0;
app.unmount();

const perRender = (heap1 - heap0) / Math.max(1, total);
/* The development build emits these; the production build emits none. A count
   above zero means the wrong React got loaded, which is the regression this
   file exists to catch. */
const measures = performance.getEntriesByType('measure').length;

console.log(
  `\n${megabytes(heap0)} -> ${megabytes(heap1)} over ${total} renders = ${(perRender / 1024).toFixed(1)} KB/render`,
);
console.log(`react performance-timeline entries: ${measures.toLocaleString()}`);

if (perRender > 0) {
  /* What the number means where the user is: the default tier, and the 4 GB
     ceiling V8 dies at. */
  const days = HEAP_LIMIT_BYTES / perRender / RENDERS_PER_DAY_AT_DEFAULT_TIER;
  console.log(`at this rate and the 10s default tier, 4 GB in ~${days.toFixed(0)} days`);
}

const drew = total >= MIN_RENDERS;
const flat = perRender <= BUDGET_BYTES_PER_RENDER;
const clean = measures === 0;

if (!drew) {
  console.log(
    `\nonly ${total} renders in ${RUN_MS / 1000}s — the app is not drawing, so a flat\n` +
      'heap means nothing. This is what mismatched React builds look like: check\n' +
      'that nothing imported react before preferProductionReact() ran.',
  );
}

console.log(
  `I-10b: ${drew && flat && clean ? 'PASS' : 'FAIL'}  ` +
    `(renders ${total} >= ${MIN_RENDERS}: ${drew ? 'ok' : 'NO'}; ` +
    `${(perRender / 1024).toFixed(1)} <= ${BUDGET_BYTES_PER_RENDER / 1024} KB/render: ${flat ? 'ok' : 'NO'}; ` +
    `${measures} timeline entries: ${clean ? 'ok' : 'NO'})`,
);
process.exit(drew && flat && clean ? 0 : 1);
