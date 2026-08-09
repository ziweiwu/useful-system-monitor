import { render } from 'ink-testing-library';
import { describe, expect, it, vi } from 'vitest';
import { App } from '../src/app.js';
import { displayWidth } from '../src/core/width.js';
import { MockProvider } from '../src/providers/mock/provider.js';
import { DEFAULT_TIERS } from '../src/providers/types.js';
import { wattsAreMeaningful } from '../src/ui/ProcessTable.js';

const ESC = String.fromCharCode(27);
const DOWN = `${ESC}[B`;
const UP = `${ESC}[A`;
const ENTER = '\r';
const ANSI = new RegExp(`${ESC}\\[[0-9;]*m`, 'g');

function lines(frame: string | undefined): string[] {
  return (frame ?? '').replace(ANSI, '').split('\n');
}

const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

async function mount(columns: number, rows = 34, killFn: (pid: number, signal: string) => void = () => {}) {
  const prevCols = process.stdout.columns;
  const prevRows = process.stdout.rows;
  process.stdout.columns = columns;
  process.stdout.rows = rows;
  const app = render(
    <App provider={new MockProvider()} tiers={DEFAULT_TIERS} demo killFn={killFn} />,
  );
  await wait(250);
  return {
    ...app,
    restore() {
      app.unmount();
      process.stdout.columns = prevCols;
      process.stdout.rows = prevRows;
    },
  };
}

/** The PID on the row currently marked with the selection cursor. */
function selectedPid(frame: string | undefined): string | null {
  for (const l of lines(frame)) {
    const m = /^\s*>\s+(\d+)\s/.exec(l);
    if (m) return m[1]!;
  }
  return null;
}

describe('I-19: layout never overflows the terminal', () => {
  it.each([80, 100, 160, 200])('fits within %i columns', async (columns) => {
    const app = await mount(columns);
    try {
      for (const l of lines(app.lastFrame())) {
        expect(displayWidth(l)).toBeLessThanOrEqual(columns);
      }
    } finally {
      app.restore();
    }
  });

  it('still renders the dashboard at 80x24', async () => {
    const app = await mount(80, 24);
    try {
      expect(app.lastFrame()).toContain('PID');
      expect(app.lastFrame()).toContain('useful-system-monitor');
    } finally {
      app.restore();
    }
  });
});

describe('I-21: selection is keyed by PID, not row index', () => {
  it('keeps the same process selected when the sort key changes', async () => {
    const app = await mount(120);
    try {
      // Move off the first row so an index-based selection would be exposed.
      app.stdin.write(DOWN);
      await wait(60);
      app.stdin.write(DOWN);
      await wait(120);
      const before = selectedPid(app.lastFrame());
      expect(before).not.toBeNull();

      // Re-sort by memory: the row order changes completely.
      app.stdin.write('m');
      await wait(250);

      expect(selectedPid(app.lastFrame())).toBe(before);
    } finally {
      app.restore();
    }
  });

  it('moves the cursor with the arrow keys', async () => {
    const app = await mount(120);
    try {
      const first = selectedPid(app.lastFrame());
      app.stdin.write(DOWN);
      await wait(150);
      expect(selectedPid(app.lastFrame())).not.toBe(first);
    } finally {
      app.restore();
    }
  });
});

describe('I-14 / I-15: kill modal', () => {
  it('names the process and requires a second press for SIGKILL', async () => {
    const app = await mount(120);
    try {
      app.stdin.write('k');
      await wait(200);
      const frame = app.lastFrame() ?? '';
      expect(frame).toMatch(/KILL PROCESS|REFUSED/);
      expect(frame).toContain('SIGTERM');
      expect(frame).toMatch(/press twice/);
    } finally {
      app.restore();
    }
  });

  it('refuses a protected process and explains the consequence', async () => {
    const app = await mount(120);
    try {
      app.stdin.write('/');
      await wait(60);
      app.stdin.write('WindowServer');
      await wait(150);
      app.stdin.write(ENTER);
      await wait(150);
      app.stdin.write('k');
      await wait(200);
      const frame = app.lastFrame() ?? '';
      expect(frame).toContain('REFUSED');
      expect(frame).toMatch(/log you out|wedge/i);
    } finally {
      app.restore();
    }
  });
});

