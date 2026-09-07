/**
 * Which React build this process gets, decided before React is loaded.
 *
 * React and react-reconciler branch on `process.env.NODE_ENV` at require time:
 *
 *   if (process.env.NODE_ENV === 'production') require('./cjs/react.production.js')
 *   else                                       require('./cjs/react.development.js')
 *
 * A globally installed CLI runs with `NODE_ENV` unset, so it got the
 * development reconciler — which narrates every render to the Performance
 * Timeline for React DevTools (`performance.measure()` per component, per
 * commit, on the "Components ⚛" track). Node buffers user-timing entries for
 * the life of the process and never evicts them, so a dashboard that renders
 * for days accumulates them for days: measured at 54 KB per render, which
 * reaches V8's 4 GB ceiling and dies on `Ineffective mark-compacts near heap
 * limit`. Nothing in this app can release those entries — they are held by the
 * runtime — so the fix is to not emit them.
 *
 * Deliberately a default and not an override, so `NODE_ENV=development npm
 * start` still gets React's warnings, and vitest's own `NODE_ENV=test` still
 * gets its dev-mode checks. Empty counts as unset: `NODE_ENV=` in a shell
 * profile would otherwise take the development branch just as surely as
 * omitting it, while looking like a deliberate choice.
 *
 * This module must import nothing. It is called for its ordering — before the
 * first React import — and an import of its own would be one more chance for
 * something to pull React in first. See I-10b.
 */
export function preferProductionReact(env: NodeJS.ProcessEnv = process.env): void {
  if (!env['NODE_ENV']) env['NODE_ENV'] = 'production';
}
