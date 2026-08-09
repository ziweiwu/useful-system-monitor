import { useEffect, useState } from 'react';

/** Tracks terminal size so the layout adapts on SIGWINCH. See I-19. */
export function useTerminalSize(): { columns: number; rows: number } {
  const [size, setSize] = useState({
    columns: process.stdout.columns || 100,
    rows: process.stdout.rows || 30,
  });

  useEffect(() => {
    const onResize = () =>
      setSize({ columns: process.stdout.columns || 100, rows: process.stdout.rows || 30 });
    process.stdout.on('resize', onResize);
    return () => {
      process.stdout.off('resize', onResize);
    };
  }, []);

  return size;
}