describe('view switching', () => {
  it.each([
    ['2', /P0|load/],
    ['3', /wired|swap/],
    ['4', /BATTERY|TOP ENERGY/],
    ['5', /VOLUMES/],
  ])('key %s renders its view', async (keyPress, expected) => {
    const app = await mount(120);
    try {
      app.stdin.write(keyPress);
      await wait(250);
      expect(app.lastFrame() ?? '').toMatch(expected);
    } finally {
      app.restore();
    }
  });
});

describe('disk view', () => {
  it('lists every mounted volume, not just the root filesystem', async () => {
    const app = await mount(120);
    try {
      app.stdin.write('5');
      await wait(250);
      const frame = app.lastFrame() ?? '';
      expect(frame).toContain('/Volumes/Backup');
      expect(frame).toContain('/Volumes/media');
      // The root row is present as a bare mount point.
      expect(frame).toMatch(/^\s*\/\s/m);
    } finally {
      app.restore();
    }
  });

  it('marks network shares and warns on a nearly-full volume', async () => {
    const app = await mount(120);
    try {
      app.stdin.write('5');
      await wait(250);
      const frame = app.lastFrame() ?? '';
      expect(frame).toContain('net');
      // The mock share sits at ~93%, which is what the warning exists for.
      expect(frame).toMatch(/above 90%/);
    } finally {
      app.restore();
    }
  });

  it('fits its widest row inside the terminal (I-19)', async () => {
    for (const columns of [80, 100, 160]) {
      const app = await mount(columns);
      try {
        app.stdin.write('5');
        await wait(250);
        for (const l of lines(app.lastFrame())) {
          expect(displayWidth(l)).toBeLessThanOrEqual(columns);
        }
      } finally {
        app.restore();
      }
    }
  });
});

describe('I-14: a refused modal is inert, not merely labelled', () => {
  it('sends no signal however many times the signal keys are pressed', async () => {
    const kill = vi.fn();
    const app = await mount(120, 34, kill);
    try {
      app.stdin.write('/');
      await wait(60);
      app.stdin.write('WindowServer');
      await wait(150);
      app.stdin.write(ENTER);
      await wait(150);
      app.stdin.write('k');
      await wait(200);
      expect(app.lastFrame() ?? '').toContain('REFUSED');

      // t and k must do nothing at all here.
      app.stdin.write('t');
      await wait(80);
      app.stdin.write('k');
      await wait(80);
      app.stdin.write('k');
      await wait(150);

      expect(kill).not.toHaveBeenCalled();
      expect(app.lastFrame() ?? '').toContain('REFUSED');
    } finally {
      app.restore();
    }
  });

  it('kills an ordinary process when confirmed', async () => {
    const kill = vi.fn();
    const app = await mount(120, 34, kill);
    try {
      app.stdin.write('k');
      await wait(200);
      expect(app.lastFrame() ?? '').toContain('KILL PROCESS');
      app.stdin.write('t');
      await wait(200);
      expect(kill).toHaveBeenCalledTimes(1);
      expect(kill.mock.calls[0]![1]).toBe('SIGTERM');
    } finally {
      app.restore();
    }
  });
});

