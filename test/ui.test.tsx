import { render } from 'ink-testing-library';
import { describe, expect, it, vi } from 'vitest';
import { App } from '../src/app.js';
import { displayWidth } from '../src/core/width.js';
import type { CpuData, DiskData, HostInfo } from '../src/core/types.js';
import { MockProvider } from '../src/providers/mock/provider.js';
import { DEFAULT_TIERS, type MetricsProvider } from '../src/providers/types.js';
import { wattsAreMeaningful } from '../src/ui/ProcessTable.js';

const ESC = String.fromCharCode(27);
const DOWN = `${ESC}[B`;
const UP = `${ESC}[A`;
const RIGHT = `${ESC}[C`;
const LEFT = `${ESC}[D`;
const ENTER = '\r';
const ANSI = new RegExp(`${ESC}\\[[0-9;]*m`, 'g');

function lines(frame: string | undefined): string[] {
  return (frame ?? '').replace(ANSI, '').split('\n');
}

const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

/**
 * A machine much larger than the mock's: 24 cores and 14 mounted volumes.
 *
 * Core counts and mount counts are the two list lengths that come from the
 * hardware rather than from the design, so they are what push a screen past the
 * bottom of the terminal. See I-26.
 */
class BigMachine extends MockProvider {
  override async host(): Promise<HostInfo> {
    return { ...(await super.host()), cores: 24, perfCores: 16 };
  }
  override async cpu(): Promise<CpuData> {
    return { ...(await super.cpu()), perCore: Array.from({ length: 24 }, (_, i) => (i * 17) % 100) };
  }
  override async disk(): Promise<DiskData> {
    const d = await super.disk();
    return {
      ...d,
      volumes: [
        ...d.volumes,
        ...Array.from({ length: 10 }, (_, i) => ({
          mount: `/Volumes/share${i}`,
          device: `//nas/share${i}`,
          totalBytes: 2_000_000_000_000,
          usedBytes: 1_000_000_000_000,
          freeBytes: 1_000_000_000_000,
          network: true,
        })),
      ],
    };
  }
}

