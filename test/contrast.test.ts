import { describe, expect, it } from 'vitest';
import { theme } from '../src/ui/theme.js';

/** One sRGB channel, linearised. */
function channel(pair: string): number {
  const c = parseInt(pair, 16) / 255;
  return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
}

/** WCAG 2.1 relative luminance. */
function luminance(hex: string): number {
  const h = hex.replace('#', '');
  return (
    0.2126 * channel(h.slice(0, 2)) +
    0.7152 * channel(h.slice(2, 4)) +
    0.0722 * channel(h.slice(4, 6))
  );
}

export function contrast(a: string, b: string): number {
  const [hi, lo] = [luminance(a), luminance(b)].toSorted((x, y) => y - x);
  return (hi! + 0.05) / (lo! + 0.05);
}

/*
 * The two terminal backgrounds this palette will actually sit on: a pure black
 * terminal, and the Tokyo-Night-ish dark the palette is clearly built for. The
 * second is the stricter of the two, so both are asserted.
 *
 * This is a measurement, not a judgement — which is exactly why it belongs in
 * the suite. `theme.dim` shipped at 3.39:1 / 2.76:1 while carrying 64 pieces of
 * real text, and nothing caught it because nobody had computed it.
 */
const BACKGROUNDS = { black: '#000000', 'tokyo-night': '#1a1b26' } as const;

/** Colours used as actual text. */
const TEXT_COLOURS = [
  'dim', 'text', 'headline', 'mem', 'disk', 'battery',
  'cpuLow', 'cpuMid', 'cpuHigh', 'danger', 'root',
] as const;

describe('WCAG 1.4.3: text clears 4.5:1 on the backgrounds it is drawn on', () => {
  it.each(TEXT_COLOURS)('%s', (name) => {
    for (const [label, bg] of Object.entries(BACKGROUNDS)) {
      const ratio = contrast(theme[name], bg);
      expect(ratio, `${name} on ${label} measured ${ratio.toFixed(2)}:1`).toBeGreaterThanOrEqual(4.5);
    }
  });
});

describe('WCAG 1.4.11: non-text UI clears 3:1', () => {
  it('the panel border', () => {
    for (const bg of Object.values(BACKGROUNDS)) {
      expect(contrast(theme.frame, bg)).toBeGreaterThanOrEqual(3);
    }
  });
});

describe('WCAG 1.4.3: the selected row stays readable on its own highlight', () => {
  /*
   * The selected row is the one the user is about to press enter or k against,
   * and it draws on `selectionBg` rather than the terminal background. The PID
   * and USER cells were the only ones that did not brighten with the rest of
   * the row, leaving the PID at 1.97:1 there.
   */
  it('every colour a selected row uses clears 4.5:1 on selectionBg', () => {
    for (const name of ['text', 'headline', 'mem', 'root'] as const) {
      const ratio = contrast(theme[name], theme.selectionBg);
      expect(ratio, `${name} on selectionBg measured ${ratio.toFixed(2)}:1`).toBeGreaterThanOrEqual(4.5);
    }
  });

  it('dim is not used on a selected row, because it does not clear it', () => {
    // Pins the reason the cells above are selection-aware: if someone later
    // "simplifies" them back to dim, this says why they cannot.
    expect(contrast(theme.dim, theme.selectionBg)).toBeLessThan(4.5);
  });
});