describe('power column never shows a column of zeros', () => {
  it('treats a plugged-in, non-moving battery as having no meaningful draw', () => {
    // On AC and holding charge, total draw is ~0; per-process watts would all
    // round to "0.0W", which tells the user nothing.
    expect(wattsAreMeaningful(0)).toBe(false);
    expect(wattsAreMeaningful(0.2)).toBe(false);
    expect(wattsAreMeaningful(null)).toBe(false);
  });

  it('uses watts once charge is actually moving, in either direction', () => {
    expect(wattsAreMeaningful(-18.4)).toBe(true);
    expect(wattsAreMeaningful(34.9)).toBe(true);
  });

  it('labels the column with whichever unit it is showing', async () => {
    // The mock discharges at ~17W, so the column must be in watts.
    const app = await mount(120);
    try {
      const frame = app.lastFrame() ?? '';
      expect(frame).toContain('POWER est');
      expect(frame).not.toContain('ENERGY est');
    } finally {
      app.restore();
    }
  });
});

describe('process detail view', () => {
  it('opens on enter and shows identity, path and argv', async () => {
    const app = await mount(120, 40);
    try {
      app.stdin.write(ENTER);
      await wait(400);
      const frame = app.lastFrame() ?? '';
      expect(frame).toContain('PID');
      expect(frame).toContain('Parent');
      expect(frame).toContain('Path');
      expect(frame).toContain('Command');
      expect(frame).toContain('esc back');
    } finally {
      app.restore();
    }
  });

  it('closes on esc and returns to the table', async () => {
    const app = await mount(120, 40);
    try {
      app.stdin.write(ENTER);
      await wait(300);
      expect(app.lastFrame() ?? '').toContain('esc back');
      app.stdin.write(ESC);
      await wait(300);
      const frame = app.lastFrame() ?? '';
      expect(frame).toContain('enter info');
      expect(frame).not.toContain('esc back');
    } finally {
      app.restore();
    }
  });

  it('goes straight to the kill modal with k', async () => {
    const app = await mount(120, 40);
    try {
      app.stdin.write(ENTER);
      await wait(300);
      app.stdin.write('k');
      await wait(300);
      expect(app.lastFrame() ?? '').toMatch(/KILL PROCESS|REFUSED/);
    } finally {
      app.restore();
    }
  });

  it('never overflows the terminal, even with very long paths', async () => {
    for (const columns of [80, 120]) {
      const app = await mount(columns, 40);
      try {
        app.stdin.write(ENTER);
        await wait(400);
        for (const l of lines(app.lastFrame())) {
          expect(displayWidth(l)).toBeLessThanOrEqual(columns);
        }
      } finally {
        app.restore();
      }
    }
  });
});

