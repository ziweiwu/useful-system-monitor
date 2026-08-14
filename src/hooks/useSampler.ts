import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Ring } from '../core/ring.js';
import type { HostInfo, Panel, Snapshot } from '../core/types.js';
import type { MetricsProvider, Tiers } from '../providers/types.js';

export const HISTORY_LEN = 60;

/**
 * How soon after launch the delta-based collectors take their second sample.
 *
 * Long enough that `ps`'s centisecond CPU-time column still quantises finely
 * (~1.4% per process), short enough that the first real numbers are on screen
 * before the user has finished reading the header. Never shortened below the
 * point where the two samples could land in the same centisecond tick.
 */
export const PRIMING_DELAY_MS = 700;

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
export function useSampler(provider: MetricsProvider, tiers: Tiers, workingSetSize: number) {
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
  const refreshProcsRef = useRef<() => void>(() => {});

  /*
   * Read through a ref, not captured by the effect.
   *
   * Putting workingSetSize in the effect's dependency list would clear and
   * rebuild every interval on each `+` press, which resets all five tier
   * phases and re-runs cpu() out of band — shortening one delta window and
   * printing a bogus CPU reading. See I-4.
   */
  const sizeRef = useRef(workingSetSize);
  sizeRef.current = workingSetSize;

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
    /* A ratio, or 0 when there is no denominator. A NaN pushed into a ring
       stays there for HISTORY_LEN samples and poisons the Math.max() that
       scales the sparkline drawn from it. See I-19. */
    const ratio = (used: number, total: number) => (total > 0 ? (used / total) * 100 : 0);
    const runMem = poll('memory', () => provider.memory(), setMemory, (d) =>
      histories.memory.push(ratio(d.usedBytes, d.totalBytes)),
    );
    const runDisk = poll('disk', () => provider.disk(), setDisk, (d) =>
      histories.disk.push(ratio(d.usedBytes, d.totalBytes)),
    );
    const runBattery = poll('battery', () => provider.battery(), setBattery, (d) =>
      histories.battery.push(d.percent),
    );
    const runProcs = poll('processes', () => provider.processes(sizeRef.current), setProcesses);

    /*
     * I-11: host() is the one collector that does not go through poll(), so it
     * is the one whose rejection nothing was catching — and an unhandled
     * rejection takes the whole process down with a React stack trace, which is
     * the exact failure mode this invariant exists to rule out. The sentinel
     * host renders as "detecting hardware…", which is the right thing to keep
     * showing when the hardware cannot be identified.
     */
    void provider
      .host()
      .then((h) => {
        if (!cancelled) setHost(h);
      })
      .catch(() => {
        /* keep EMPTY_HOST */
      });

    const runAll = () => {
      void runCpu();
      void runMem();
      void runDisk();
      void runBattery();
      void runProcs();
    };
    refreshRef.current = runAll;
    refreshProcsRef.current = () => void runProcs();
    runAll();

    const timers = [
      setInterval(() => void runCpu(), tiers.cpu),
      setInterval(() => void runMem(), tiers.memory),
      setInterval(() => void runDisk(), tiers.disk),
      setInterval(() => void runBattery(), tiers.battery),
      setInterval(() => void runProcs(), tiers.processes),
    ];

    /*
     * The priming sample, so the first screen is not empty for a whole tier.
     *
     * Both CPU sources are deltas by construction: CpuDeltaTracker returns null
     * the first time it sees a PID (I-1), and per-core utilisation divides two
     * os.cpus() readings taken microseconds apart. The run above is therefore
     * only ever the *first* half of a measurement, and the second half used to
     * arrive on the next tick — so on the default 10s tier the app opened with
     * every CPU% reading "—" and the gauge reading 0.0% for ten seconds, which
     * is exactly when the user is looking at it.
     *
     * One extra early run closes the window instead of raising the rate: the
     * intervals above keep the phase they were created with, so this costs a
     * single extra `ps` per launch and nothing thereafter. The delta is over
     * true elapsed wall-clock either way, so a short first window is noisier
     * but not wrong — `ps` TIME has centisecond resolution, which over 700ms
     * quantises to ~1.4%. `oneShot` in cli.tsx has always primed this way.
     */
    const priming = setTimeout(() => {
      void runCpu();
      void runProcs();
    }, PRIMING_DELAY_MS);

    return () => {
      cancelled = true;
      clearTimeout(priming);
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
  const refreshProcesses = useCallback(() => refreshProcsRef.current(), []);

  // A new cap is only visible after a resample, and waiting out a 10s tier for
  // a keypress reads as a dead key. Processes only — see the note on sizeRef.
  useEffect(() => {
    refreshProcesses();
  }, [workingSetSize, refreshProcesses]);

  return { snapshot, histories, refresh, refreshProcesses };
}
