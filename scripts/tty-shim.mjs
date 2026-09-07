/**
 * Runs a built entry point as if its pipes were a terminal.
 *
 * The dashboard refuses to start unless stdin *and* stdout are TTYs (I-22), so
 * every check that captures output by piping it gets the one-shot text path
 * instead — which is the half of the program that cannot regress the way the
 * dashboard can. A pty would be the honest fix and needs a native dependency;
 * this lies to exactly the four properties the check consults, and nothing
 * else, before handing control to the real `dist/cli.js`.
 *
 * Usage: node scripts/tty-shim.mjs dist/cli.js [args...]
 *   TUI_COLS / TUI_ROWS set the size the program is told it has.
 *
 * Deliberately not part of the app. Nothing under `src/` may read `TUI_COLS`.
 */
import { pathToFileURL } from 'node:url';

/** The terminal the shim pretends to be, unless the env says otherwise. */
const DEFAULT_COLS = 100;
const DEFAULT_ROWS = 32;
const cols = Number(process.env['TUI_COLS'] ?? DEFAULT_COLS);
const rows = Number(process.env['TUI_ROWS'] ?? DEFAULT_ROWS);

Object.defineProperty(process.stdout, 'isTTY', { value: true, configurable: true });
Object.defineProperty(process.stdout, 'columns', { value: cols, configurable: true });
Object.defineProperty(process.stdout, 'rows', { value: rows, configurable: true });
Object.defineProperty(process.stderr, 'isTTY', { value: true, configurable: true });
Object.defineProperty(process.stdin, 'isTTY', { value: true, configurable: true });

/* ink calls these to take over the keyboard and to stop stdin holding the event
   loop open. On a pipe or /dev/null they are missing: the absence of
   `setRawMode` is what "Raw mode is not supported" comes from, and the absence
   of `ref` made ink's own exit path throw `stdin.ref is not a function` — which
   the program reported as its own failure while still drawing perfectly well. */
process.stdin.setRawMode ??= function setRawMode() {
  return process.stdin;
};
process.stdin.ref ??= function ref() {
  return process.stdin;
};
process.stdin.unref ??= function unref() {
  return process.stdin;
};

const entry = process.argv[2];
if (!entry) {
  console.error('tty-shim: expected an entry point, e.g. node scripts/tty-shim.mjs dist/cli.js');
  process.exit(2);
}

/* The program reads process.argv itself, so rewrite it to what it would have
   seen had it been launched directly: drop `node`, this shim, and the entry
   point it was handed. */
const SHIM_ARGS = 3;
process.argv = [process.argv[0], entry, ...process.argv.slice(SHIM_ARGS)];

await import(pathToFileURL(entry).href);
