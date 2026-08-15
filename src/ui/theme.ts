/**
 * One hue per metric family: a metric's card border, sparkline, gauge fill and
 * its process-table column all share a colour, so the eye tracks CPU vs MEM vs
 * ENERGY without reading headers.
 *
 * Colour is always *redundant* encoding — every state is readable from text
 * alone, so the dashboard still works under NO_COLOR and stays greppable (I-23).
 */
export const theme = {
  cpuLow: '#9ece6a',
  cpuMid: '#e0af68',
  cpuHigh: '#f7768e',
  mem: '#7aa2f7',
  disk: '#bb9af7',
  battery: '#ff9e64',
  headline: '#ffffff',
  text: '#c0caf5',
  /* Measured, not chosen by eye: the old #565f89 was 3.39:1 on black and
     2.76:1 on a Tokyo-Night background, below WCAG 1.4.3's 4.5:1 — and it is
     the colour of 64 pieces of real text, including the PID column, the sample
     ages and the footer legend. This is the least lightening along the same
     hue that clears 4.5:1 on both: 5.60:1 and 4.56:1. */
  dim: '#7982ac',
  track: '#343a54',
  /* Borders are non-text UI, so 1.4.11's 3:1 applies rather than 4.5:1. The
     old #454c69 measured 2.49:1 / 2.03:1; this measures 3.71:1 / 3.02:1. */
  frame: '#5f6683',
  selectionBg: '#2d3452',
  root: '#e0af68',
  danger: '#f7768e',
} as const;

/** Green below 60%, amber to 85%, red above. */
export function severity(pct: number): string {
  if (pct < 60) return theme.cpuLow;
  if (pct < 85) return theme.cpuMid;
  return theme.cpuHigh;
}

export const BLOCKS = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'] as const;

/*
 * I-19: a value that is not a number must still occupy its cell.
 *
 * Both functions below are pure width arithmetic, and NaN propagates straight
 * through it: `'█'.repeat(NaN)` is '' and `BLOCKS[NaN]` is undefined, so a
 * single bad sample silently made a bar 0 cells wide instead of 10, and a
 * sparkline 4 characters instead of 5 — a layout break with no error anywhere.
 * NaN reaches here from any ratio with a zero denominator (df reporting 0
 * blocks for a mount), and once one lands in a history ring it also poisons
 * the Math.max() that scales the whole sparkline, for sixty samples.
 */
function finite(n: number): number {
  return Number.isFinite(n) ? n : 0;
}

/** Unicode block sparkline. All glyphs are single-width (see width.ts). */
export function sparkline(values: readonly number[], width: number, max = 100): string {
  if (!Number.isFinite(width) || width <= 0) return '';
  const scale = Number.isFinite(max) && max > 0 ? max : 100;
  const slice = values.slice(-width);
  const pad = width - slice.length;
  const cells = slice.map((v) => {
    const r = Math.max(0, Math.min(1, finite(v) / scale));
    return BLOCKS[Math.min(BLOCKS.length - 1, Math.round(r * (BLOCKS.length - 1)))]!;
  });
  return ' '.repeat(Math.max(0, pad)) + cells.join('');
}

/** Horizontal bar: filled cells plus a dim track remainder. */
export function barCells(pct: number, width: number): { filled: number; empty: number } {
  const w = Math.max(0, Math.trunc(finite(width)));
  const f = Math.max(0, Math.min(w, Math.round((finite(pct) / 100) * w)));
  return { filled: f, empty: w - f };
}
