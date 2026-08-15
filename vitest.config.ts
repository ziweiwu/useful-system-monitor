import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    /*
     * 30s, not vitest's 5s default.
     *
     * A good part of this suite mounts a real Ink app, writes keys to it and
     * waits for frames — work that takes hundreds of milliseconds when the
     * machine is idle and several seconds when it is not. The 5s default is a
     * bet that the machine is idle, and a test suite runs precisely when it is
     * not: at load average 125 and again at 343 the suite failed a *different*
     * handful of tests on each run, all of them passing on their own and all
     * passing together under `--testTimeout=30000`.
     *
     * This raises the ceiling; it does not slow anything down, because a
     * passing test still finishes when it finishes. The suite is now part of
     * the release gate, and a gate that fails at random teaches people to
     * re-run it rather than read it.
     */
    testTimeout: 30_000,
    hookTimeout: 30_000,
  },
});
