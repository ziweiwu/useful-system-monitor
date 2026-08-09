import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { parseTopPower } from '../src/providers/darwin/power.js';

const fixture = (name: string): string =>
  readFileSync(fileURLToPath(new URL(`./fixtures/${name}`, import.meta.url)), 'utf8');

describe('Phase 4: macOS Energy Impact', () => {
  const raw = fixture('top-power.txt');
  const parsed = parseTopPower(raw);

  it('parses the sample block', () => {
    expect(parsed.size).toBeGreaterThan(5);
  });

  it('reads the SECOND block, not the first', () => {
    // `top -l 2` always reports 0.0 for everything in the first block, because
    // energy impact is a rate with nothing yet to compare against. Parsing the
    // first block yields a table of zeros that looks plausible and is useless.
    const nonZero = [...parsed.values()].filter((v) => v > 0);
    expect(nonZero.length).toBeGreaterThan(0);
  });

  it('produces finite non-negative values keyed by pid', () => {
    for (const [pid, power] of parsed) {
      expect(Number.isInteger(pid)).toBe(true);
      expect(pid).toBeGreaterThan(0);
      expect(power).toBeGreaterThanOrEqual(0);
      expect(Number.isFinite(power)).toBe(true);
    }
  });

  it('returns an empty map rather than throwing on junk', () => {
    expect(parseTopPower('').size).toBe(0);
    expect(parseTopPower('no header here\n1 2 3\n').size).toBe(0);
  });

  it('ignores the first block even when given only that', () => {
    const firstOnly = raw.slice(0, raw.lastIndexOf('PID'));
    const p = parseTopPower(firstOnly);
    for (const v of p.values()) expect(v).toBe(0);
  });
});
