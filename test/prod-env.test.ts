import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const read = (repoPath: string) =>
  readFileSync(new URL(`../${repoPath}`, import.meta.url), 'utf8');

/**
 * I-10b. The leak this guards against is not in any structure the app owns — it
 * is React's development build writing ~40 Performance Tracks entries per
 * commit into Node's uncapped User Timing buffer, which nothing reads and
 * nothing clears.
 *
 * These check the shape that makes the fix possible. That the *built* binary
 * actually achieves it is checked against the artifact itself, in
 * `scripts/smoke.sh`, by inspecting the require cache of a real run — because
 * the first version of this fix looked correct in the source and did not work.
 */
describe('I-10b: React runs its production build', () => {
  it('keeps the launcher free of static imports', () => {
    /*
     * The reason this is a *file-shape* rule rather than an ordering one:
     * ordering cannot be enforced here. `tsc` hoists its own
     * `import ... from "react/jsx-runtime"` above every hand-written import in
     * any file containing JSX, and React, the JSX runtime, the reconciler and
     * the scheduler are all CJS, which Node evaluates while linking the ES
     * module graph — before any ESM body in it, whatever the source says. An
     * earlier fix put the assignment in `cli.tsx`'s first import and still
     * shipped a binary with react.development.js in its require cache.
     *
     * With no static import at all, there is nothing left to lose the race to.
     */
    const cli = read('src/cli.ts');
    const staticImports = cli.match(/^import\s.*$/gm) ?? [];
    expect(staticImports).toEqual([]);
    // And it must still actually reach both, dynamically.
    expect(cli).toMatch(/await import\('\.\/core\/prod-env\.js'\)/);
    expect(cli).toMatch(/await import\('\.\/main\.js'\)/);
  });

  it('has no JSX in the launcher, so the compiler adds no import either', () => {
    /* A `.ts` extension is what stops tsc emitting the jsx-runtime import that
       would otherwise land above everything. Renaming this file to `.tsx` and
       adding a single element would reintroduce the bug silently. */
    expect(read('src/cli.ts')).not.toMatch(/<[A-Za-z]/);
  });

  it('leaves an explicit NODE_ENV alone', async () => {
    /*
     * Vitest sets NODE_ENV=test, which makes this the interesting case rather
     * than an awkward one: a developer who asked for a mode must keep it, or
     * `NODE_ENV=development npm start` would lose React's warnings — the one
     * setting whose entire purpose is to be noisy.
     */
    const before = process.env['NODE_ENV'];
    expect(before).toBeTruthy();
    await import('../src/core/prod-env.js');
    expect(process.env['NODE_ENV']).toBe(before);
  });
});
