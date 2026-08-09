import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import {
  parseCpuTimeMs,
  parseDf,
  parseIoregBattery,
  parseMemory,
  parsePmset,
  parsePsHot,
  parsePsStatic,
  parseSignedInt64,
} from '../src/providers/darwin/parse.js';

/** Real command output captured from this machine. */
const fixture = (name: string): string =>
  readFileSync(fileURLToPath(new URL(`./fixtures/${name}`, import.meta.url)), 'utf8');

describe('parseCpuTimeMs', () => {
  it('reads centisecond resolution — the field the whole design depends on', () => {
    expect(parseCpuTimeMs('2:36.10')).toBe(156_100);
    expect(parseCpuTimeMs('0:00.01')).toBe(10);
  });

  it('handles minute counts that never roll into hours', () => {
    // Observed on this machine: ps prints 1206:18.96, not 20:06:18.96.
    expect(parseCpuTimeMs('1206:18.96')).toBe((1206 * 60 + 18) * 1000 + 960);
    expect(parseCpuTimeMs('383:24.67')).toBe((383 * 60 + 24) * 1000 + 670);
  });

  it('accepts an hours group defensively', () => {
    expect(parseCpuTimeMs('1:02:03.50')).toBe(((62 * 60) + 3) * 1000 + 500);
  });

  it('rejects junk rather than returning a wrong number', () => {
    expect(parseCpuTimeMs('-')).toBeNull();
    expect(parseCpuTimeMs('')).toBeNull();
  });
});

describe('parsePsHot against real output', () => {
  const procs = parsePsHot(fixture('ps-hot.txt'));

  it('parses every data row', () => {
    expect(procs.length).toBeGreaterThan(700);
  });

  it('produces finite, non-negative values throughout', () => {
    for (const p of procs) {
      expect(Number.isFinite(p.pid)).toBe(true);
      expect(p.cpuTimeMs).toBeGreaterThanOrEqual(0);
      expect(p.rssBytes).toBeGreaterThanOrEqual(0);
    }
  });

  it('converts RSS from kilobytes to bytes', () => {
    expect(procs.every((p) => p.rssBytes % 1024 === 0)).toBe(true);
  });

  it('includes launchd but not a PID 0 row (kernel_task is invisible to ps)', () => {
    expect(procs.some((p) => p.pid === 1)).toBe(true);
    expect(procs.some((p) => p.pid === 0)).toBe(false);
  });
});

describe('parsePsStatic against real output', () => {
  const metas = parsePsStatic(fixture('ps-static.txt'));

  it('parses every data row', () => {
    expect(metas.length).toBeGreaterThan(700);
  });

  it('parses every row without dropping any', () => {
    const dataLines = fixture('ps-static.txt').split('\n').filter((l) => l.trim()).length - 1;
    expect(metas.length).toBe(dataLines);
  });

  it('keeps command paths that contain spaces intact', () => {
    // The exact class of path that broke processName earlier.
    const chrome = metas.find((m) => m.command.includes('Google Chrome'));
    if (chrome) expect(chrome.command).toMatch(/^\/.*Google Chrome/);
    const spaced = metas.filter((m) => m.command.includes(' '));
    expect(spaced.length).toBeGreaterThan(0);
  });

  it('preserves COMM values that are not paths at all', () => {
    // ps -o comm is not always a path: macOS reports audio plugins as
    // "Core Audio Driver (Foo.driver)". Truncating at the first space would
    // collapse all of these into an indistinguishable "Core".
    const drivers = metas.filter((m) => m.command.startsWith('Core Audio Driver'));
    for (const d of drivers) expect(d.command).toContain('(');
  });

  it('parses lstart into a plausible timestamp', () => {
    const launchd = metas.find((m) => m.pid === 1);
    expect(launchd).toBeDefined();
    expect(launchd!.startTime).toBeGreaterThan(Date.parse('2020-01-01'));
    expect(launchd!.startTime).toBeLessThanOrEqual(Date.now());
  });

  it('never leaves user or state empty', () => {
    for (const m of metas) {
      expect(m.user).not.toBe('');
      expect(m.state).not.toBe('');
    }
  });
});

