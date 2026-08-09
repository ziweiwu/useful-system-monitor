import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Ring } from '../core/ring.js';
import type { HostInfo, Panel, Snapshot } from '../core/types.js';
import type { MetricsProvider, Tiers } from '../providers/types.js';

export const HISTORY_LEN = 60;

export interface Histories {
  cpu: Ring;
  memory: Ring;
  disk: Ring;
  battery: Ring;
}

const PENDING = { status: 'pending' } as const;

/** Sentinel until host() resolves; the UI renders "detecting…" for this. */
const EMPTY_HOST: HostInfo = {
  model: 'unknown',
  cores: 0,
  perfCores: 1,
  effCores: 0,
  totalMemBytes: 0,
  uptimeSec: 0,
};

/**
 * Drives every collector on its own tier.
 *
 * - Each collector is independent, so a failure degrades one panel (I-11).
 * - Collection is async and never blocks input or render (I-7).
 * - At most one run per collector is in flight; an overrun skips the next tick
 *   rather than queueing it, so slow samples cannot pile up (I-8).
 */
export function useSampler(provider: MetricsProvider, tiers: Tiers) {
  const [host, setHost] = useState<HostInfo>(EMPTY_HOST);
  const [cpu, setCpu] = useState<Panel<Snapshot['cpu'] extends Panel<infer T> ? T : never>>(PENDING);
  const [memory, setMemory] = useState<Snapshot['memory']>(PENDING);
  const [disk, setDisk] = useState<Snapshot['disk']>(PENDING);
  const [battery, setBattery] = useState<Snapshot['battery']>(PENDING);
  const [processes, setProcesses] = useState<Snapshot['processes']>(PENDING);

  const histories = useMemo<Histories>(
    () => ({
      cpu: new Ring(HISTORY_LEN),
      memory: new Ring(HISTORY_LEN),
      disk: new Ring(HISTORY_LEN),
      battery: new Ring(HISTORY_LEN),
    }),
    [],
  );

  const inFlight = useRef<Record<string, boolean>>({});
  const refreshRef = useRef<() => void>(() => {});

  useEffect(() => {
    let cancelled = false;
    // Captured once: reading `inFlight.current` inside async callbacks that may
    // outlive the effect is exactly what the exhaustive-deps rule warns about.
    const flights = inFlight.current;

    /** One collector's poll loop, with the in-flight guard from I-8. */
    function poll<T>(
      name: string,
      fetch: () => Promise<T>,
      set: (p: Panel<T>) => void,
      record?: (d: T) => void,
    ): () => Promise<void> {
      return async () => {
        if (cancelled || flights[name]) return;
        flights[name] = true;
        try {
          const data = await fetch();
          if (cancelled) return;
          record?.(data);
          set({ status: 'ok', data, sampledAt: Date.now() });
        } catch (err) {
          if (cancelled) return;
          set({
            status: 'error',
            message: err instanceof Error ? err.message : String(err),
            sampledAt: Date.now(),
          });
        } finally {
          flights[name] = false;
        }
      };
    }

    const runCpu = poll('cpu', () => provider.cpu(), setCpu, (d) => histories.cpu.push(d.system));
    const runMem = poll('memory', () => provider.memory(), setMemory, (d) =>
      histories.memory.push((d.usedBytes / d.totalBytes) * 100),
    );
    const runDisk = poll('disk', () => provider.disk(), setDisk, (d) =>
      histories.disk.push((d.usedBytes / d.totalBytes) * 100),
    );
    const runBattery = poll('battery', () => provider.battery(), setBattery, (d) =>
      histories.battery.push(d.percent),
    );
    const runProcs = poll('processes', () => provider.processes(), setProcesses);

    void provider.host().then((h) => {
      if (!cancelled) setHost(h);
    });

    const runAll = () => {
      void runCpu();
      void runMem();
      void runDisk();
      void runBattery();
      void runProcs();
    };
    refreshRef.current = runAll;
    runAll();

    const timers = [
      setInterval(() => void runCpu(), tiers.cpu),
      setInterval(() => void runMem(), tiers.memory),
      setInterval(() => void runDisk(), tiers.disk),
      setInterval(() => void runBattery(), tiers.battery),
      setInterval(() => void runProcs(), tiers.processes),
    ];

    return () => {
      cancelled = true;
      for (const t of timers) clearInterval(t);
    };
  }, [provider, tiers, histories]);

  /*
   * C-6: the snapshot must keep its identity between ticks.
   *
   * Rebuilding this object on every render gives every memoized child a new
   * prop, so a once-per-second clock tick would re-render all 50 process rows
   * and force Ink to diff and rewrite the whole frame.
   */
  const snapshot: Snapshot = useMemo(
    () => ({ host, cpu, memory, disk, battery, processes }),
    [host, cpu, memory, disk, battery, processes],
  );
  const refresh = useCallback(() => refreshRef.current(), []);
  return { snapshot, histories, refresh };
}
