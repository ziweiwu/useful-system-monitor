import { useEffect, useRef } from 'react';
import { Ring } from '../core/ring.js';
import type { ProcessesData } from '../core/types.js';

export const PROC_HISTORY_LEN = 40;

export interface ProcHistory {
  cpu: Ring;
  mem: Ring;
  energy: Ring;
  /**
   * Which process these rings belong to.
   *
   * A PID on its own is not an identity: recycle it while the process is still
   * inside the working set and the new one inherits the dead one's sparklines,
   * so the detail panel draws another program's history under this name. The
   * delta tracker already treats a PID as (pid, counter direction) for the same
   * reason — see I-3.
   */
  startTime: number;
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
      if (!h || h.startTime !== p.startTime) {
        h = {
          cpu: new Ring(PROC_HISTORY_LEN),
          mem: new Ring(PROC_HISTORY_LEN),
          energy: new Ring(PROC_HISTORY_LEN),
          startTime: p.startTime,
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
