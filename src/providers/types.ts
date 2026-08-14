import type {
  BatteryData,
  CpuData,
  DiskData,
  HostInfo,
  MemoryData,
  ProcessesData,
} from '../core/types.js';

/**
 * Every collector the app can read from. Collectors are independent so one
 * failing degrades only its own panel (I-11), and each is polled on its own
 * tier (see the Cost budget) rather than on a single global tick.
 *
 * The mock provider implements the same interface as the real macOS one, which
 * is what makes the entire UI testable from fixtures without spawning anything.
 */
export interface MetricsProvider {
  readonly name: string;
  host(): Promise<HostInfo>;
  cpu(): Promise<CpuData>;
  memory(): Promise<MemoryData>;
  disk(): Promise<DiskData>;
  battery(): Promise<BatteryData>;
  /**
   * `limit` caps the working set (the rows that get names, history and a place
   * in the table). Everything above it is rolled into `others`. Defaults to
   * WORKING_SET_SIZE; the UI raises it on demand with `+`.
   */
  processes(limit?: number): Promise<ProcessesData>;
  /**
   * The identity of one live process, read *now* rather than from the last
   * sample. `null` means no process with that PID exists any more.
   *
   * This is what makes I-16 real. Binding a kill to (pid, startTime) is only
   * worth anything if the start time it compares against is fresh: a PID
   * recycled inside the sampling interval matches the *old* process's start
   * time in the last sample, and the guard waves it through. A provider that
   * cannot answer must say so by omitting this method, and the caller then
   * refuses rather than signalling something it cannot identify.
   */
  identity?(pid: number): Promise<{ startTime: number } | null>;

  /**
   * Full argv for one process, fetched on demand.
   *
   * Not part of the regular sample: carrying full command lines for ~800
   * processes is pure cost when only the selected one is ever shown.
   */
  commandLine?(pid: number): Promise<string | null>;
}

/** Polling tiers in ms. Chosen from measured collector cost; see plan. */
export interface Tiers {
  cpu: number;
  memory: number;
  processes: number;
  battery: number;
  disk: number;
}

export const DEFAULT_TIERS: Tiers = {
  /*
   * Not 1s, even though os.cpus() itself costs ~0ms.
   *
   * "Free to collect" is not "free to display". Every CPU tick triggers a
   * render, and a render costs ~30ms of Ink layout and terminal diffing —
   * an order of magnitude more than every collector combined.
   *
   * Measured self-cost of the compiled build, by tier:
   *   1s -> 2.14%   2s -> 1.92%   5s -> 1.14%   10s -> 0.85% of one core
   *
   * 10s is the chosen default: battery life beats gauge smoothness for a tool
   * that lives in a background pane. Pass --interval 2 when actively hunting a
   * spike, which is the case where fidelity is worth the cost.
   */
  cpu: 10_000,
  memory: 10_000,
  processes: 10_000,
  battery: 60_000,
  disk: 300_000,
};
