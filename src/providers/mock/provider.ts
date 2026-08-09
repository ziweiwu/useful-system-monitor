import { energyProxy } from '../../core/scoring.js';
import { selectWorkingSet, WORKING_SET_SIZE } from '../../core/workingSet.js';
import type {
  BatteryData,
  CpuData,
  DiskData,
  HostInfo,
  MemoryData,
  ProcessSample,
  ProcessesData,
  VolumeUsage,
} from '../../core/types.js';
import { isProtectedName } from '../../kill/guards.js';
import type { MetricsProvider } from '../types.js';

/** Deterministic PRNG so the mock and its tests are reproducible. */
function mulberry32(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

interface FakeProc {
  pid: number;
  ppid: number;
  command: string;
  user: string;
  /** Baseline CPU% it hovers around. */
  cpuBase: number;
  /** How far it swings. */
  cpuSwing: number;
  rssBytes: number;
  rssDrift: number;
}

const GB = 1024 ** 3;

function vol(
  mount: string,
  device: string,
  totalBytes: number,
  usedBytes: number,
  network: boolean,
): VolumeUsage {
  return { mount, device, totalBytes, usedBytes, freeBytes: totalBytes - usedBytes, network };
}
const MB = 1024 ** 2;

const FAKE: FakeProc[] = [
  { pid: 38458, ppid: 16758, command: '/Applications/Google Chrome.app/Contents/Frameworks/Google Chrome Helper (Renderer).app/Contents/MacOS/Google Chrome Helper (Renderer)', user: 'ziweiwu', cpuBase: 68, cpuSwing: 22, rssBytes: 780 * MB, rssDrift: 40 * MB },
  { pid: 1578, ppid: 1, command: '/Library/Application Support/Logitech.localized/LogiOptionsPlus/logioptionsplus_agent.app/Contents/MacOS/logioptionsplus_agent', user: 'root', cpuBase: 62, cpuSwing: 18, rssBytes: 33 * MB, rssDrift: 2 * MB },
  { pid: 16758, ppid: 1, command: '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', user: 'ziweiwu', cpuBase: 52, cpuSwing: 20, rssBytes: 492 * MB, rssDrift: 30 * MB },
  { pid: 24779, ppid: 58976, command: '/Applications/Firefox.app/Contents/MacOS/plugin-container', user: 'ziweiwu', cpuBase: 36, cpuSwing: 16, rssBytes: 323 * MB, rssDrift: 25 * MB },
  { pid: 616, ppid: 1, command: '/System/Library/PrivateFrameworks/SkyLight.framework/Resources/WindowServer', user: '_windowserver', cpuBase: 24, cpuSwing: 10, rssBytes: 937 * MB, rssDrift: 20 * MB },
  { pid: 1285, ppid: 1, command: '/Applications/iTerm.app/Contents/MacOS/iTerm2', user: 'ziweiwu', cpuBase: 20, cpuSwing: 14, rssBytes: 117 * MB, rssDrift: 8 * MB },
  { pid: 58976, ppid: 1, command: '/Applications/Firefox.app/Contents/MacOS/firefox', user: 'ziweiwu', cpuBase: 11, cpuSwing: 8, rssBytes: 168 * MB, rssDrift: 12 * MB },
  { pid: 442, ppid: 1, command: '/System/Library/Frameworks/CoreServices.framework/Frameworks/Metadata.framework/Support/mds_stores', user: 'root', cpuBase: 8, cpuSwing: 9, rssBytes: 92 * MB, rssDrift: 6 * MB },
  { pid: 99445, ppid: 1, command: '/Applications/Spotify.app/Contents/MacOS/Spotify', user: 'ziweiwu', cpuBase: 4, cpuSwing: 5, rssBytes: 314 * MB, rssDrift: 10 * MB },
  { pid: 5521, ppid: 1, command: '/Applications/Slack.app/Contents/MacOS/Slack', user: 'ziweiwu', cpuBase: 3, cpuSwing: 6, rssBytes: 612 * MB, rssDrift: 18 * MB },
  { pid: 7781, ppid: 1, command: '/Applications/Docker.app/Contents/MacOS/com.docker.backend', user: 'ziweiwu', cpuBase: 2, cpuSwing: 4, rssBytes: 1.4 * GB, rssDrift: 40 * MB },
  { pid: 3312, ppid: 1, command: '/usr/libexec/secinitd', user: 'root', cpuBase: 0.4, cpuSwing: 0.6, rssBytes: 4 * MB, rssDrift: 512 * 1024 },
  { pid: 2201, ppid: 1, command: '/System/Library/CoreServices/Dock.app/Contents/MacOS/Dock', user: 'ziweiwu', cpuBase: 1.1, cpuSwing: 2, rssBytes: 88 * MB, rssDrift: 4 * MB },
  { pid: 9110, ppid: 1, command: '/Applications/Visual Studio Code.app/Contents/MacOS/Electron', user: 'ziweiwu', cpuBase: 6, cpuSwing: 9, rssBytes: 540 * MB, rssDrift: 22 * MB },
  { pid: 4477, ppid: 1, command: '/usr/sbin/bluetoothd', user: 'root', cpuBase: 0.8, cpuSwing: 1.2, rssBytes: 12 * MB, rssDrift: 1 * MB },
];

const TOTAL_PROCESSES = 811;

/**
 * Scripted provider used by `--mock`. Produces plausible, slowly-drifting data
 * so the layout, colours, sorting, filtering and the kill flow can all be
 * reviewed without touching the real system.
 */
export class MockProvider implements MetricsProvider {
  readonly name = 'mock';
  private t = 0;
  private readonly rand: () => number;
  private readonly killed = new Set<number>();

  constructor(seed = 42) {
    this.rand = mulberry32(seed);
  }

  /** Advance the simulation. Called by the sampler on each processes tick. */
  private tick(): void {
    this.t += 1;
  }

  /** Mock kills remove the process from later samples but send no signal. */
  simulateKill(pid: number): void {
    this.killed.add(pid);
  }

  async host(): Promise<HostInfo> {
    return {
      model: 'Apple M1 Pro',
      cores: 10,
      perfCores: 8,
      effCores: 2,
      totalMemBytes: 16 * GB,
      uptimeSec: 7 * 86400 + 3 * 3600 + 24 * 60,
    };
  }

  async cpu(): Promise<CpuData> {
    this.t += 0.35;
    const wave = (i: number) => {
      const base = 55 + 35 * Math.sin(this.t / 4 + i * 0.7);
      return Math.max(2, Math.min(100, base + (this.rand() - 0.5) * 18));
    };
    // 8 performance cores then 2 efficiency cores, the split that matters on
    // Apple Silicon.
    const perCore = [
      ...Array.from({ length: 8 }, (_, i) => wave(i)),
      ...Array.from({ length: 2 }, (_, i) => wave(i + 8) * 0.45),
    ];
    const system = perCore.reduce((s, v) => s + v, 0) / perCore.length;
    const userPercent = system * 0.46;
    return {
      perCore,
      system,
      userPercent,
      sysPercent: system - userPercent,
      loadAvg: [16.2, 18.8, 14.9],
    };
  }

  async memory(): Promise<MemoryData> {
    const total = 16 * GB;
    const used = total * (0.84 + 0.06 * Math.sin(this.t / 9));
    return {
      totalBytes: total,
      usedBytes: used,
      freeBytes: 0.09 * GB,
      availableBytes: total - used,
      wiredBytes: 4.0 * GB,
      activeBytes: 2.4 * GB,
      inactiveBytes: 2.3 * GB,
      compressedBytes: 5.9 * GB,
      swapTotalBytes: 12 * GB,
      swapUsedBytes: 10.6 * GB,
    };
  }

  async disk(): Promise<DiskData> {
    const total = 931 * GB;
    const used = 289 * GB;
    /* Deliberately covers the three cases the view has to render well: the
       root volume, an external disk, and a nearly-full network share (which is
       what the severity colouring exists for). */
    const volumes: VolumeUsage[] = [
      vol('/', '/dev/disk3s1s1', total, used, false),
      vol('/Volumes/Backup', '/dev/disk5s2', 2000 * GB, 812 * GB, false),
      vol('/Volumes/media', '//user@nas._smb._tcp.local/media', 14 * 1024 * GB, 13 * 1024 * GB, true),
    ];
    return { mount: '/', totalBytes: total, usedBytes: used, freeBytes: total - used, volumes };
  }

  async battery(): Promise<BatteryData> {
    const pct = Math.max(3, 48 - Math.floor(this.t / 40));
    return {
      present: true,
      percent: pct,
      charging: false,
      onAcPower: false,
      timeRemainingMin: Math.round(pct * 0.9),
      watts: -(17 + 3 * Math.sin(this.t / 6)),
      cycleCount: 194,
      healthPercent: 87,
      temperatureC: 30.99,
    };
  }

  async commandLine(pid: number): Promise<string | null> {
    const f = FAKE.find((x) => x.pid === pid);
    return f ? `${f.command} --type=renderer --enable-features=Foo` : null;
  }

  async processes(limit: number = WORKING_SET_SIZE): Promise<ProcessesData> {
    this.tick();
    const all: ProcessSample[] = [];

    for (const f of FAKE) {
      if (this.killed.has(f.pid)) continue;
      const swing = Math.sin(this.t / 3 + f.pid) * f.cpuSwing;
      const cpu = Math.max(0, f.cpuBase + swing + (this.rand() - 0.5) * 4);
      const rss = Math.max(1 * MB, f.rssBytes + Math.sin(this.t / 5 + f.pid) * f.rssDrift);
      all.push({
        pid: f.pid,
        ppid: f.ppid,
        startTime: 1_700_000_000_000 + f.pid * 1000,
        command: f.command,
        user: f.user,
        state: 'S',
        cpuPercent: cpu,
        rssBytes: rss,
        energy: energyProxy(cpu),
        protected: isProtectedName(f.command),
      });
    }

    // A long tail of small processes, so the "… N others" roll-up is real
    // rather than a decorative constant.
    const tailCount = TOTAL_PROCESSES - all.length - this.killed.size;
    for (let i = 0; i < tailCount; i++) {
      const cpu = this.rand() * 0.12;
      all.push({
        pid: 20000 + i,
        ppid: 1,
        startTime: 1_700_000_000_000 + i,
        command: `/usr/libexec/helperd${i}`,
        user: i % 3 === 0 ? 'root' : 'ziweiwu',
        state: 'S',
        cpuPercent: cpu,
        rssBytes: (1 + this.rand() * 3) * MB,
        energy: energyProxy(cpu),
        protected: false,
      });
    }

    const { visible, others } = selectWorkingSet(all, limit);
    const parents = new Map<number, number>(all.map((p) => [p.pid, p.ppid]));
    return { total: all.length, visible, others, parents, energyAccurate: false };
  }
}
