/**
 * I-22b: the built binary really mounts and draws.
 *
 * A build that loads is not a build that mounts, and nothing else in the gate
 * list can tell the difference:
 *
 *   - the suite mounts `App` under a test renderer, which never runs
 *     `dist/cli.js` and so never exercises the entry point at all;
 *   - `verify:smoke` runs `--json`, which returns before `render()`;
 *   - `verify:longrun` measures the heap, and an app that never redraws never
 *     allocates — it called a dead binary flat and reported PASS.
 *
 * That combination has already passed a build that drew six bytes and exited 0.
 * It happened when React's development build was loaded before the production
 * reconciler: the dev `react` calls `dispatcher.getOwner`, the production
 * reconciler does not provide it, and the `TypeError` went into the empty
 * `onUncaughtError` callback ink hands to `createContainer`. React discarded
 * the tree and committed an empty root. No crash, no stderr, exit 0, and a
 * blank screen.
 *
 * So this check reads the frames. It asserts the dashboard's actual furniture
 * is on screen and that the screen changes at least once, which is the part
 * that only a real mount can produce.
 *
 * Runs through `scripts/tty-shim.mjs`, because the dashboard refuses to start
 * unless both pipes are TTYs (I-22) and capturing output means piping.
 */
import { spawn } from 'node:child_process';
import { existsSync } from 'node:fs';

const ENTRY = process.argv[2] ?? 'dist/cli.js';
const RUN_MS = Number(process.env['RUN_MS'] ?? 9_000);

if (!existsSync(ENTRY)) {
  console.error(`verify:tui: ${ENTRY} does not exist — run \`npm run build\` first.`);
  process.exit(1);
}

/** Lines of context printed with a failure. */
const CONTEXT_LINES = 6;

/** Strip the escape sequences, leaving what a person would actually see. */
const visible = (text) =>
  text
    // eslint-disable-next-line no-control-regex
    .replace(/\[[\d;?]*[a-zA-Z]/g, '')
    // eslint-disable-next-line no-control-regex
    .replace(/[()][A-Z\d]/g, '');

const child = spawn(process.execPath, ['scripts/tty-shim.mjs', ENTRY], {
  env: { ...process.env, FORCE_COLOR: '3', TUI_COLS: '100', TUI_ROWS: '32' },
  stdio: ['ignore', 'pipe', 'pipe'],
});

let out = '';
let err = '';
/* Counted from ink's synchronized-update markers, which bracket every frame it
   paints. A count above one is the claim that matters: the app is not a single
   static banner printed on the way past. */
child.stdout.on('data', (d) => {
  out += d.toString();
});
child.stderr.on('data', (d) => {
  err += d.toString();
});

const exited = new Promise((resolve) => child.on('exit', (code, signal) => resolve({ code, signal })));

await new Promise((r) => setTimeout(r, RUN_MS));
child.kill('SIGTERM');
/* Do not let a binary that ignores SIGTERM hang the gate list. */
const guard = setTimeout(() => child.kill('SIGKILL'), 3_000);
await exited;
clearTimeout(guard);

const text = visible(out);
const frames = (out.match(/\[\?2026l/g) ?? []).length;

/* The furniture, not the data: a number would make this flaky on an idle or a
   loaded machine, while the chrome is there on every frame the app ever draws. */
const EXPECTED = [
  ['brand', /useful-system-monitor/],
  ['view tabs', /OVERVIEW/],
  ['cards', /CPU/],
  ['card borders', /[╭╰│]/],
  ['footer legend', /q quit/],
];

const missing = EXPECTED.filter(([, re]) => !re.test(text)).map(([name]) => name);

console.log(`entry:  ${ENTRY}`);
console.log(`ran:    ${RUN_MS / 1000}s at 100x32`);
console.log(`output: ${out.length} bytes, ${frames} frames, ${text.split('\n').length} lines`);
if (err.trim()) console.log(`stderr: ${err.trim().slice(0, 400)}`);

const drew = out.length > 1_000;
const redrew = frames >= 2;
const complete = missing.length === 0;

if (!drew) console.log('the binary produced almost no output — it loaded but never mounted.');
if (!redrew) console.log(`only ${frames} frame(s) — the app painted once and then stopped.`);
if (!complete) console.log(`missing from the screen: ${missing.join(', ')}`);

const ok = drew && redrew && complete;
if (ok) {
  console.log('\nfirst frame as drawn:');
  console.log(
    text
      .split('\n')
      .filter((l) => l.trim())
      .slice(0, CONTEXT_LINES)
      .map((l) => `  ${l}`)
      .join('\n'),
  );
}
console.log(`\nI-22b: ${ok ? 'PASS' : 'FAIL'}`);
process.exit(ok ? 0 : 1);
