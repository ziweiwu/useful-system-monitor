import { render } from 'ink-testing-library';
import { describe, expect, it } from 'vitest';
import { App } from '../src/app.js';
import { fitter, displayWidth } from '../src/core/width.js';
import type { HostInfo } from '../src/core/types.js';
import { MockProvider } from '../src/providers/mock/provider.js';
import { DEFAULT_TIERS } from '../src/providers/types.js';
import { waitForFrame } from './helpers.js';

const ESC = String.fromCharCode(27);
const ANSI = new RegExp(`${ESC}\\[[0-9;]*m`, 'g');
const plain = (frame: string | undefined) => (frame ?? '').replace(ANSI, '');

/**
 * I-10b, at the level this repo can act on.
 *
 * Ink memoises every string it has to *wrap* in a module-level cache keyed on
 * the text, which never evicts (7.1.1, `build/wrap-text.js`). A row that
 * overflows with a live number in it therefore mints a permanent entry per
 * distinct value. Measured against the real provider: 386 entries after 30s and
 * still climbing, versus 0 once the two offending rows are fitted.
 *
 * `fitter` is what makes a row unable to overflow, so these pin its arithmetic;
 * the rows that use it are pinned by `verify:layout`, which already sweeps 1260
 * frames for anything wider than its terminal.
 */
describe('I-10b: rows are fitted, so ink never has to wrap one', () => {
  it('spends the budget in render order and never exceeds it', () => {
    const fit = fitter(10);
    expect(fit('abc')).toBe('abc');
    expect(fit('defg')).toBe('defg');
    // 7 spent, 3 left: the third segment is cut to fit rather than overflowing.
    expect(fit('hijklmn')).toBe('hi…');
    // Nothing left: later segments vanish instead of pushing the row wider.
    expect(fit('opq')).toBe('');
  });

  it('never returns more cells than it was given, for any split', () => {
    const segments = ['1-5 of 50 · top 50 of 668', ' · sort ', 'energy', ' · filter ', 'chrome'];
    for (let max = 0; max <= 60; max++) {
      const fit = fitter(max);
      const total = segments.reduce((n, s) => n + displayWidth(fit(s)), 0);
      expect(total).toBeLessThanOrEqual(max);
    }
  });

  it('counts wide characters as two cells', () => {
    const fit = fitter(4);
    expect(displayWidth(fit('日本語'))).toBeLessThanOrEqual(4);
  });
});

/**
 * The header's uptime is derived from the boot instant, not from the one
 * `host()` sample.
 *
 * `host()` costs two `sysctl` spawns and is fetched exactly once, so
 * `host.uptimeSec` is frozen at launch — the header read the same "up 7d 3h"
 * three days later, wrong on precisely the long sessions this program is for.
 */
describe('the header uptime advances', () => {
  it('crosses a minute boundary while the app is running', async () => {
    class YoungMachine extends MockProvider {
      override async host(): Promise<HostInfo> {
        // 119s: the display rolls 1m -> 2m one second after launch, so the
        // assertion needs no fake clock and still fails outright if the value
        // is frozen at its sampled reading.
        return { ...(await super.host()), uptimeSec: 119 };
      }
    }
    const app = render(
      <App provider={new YoungMachine()} tiers={{ ...DEFAULT_TIERS, cpu: 100, processes: 100 }} />,
    );
    try {
      await waitForFrame(
        () => plain(app.lastFrame()),
        (f) => /cores · up 2m/.test(f),
        'the header uptime to advance past 2m',
      );
      expect(plain(app.lastFrame())).toMatch(/cores · up 2m/);
    } finally {
      app.unmount();
    }
  });
});
