import os from 'node:os';
import { CpuDeltaTracker, coreUtilisation, type CpuTimes } from '../../core/deltas.js';
import { energyProxy } from '../../core/scoring.js';
import type {
  BatteryData,
  CpuData,
  DiskData,
  HostInfo,
  MemoryData,
  ProcessMeta,
  ProcessSample,
  RawProcess,
  ProcessesData,
} from '../../core/types.js';
import { selectWorkingSet, WORKING_SET_SIZE } from '../../core/workingSet.js';
import { isProtectedName } from '../../kill/guards.js';
import type { MetricsProvider } from '../types.js';
import { BIN, run } from './exec.js';
import { sampleEnergyImpact } from './power.js';
import {
  parseDf,
  parseIoregBattery,
  parseMemory,
  parsePmset,
  parsePsHot,
  parsePsStatic,
} from './parse.js';

function times(c: os.CpuInfo): CpuTimes {
  return { user: c.times.user, nice: c.times.nice, sys: c.times.sys, idle: c.times.idle, irq: c.times.irq };
}

/** Live macOS collectors. See the Cost budget for why each is shaped this way. */
export class DarwinProvider implements MetricsProvider {
  readonly name = 'darwin';

  /**
   * Opt-in accurate energy. Off by default because `top -stats power` costs
   * ~1.0s of CPU per sample — about 5x this app's entire default budget.
   */
  private readonly accurateEnergy: boolean;
  private energyImpact = new Map<number, number>();
  private energyFetchedAt = 0;
  private energyInFlight = false;

  constructor(opts: { accurateEnergy?: boolean } = {}) {
    this.accurateEnergy = opts.accurateEnergy ?? false;
  }

  /** Slow lane: refreshed in the background, never awaited. See I-7, I-8. */
  private maybeRefreshEnergy(): void {
    if (!this.accurateEnergy || this.energyInFlight) return;
    if (Date.now() - this.energyFetchedAt < DarwinProvider.ENERGY_INTERVAL_MS) return;
    this.energyInFlight = true;
    void sampleEnergyImpact()
      .then((m) => {
        this.energyImpact = m;
        this.energyFetchedAt = Date.now();
      })
      .catch(() => {
        // I-11: losing the accurate lane silently falls back to the estimate
        // rather than blanking the column.
        this.energyFetchedAt = Date.now();
      })
      .finally(() => {
        this.energyInFlight = false;
      });
  }

  private static readonly ENERGY_INTERVAL_MS = 60_000;

  private readonly ncpu = os.cpus().length;
  private readonly deltas = new CpuDeltaTracker(100 * os.cpus().length);
  private prevCpuTimes: CpuTimes[] = os.cpus().map(times);

  /**
   * Static metadata is expensive (it doubles the cost of `ps`: 22ms -> 44ms,
   * because it resolves executable paths and maps UIDs to names) but never
   * changes for a live PID, so it is cached and only refetched when the PID
   * set gains members. See C-1.
   */
  private metaCache = new Map<number, ProcessMeta>();

  async host(): Promise<HostInfo> {
    const cpus = os.cpus();
    // Apple Silicon reports performance cores first. Fall back to treating
    // everything as performance cores on Intel.
    let perfCores = cpus.length;
    try {
      const n = Number((await run(BIN.sysctl, ['-n', 'hw.perflevel0.logicalcpu'])).trim());
      if (Number.isFinite(n) && n > 0 && n <= cpus.length) perfCores = n;
    } catch {
      // Intel Macs have no perflevel keys; the fallback above is correct there.
    }
    let model = cpus[0]?.model ?? 'unknown';
    try {
      model = (await run(BIN.sysctl, ['-n', 'machdep.cpu.brand_string'])).trim() || model;
    } catch {
      /* keep os.cpus() model */
    }
    return {
      model,
      cores: cpus.length,
      perfCores,
      effCores: cpus.length - perfCores,
      totalMemBytes: os.totalmem(),
      uptimeSec: os.uptime(),
    };
  }

  /** Free: no spawn, so this can run every second. See I-6. */
  async cpu(): Promise<CpuData> {
    const cur = os.cpus().map(times);
    const perCore = coreUtilisation(this.prevCpuTimes, cur);

    let dUser = 0;
    let dSys = 0;
    let dTotal = 0;
    for (let i = 0; i < cur.length; i++) {
      const a = this.prevCpuTimes[i];
      const b = cur[i];
      if (!a || !b) continue;
      dUser += b.user - a.user + (b.nice - a.nice);
      dSys += b.sys - a.sys;
      dTotal += b.user - a.user + (b.nice - a.nice) + (b.sys - a.sys) + (b.idle - a.idle) + (b.irq - a.irq);
    }
    this.prevCpuTimes = cur;

    const system = perCore.length ? perCore.reduce((s, v) => s + v, 0) / perCore.length : 0;
    const load = os.loadavg();
    return {
      perCore,
      system,
      userPercent: dTotal > 0 ? (dUser / dTotal) * 100 : 0,
      sysPercent: dTotal > 0 ? (dSys / dTotal) * 100 : 0,
      loadAvg: [load[0] ?? 0, load[1] ?? 0, load[2] ?? 0],
    };
  }

