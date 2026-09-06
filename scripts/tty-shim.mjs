/*
 * Makes a piped stdio pair look like a terminal, so the built CLI takes its
 * dashboard path (I-22) under a harness that has no pty.
 *
 * Loaded with `--import`, so it runs before `dist/cli.js` decides. It fakes
 * only what I-22 tests and what ink needs to enter raw mode; everything above
 * that — the launcher, prod-env, React, ink's reconciler, every component — is
 * the real built code doing its real work. See scripts/verify-tui.ts for why
 * this is a shim rather than a pty.
 */
/* The size the harness draws into when it does not say otherwise. Wide
   enough for the full key legend, tall enough for the overview's cards. */
const DEFAULT_COLS = 100;
const DEFAULT_ROWS = 36;

const define = (stream, props) => {
  for (const [k, v] of Object.entries(props)) {
    Object.defineProperty(stream, k, { value: v, configurable: true, writable: true });
  }
};

define(process.stdout, {
  isTTY: true,
  columns: Number(process.env['TUI_COLS'] ?? DEFAULT_COLS),
  rows: Number(process.env['TUI_ROWS'] ?? DEFAULT_ROWS),
});
define(process.stdin, {
  isTTY: true,
  /* Ink calls this before reading keys and throws without it. Nothing here has
     a termios to change, and nothing needs one: the keys arrive down a pipe. */
  setRawMode: () => process.stdin,
});
