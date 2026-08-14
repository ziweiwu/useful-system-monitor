import { render } from 'ink-testing-library';
import { describe, expect, it, vi } from 'vitest';
import { App } from '../src/app.js';
import { MockProvider } from '../src/providers/mock/provider.js';
import { DEFAULT_TIERS, type MetricsProvider } from '../src/providers/types.js';

const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

async function mount(provider: MetricsProvider, kill: (p: number, s: string) => void) {
  const prev = [process.stdout.columns, process.stdout.rows] as const;
  process.stdout.columns = 100;
  process.stdout.rows = 30;
  const app = render(<App provider={provider} tiers={DEFAULT_TIERS} demo killFn={kill} />);
  await wait(300);
  return {
    ...app,
    restore() {
      app.unmount();
      process.stdout.columns = prev[0];
      process.stdout.rows = prev[1];
    },
  };
}

/**
 * I-16 end to end.
 *
 * The guard is only worth anything if the start time it compares against is
 * newer than the sample. `doKill` used to look the target up in the last
 * `processes()` result, which is a whole tier old and — when the target had
 * dropped out of the top-50 working set — missing entirely, in which case the
 * check was skipped rather than failed.
 */
describe('I-16: the kill path reads the identity at signal time', () => {
  it('asks the provider who owns the PID before signalling', async () => {
    const provider = new MockProvider();
    const identity = vi.spyOn(provider, 'identity');
    const kill = vi.fn();
    const app = await mount(provider, kill);
    try {
      app.stdin.write('k');
      await wait(200);
      expect(app.lastFrame() ?? '').toContain('KILL PROCESS');
      app.stdin.write('t');
      await wait(300);
      expect(identity).toHaveBeenCalledTimes(1);
      expect(kill).toHaveBeenCalledTimes(1);
      expect(identity.mock.calls[0]![0]).toBe(kill.mock.calls[0]![0]);
    } finally {
      identity.mockRestore();
      app.restore();
    }
  });

  it('sends nothing when the PID now belongs to a different process', async () => {
    const provider = new MockProvider();
    // Same PID, different start time: a recycled PID, which is the whole point.
    vi.spyOn(provider, 'identity').mockResolvedValue({ startTime: 999_999_999 });
    const kill = vi.fn();
    const app = await mount(provider, kill);
    try {
      app.stdin.write('k');
      await wait(200);
      app.stdin.write('t');
      await wait(300);
      expect(kill).not.toHaveBeenCalled();
      expect(app.lastFrame() ?? '').toMatch(/reused/i);
    } finally {
      vi.restoreAllMocks();
      app.restore();
    }
  });

  it('sends nothing when the identity cannot be read at all', async () => {
    const provider = new MockProvider();
    vi.spyOn(provider, 'identity').mockRejectedValue(new Error('ps timed out'));
    const kill = vi.fn();
    const app = await mount(provider, kill);
    try {
      app.stdin.write('k');
      await wait(200);
      app.stdin.write('t');
      await wait(300);
      expect(kill).not.toHaveBeenCalled();
      expect(app.lastFrame() ?? '').toMatch(/could not confirm/i);
    } finally {
      vi.restoreAllMocks();
      app.restore();
    }
  });

  it('reports an already-exited process instead of signalling it', async () => {
    const provider = new MockProvider();
    vi.spyOn(provider, 'identity').mockResolvedValue(null);
    const kill = vi.fn();
    const app = await mount(provider, kill);
    try {
      app.stdin.write('k');
      await wait(200);
      app.stdin.write('t');
      await wait(300);
      expect(kill).not.toHaveBeenCalled();
      expect(app.lastFrame() ?? '').toMatch(/already exited/i);
    } finally {
      vi.restoreAllMocks();
      app.restore();
    }
  });

  it('sends one signal even if the confirm key is pressed twice quickly', async () => {
    // doKill awaits a spawn now, so a second keypress inside that window used
    // to start a second, concurrent kill.
    const provider = new MockProvider();
    vi.spyOn(provider, 'identity').mockImplementation(
      async (pid: number) =>
        new Promise((r) => setTimeout(() => r({ startTime: 1_700_000_000_000 + pid * 1000 }), 80)),
    );
    const kill = vi.fn();
    const app = await mount(provider, kill);
    try {
      app.stdin.write('k');
      await wait(200);
      app.stdin.write('t');
      app.stdin.write('t');
      await wait(400);
      expect(kill).toHaveBeenCalledTimes(1);
    } finally {
      vi.restoreAllMocks();
      app.restore();
    }
  });
});

describe('I-13: the confirmation refuses when there is no process sample', () => {
  it('cannot be confirmed while the process collector is failing', async () => {
    const provider = new MockProvider();
    const kill = vi.fn();
    const app = await mount(provider, kill);
    try {
      app.stdin.write('k');
      await wait(200);
      // The sample disappears (a `ps` timeout is enough), taking the parent map
      // — and with it the ancestor guard — with it.
      vi.spyOn(provider, 'processes').mockRejectedValue(new Error('ps timed out'));
      app.stdin.write('r');
      await wait(400);
      app.stdin.write('t');
      await wait(300);
      expect(kill).not.toHaveBeenCalled();
    } finally {
      vi.restoreAllMocks();
      app.restore();
    }
  });
});