describe('I-5: memory reconciles', () => {
  const mem = parseMemory(fixture('vm_stat.txt'), fixture('swapusage.txt'), 16 * 1024 ** 3);

  it('splits total into used and available exactly', () => {
    expect(mem.usedBytes + mem.availableBytes).toBe(mem.totalBytes);
  });

  it('keeps free and available distinct', () => {
    // "free" is true vm_stat free and is near zero on a healthy Mac; the gauge
    // must measure against "available", which adds reclaimable inactive pages.
    expect(mem.freeBytes).toBeLessThan(mem.availableBytes);
    expect(mem.availableBytes).toBeGreaterThanOrEqual(mem.inactiveBytes);
  });

  it('breakdown rows partition memory without overlapping', () => {
    // The regression this pins: `free` was briefly redefined as `available`,
    // which already contains `inactive`. The memory breakdown listed both, so
    // the bars summed past 100% of physical RAM.
    const partition =
      mem.wiredBytes + mem.activeBytes + mem.inactiveBytes + mem.compressedBytes + mem.freeBytes;
    expect(partition).toBeLessThanOrEqual(mem.totalBytes * 1.02);
    expect(partition).toBeGreaterThan(mem.totalBytes * 0.9);
  });

  it('reads the 16K page size from the header rather than assuming 4K', () => {
    // A 4K assumption would under-report memory by 4x on Apple Silicon.
    expect(mem.wiredBytes).toBeGreaterThan(1024 ** 3);
  });

  it('excludes reclaimable inactive pages from "used"', () => {
    // top-style accounting (total - free) reads 99.5% on this machine, which
    // is technically true and useless as a gauge. Used must be strictly less.
    const topStyle = mem.totalBytes - (mem.totalBytes - mem.usedBytes - mem.inactiveBytes);
    expect(mem.usedBytes).toBeLessThan(topStyle);
    expect(mem.usedBytes).toBe(mem.wiredBytes + mem.activeBytes + mem.compressedBytes);
  });

  it('lands in a plausible range rather than pinned near 100%', () => {
    const pct = (mem.usedBytes / mem.totalBytes) * 100;
    expect(pct).toBeGreaterThan(10);
    expect(pct).toBeLessThan(97);
  });

  it('parses swap totals', () => {
    expect(mem.swapTotalBytes).toBeGreaterThan(0);
    expect(mem.swapUsedBytes).toBeGreaterThan(0);
    expect(mem.swapUsedBytes).toBeLessThanOrEqual(mem.swapTotalBytes);
  });
});

describe('parseDf', () => {
  const disk = parseDf(fixture('df.txt'), '/');

  it('finds the root filesystem', () => {
    expect(disk).not.toBeNull();
    expect(disk!.totalBytes).toBeGreaterThan(100 * 1024 ** 3);
  });

  it('returns null for a mount that is not present', () => {
    expect(parseDf(fixture('df.txt'), '/nope')).toBeNull();
  });

  it('reports APFS container usage, not the sealed snapshot', () => {
    // df's Used column for / is the read-only system snapshot (~12G here).
    // Using it would show a 926G disk as ~1% full.
    const d = parseDf(fixture('df.txt'), '/')!;
    const pct = (d.usedBytes / d.totalBytes) * 100;
    expect(pct).toBeGreaterThan(10);
    expect(d.usedBytes).toBe(d.totalBytes - d.freeBytes);
  });

  it('agrees with the Data volume, which shares the container', () => {
    const root = parseDf(fixture('df.txt'), '/')!;
    const data = parseDf(fixture('df.txt'), '/System/Volumes/Data');
    if (data) {
      expect(data.freeBytes).toBe(root.freeBytes);
      expect(data.usedBytes).toBe(root.usedBytes);
    }
  });
});

