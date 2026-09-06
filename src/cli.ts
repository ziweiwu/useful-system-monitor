#!/usr/bin/env node
/*
 * The launcher, and deliberately the whole of it.
 *
 * This file has NO static imports, which is the entire point: `NODE_ENV` has to
 * be set before React's module body runs, and a static import cannot be beaten
 * to it from inside the same file. Two things get there first otherwise:
 *
 *   - `tsc` hoists its own `import { jsx as _jsx } from "react/jsx-runtime"`
 *     to the top of any file containing JSX, above every import written by
 *     hand. "Keep this import first" is therefore not a rule this codebase can
 *     enforce — the compiler inserts one above it.
 *   - React, the JSX runtime, the reconciler and the scheduler are all CJS.
 *     Node evaluates CJS dependencies while *linking* the ES module graph, so
 *     they run before any ESM body in that graph, whatever the source order.
 *
 * An earlier version of this fix put the assignment in the first import of the
 * old `cli.tsx` and looked right; the shipped binary still had
 * `react.development.js` in its require cache, because both of the above
 * happened first. Dynamic `import()` is the fix: it resolves during *this*
 * module's evaluation, so everything below is guaranteed to load afterwards.
 *
 * Keep it that way. Adding a static import here silently restores the leak
 * that I-10b exists to prevent; `verify:smoke` checks the built artifact for
 * exactly that.
 */
/* Nothing imports this; its only job is to make the file an ES module without
   importing anything. Without an export tsc emits CommonJS, top-level await
   becomes a syntax error, and the shebang ends up in front of a `require`. An
   export costs nothing at load time; an import would cost everything. */
export const launcher = true;

await import('./core/prod-env.js');
const { run } = await import('./main.js');
await run();
