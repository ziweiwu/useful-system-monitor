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
  dim: '#565f89',
  track: '#343a54',
  frame: '#454c69',
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

/** Unicode block sparkline. All glyphs are single-width (see width.ts). */
export function sparkline(values: readonly number[], width: number, max = 100): string {
  if (width <= 0) return '';
  const slice = values.slice(-width);
  const pad = width - slice.length;
  const cells = slice.map((v) => {
    const r = Math.max(0, Math.min(1, v / max));
    return BLOCKS[Math.min(BLOCKS.length - 1, Math.round(r * (BLOCKS.length - 1)))]!;
  });
  return ' '.repeat(Math.max(0, pad)) + cells.join('');
}

/** Horizontal bar: filled cells plus a dim track remainder. */
export function barCells(pct: number, width: number): { filled: number; empty: number } {
  const f = Math.max(0, Math.min(width, Math.round((pct / 100) * width)));
  return { filled: f, empty: width - f };
}
