import type { BatteryData, DiskData, MemoryData, ProcessMeta, RawProcess } from '../../core/types.js';

/**
 * Pure parsers for macOS command output. Kept separate from process spawning so
 * every one is testable against captured fixtures (see test/fixtures/).
 */

/**
 * `ps -o time` prints cumulative CPU time as `MMMM:SS.ss` — minutes, seconds
 * and hundredths. It does not roll over into hours: this machine shows
 * `1206:18.96` (20+ hours of CPU) rather than `20:06:18`.
 *
 * The centisecond field is the reason per-process CPU% is possible at all; a
 * whole-second resolution would make short sampling windows meaningless.
 */
export function parseCpuTimeMs(field: string): number | null {
  const s = field.trim();
  // Defensive: accept an optional hours group in case a future format adds one.
  const m = /^(?:(\d+):)?(\d+):(\d+)(?:\.(\d{1,2}))?$/.exec(s);
  if (!m) return null;
  const hours = m[1] ? Number(m[1]) : 0;
  const minutes = Number(m[2]);
  const seconds = Number(m[3]);
  const frac = m[4] ? Number(m[4].padEnd(2, '0')) : 0;
  return ((hours * 60 + minutes) * 60 + seconds) * 1000 + frac * 10;
}

/** `ps -Ao pid,ppid,time,rss`. RSS is in kilobytes. */
export function parsePsHot(stdout: string): RawProcess[] {
  const out: RawProcess[] = [];
  const lines = stdout.split('\n');
  for (let i = 1; i < lines.length; i++) {
    const line = lines[i]!.trim();
    if (!line) continue;
    const parts = line.split(/\s+/);
    if (parts.length < 4) continue;
    const pid = Number(parts[0]);
    const ppid = Number(parts[1]);
    const cpuTimeMs = parseCpuTimeMs(parts[2]!);
    const rssKb = Number(parts[3]);
    if (!Number.isFinite(pid) || cpuTimeMs === null || !Number.isFinite(rssKb)) continue;
    out.push({ pid, ppid, cpuTimeMs, rssBytes: rssKb * 1024 });
  }
  return out;
}

/**
 * `ps -Ao pid,lstart,user,state,comm`.
 *
 * `lstart` is five whitespace-separated tokens with a padded day
 * ("Sat Aug  1 17:46:44 2026"), and COMM is a path that may itself contain
 * spaces, so this is positional rather than a naive split.
 */
export function parsePsStatic(stdout: string): ProcessMeta[] {
  const out: ProcessMeta[] = [];
  const lines = stdout.split('\n');
  for (let i = 1; i < lines.length; i++) {
    const raw = lines[i]!;
    if (!raw.trim()) continue;
    const m = /^\s*(\d+)\s+(\S+\s+\S+\s+\d+\s+\d+:\d+:\d+\s+\d+)\s+(\S+)\s+(\S+)\s+(.*)$/.exec(raw);
    if (!m) continue;
    const pid = Number(m[1]);
    const started = Date.parse(m[2]!.replace(/\s+/g, ' '));
    out.push({
      pid,
      startTime: Number.isFinite(started) ? started : 0,
      user: m[3]!,
      state: m[4]!,
      command: m[5]!.trim(),
    });
  }
  return out;
}

/**
 * `vm_stat` + `sysctl vm.swapusage`.
 *
 * `os.freemem()` is deliberately not used: it reported 170 MB on a machine with
 * over a gigabyte genuinely available, because macOS counts compressed and
 * purgeable pages differently than the number that call returns.
 */
export function parseMemory(
  vmStat: string,
  swapUsage: string,
  totalBytes: number,
): MemoryData {
  const pageSize = Number(/page size of (\d+) bytes/.exec(vmStat)?.[1] ?? 4096);
  const pages = (label: string): number => {
    const m = new RegExp(`^${label}:\\s+(\\d+)\\.`, 'm').exec(vmStat);
    return m ? Number(m[1]) * pageSize : 0;
  };

  const free = pages('Pages free') + pages('Pages speculative');
  const active = pages('Pages active');
  const inactive = pages('Pages inactive');
  const wired = pages('Pages wired down');
  // "occupied by compressor" is the compressed footprint. "stored in
  // compressor" is the pre-compression size of the same data and would
  // massively over-count.
  const compressed = pages('Pages occupied by compressor');

  /*
   * What counts as "used".
   *
   * `top` reports total-minus-free, which on this machine reads 99.5% — true,
   * but useless as a gauge, because macOS deliberately leaves almost nothing
   * free and reclaims inactive pages on demand.
   *
   * Activity Monitor's "Memory Used" is wired + app memory + compressed, which
   * measured 76.6% at the same instant. That is the number that actually moves
   * when you close something, so it is the one on the gauge. Inactive is
   * reported separately as reclaimable.
   */
  const used = wired + active + compressed;

  const swapNum = (label: string): number => {
    const m = new RegExp(`${label}\\s*=\\s*([\\d.]+)([KMG])`, 'i').exec(swapUsage);
    if (!m) return 0;
    const mult = { K: 1024, M: 1024 ** 2, G: 1024 ** 3 }[m[2]!.toUpperCase()] ?? 1;
    return Number(m[1]) * mult;
  };

  const usedClamped = Math.min(totalBytes, used);
  return {
    totalBytes,
    usedBytes: usedClamped,
    // True free, kept distinct from available: listing both `inactive` and an
    // "available" figure that already contains inactive would double-count it
    // in the memory breakdown.
    freeBytes: free,
    // I-5: used + available == total by construction.
    availableBytes: Math.max(0, totalBytes - usedClamped),
    wiredBytes: wired,
    activeBytes: active,
    inactiveBytes: inactive,
    compressedBytes: compressed,
    swapTotalBytes: swapNum('total'),
    swapUsedBytes: swapNum('used'),
  };
}

