import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { preferProductionReact } from '../src/core/reactEnv.js';

const read = (rel: string) =>
  readFileSync(fileURLToPath(new URL(`../${rel}`, import.meta.url)), 'utf8');

/*
 * I-10b, the cheap half.
 *
 * The expensive half is `npm run verify:longrun`, which mounts the app and
 * weighs the heap per render. This file guards the property that run depends
 * on, in the milliseconds a Stop hook can afford: the entry point decides which
 * React build the process gets, and it can only decide that *before* React is
 * imported.
 *
 * The failure this prevents is silent in every direction. React's development
 * reconciler narrates each render to the Performance Timeline; Node buffers
 * user-timing entries for the life of the process and never evicts them; so the
 * dashboard grew ~174 KB per render and died at V8's 4 GB ceiling after a day
 * and a half, with nothing in the logs but `FATAL ERROR: Ineffective
 * mark-compacts near heap limit`.
 */
describe('I-10b: the entry point picks React before React is loaded', () => {
  const cli = read('src/cli.tsx');

  it('sets NODE_ENV before importing anything that renders', () => {
    const setsEnv = cli.indexOf('preferProductionReact()');
    const importsMain = cli.indexOf("import('./main.js')");
    expect(setsEnv, 'cli.tsx must call preferProductionReact()').toBeGreaterThan(-1);
    expect(importsMain, 'cli.tsx must import main.js').toBeGreaterThan(-1);
    expect(setsEnv, 'the call must come before the import').toBeLessThan(importsMain);
  });

  /*
   * The whole point of the launcher. A static `import` is hoisted above every
   * statement in the module, so one added here that transitively reaches react
   * or ink would load the development build first and put the leak back — and
   * would do it without a warning, a crash or a failing unit test.
   */
  it('statically imports nothing but the env helper', () => {
    const statics = [...cli.matchAll(/^\s*import\s.*?from\s*'([^']+)'/gm)].map((m) => m[1]!);
    expect(statics).toEqual(['./core/reactEnv.js']);
  });

  /*
   * `main.js` has to arrive through a dynamic import for the same reason. This
   * is the line most likely to be "tidied" back into a static import by someone
   * who reads the file as an indirection with no purpose.
   */
  it('pulls the app in dynamically, not statically', () => {
    expect(cli).toMatch(/await import\('\.\/main\.js'\)/);
    expect(cli).not.toMatch(/^\s*import\s.*'\.\/main\.js'/m);
  });

  /*
   * JSX compiles to a *static* `react/jsx-runtime` import under the automatic
   * runtime, which would be hoisted above the env call — the exact mistake that
   * loaded development react against the production reconciler, whereupon
   * React caught the resulting TypeError in ink's empty `onUncaughtError` and
   * committed an empty tree. The app drew nothing and reported no error.
   */
  it('contains no JSX, whatever its extension says', () => {
    expect(cli).not.toMatch(/<[A-Za-z][^>]*\/?>/);
  });

  /* Called for its ordering, so it must not drag a module graph in with it. */
  it('the env helper imports nothing', () => {
    expect(read('src/core/reactEnv.ts')).not.toMatch(/^\s*import\s/m);
  });
});

describe('I-10b: preferProductionReact', () => {
  it('defaults an unset NODE_ENV to production', () => {
    const env: NodeJS.ProcessEnv = {};
    preferProductionReact(env);
    expect(env['NODE_ENV']).toBe('production');
  });

  it('leaves a deliberate choice alone', () => {
    const env: NodeJS.ProcessEnv = { NODE_ENV: 'development' };
    preferProductionReact(env);
    expect(env['NODE_ENV']).toBe('development');
  });

  /*
   * `NODE_ENV=` in a shell profile reads as a deliberate choice and behaves
   * like an omission: React's `=== 'production'` test fails, so it takes the
   * development branch and the leak comes back.
   */
  it('treats empty as unset', () => {
    const env: NodeJS.ProcessEnv = { NODE_ENV: '' };
    preferProductionReact(env);
    expect(env['NODE_ENV']).toBe('production');
  });
});
