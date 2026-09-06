/**
 * Selects React's production build for the compiled binary.
 *
 * Loaded by `cli.ts`, which is a launcher with no static imports precisely so
 * that this runs before anything can pull React in. Ordering cannot be won any
 * other way: `tsc` hoists its own `react/jsx-runtime` import above every
 * hand-written one in a file containing JSX, and React, the JSX runtime, the
 * reconciler and the scheduler are all CJS, which Node evaluates while linking
 * the ES module graph — before any ESM body in it. A first attempt that put
 * this in the entry point's first import shipped a binary that still evaluated
 * `react.development.js`.
 *
 * React 19.2's development build emits Performance Tracks — roughly 40
 * `performance.measure()` calls per commit, tagged "Components ⚛" and
 * "Scheduler ⚛", meant to be read in a DevTools flame chart. Node has no
 * DevTools attached and no cap on the User Timing buffer, so every entry is
 * retained for the life of the process. Nothing clears it and nothing reads it.
 *
 * A dashboard re-renders forever by definition, so that buffer is an unbounded
 * leak proportional to uptime, not to anything the app holds. Measured at ~223
 * KB per render against 3.9 KB fixed — ~1.9 GB/day at the 10s default, which
 * reaches V8's ~4 GB heap ceiling in about three and a half days:
 *
 *   FATAL ERROR: Ineffective mark-compacts near heap limit
 *
 * `dist/cli.js` is run straight from a user's shell, where NODE_ENV is
 * conventionally unset, so the shipped binary always took the development
 * path. This is a runtime concern rather than a build-time one for exactly that
 * reason: nothing in the build tells React which branch to take.
 *
 * Checked end to end by `verify:smoke`, which asks a real run which React it
 * actually evaluated — the only question that turned out to be decisive.
 *
 * See I-10b.
 */

/*
 * Only for the compiled build, and this guard is load-bearing.
 *
 * NODE_ENV picks React's build at *runtime*, but it also picks the JSX
 * transform at *compile* time, and the two have to agree. `tsc` honours
 * `jsx: react-jsx` and emits `react/jsx-runtime`, which is fine in either
 * build — so `dist` is safe. `tsx` (esbuild) instead emits `jsxDEV` from
 * `react/jsx-dev-runtime`, whose production build exports `jsxDEV: undefined`.
 * Setting NODE_ENV from inside the process cannot retroactively change a
 * transform that already happened, so under `npm start` this line would hand a
 * production React a development JSX call and the dashboard would render
 * nothing at all — no error, just an empty screen.
 *
 * `import.meta.url` is the one thing that distinguishes the two without
 * importing React first: `.js` under `dist`, `.ts` under `tsx`. A dev run
 * therefore keeps the development build, which is what a dev run wants; it
 * leaks at the same rate, which is a session, not three days.
 *
 * Set only when unset, so `NODE_ENV=development` still wins and keeps React's
 * warnings. An empty string counts as unset: React tests for `=== 'production'`,
 * so `NODE_ENV=` would otherwise be development with none of the intent.
 */
if (import.meta.url.endsWith('.js') && !process.env['NODE_ENV']) {
  process.env['NODE_ENV'] = 'production';
}