describe('I-26: the scroll window always contains the selection', () => {
  it('keeps the cursor on screen past the bottom of the window', async () => {
    const app = await mount(120, 34);
    try {
      // Far more presses than the window is tall, so the old fixed
      // slice(0, rows) would have left the cursor off-screen entirely.
      for (let i = 0; i < 45; i++) {
        app.stdin.write(DOWN);
        await wait(5);
      }
      const cursors = lines(app.lastFrame()).filter((l) => /^\s*>\s+\d+/.test(l));
      expect(cursors).toHaveLength(1);
      expect(selectedPid(app.lastFrame())).not.toBeNull();
    } finally {
      app.restore();
    }
  });

  it('scrolls back up and returns to the first row', async () => {
    const app = await mount(120, 34);
    try {
      const first = selectedPid(app.lastFrame());
      for (let i = 0; i < 40; i++) {
        app.stdin.write(DOWN);
        await wait(4);
      }
      expect(selectedPid(app.lastFrame())).not.toBe(first);
      for (let i = 0; i < 60; i++) {
        app.stdin.write(UP);
        await wait(4);
      }
      expect(selectedPid(app.lastFrame())).toBe(first);
      expect(lines(app.lastFrame()).filter((l) => /^\s*>\s+\d+/.test(l))).toHaveLength(1);
    } finally {
      app.restore();
    }
  });

  it('never renders more lines than the terminal has rows', async () => {
    for (const rows of [24, 30, 34, 50]) {
      const app = await mount(120, rows);
      try {
        expect(lines(app.lastFrame()).length).toBeLessThanOrEqual(rows);
        for (let i = 0; i < 45; i++) {
          app.stdin.write(DOWN);
          await wait(3);
        }
        expect(lines(app.lastFrame()).length).toBeLessThanOrEqual(rows);
      } finally {
        app.restore();
      }
    }
  });

  it('fits with the detail panel open, down to the 80x24 minimum', async () => {
    // The panel used to render *below* the full table rather than replacing it,
    // which put 93 lines into a 24-row terminal and scrolled away the header
    // and every card. Verified against a real pty, not just this harness.
    for (const rows of [24, 30, 40]) {
      const app = await mount(100, rows);
      try {
        app.stdin.write(ENTER);
        await wait(300);
        expect(app.lastFrame() ?? '').toContain('esc back');
        expect(lines(app.lastFrame()).length).toBeLessThanOrEqual(rows);
      } finally {
        app.restore();
      }
    }
  });

  it('fits with a toast showing', async () => {
    for (const rows of [24, 34]) {
      const app = await mount(100, rows);
      try {
        app.stdin.write('k');
        await wait(250);
        app.stdin.write('t');
        await wait(300);
        expect(lines(app.lastFrame()).length).toBeLessThanOrEqual(rows);
      } finally {
        app.restore();
      }
    }
  });

  it('closes the detail panel rather than blanking when its process exits', async () => {
    const app = await mount(120, 34);
    try {
      app.stdin.write(ENTER);
      await wait(300);
      expect(app.lastFrame() ?? '').toContain('esc back');
      app.stdin.write('k');
      await wait(250);
      app.stdin.write('t');
      await wait(600);
      // Back to the table, not an empty screen.
      const frame = app.lastFrame() ?? '';
      expect(frame).toContain('PROCESS');
      expect(lines(frame).length).toBeGreaterThan(10);
    } finally {
      app.restore();
    }
  });
});

describe('expanding the working set on request', () => {
  it('+ widens the set and - narrows it again', async () => {
    const app = await mount(120, 34);
    try {
      const cap = () => /top (\S+) of/.exec(lines(app.lastFrame()).join('\n'))?.[1] ?? '';
      expect(cap()).toBe('50');
      app.stdin.write('+');
      await wait(150);
      expect(cap()).toBe('150');
      app.stdin.write('+');
      await wait(150);
      expect(cap()).toBe('300');
      app.stdin.write('+');
      await wait(150);
      expect(cap()).toBe('all');
      // Saturates rather than wrapping round to the smallest step.
      app.stdin.write('+');
      await wait(150);
      expect(cap()).toBe('all');
      app.stdin.write('-');
      await wait(150);
      expect(cap()).toBe('300');
    } finally {
      app.restore();
    }
  });

  it('offers the expansion where the roll-up says rows are hidden', async () => {
    const app = await mount(120, 34);
    try {
      expect(lines(app.lastFrame()).join('\n')).toMatch(/… \d+ others {2}\(\+ show more\)/);
      // At the widest step nothing is rolled up, so the offer is withdrawn.
      for (let i = 0; i < 3; i++) {
        app.stdin.write('+');
        await wait(150);
      }
      expect(lines(app.lastFrame()).join('\n')).not.toMatch(/show more/);
    } finally {
      app.restore();
    }
  });

  it('stays within the terminal when expanded and scrolled deep', async () => {
    const app = await mount(100, 34);
    try {
      for (let i = 0; i < 3; i++) {
        app.stdin.write('+');
        await wait(150);
      }
      for (let i = 0; i < 120; i++) {
        app.stdin.write(DOWN);
        await wait(3);
      }
      expect(lines(app.lastFrame()).length).toBeLessThanOrEqual(34);
      for (const l of lines(app.lastFrame())) {
        expect(displayWidth(l)).toBeLessThanOrEqual(100);
      }
      expect(lines(app.lastFrame()).filter((l) => /^\s*>\s+\d+/.test(l))).toHaveLength(1);
    } finally {
      app.restore();
    }
  });
});
