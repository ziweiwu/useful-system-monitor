import { render } from 'ink-testing-library';
import { describe, expect, it, vi } from 'vitest';
import { App } from '../src/app.js';
import { MockProvider } from '../src/providers/mock/provider.js';
import { DEFAULT_TIERS } from '../src/providers/types.js';

const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));
const ESC = String.fromCharCode(27);
const ANSI = new RegExp(`${ESC}\\[[0-9;]*m`, 'g');

async function mount(kill: (p: number, s: string) => void = () => {}) {
  const prev = [process.stdout.columns, process.stdout.rows] as const;
  process.stdout.columns = 100;
  process.stdout.rows = 30;
  const app = render(<App provider={new MockProvider()} tiers={DEFAULT_TIERS} demo killFn={kill} />);
  await wait(350);
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
 * A terminal delivers a burst of keys — a paste, or a fast typist — as one
 * chunk, and `useInput` handles every key in that chunk against the state of
 * the render that created it. So `/` set filter mode and the characters after
 * it still saw `false`, falling through to the main keymap: pasting "chrome"
 * silently re-sorted by energy (the `e`), and pasting "book" opened the kill
 * confirmation (the `k`).
 */
describe('a pasted filter is filter text, not a burst of commands', () => {
  it('filters the same whether typed or pasted', async () => {
    const typed = await mount();
    try {
      typed.stdin.write('/');
      for (const ch of 'chrome') {
        typed.stdin.write(ch);
        await wait(40);
      }
      await wait(250);
      const slow = typed.frame();
      expect(slow).toContain('filter chrome');

      const pasted = await mount();
      try {
        // No awaits: one chunk, one closure — the case that used to break.
        pasted.stdin.write('/');
        for (const ch of 'chrome') pasted.stdin.write(ch);
        await wait(250);
        expect(pasted.frame()).toContain('filter chrome');
        expect(pasted.frame()).toContain('sort cpu');
      } finally {
        pasted.restore();
      }
    } finally {
      typed.restore();
    }
  }, 20_000);

  it.each(['book', 'kill-me', 'kkk'])(
    'pasting %s does not open the kill confirmation',
    async (text) => {
      const kill = vi.fn();
      const app = await mount(kill);
      try {
        app.stdin.write('/');
        for (const ch of text) app.stdin.write(ch);
        await wait(300);
        expect(app.frame()).not.toMatch(/KILL PROCESS|REFUSED/);
        expect(app.frame()).toContain(`filter ${text}`);
        expect(kill).not.toHaveBeenCalled();
      } finally {
        app.restore();
      }
    },
    20_000,
  );

  it('does not let one chunk both open a confirmation and answer it', async () => {
    /*
     * The other half of the same problem, deliberately left fail-safe: the kill
     * and detail modes still read possibly-stale state, so a burst cannot drive
     * the confirmation it just opened. I-15 wants a second, distinct keypress,
     * and one chunk is not two keypresses.
     */
    const kill = vi.fn();
    const app = await mount(kill);
    try {
      for (const ch of ['k', 't']) app.stdin.write(ch);
      await wait(400);
      expect(kill).not.toHaveBeenCalled();
      for (const ch of ['k', 'k', 'k']) app.stdin.write(ch);
      await wait(400);
      expect(kill).not.toHaveBeenCalled();
    } finally {
      app.restore();
    }
  }, 20_000);
});
