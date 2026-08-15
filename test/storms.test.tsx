import { render } from 'ink-testing-library';
import { describe, expect, it } from 'vitest';
import { App } from '../src/app.js';
import { displayWidth } from '../src/core/width.js';
import { MockProvider } from '../src/providers/mock/provider.js';
import { parseTopPower } from '../src/providers/darwin/power.js';
import type { Tiers } from '../src/providers/types.js';

const FAST: Tiers = { cpu: 60, memory: 120, processes: 120, battery: 200, disk: 300 };
const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));
const ESC = String.fromCharCode(27);
const ANSI = new RegExp(`${ESC}\\[[0-9;]*m`, 'g');

function rng(seed: number) {
  let s = seed >>> 0;
  return () => (s = (s * 1664525 + 1013904223) >>> 0) / 2 ** 32;
}

/** A resize is one event; a drag is forty of them with no settle between. */
describe('I-19: a burst of resizes settles into a coherent frame', () => {
  it.each([1, 2, 3])('seed %i', async (seed) => {
    const rand = rng(seed);
    const prev = [process.stdout.columns, process.stdout.rows] as const;
    process.stdout.columns = 100;
    process.stdout.rows = 30;
    const app = render(<App provider={new MockProvider()} tiers={FAST} demo killFn={() => {}} />);
    try {
      await wait(300);
      for (let i = 0; i < 40; i++) {
        process.stdout.columns = 44 + Math.floor(rand() * 100);
        process.stdout.rows = 8 + Math.floor(rand() * 34);
        process.stdout.emit('resize');
        if (i % 5 === 0) app.stdin.write(['1', '2', '3', '4', '5', 'k', '/', '\r', ESC][i % 9]!);
      }
      process.stdout.columns = 100;
      process.stdout.rows = 30;
      process.stdout.emit('resize');
      await wait(400);
      const lines = (app.lastFrame() ?? '').replace(ANSI, '').split('\n');
      expect(lines.join('\n')).toContain('useful-system-monitor');
      expect(lines.length).toBeLessThanOrEqual(30);
      for (const l of lines) expect(displayWidth(l)).toBeLessThanOrEqual(100);
    } finally {
      app.unmount();
      process.stdout.columns = prev[0];
      process.stdout.rows = prev[1];
    }
  }, 30_000);
});

describe('I-8: holding the refresh key skips overruns rather than queueing them', () => {
  it('60 presses do not become 60 collector runs', async () => {
    let calls = 0;
    class Counting extends MockProvider {
      override async processes(limit?: number) {
        calls++;
        await wait(40); // still in flight when the next press lands
        return super.processes(limit);
      }
    }
    const prev = [process.stdout.columns, process.stdout.rows] as const;
    process.stdout.columns = 100;
    process.stdout.rows = 30;
    const app = render(<App provider={new Counting()} tiers={FAST} demo killFn={() => {}} />);
    try {
      await wait(300);
      const before = calls;
      for (let i = 0; i < 60; i++) app.stdin.write('r');
      await wait(600);
      const fired = calls - before;
      expect(fired, `60 presses queued ${fired} runs`).toBeLessThan(25);
      expect(fired).toBeGreaterThan(0);
    } finally {
      app.unmount();
      process.stdout.columns = prev[0];
      process.stdout.rows = prev[1];
    }
  }, 30_000);
});

describe('parseTopPower survives hostile `top` output', () => {
  it.each([
    ['empty', ''],
    ['header only', 'PID    POWER\n'],
    ['no header', '123 4.5\n'],
    ['truncated', 'PID  POWER\n123'],
    ['negative', 'PID  POWER\n123  -5.0\n'],
    ['absurd', 'PID  POWER\n123  99999999999999\n'],
    ['comma decimal', 'PID  POWER\n123  4,5\n'],
  ])('%s', (_label, input) => {
    const m = parseTopPower(input);
    for (const [pid, v] of m) {
      expect(Number.isFinite(pid)).toBe(true);
      expect(Number.isFinite(v)).toBe(true);
    }
  });

  it('reads the last sample block, not the first', () => {
    // `top -l 2` prints an all-zero block first, because energy impact is
    // itself a rate and top has nothing to compare against yet.
    expect(parseTopPower('PID POWER\n1 0.0\nPID POWER\n1 12.5\n').get(1)).toBe(12.5);
  });
});