/**
 * `df -k`, for one mount point. Blocks are 1024 bytes.
 *
 * Usage is computed as total minus available, NOT df's own "Used" column. On
 * APFS, `/` is a sealed read-only system snapshot whose Used column reads 12 G
 * on this machine, while the shared container actually holds 285 G. Reporting
 * the Used column shows a 926 G disk as 1% full.
 */
export function parseDf(stdout: string, mount = '/'): DiskData | null {
  const lines = stdout.split('\n');
  for (let i = 1; i < lines.length; i++) {
    const line = lines[i]!;
    if (!line.trim()) continue;
    // Mount point is the remainder after the 8 fixed numeric/percent columns.
    const m = /^(\S+)\s+(\d+)\s+(\d+)\s+(\d+)\s+(\d+)%\s+(\d+)\s+(\d+)\s+(\S+)\s+(.*)$/.exec(line);
    if (!m) continue;
    if (m[9]!.trim() !== mount) continue;
    const totalBytes = Number(m[2]) * 1024;
    const freeBytes = Number(m[4]) * 1024;
    return {
      mount,
      totalBytes,
      usedBytes: Math.max(0, totalBytes - freeBytes),
      freeBytes,
    };
  }
  return null;
}

/**
 * `pmset -g batt`.
 *
 * macOS reports at least five states, and they are not all lowercase:
 *   "discharging" | "charging" | "finishing charge" | "charged" | "AC attached"
 * The last one appears whenever macOS deliberately holds the charge level
 * (optimised charging), and is neither charging nor draining. A lowercase-only
 * pattern silently fails to match it and reports "no battery".
 */
export function parsePmset(stdout: string): {
  present: boolean;
  percent: number;
  charging: boolean;
  onAcPower: boolean;
  timeRemainingMin: number | null;
} | null {
  const m = /(\d+)%;\s*([A-Za-z ]+?);/.exec(stdout);
  if (!m) return null;
  const state = m[2]!.trim().toLowerCase();
  const t = /(\d+):(\d{2})\s+remaining/.exec(stdout);
  const mins = t ? Number(t[1]) * 60 + Number(t[2]) : null;
  return {
    present: /present:\s*true/.test(stdout),
    percent: Number(m[1]),
    charging: state === 'charging' || state === 'finishing charge',
    onAcPower: /drawing from 'AC Power'/.test(stdout) || state !== 'discharging',
    // "0:00 remaining" and "(no estimate)" both mean "unknown", not zero.
    timeRemainingMin: mins && mins > 0 ? mins : null,
  };
}

const TWO_POW_64 = 2n ** 64n;
const TWO_POW_63 = 2n ** 63n;

/**
 * ioreg prints Amperage as an unsigned 64-bit integer, so a discharging battery
 * appears as e.g. 18446744073709548164 rather than -3452 mA. Reinterpret it as
 * signed, otherwise the sign of the wattage — the whole point of the field —
 * is wrong.
 */
export function parseSignedInt64(text: string): number | null {
  try {
    let v = BigInt(text.trim());
    if (v >= TWO_POW_63) v -= TWO_POW_64;
    return Number(v);
  } catch {
    return null;
  }
}

/** `ioreg -rn AppleSmartBattery -w0`. */
export function parseIoregBattery(stdout: string): Partial<BatteryData> {
  const num = (key: string): number | null => {
    const m = new RegExp(`"${key}"\\s*=\\s*(-?\\d+)`).exec(stdout);
    return m ? parseSignedInt64(m[1]!) : null;
  };
  const yesNo = (key: string): boolean | null => {
    const m = new RegExp(`"${key}"\\s*=\\s*(Yes|No)`).exec(stdout);
    return m ? m[1] === 'Yes' : null;
  };

  const voltageMv = num('Voltage');
  const amperageMa = num('InstantAmperage') ?? num('Amperage');
  const rawMax = num('AppleRawMaxCapacity');
  const design = num('DesignCapacity');
  const tempCentiC = num('Temperature');
  const charging = yesNo('IsCharging');

  return {
    watts:
      voltageMv !== null && amperageMa !== null ? (voltageMv * amperageMa) / 1_000_000 : null,
    cycleCount: num('CycleCount'),
    healthPercent: rawMax !== null && design ? Math.round((rawMax / design) * 100) : null,
    temperatureC: tempCentiC !== null ? tempCentiC / 100 : null,
    ...(charging !== null ? { charging } : {}),
  };
}
