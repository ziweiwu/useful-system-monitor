import { render } from 'ink-testing-library';
import { describe, expect, it, vi } from 'vitest';
import { App } from '../src/app.js';
import { MockProvider } from '../src/providers/mock/provider.js';
import type { MetricsProvider, Tiers } from '../src/providers/types.js';

const FAST: Tiers = { cpu: 100, memory: 200, processes: 200, battery: 300, disk: 400 };
const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));
const ESC = String.fromCharCode(27);
const ANSI = new RegExp(`${ESC}\\[[0-9;]*m`, 'g');
const ENTER = '\r';

/**
 * Captures unhandled rejections for the duration of a test.
 *
 * Node's default is to throw on one, so an uncaught rejection inside the app is
 * not a warning — it terminates the process. Under a test runner it is merely
 * reported alongside a passing test, so asserting on it explicitly is the only
 * way this suite can actually fail when the catch is removed.
 */
function watchRejections() {
  const seen: unknown[] = [];
  const onRejection = (err: unknown) => seen.push(err);
  process.on('unhandledRejection', onRejection);
  return {
    seen,
    stop() {
      process.off('unhandledRejection', onRejection);
    },
  };
}

async function mount(provider: MetricsProvider) {
  const prev = [process.stdout.columns, process.stdout.rows] as const;
  process.stdout.columns = 100;
  process.stdout.rows = 30;
  const app = render(<App provider={provider} tiers={FAST} demo killFn={() => {}} />);
  await wait(400);
  return {
    ...app,
    frame: () => (app.lastFrame() ?? '').replace(ANSI, ''),
    restore() {
      app.unmount();
      process.stdout.columns = prev[0];
      process.stdout.rows = prev[1];
    },
  };
}

/**
 * I-11 — and specifically the collectors that are *not* driven by `poll()`.
 *
 * `poll()` wraps every panel collector in a try/catch, so those were covered.
 * `host()` and `commandLine()` are called directly, and their rejections were
 * caught by nothing: an unhandled rejection takes the whole process down with
 * a React stack trace, which is the exact failure this invariant rules out.
 */
describe('I-11: a failing collector degrades one panel, never the app', () => {
  it.each(['cpu', 'memory', 'disk', 'battery', 'processes', 'host'] as const)(
    'survives %s() rejecting, on every screen',
    async (broken) => {
      const provider = new MockProvider();
      vi.spyOn(provider, broken).mockRejectedValue(new Error(`${broken} exploded`));
      const rejections = watchRejections();
      const app = await mount(provider);
      try {
        for (const key of ['2', '3', '4', '5', '1']) {
          app.stdin.write(key);
          await wait(100);
        }
        await wait(150);
        // The app is still drawing, and the frame still fits its terminal.
        expect(app.frame()).toContain('useful-system-monitor');
        expect(app.frame().split('\n').length).toBeLessThanOrEqual(30);
        // And nothing escaped: an unhandled rejection would kill the process.
        expect(rejections.seen).toEqual([]);
      } finally {
        rejections.stop();
        vi.restoreAllMocks();
        app.restore();
      }
    },
  );

  it('survives commandLine() rejecting when the detail panel opens', async () => {
    const provider = new MockProvider();
    vi.spyOn(provider, 'commandLine').mockRejectedValue(new Error('ps timed out'));
    const rejections = watchRejections();
    const app = await mount(provider);
    try {
      app.stdin.write(ENTER);
      await wait(400);
      expect(app.frame()).toContain('esc back');
      expect(app.frame().split('\n').length).toBeLessThanOrEqual(30);
      expect(rejections.seen).toEqual([]);
    } finally {
      rejections.stop();
      vi.restoreAllMocks();
      app.restore();
    }
  });

  it('keeps the other panels when one collector is down', async () => {
    const provider = new MockProvider();
    vi.spyOn(provider, 'battery').mockRejectedValue(new Error('no battery service'));
    const app = await mount(provider);
    try {
      const f = app.frame();
      expect(f).toContain('CPU');
      expect(f).toContain('MEM');
      expect(f).toContain('DISK');
      // The broken one says so rather than showing a stale or invented number.
      expect(f).toContain('unavailable');
    } finally {
      vi.restoreAllMocks();
      app.restore();
    }
  });
});
