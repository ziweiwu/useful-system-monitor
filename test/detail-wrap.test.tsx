import { render } from 'ink-testing-library';
import { describe, expect, it } from 'vitest';
import { displayWidth } from '../src/core/width.js';
import { ProcessDetail } from '../src/ui/ProcessDetail.js';
import { sample } from './helpers.js';

const ESC = String.fromCharCode(27);
const ANSI = new RegExp(`${ESC}\\[[0-9;]*m`, 'g');

/**
 * The wiring, not just the helper.
 *
 * `Block` split its value with `slice(0, avail)`, which counts UTF-16 units
 * while `avail` is a cell budget. A path of 66 units measures 98 cells, so the
 * whole thing went into one "line" and the renderer clipped two thirds of it —
 * on the panel whose stated job is telling two identically-named helpers apart,
 * and after the panel had already budgeted several lines for it. See I-19.
 */
describe('I-19: the detail panel wraps a wide path across the lines it budgeted', () => {
  const PATH =
    '/Applications/微信读书助手测试应用程序目录名称很长很长.app/Contents/MacOS/微信读书助手测试应用程序';

  it('keeps every line inside the box', () => {
    const { lastFrame } = render(
      <ProcessDetail
        p={sample({ pid: 4242, command: PATH })}
        history={undefined}
        commandLine={null}
        width={98}
        maxRows={40}
      />,
    );
    const rows = (lastFrame() ?? '').replace(ANSI, '').split('\n');
    const widest = Math.max(...rows.map(displayWidth));
    for (const r of rows) expect(displayWidth(r), JSON.stringify(r)).toBe(widest);
    expect(widest).toBeLessThanOrEqual(98);
  });

  it('shows the tail that used to be clipped away', () => {
    const { lastFrame } = render(
      <ProcessDetail
        p={sample({ pid: 4242, command: PATH })}
        history={undefined}
        commandLine={null}
        width={98}
        maxRows={40}
      />,
    );
    const rows = (lastFrame() ?? '').replace(ANSI, '').split('\n');
    const from = rows.findIndex((r) => r.includes('Path'));
    const to = rows.findIndex((r) => r.includes('Command'));
    expect(from).toBeGreaterThanOrEqual(0);
    expect(to).toBeGreaterThan(from);

    const block = rows.slice(from + 1, to);
    // More than one line, because 98 cells do not fit in one 76-cell row...
    expect(block.length).toBeGreaterThan(1);
    // ...and the end of the path is now on screen rather than truncated off.
    expect(block.join('')).toContain('/Contents/MacOS/');
  });
});
