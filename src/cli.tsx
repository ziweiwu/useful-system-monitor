#!/usr/bin/env node
/**
 * The launcher, and nothing else. All the real work is in `main.tsx`.
 *
 * This file exists for one reason: `NODE_ENV` has to be set *before* React is
 * loaded, and an ESM `import` cannot be sequenced after a statement — the whole
 * static import graph is evaluated before the first line of any module body. So
 * the entry point cannot both set the variable and import the app; it has to
 * set the variable and then pull the app in dynamically. That is all this is.
 *
 * `preferProductionReact` carries the reasoning, and imports nothing itself, so
 * this file's static graph cannot reach React. Keep it that way: an `import`
 * added here that transitively loads react or ink puts the development
 * reconciler back and reinstates the leak, silently. See I-10b.
 */
import { preferProductionReact } from './core/reactEnv.js';

preferProductionReact();

/* Awaited, not fired and forgotten: a bare `void import(...)` would surface a
   failure inside `main` as an unhandled rejection rather than through `main`'s
   own stderr-and-exit-code path (I-24). The import above makes this a module,
   which is what licenses top-level `await` here. */
await import('./main.js');