  async memory(): Promise<MemoryData> {
    const [vmStat, swap] = await Promise.all([
      run(BIN.vmStat, []),
      run(BIN.sysctl, ['-n', 'vm.swapusage']),
    ]);
    return parseMemory(vmStat, swap, os.totalmem());
  }

  async disk(): Promise<DiskData> {
    const out = await run(BIN.df, ['-k']);
    const d = parseDf(out, '/');
    if (!d) throw new Error('no filesystem mounted at / in df output');
    return d;
  }

  async battery(): Promise<BatteryData> {
    const [pmsetOut, ioregOut] = await Promise.all([
      run(BIN.pmset, ['-g', 'batt']),
      run(BIN.ioreg, ['-rn', 'AppleSmartBattery', '-w0']),
    ]);
    const base = parsePmset(pmsetOut);
    if (!base) {
      /*
       * No battery is a normal state, not an error: every desktop Mac (mini,
       * Studio, Pro, iMac) and every CI runner reports none. Throwing here
       * would degrade the panel to "error" on a machine that is working
       * perfectly, so report absence and let the UI say so.
       */
      return {
        present: false,
        percent: 0,
        charging: false,
        onAcPower: true,
        timeRemainingMin: null,
        watts: null,
        cycleCount: null,
        healthPercent: null,
        temperatureC: null,
      };
    }
    const detail = parseIoregBattery(ioregOut);
    return {
      present: base.present,
      percent: base.percent,
      charging: detail.charging ?? base.charging,
      onAcPower: base.onAcPower,
      timeRemainingMin: base.timeRemainingMin,
      watts: detail.watts ?? null,
      cycleCount: detail.cycleCount ?? null,
      healthPercent: detail.healthPercent ?? null,
      temperatureC: detail.temperatureC ?? null,
    };
  }

  async commandLine(pid: number): Promise<string | null> {
    try {
      const out = await run(BIN.ps, ['-o', 'command=', '-p', String(pid)]);
      return out.trim() || null;
    } catch {
      // The process may have exited between selection and this call.
      return null;
    }
  }

  async processes(limit: number = WORKING_SET_SIZE): Promise<ProcessesData> {
    // Hot columns only: 22ms rather than 48ms. See C-1.
    const hot = parsePsHot(await run(BIN.ps, ['-Ao', 'pid,ppid,time,rss']));
    const now = Date.now();
    this.maybeRefreshEnergy();

    // I-4b: the tracker sees every PID, so a process entering the working set
    // already has a real CPU% instead of showing "—" for a whole interval.
    const cpuByPid = this.deltas.update(hot, now);

    const build = (p: RawProcess): ProcessSample => {
      const meta = this.metaCache.get(p.pid);
      const command = meta?.command ?? `pid ${p.pid}`;
      const cpuPercent = cpuByPid.get(p.pid) ?? null;
      return {
        pid: p.pid,
        ppid: p.ppid,
        startTime: meta?.startTime ?? 0,
        command,
        user: meta?.user ?? '?',
        state: meta?.state ?? '?',
        cpuPercent,
        rssBytes: p.rssBytes,
        // Measured Energy Impact when the slow lane has it, estimate otherwise.
        energy: this.energyImpact.get(p.pid) ?? energyProxy(cpuPercent),
        // Unnamed processes are treated as protected: refusing to kill
        // something we cannot identify is the safe default. See I-14.
        protected: meta ? isProtectedName(command) : true,
      };
    };

    /*
     * Deciding when to pay for names.
     *
     * The static columns cost 48ms because ps resolves executable paths and
     * maps UIDs. Refetching whenever *any* new PID appears means paying that on
     * almost every tick — a busy Mac spawns short-lived helpers constantly.
     *
     * But selectWorkingSet only reads cpuPercent and rssBytes, so the top 50
     * can be chosen before any name is known. That lets us fetch names only
     * when an unnamed process is actually going to be displayed, which on a
     * steady system is rare.
     */
    let all = hot.map(build);
    let { visible, others } = selectWorkingSet(all, limit);

    if (visible.some((p) => !this.metaCache.has(p.pid))) {
      const metas = parsePsStatic(await run(BIN.ps, ['-Ao', 'pid,lstart,user,state,comm']));
      const live = new Set(hot.map((p) => p.pid));
      // I-10: rebuilding from the live set also evicts exited PIDs, so the
      // cache cannot grow without bound.
      this.metaCache = new Map(metas.filter((m) => live.has(m.pid)).map((m) => [m.pid, m]));
      all = hot.map(build);
      ({ visible, others } = selectWorkingSet(all, limit));
    }

    // Built from every row, so the ancestor guard can walk out of the
    // working set. See I-13.
    const parents = new Map<number, number>(hot.map((p) => [p.pid, p.ppid]));
    return {
      total: all.length,
      visible,
      others,
      parents,
      energyAccurate: this.accurateEnergy && this.energyImpact.size > 0,
    };
  }
}
