import { render } from 'ink-testing-library';
import { describe, expect, it, vi } from 'vitest';
import { App, MIN_COLUMNS, MIN_ROWS } from '../src/app.js';
import { MockProvider } from '../src/providers/mock/provider.js';
import { DEFAULT_TIERS } from '../src/providers/types.js';
import { waitForFrame } from './helpers.js';

const ESC = String.fromCharCode(27);
const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

async function mount(columns: number, rows: number) {
  const prev = [process.stdout.columns, process.stdout.rows] as const;
  process.stdout.columns = columns;
  process.stdout.rows = rows;
  const kill = vi.fn();
  const app = render(<App provider={new MockProvider()} tiers={DEFAULT_TIERS} demo killFn={kill} />);
  await wait(300);
  return {
    ...app,
    kill,
    restore() {
      app.unmount();
      process.stdout.columns = prev[0];
      process.stdout.rows = prev[1];
    },
  };
}

/**
 * I-15 asks for a kill confirmed *by name*, and I-26 says nothing is drawn
 * below MIN_COLUMNS x MIN_ROWS.
 *
 * Those two met badly. The confirmation is a mode, and a mode that is not
 * rendered is still entered: on a 49-column terminal `k` then `t` sent SIGTERM,
 * and `k` `k` `k` sent SIGKILL, with the process name, the "unsaved work" line
 * and the whole panel unrendered — the user confirming nothing they could read.
 *
 * `verify:layout` drove exactly this path at 44 and 49 columns and passed,
 * because it only ever asserted the frame did not overflow; `qa:fuzz` picks
 * sizes from MIN_COLUMNS upward and never looked below. So this is checked by
 * the outcome that matters: whether a signal left the app.
 */
describe('I-15 / I-26: keys are inert on a terminal too small to draw the confirmation', () => {
  it('sends no SIGTERM when the confirmation cannot be shown', async () => {
    const app = await mount(MIN_COLUMNS - 1, 24);
    try {
      expect(app.lastFrame() ?? '').toContain('too small');
      app.stdin.write('k');
      await wait(200);
      expect(app.lastFrame() ?? '', 'no confirmation is drawn').not.toContain('KILL PROCESS');
      app.stdin.write('t');
      await wait(400);
      expect(app.kill, 'signalled a process the user could not see').not.toHaveBeenCalled();
    } finally {
      app.restore();
    }
  });

  it('sends no SIGKILL either, however many times k is pressed', async () => {
    const app = await mount(MIN_COLUMNS - 1, 24);
    try {
      for (const k of ['k', 'k', 'k']) {
        app.stdin.write(k);
        await wait(150);
      }
      await wait(300);
      expect(app.kill).not.toHaveBeenCalled();
    } finally {
      app.restore();
    }
  });

  it('is the height bound too, not just the width', async () => {
    const app = await mount(80, MIN_ROWS - 1);
    try {
      expect(app.lastFrame() ?? '').toContain('too small');
      app.stdin.write('k');
      await wait(150);
      app.stdin.write('t');
      await wait(400);
      expect(app.kill).not.toHaveBeenCalled();
    } finally {
      app.restore();
    }
  });

  /*
   * Blocking the keymap must not block the way out. `exit()` unmounts, which
   * clears the frame — and an unmounted app stays cleared across a resize that
   * would otherwise bring the whole dashboard back.
   */
  it('still quits, because that is the one thing it can honestly offer', async () => {
    const app = await mount(MIN_COLUMNS - 1, 24);
    try {
      expect(app.lastFrame() ?? '').toContain('too small');
      app.stdin.write('q');
      await waitForFrame(() => app.lastFrame(), (f) => f.trim() === '', 'the app to unmount');

      process.stdout.columns = 100;
      process.stdout.rows = 30;
      process.stdout.emit('resize');
      await wait(400);

      expect(app.lastFrame()?.trim() ?? '', 'still rendering after q').toBe('');
    } finally {
      app.restore();
    }
  });

  /*
   * A mode can still be open here: the user opens the confirmation at a normal
   * size and then drags the window narrow. Growing it back must not reveal an
   * armed confirmation they have long since forgotten about, so `esc` is the
   * other key that keeps working.
   */
  it('lets esc clear a mode inherited from a resize', async () => {
    const app = await mount(100, 30);
    try {
      app.stdin.write('k');
      await waitForFrame(() => app.lastFrame(), (f) => f.includes('KILL PROCESS'), 'the confirmation');

      process.stdout.columns = MIN_COLUMNS - 1;
      process.stdout.emit('resize');
      await waitForFrame(() => app.lastFrame(), (f) => f.includes('too small'), 'the size complaint');

      app.stdin.write(ESC);
      await wait(200);
      process.stdout.columns = 100;
      process.stdout.emit('resize');
      await waitForFrame(() => app.lastFrame(), (f) => !f.includes('too small'), 'the dashboard back');

      expect(app.lastFrame() ?? '', 'a confirmation survived the resize').not.toContain('KILL PROCESS');
      expect(app.kill).not.toHaveBeenCalled();
    } finally {
      app.restore();
    }
  });
});
