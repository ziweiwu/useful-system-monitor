import { useEffect, useRef } from 'react';
import { Ring } from '../core/ring.js';
import type { ProcessesData } from '../core/types.js';

export const PROC_HISTORY_LEN = 40;

export interface ProcHistory {
  cpu: Ring;
  mem: Ring;
  energy: Ring;
}

/**
 * Per-process history, kept only for the working set.
 *
 * I-10: rings are fixed-capacity and are evicted the moment a process leaves
 * the visible set, so memory is bounded by (working set x history length)
 * rather than growing with the ~800 live processes or with uptime.
 */
export function useProcessHistory(data: ProcessesData | null) {
  const ref = useRef(new Map<number, ProcHistory>());

  useEffect(() => {
    if (!data) return;
    const map = ref.current;
    const live = new Set<number>();

    for (const p of data.visible) {
      live.add(p.pid);
      let h = map.get(p.pid);
      if (!h) {
        h = {
          cpu: new Ring(PROC_HISTORY_LEN),
          mem: new Ring(PROC_HISTORY_LEN),
          energy: new Ring(PROC_HISTORY_LEN),
        };
        map.set(p.pid, h);
      }
      h.cpu.push(p.cpuPercent ?? 0);
      h.mem.push(p.rssBytes);
      h.energy.push(p.energy ?? 0);
    }

    // Deleting the current entry while iterating a Map is well-defined, so no
    // defensive copy is needed here.
    for (const pid of map.keys()) {
      if (!live.has(pid)) map.delete(pid);
    }
  }, [data]);

  return ref.current;
}