async function mount(
  columns: number,
  rows = 34,
  killFn: (pid: number, signal: string) => void = () => {},
  provider: MetricsProvider = new MockProvider(),
) {
  const prevCols = process.stdout.columns;
  const prevRows = process.stdout.rows;
  process.stdout.columns = columns;
  process.stdout.rows = rows;
  const app = render(<App provider={provider} tiers={DEFAULT_TIERS} demo killFn={killFn} />);
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

describe('I-27: arrow keys walk the view strip', () => {
  it('marks the active view in the tab strip without relying on colour', async () => {
    const app = await mount(120);
    try {
      // Brackets, not just a hue: the strip has to survive NO_COLOR (I-23).
      expect(app.lastFrame() ?? '').toContain('[1 OVERVIEW]');
      expect(app.lastFrame() ?? '').toContain(' 3 MEMORY ');
      app.stdin.write('3');
      await wait(250);
      expect(app.lastFrame() ?? '').toContain('[3 MEMORY]');
      expect(app.lastFrame() ?? '').not.toContain('[1 OVERVIEW]');
    } finally {
      app.restore();
    }
  });

  it('moves right and left through the five screens', async () => {
    const app = await mount(120);
    try {
      app.stdin.write(RIGHT);
      await wait(200);
      expect(app.lastFrame() ?? '').toContain('[2 CPU]');
      app.stdin.write(RIGHT);
      await wait(200);
      expect(app.lastFrame() ?? '').toContain('[3 MEMORY]');
      app.stdin.write(LEFT);
      await wait(200);
      expect(app.lastFrame() ?? '').toContain('[2 CPU]');
    } finally {
      app.restore();
    }
  });

  it('wraps at both ends rather than dead-ending', async () => {
    const app = await mount(120);
    try {
      // Left from the first screen lands on the last.
      app.stdin.write(LEFT);
      await wait(250);
      expect(app.lastFrame() ?? '').toContain('[5 DISK]');
      app.stdin.write(RIGHT);
      await wait(250);
      expect(app.lastFrame() ?? '').toContain('[1 OVERVIEW]');
    } finally {
      app.restore();
    }
  });

  it('leaves the process cursor to up/down: switching views keeps the selection', async () => {
    const app = await mount(120);
    try {
      app.stdin.write(DOWN);
      await wait(120);
      const before = selectedPid(app.lastFrame());
      app.stdin.write(RIGHT);
      await wait(200);
      app.stdin.write(LEFT);
      await wait(250);
      expect(selectedPid(app.lastFrame())).toBe(before);
    } finally {
      app.restore();
    }
  });

  it('does not switch views while typing a filter', async () => {
    const app = await mount(120);
    try {
      app.stdin.write('/');
      await wait(80);
      app.stdin.write(RIGHT);
      await wait(200);
      // Still on the overview, and the arrow left no escape junk in the filter.
      const frame = app.lastFrame() ?? '';
      expect(frame).toContain('type to filter');
      expect(frame).toMatch(/filter\s+…/);
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

describe('I-26: every mode fits the terminal it was given', () => {
  /** Rendered height of the frame after `keys` have been typed. */
  async function frameHeight(
    columns: number,
    rows: number,
    keys: string[],
    provider?: MetricsProvider,
  ): Promise<number> {
    const app = await mount(columns, rows, () => {}, provider);
    try {
      for (const k of keys) {
        app.stdin.write(k);
        await wait(120);
      }
      await wait(250);
      return lines(app.lastFrame()).length;
    } finally {
      app.restore();
    }
  }

  it.each(['1', '2', '3', '4', '5'])('screen %s fits an 80x24 terminal', async (key) => {
    expect(await frameHeight(80, 24, [key])).toBeLessThanOrEqual(24);
  });

  it.each(['1', '2', '5'])('screen %s fits on a 24-core, 14-volume Mac', async (key) => {
    // The CPU screen used to draw one bar per core unconditionally, which ran
    // seven lines past the bottom here, and the disk screen one row per mount.
    expect(await frameHeight(80, 24, [key], new BigMachine())).toBeLessThanOrEqual(24);
    expect(await frameHeight(100, 30, [key], new BigMachine())).toBeLessThanOrEqual(30);
  });

  it('rolls the cores it cannot draw into a count rather than dropping them', async () => {
    const app = await mount(80, 24, () => {}, new BigMachine());
    try {
      app.stdin.write('2');
      await wait(300);
      expect(app.lastFrame() ?? '').toMatch(/… \d+ more cores/);
    } finally {
      app.restore();
    }
  });

  it('says how many volumes it could not draw', async () => {
    const app = await mount(80, 24, () => {}, new BigMachine());
    try {
      app.stdin.write('5');
      await wait(300);
      expect(app.lastFrame() ?? '').toMatch(/… \d+ more volumes/);
    } finally {
      app.restore();
    }
  });

  it.each([24, 30, 40])('the kill confirmation fits a %i-row terminal', async (rows) => {
    // It used to render *below* a full-height table: 40 lines in a 24-row
    // terminal, scrolling away the header and the cards at the exact moment the
    // user is being asked to confirm something irreversible.
    const app = await mount(80, rows);
    try {
      app.stdin.write('k');
      await wait(300);
      expect(app.lastFrame() ?? '').toContain('KILL PROCESS');
      expect(lines(app.lastFrame()).length).toBeLessThanOrEqual(rows);
    } finally {
      app.restore();
    }
  });

  it.each([24, 26, 30, 40])('the detail panel fits a %i-row terminal, protected or not', async (rows) => {
    // A protected process adds a warning line the panel's row budget did not
    // count, so at 80x24 it drew two rows past the bottom of the screen.
    for (const [keys, marker] of [
      [[ENTER], 'PID'],
      [['/', 'WindowServer', ENTER, ENTER], 'Protected'],
      // A 150-character path and a longer argv: the two blocks are the only
      // part of the panel that grows, so this is its worst case.
      [['/', 'Renderer', ENTER, ENTER], 'Chrome'],
    ] as const) {
      const app = await mount(80, rows);
      try {
        for (const k of keys) {
          app.stdin.write(k);
          await wait(150);
        }
        await wait(300);
        expect(app.lastFrame() ?? '').toContain('esc back');
        expect(app.lastFrame() ?? '').toContain(marker);
        expect(lines(app.lastFrame()).length).toBeLessThanOrEqual(rows);
      } finally {
        app.restore();
      }
    }
  });

  it('keeps the overview inside a narrow terminal, where the status line used to wrap', async () => {
    // At 80 columns the status line ran to 84 cells and wrapped onto a second
    // row that CHROME_ROWS had not budgeted for.
    expect(await frameHeight(80, 24, [])).toBeLessThanOrEqual(24);
    expect(await frameHeight(90, 24, [])).toBeLessThanOrEqual(24);
  });
});

describe('I-19: the layout follows the terminal when it is resized', () => {
  it('reflows to a narrower terminal without overflowing it', async () => {
    const app = await mount(100, 30);
    try {
      process.stdout.columns = 80;
      process.stdout.rows = 24;
      process.stdout.emit('resize');
      await wait(300);
      const frame = lines(app.lastFrame());
      expect(frame.length).toBeLessThanOrEqual(24);
      for (const l of frame) expect(displayWidth(l)).toBeLessThanOrEqual(80);
    } finally {
      app.restore();
    }
  });
});

/*
 * The sizes below 80x24 that the app used to corrupt.
 *
 * Every existing layout test above starts at 80x24, which is why these shipped:
 * at 72 columns every process row was one cell wider than its box and wrapped,
 * costing the table twice its budgeted height; the header's hardware line
 * pushed the clock out and became two lines; and four cards floored at 14
 * columns each asked for 59 columns on a 60-column terminal. See I-19, I-26.
 */
describe('I-19 / I-26: the layout holds below 80x24', () => {
  const SIZES: Array<[number, number]> = [
    [50, 10], [50, 14], [56, 12], [60, 14], [60, 20], [64, 16],
    [70, 18], [72, 22], [76, 20], [80, 24],
  ];

  it.each(SIZES)('every screen fits a %ix%i terminal', async (columns, rows) => {
    for (const key of ['1', '2', '3', '4', '5']) {
      const app = await mount(columns, rows);
      try {
        app.stdin.write(key);
        await wait(250);
        const frame = lines(app.lastFrame());
        expect(frame.length).toBeLessThanOrEqual(rows);
        for (const l of frame) expect(displayWidth(l)).toBeLessThanOrEqual(columns);
      } finally {
        app.restore();
      }
    }
  });

  it.each(SIZES)('the kill confirmation fits a %ix%i terminal', async (columns, rows) => {
    const app = await mount(columns, rows);
    try {
      app.stdin.write('k');
      await wait(300);
      const frame = lines(app.lastFrame());
      expect(frame.length).toBeLessThanOrEqual(rows);
      for (const l of frame) expect(displayWidth(l)).toBeLessThanOrEqual(columns);
    } finally {
      app.restore();
    }
  });

  it('gives up the USER column before it lets a row wrap', async () => {
    // The row is one <Text>: if it does not fit, Ink wraps it and the scrollbar
    // glyph lands on a line of its own, which is how the table used to cost two
    // rows per process.
    const app = await mount(70, 24);
    try {
      const frame = lines(app.lastFrame());
      expect(frame.some((l) => /^\s*>\s+\d+/.test(l))).toBe(true);
      expect(frame.join('\n')).not.toContain('USER');
      // No line consisting only of the scrollbar gutter.
      expect(frame.filter((l) => /^[█│]$/.test(l.trim()) && l.trim())).toHaveLength(0);
    } finally {
      app.restore();
    }
  });

  it('keeps the process name at every width it will draw at', async () => {
    // Dropping columns is only correct if the one column you cannot infer from
    // another screen survives. A table of bare PIDs is not a system monitor.
    // The name is truncated at narrow widths ("Google Ch…"), so this asserts
    // that a name is *there*, not which one.
    for (const columns of [50, 60, 72, 80, 120]) {
      const app = await mount(columns, 24);
      try {
        const rows = lines(app.lastFrame()).filter((l) => /^\s*>?\s*\d{2,}\s/.test(l));
        expect(rows.length).toBeGreaterThan(0);
        for (const r of rows) {
          // PID, then at least three characters of a name before the bars.
          expect(r).toMatch(/^\s*>?\s*\d{2,}\s+\S{3,}/);
        }
      } finally {
        app.restore();
      }
    }
  });

  it('drops the cards and the core strip rather than overflowing a short terminal', async () => {
    const app = await mount(80, 12);
    try {
      const frame = lines(app.lastFrame());
      expect(frame.length).toBeLessThanOrEqual(12);
      // The table is what survives: it is the only thing that answers the
      // question the app exists to answer.
      expect(frame.join('\n')).toContain('PID');
    } finally {
      app.restore();
    }
  });

  it('says the terminal is too small instead of drawing a corrupted frame', async () => {
    for (const [columns, rows] of [[40, 20], [80, 8], [30, 6]] as const) {
      const app = await mount(columns, rows);
      try {
        const frame = lines(app.lastFrame());
        expect(frame.join('\n')).toContain('too small');
        expect(frame.length).toBeLessThanOrEqual(rows);
        for (const l of frame) expect(displayWidth(l)).toBeLessThanOrEqual(columns);
      } finally {
        app.restore();
      }
    }
  });

  it('recovers when a too-small terminal is resized back up', async () => {
    const app = await mount(40, 12);
    try {
      expect(app.lastFrame() ?? '').toContain('too small');
      process.stdout.columns = 100;
      process.stdout.rows = 30;
      process.stdout.emit('resize');
      await wait(300);
      const frame = lines(app.lastFrame());
      expect(frame.join('\n')).not.toContain('too small');
      expect(frame.join('\n')).toContain('PID');
      expect(frame.length).toBeLessThanOrEqual(30);
    } finally {
      app.restore();
    }
  });

  it('fits a protected process, which costs both panels an extra warning line', async () => {
    // The row budget missing this line is how the detail panel used to run two
    // rows past the bottom of an 80x24 terminal; at 50x10 there is no slack at
    // all, so it is the case worth pinning.
    for (const [columns, rows] of [[50, 10], [56, 13], [64, 16], [80, 24]] as const) {
      for (const [last, marker] of [['\r', 'esc back'], ['k', 'REFUSED']] as const) {
        const app = await mount(columns, rows);
        try {
          for (const k of ['/', 'WindowServer', ENTER, last]) {
            app.stdin.write(k);
            await wait(120);
          }
          await wait(200);
          const frame = lines(app.lastFrame());
          expect(frame.join('\n')).toContain(marker);
          expect(frame.length).toBeLessThanOrEqual(rows);
          for (const l of frame) expect(displayWidth(l)).toBeLessThanOrEqual(columns);
        } finally {
          app.restore();
        }
      }
    }
  }, 30_000);

  it('does not spin the render loop when a toast squeezes the table to nothing', async () => {
    /*
     * "The visible window contains the selection" has no solution for a window
     * of zero rows, and `viewOffset` is a fixpoint written back to `scrollTop`.
     * At `tableRows === 0` it alternated between selIndex and selIndex+1 on
     * every render — an infinite loop, which React reports as "Maximum update
     * depth exceeded". The kill toast costs two rows and appears exactly when
     * this can happen, so it is the way in.
     */
    const errors: unknown[] = [];
    const spy = vi.spyOn(console, 'error').mockImplementation((...a) => errors.push(a));
    const app = await mount(60, 10);
    try {
      app.stdin.write('k');
      await wait(200);
      app.stdin.write('t');
      await wait(600);
      const frame = lines(app.lastFrame());
      expect(frame.length).toBeLessThanOrEqual(10);
      expect(errors.map(String).join(' ')).not.toMatch(/Maximum update depth/);
    } finally {
      spy.mockRestore();
      app.restore();
    }
  }, 20_000);

  it('I-27: names the screen you are on at every width it draws at', async () => {
    // `wrap="truncate"` alone rendered the fifth screen as "[5 DISK…" at 50
    // columns — closing bracket and arrow hint cut off. The inactive tabs drop
    // to bare numbers instead, which are still the keys that select them.
    for (const columns of [50, 60, 72, 80, 120]) {
      for (const [key, label] of [['1', 'OVERVIEW'], ['3', 'MEMORY'], ['5', 'DISK']] as const) {
        const app = await mount(columns, 24);
        try {
          app.stdin.write(key);
          await wait(200);
          const strip = lines(app.lastFrame())[1] ?? '';
          expect(strip).toContain(`[${key} ${label}]`);
          expect(strip).toContain('←/→');
          expect(displayWidth(strip)).toBeLessThanOrEqual(columns);
        } finally {
          app.restore();
        }
      }
    }
  }, 30_000);

  it('reflows down to a small terminal as well as up', async () => {
    const app = await mount(120, 40);
    try {
      for (const [columns, rows] of [[60, 16], [50, 10], [90, 28]] as const) {
        process.stdout.columns = columns;
        process.stdout.rows = rows;
        process.stdout.emit('resize');
        await wait(250);
        const frame = lines(app.lastFrame());
        expect(frame.length).toBeLessThanOrEqual(rows);
        for (const l of frame) expect(displayWidth(l)).toBeLessThanOrEqual(columns);
      }
    } finally {
      app.restore();
    }
  });
});
