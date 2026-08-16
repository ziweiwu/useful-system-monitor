import { describe, expect, it } from 'vitest';
import { displayWidth, padEnd, padStart, truncate, wrapToWidth } from '../src/core/width.js';

describe('I-19: layout uses display width, not string length', () => {
  it('counts ASCII as one cell each', () => {
    expect(displayWidth('hello')).toBe(5);
  });

  it('counts CJK as two cells', () => {
    expect(displayWidth('中文')).toBe(4);
  });

  it('counts the glyphs that broke the first mock as two cells', () => {
    // ⛔ and ⚠-family symbols render double-width; treating them as length 1
    // is exactly what misaligned the original ASCII mock.
    expect(displayWidth('⛔')).toBe(2);
    expect(displayWidth('🔥')).toBe(2);
    // Emoji presentation selector widens an otherwise-narrow glyph.
    expect(displayWidth('⚠️')).toBe(2);
  });

  it('keeps the wide-range table sorted', () => {
    // An out-of-order entry makes every later range unreachable, which is a
    // silent correctness bug rather than a crash.
    const probes = [0x26d4, 0x2757, 0x2b50, 0x1f600, 0x4e00];
    for (const cp of probes) {
      expect(displayWidth(String.fromCodePoint(cp))).toBe(2);
    }
  });

  it('ignores zero-width combining marks', () => {
    expect(displayWidth('é')).toBe(1);
  });

  it('padEnd produces an exact cell count even with wide glyphs', () => {
    expect(displayWidth(padEnd('中', 5))).toBe(5);
    expect(displayWidth(padEnd('ab', 5))).toBe(5);
    expect(displayWidth(padEnd('', 5))).toBe(5);
  });

  it('padStart produces an exact cell count', () => {
    expect(displayWidth(padStart('中文', 7))).toBe(7);
  });

  it('never exceeds the requested width when truncating', () => {
    for (const s of ['abcdef', '中文中文中文', 'a中b文c', '🔥🔥🔥🔥']) {
      for (let n = 1; n <= 10; n++) {
        expect(displayWidth(truncate(s, n))).toBeLessThanOrEqual(n);
      }
    }
  });

  it('leaves short strings untouched', () => {
    expect(truncate('abc', 10)).toBe('abc');
  });
});

describe('I-19: padding and truncation agree with displayWidth', () => {
  /*
   * `truncate` had its own width rule — `displayWidth(ch)` per character —
   * which cannot see that U+FE0F widens the character *before* it. `⚠`
   * measures 1 alone and the variation selector 0 alone, but "⚠️" occupies 2
   * cells, so every such pair was under-counted by one: `truncate('⚠️abc', 3)`
   * returned 4 cells and `padEnd` padded one cell too far, overflowing the row
   * it was drawn in. macOS application names really do contain emoji.
   *
   * Both now consume one shared generator, so the rule cannot disagree with
   * itself. These cases are the ones that failed: 33 of them.
   */
  const TRICKY = ['⚠️', '⚠️abc', 'x⚠️y', '⚠️⚠️', '⚠️ Warning App', '⚠', '🔥', '🔥a', '日本', 'é', ''];

  it.each(TRICKY)('padEnd(%j, n) is exactly n cells', (s) => {
    for (const n of [0, 1, 2, 3, 4, 6, 10, 20]) {
      expect(displayWidth(padEnd(s, n)), `padEnd(${JSON.stringify(s)}, ${n})`).toBe(n);
    }
  });

  it.each(TRICKY)('padStart(%j, n) is exactly n cells', (s) => {
    for (const n of [0, 1, 2, 3, 4, 6, 10, 20]) {
      expect(displayWidth(padStart(s, n)), `padStart(${JSON.stringify(s)}, ${n})`).toBe(n);
    }
  });

  it.each(TRICKY)('truncate(%j, n) never exceeds n cells', (s) => {
    for (const n of [0, 1, 2, 3, 4, 6, 10, 20]) {
      expect(displayWidth(truncate(s, n)), `truncate(${JSON.stringify(s)}, ${n})`).toBeLessThanOrEqual(n);
    }
  });
});

/**
 * The detail panel's Path and Command blocks wrap long values across several
 * lines. They used to do it with `slice(0, max)`, which counts UTF-16 units:
 * a 66-unit CJK path measures 98 cells, so the whole thing was packed into one
 * "line" and the renderer truncated two thirds of it away, on the one panel
 * whose job is telling two identically-named helpers apart.
 */
describe('I-19: wrapToWidth breaks lines by cells, not by code units', () => {
  const CASES = [
    'hello world',
    '/Applications/微信读书助手.app/Contents/MacOS/微信读书助手',
    '⚠️⚠️⚠️⚠️⚠️',
    '中文中文中文中文',
    'a中b文c',
    '🔥🔥🔥🔥',
    '',
  ];

  it.each(CASES)('no line of %j ever exceeds the budget', (s) => {
    for (const max of [1, 2, 3, 5, 8, 20]) {
      for (const line of wrapToWidth(s, max)) {
        expect(displayWidth(line), `${JSON.stringify(line)} in budget ${max}`).toBeLessThanOrEqual(max);
      }
    }
  });

  it.each(CASES)('keeps every character of %j at any budget a glyph fits in', (s) => {
    // 2 cells is the widest single glyph, so nothing is dropped from here up.
    for (const max of [2, 3, 5, 8, 20]) {
      expect(wrapToWidth(s, max).join(''), `budget ${max}`).toBe(s);
    }
  });

  it('drops a glyph too wide for the whole budget rather than overflowing', () => {
    // Unreachable from the app — the only caller floors its budget at 20 —
    // but the budget has to win here for the same reason it does in truncate.
    expect(wrapToWidth('中', 1)).toEqual([]);
    expect(wrapToWidth('a中b', 1)).toEqual(['a', 'b']);
  });

  it('uses the lines it is given rather than truncating into one', () => {
    // 12 cells of CJK into a 4-cell budget is three lines, not one clipped one.
    expect(wrapToWidth('中文中文中文', 4)).toEqual(['中文', '中文', '中文']);
  });

  it('does not split a wide glyph across the edge of a line', () => {
    // An odd budget cannot hold a third cell, so the glyph moves down whole.
    expect(wrapToWidth('中文中', 3)).toEqual(['中', '文', '中']);
  });

  it('returns nothing for a budget that cannot hold anything', () => {
    expect(wrapToWidth('中文', 0)).toEqual([]);
  });
});
