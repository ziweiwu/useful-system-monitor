/**
 * Shared data model. Every field the UI renders comes from here, and every
 * collector produces one of these shapes. See INVARIANTS.md.
 */

/** Identity that survives PID reuse. See I-16. */
export interface ProcessKey {
  pid: number;
  /** Process start time in ms since epoch. Distinguishes a recycled PID. */
  startTime: number;
}

export function keyOf(k: ProcessKey): string {
  return `${k.pid}:${k.startTime}`;
}

/** Cheap per-tick columns: `ps -Ao pid,ppid,time,rss`. See C-1. */
export interface RawProcess {
  pid: number;
  ppid: number;
  /** Cumulative CPU time in ms. Monotonic per (pid, startTime). See I-3. */
  cpuTimeMs: number;
  rssBytes: number;
}

/** Static columns, fetched only when the PID set changes. See C-1. */
export interface ProcessMeta {
  pid: number;
  startTime: number;
  command: string;
  user: string;
  state: string;
}

export interface ProcessSample {
  pid: number;
  ppid: number;
  startTime: number;
  command: string;
  user: string;
  state: string;
  /** null on first observation — never 0. See I-1. */
  cpuPercent: number | null;
  rssBytes: number;
  /** Energy-impact proxy, or macOS Energy Impact when `--energy=accurate`. */
  energy: number | null;
  /** True when this process must never be signalled. See I-12..I-14. */
  protected: boolean;
}

export interface OthersRollup {
  count: number;
  cpuPercent: number;
  rssBytes: number;
  energy: number;
}

export interface ProcessesData {
  total: number;
  /** Top-50 working set only. See C-7. */
  visible: ProcessSample[];
  others: OthersRollup;
  /**
   * pid -> ppid for **every** process, not just the working set.
   *
   * The ancestor guard (I-13) walks up from our own PID, and those ancestors
   * are usually idle shells that never make the top 50. Building the map from
   * `visible` alone silently breaks the guard.
   */
  parents: ReadonlyMap<number, number>;
  /**
   * True when `energy` carries macOS's measured Energy Impact rather than the
   * CPU-time estimate. Drives the column label so the two are never confused.
   */
  energyAccurate: boolean;
}

export interface CpuData {
  /** Per-core utilisation, each 0..100. See I-2. */
  perCore: number[];
  /** Mean across cores, 0..100. Never a sum of process rows. See I-6. */
  system: number;
  userPercent: number;
  sysPercent: number;
  loadAvg: [number, number, number];
}

export interface MemoryData {
  totalBytes: number;
  /** Wired + active + compressed: the genuinely non-reclaimable portion. */
  usedBytes: number;
  /** True vm_stat free (+ speculative). Small by design on macOS. */
  freeBytes: number;
  /**
   * Everything the system can hand out on demand: free plus reclaimable
   * inactive pages. This is `total - used`, and it is what the gauge measures
   * against — NOT `freeBytes`, which is near zero on a healthy Mac.
   */
  availableBytes: number;
  wiredBytes: number;
  activeBytes: number;
  inactiveBytes: number;
  compressedBytes: number;
  swapTotalBytes: number;
  swapUsedBytes: number;
}

export interface DiskData {
  mount: string;
  totalBytes: number;
  usedBytes: number;
  freeBytes: number;
}

export interface BatteryData {
  present: boolean;
  percent: number;
  charging: boolean;
  /**
   * On mains power. Distinct from `charging`: macOS reports "AC attached; not
   * charging" whenever it is deliberately holding the charge level, which is
   * neither charging nor draining.
   */
  onAcPower: boolean;
  timeRemainingMin: number | null;
  /** Instantaneous power. Negative = discharging. */
  watts: number | null;
  cycleCount: number | null;
  healthPercent: number | null;
  temperatureC: number | null;
}

export interface HostInfo {
  model: string;
  cores: number;
  perfCores: number;
  effCores: number;
  totalMemBytes: number;
  uptimeSec: number;
}

/**
 * Per-panel state. A failing collector degrades only its own panel. See I-11.
 * Panels sit on different tiers, so each carries its own sample time. See I-4.
 */
export type Panel<T> =
  | { status: 'pending' }
  | { status: 'ok'; data: T; sampledAt: number }
  | { status: 'error'; message: string; sampledAt: number };

export function panelData<T>(p: Panel<T>): T | null {
  return p.status === 'ok' ? p.data : null;
}

export interface Snapshot {
  host: HostInfo;
  cpu: Panel<CpuData>;
  memory: Panel<MemoryData>;
  disk: Panel<DiskData>;
  battery: Panel<BatteryData>;
  processes: Panel<ProcessesData>;
}
