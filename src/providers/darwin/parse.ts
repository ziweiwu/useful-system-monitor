import type {
  BatteryData,
  DiskData,
  MemoryData,
  ProcessMeta,
  RawProcess,
  VolumeUsage,
} from '../../core/types.js';

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
 * `ps -Ao pid,lstart,user,state,comm`, in the C locale.
 *
 * `lstart` is five whitespace-separated tokens with a padded day
 * ("Sat Aug  1 17:46:44 2026"), and COMM is a path that may itself contain
 * spaces, so this is positional rather than a naive split.
 */
const PS_STATIC_C =
  /^\s*(\d+)\s+(\S+\s+\S+\s+\d+\s+\d+:\d+:\d+\s+\d+)\s+(\S+)\s+(\S+)\s+(.*)$/;

/**
 * The same line when `lstart` did not come out in the C locale.
 *
 * `exec.ts` pins LC_TIME so this should be unreachable, but a name is the one
 * column a user cannot work around — a table of "pid 1234" is unusable, and
 * that is what a single unrecognised date format used to produce for every row
 * on the machine. The only structure every locale shares is the clock, so this
 * anchors on the first HH:MM:SS and takes an optional trailing year.
 *
 * It recovers the name, the user and the state. It does *not* recover
 * `startTime`: Date.parse cannot read a localised month, and inventing a value
 * for the field the PID-reuse guard binds to (I-16) would be worse than
 * admitting it is unknown.
 */
const PS_STATIC_ANY_LOCALE =
  /^\s*(\d+)\s+(\S.*?\d{1,2}:\d{2}:\d{2}(?:\s+\d{4})?)\s+(\S+)\s+(\S+)\s+(.*)$/;

export function parsePsStatic(stdout: string): ProcessMeta[] {
  const out: ProcessMeta[] = [];
  const lines = stdout.split('\n');
  for (let i = 1; i < lines.length; i++) {
    const raw = lines[i]!;
    if (!raw.trim()) continue;
    const m = PS_STATIC_C.exec(raw) ?? PS_STATIC_ANY_LOCALE.exec(raw);
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

  /*
   * `sysctl vm.swapusage` formats through LC_NUMERIC, so a comma-decimal
   * locale prints "total = 1024,00M". `exec.ts` pins LC_NUMERIC=C, but a
   * comma is accepted here too: the previous pattern stopped at the separator
   * and reported a machine with 1 GB of swap as having none.
   */
  const swapNum = (label: string): number => {
    const m = new RegExp(`${label}\\s*=\\s*(\\d+(?:[.,]\\d+)?)([KMG])`, 'i').exec(swapUsage);
    if (!m) return 0;
    const mult = { K: 1024, M: 1024 ** 2, G: 1024 ** 3 }[m[2]!.toUpperCase()] ?? 1;
    const n = Number(m[1]!.replace(',', '.'));
    return Number.isFinite(n) ? n * mult : 0;
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
export function parseDf(
  stdout: string,
  mount = '/',
): Omit<DiskData, 'volumes'> | null {
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
 * Every user-facing volume in `df -k` output.
 *
 * Two kinds of noise have to go, or the panel lies about how much storage the
 * machine has:
 *
 *  1. APFS container siblings. `/`, `/System/Volumes/Data`, `/System/Volumes/VM`
 *     and `/System/Volumes/Preboot` are separate mounts that share one pool, so
 *     df reports the *same* total and available blocks for each. Listing them
 *     verbatim shows a 926 G disk four times. They are grouped by container
 *     (`/dev/disk3s5` -> `disk3`) and reported once, at their outermost mount.
 *  2. Pseudo and firmware filesystems: devfs, `map auto_home` (zero blocks),
 *     and the iSCPreboot/xarts/Hardware volumes, which are macOS internals no
 *     user can act on.
 *
 * Usage is total minus available for the same APFS reason parseDf documents:
 * df's own Used column reads the sealed system snapshot, not the container.
 */
export function parseDfAll(stdout: string): VolumeUsage[] {
  interface Row {
    device: string;
    mount: string;
    totalBytes: number;
    freeBytes: number;
  }
  const rows: Row[] = [];
  const lines = stdout.split('\n');
  for (let i = 1; i < lines.length; i++) {
    const line = lines[i]!;
    if (!line.trim()) continue;
    const m = /^(\S+)\s+(\d+)\s+(\d+)\s+(\d+)\s+(\d+)%\s+(\d+)\s+(\d+)\s+(\S+)\s+(.*)$/.exec(line);
    // `map auto_home` and friends have a space in the device column and so
    // fail this pattern outright, which is the intended outcome.
    if (!m) continue;
    const device = m[1]!;
    const mount = m[9]!.trim();
    const totalBytes = Number(m[2]) * 1024;
    // Zero-block and devfs-style entries are not storage.
    if (totalBytes <= 0 || device === 'devfs' || device === 'devfs.') continue;
    rows.push({ device, mount, totalBytes, freeBytes: Number(m[4]) * 1024 });
  }

  /* Group by APFS container, so one pool reports once. Anything that is not a
     /dev/diskN device (network shares, disk images) is its own group. */
  const groups = new Map<string, Row[]>();
  for (const r of rows) {
    const c = /^\/dev\/(disk\d+)/.exec(r.device);
    const key = c ? c[1]! : r.device;
    const g = groups.get(key);
    if (g) g.push(r);
    else groups.set(key, [r]);
  }

  const out: VolumeUsage[] = [];
  for (const g of groups.values()) {
    /* The outermost mount represents the pool: `/` if the container holds it,
       otherwise the shortest path, which is the one a user would recognise. */
    const rep =
      g.find((r) => r.mount === '/') ??
      g.toSorted((a, b) => a.mount.length - b.mount.length)[0]!;
    /* Containers that surface only under /System/Volumes are macOS internals
       (Preboot, Update/SFR, the firmware volumes). `/` is never excluded here
       because it is picked as the representative above. */
    if (rep.mount.startsWith('/System/Volumes/')) continue;
    out.push({
      mount: rep.mount,
      device: rep.device,
      totalBytes: rep.totalBytes,
      usedBytes: Math.max(0, rep.totalBytes - rep.freeBytes),
      freeBytes: rep.freeBytes,
      // SMB/AFP shares are `//user@host/share`; NFS is `host:/export`.
      network: rep.device.startsWith('//') || /^[^/]+:\//.test(rep.device),
    });
  }

  // Root first, then alphabetically, so the list is stable across samples.
  return out.toSorted((a, b) =>
    a.mount === '/' ? -1 : b.mount === '/' ? 1 : a.mount.localeCompare(b.mount),
  );
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
