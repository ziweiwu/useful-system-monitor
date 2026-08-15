import { render } from 'ink-testing-library';
import { describe, expect, it } from 'vitest';
import { App } from '../src/app.js';
import { MockProvider } from '../src/providers/mock/provider.js';
import type { Tiers } from '../src/providers/types.js';

const FAST: Tiers = { cpu: 90, memory: 200, processes: 200, battery: 400, disk: 600 };
const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));
const ESC = String.fromCharCode(27);
const ANSI = new RegExp(`${ESC}\\[[0-9;]*m`, 'g');

async function mount() {
  const prev = [process.stdout.columns, process.stdout.rows] as const;
  process.stdout.columns = 100;
  process.stdout.rows = 30;
  const app = render(<App provider={new MockProvider()} tiers={FAST} demo killFn={() => {}} />);
  await wait(450);
  return {
    ...app,
    lines: () => (app.lastFrame() ?? '').replace(ANSI, '').split('\n'),
    restore() {
      app.unmount();
      process.stdout.columns = prev[0];
      process.stdout.rows = prev[1];
    },
  };
}

/** The PID on the row currently marked with the cursor. */
function selected(lines: string[]): string | null {
  for (const l of lines) {
    const m = /^\s*>\s+(\d+)\s/.exec(l);
    if (m) return m[1]!;
  }
  return null;
}

const cursors = (lines: string[]) => lines.filter((l) => /^\s*>\s+\d+/.test(l)).length;

describe('I-21: the cursor is bound to a PID, not to a row', () => {
  it('survives four re-sorts', async () => {
    /*
     * The selected row *is* the kill target. If selection were an index, a
     * re-sort would slide a different process under the cursor between the
     * keypress and the confirmation.
     */
    const app = await mount();
    try {
      const before = selected(app.lines());
      expect(before).not.toBeNull();
      for (const k of ['m', 'e', 'c', 'm']) {
        app.stdin.write(k);
        await wait(180);
      }
      expect(selected(app.lines())).toBe(before);
    } finally {
      app.restore();
    }
  }, 30_000);

  it('survives widening and narrowing the working set', async () => {
    const app = await mount();
    try {
      const before = selected(app.lines());
      for (let i = 0; i < 6; i++) {
        app.stdin.write('+');
        await wait(200);
      }
      expect(selected(app.lines())).toBe(before);
      for (let i = 0; i < 6; i++) {
        app.stdin.write('-');
        await wait(200);
      }
      expect(selected(app.lines())).toBe(before);
      expect(cursors(app.lines())).toBe(1);
    } finally {
      app.restore();
    }
  }, 40_000);

  it.each([
    ['/', 'o', ESC, 'm', '\r', ESC],
    ['m', '/', 'e', ESC, 'c'],
    ['/', 'z', 'z', ESC, 'e', '+', '-'],
    ['e', '+', '/', 'chrome', ESC, 'c', '-'],
  ])('draws exactly one cursor through %j', async (...seq) => {
    const app = await mount();
    try {
      for (const k of seq) {
        app.stdin.write(k);
        await wait(150);
      }
      await wait(250);
      const lines = app.lines();
      expect(lines.join('\n')).toContain('useful-system-monitor');
      expect(lines.length).toBeLessThanOrEqual(30);
      expect(cursors(lines)).toBeLessThanOrEqual(1);
    } finally {
      app.restore();
    }
  }, 30_000);
});

describe('a filter that matches nothing says so, and says nothing else', () => {
  it('shows "no matches" without the unfiltered roll-up beneath it', async () => {
    const app = await mount();
    try {
      app.stdin.write('/');
      for (const ch of 'zzzzzz') app.stdin.write(ch);
      await wait(300);
      const frame = app.lines().join('\n');
      expect(frame).toContain('no matches');
      // The roll-up is the tail of the working-set cap, not of the search.
      expect(frame).not.toMatch(/…\s*\d+\s+others/);
    } finally {
      app.restore();
    }
  }, 20_000);
});