describe('parsePmset', () => {
  it('reads a discharging battery', () => {
    const b = parsePmset(fixture('pmset-discharging.txt'))!;
    expect(b.percent).toBe(48);
    expect(b.charging).toBe(false);
    expect(b.timeRemainingMin).toBe(43);
    expect(b.present).toBe(true);
  });

  it('reads a charging battery', () => {
    const b = parsePmset(fixture('pmset-charging.txt'))!;
    expect(b.charging).toBe(true);
    expect(b.percent).toBeGreaterThan(0);
  });

  it('reads "AC attached; not charging" — the state that is neither', () => {
    // Optimised charging holds the level; a lowercase-only pattern misses the
    // capital "AC" and reports "no battery reported by pmset".
    const b = parsePmset(fixture('pmset-ac-attached.txt'))!;
    expect(b).not.toBeNull();
    expect(b.percent).toBeGreaterThan(0);
    expect(b.charging).toBe(false);
    expect(b.onAcPower).toBe(true);
  });

  it('reads a fully charged battery', () => {
    const b = parsePmset(fixture('pmset-charged.txt'))!;
    expect(b.percent).toBe(100);
    expect(b.charging).toBe(false);
    expect(b.onAcPower).toBe(true);
    // "0:00 remaining" means unknown, not "zero minutes left".
    expect(b.timeRemainingMin).toBeNull();
  });

  it('marks discharging as not on AC', () => {
    expect(parsePmset(fixture('pmset-discharging.txt'))!.onAcPower).toBe(false);
  });

  it('handles every state macOS reports', () => {
    const states = ['discharging', 'charging', 'finishing charge', 'charged', 'AC attached'];
    for (const st of states) {
      const out = parsePmset(`Now drawing from 'AC Power'\n -InternalBattery-0 (id=1)\t55%; ${st}; present: true\n`);
      expect(out, st).not.toBeNull();
      expect(out!.percent, st).toBe(55);
    }
  });

  it('returns null when there is no battery', () => {
    expect(parsePmset('Now drawing from AC Power\n')).toBeNull();
  });

  it('null means "desktop Mac", which the provider must not treat as an error', () => {
    // Mac mini, Studio, Pro, iMac and every CI runner report no battery.
    // Throwing here would show an error panel on a machine working perfectly.
    for (const out of ['Now drawing from AC Power\n', '', 'No adapter attached.\n']) {
      expect(parsePmset(out)).toBeNull();
    }
  });
});

describe('parseSignedInt64: the unsigned Amperage trap', () => {
  it('reinterprets the 64-bit wrap as a negative current', () => {
    // A discharging battery reports -3452 mA as 18446744073709548164.
    expect(parseSignedInt64('18446744073709548164')).toBe(-3452);
  });

  it('leaves positive values alone', () => {
    expect(parseSignedInt64('2566')).toBe(2566);
  });

  it('returns null on junk instead of NaN', () => {
    expect(parseSignedInt64('abc')).toBeNull();
  });
});

describe('parseIoregBattery against real output', () => {
  const b = parseIoregBattery(fixture('ioreg-battery.txt'));

  it('derives wattage from volts times amps', () => {
    expect(b.watts).not.toBeNull();
    expect(Math.abs(b.watts!)).toBeLessThan(200);
  });

  it('reports cycle count and health', () => {
    expect(b.cycleCount).toBeGreaterThan(0);
    expect(b.healthPercent).toBeGreaterThan(50);
    expect(b.healthPercent).toBeLessThanOrEqual(100);
  });

  it('converts centidegrees to celsius', () => {
    expect(b.temperatureC).toBeGreaterThan(0);
    expect(b.temperatureC).toBeLessThan(80);
  });

  it('signs wattage negative while discharging', () => {
    const discharging = fixture('ioreg-battery.txt')
      .replace(/"InstantAmperage" = \d+/, '"InstantAmperage" = 18446744073709548164')
      .replace(/"Amperage" = \d+/, '"Amperage" = 18446744073709548164');
    expect(parseIoregBattery(discharging).watts!).toBeLessThan(0);
  });
});

describe('I-13: the parent map must cover every process, not the working set', () => {
  it('is populated for all PIDs so ancestor walks cannot dead-end', async () => {
    if (process.platform !== 'darwin') return;
    const { DarwinProvider } = await import('../src/providers/darwin/provider.js');
    const p = new DarwinProvider();
    const data = await p.processes();
    // Our own ancestors are usually idle shells that never reach the top 50.
    // A map built from `visible` would be ~50 entries and silently break I-13.
    expect(data.parents.size).toBe(data.total);
    expect(data.parents.size).toBeGreaterThan(data.visible.length);
    expect(data.parents.has(process.pid)).toBe(true);

    // Walk the real chain from this test process up to launchd.
    let cur = process.pid;
    const seen = new Set<number>();
    let hops = 0;
    while (cur > 1 && !seen.has(cur) && hops < 50) {
      seen.add(cur);
      const parent = data.parents.get(cur);
      if (parent === undefined) break;
      cur = parent;
      hops++;
    }
    expect(cur).toBe(1);
  }, 20_000);
});
