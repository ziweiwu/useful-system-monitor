import { useEffect, useState } from 'react';

/**
 * The terminal's size, when it reports one, and 80x24 when it does not.
 *
 * The fallback matters more than it looks. Ink falls back to 80x24 when
 * `stdout.columns` is 0 or undefined — which is what a pty with no size set
 * reports, and what several CI runners report. This hook used to fall back to
 * 100x30, so the layout was computed 20 columns wider than the frame Ink
 * actually drew, and the fourth card came out clipped and ragged. Laying out
 * narrower than the renderer only wastes space; laying out wider corrupts the
 * frame, so the two must not disagree. See I-19.
 */
const FALLBACK = { columns: 80, rows: 24 } as const;

function measure(): { columns: number; rows: number } {
  return {
    columns: process.stdout.columns || FALLBACK.columns,
    rows: process.stdout.rows || FALLBACK.rows,
  };
}

/** Tracks terminal size so the layout adapts on SIGWINCH. See I-19. */
export function useTerminalSize(): { columns: number; rows: number } {
  const [size, setSize] = useState(measure);

  useEffect(() => {
    const onResize = () => setSize(measure());
    process.stdout.on('resize', onResize);
    return () => {
      process.stdout.off('resize', onResize);
    };
  }, []);

  return size;
}
