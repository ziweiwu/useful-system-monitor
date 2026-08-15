import type { ProcessSample } from '../src/core/types.js';

export function sample(over: Partial<ProcessSample> & { pid: number }): ProcessSample {
  return {
    ppid: 1,
    startTime: 1_000,
    command: `/usr/bin/proc${over.pid}`,
    user: 'ziweiwu',
    state: 'S',
    cpuPercent: 0,
    rssBytes: 1024,
    energy: 0,
    protected: false,
    ...over,
  };
}

/**
 * Waits until a rendered frame satisfies `predicate`, or fails loudly.
 *
 * Fixed `await wait(150)` between keystrokes is a bet that the machine is idle,
 * and the suite runs precisely when it is not — a loaded machine produced a
 * different failure on each run, all of them the same shape: the assertion ran
 * before the app had processed the keys. Polling for the state the test is
 * actually about turns "slow" into "slower" instead of into "failed", and it
 * makes the test say what it is waiting for.
 */
export async function waitForFrame(
  frame: () => string | undefined,
  predicate: (f: string) => boolean,
  what: string,
  timeoutMs = 5_000,
): Promise<string> {
  const deadline = Date.now() + timeoutMs;
  let last = '';
  while (Date.now() < deadline) {
    last = frame() ?? '';
    if (predicate(last)) return last;
    await new Promise((r) => setTimeout(r, 25));
  }
  throw new Error(`timed out after ${timeoutMs}ms waiting for ${what}\n--- last frame ---\n${last}`);
}
