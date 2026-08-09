#!/usr/bin/env node
import { render } from 'ink';
import { bytes, percent } from './core/format.js';
import { sortProcesses, type SortKey } from './core/scoring.js';
import { DarwinProvider } from './providers/darwin/provider.js';
import { MockProvider } from './providers/mock/provider.js';
import { DEFAULT_TIERS, type MetricsProvider, type Tiers } from './providers/types.js';
import { processName } from './kill/guards.js';
import { App } from './app.js';

interface Options {
  accurateEnergy: boolean;
  mock: boolean;
  json: boolean;
  top: number;
  sort: SortKey;
  interval: number | null;
  help: boolean;
}

function parseArgs(argv: readonly string[]): Options {
  const o: Options = {
    accurateEnergy: false,
    mock: false,
    json: false,
    top: 10,
    sort: 'cpu',
    interval: null,
    help: false,
  };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i]!;
    if (a === '--mock') o.mock = true;
    else if (a === '--energy=accurate') o.accurateEnergy = true;
    else if (a === '--energy' && argv[i + 1] === 'accurate') {
      o.accurateEnergy = true;
      i++;
    }
    else if (a === '--json') o.json = true;
    else if (a === '--help' || a === '-h') o.help = true;
    else if (a === '--top') o.top = Number(argv[++i]) || 10;
    else if (a.startsWith('--top=')) o.top = Number(a.slice(6)) || 10;
    else if (a === '--sort') o.sort = (argv[++i] as SortKey) ?? 'cpu';
    else if (a.startsWith('--sort=')) o.sort = a.slice(7) as SortKey;
    else if (a === '--interval') o.interval = Number(argv[++i]) || null;
    else if (a.startsWith('--interval=')) o.interval = Number(a.slice(11)) || null;
  }
  return o;
}

const HELP = `useful-system-monitor — terminal system resource monitor

Usage
  useful-system-monitor [options]

Options
  --mock              Run with scripted data (no system access)
  --json              One-shot JSON to stdout, for scripting
  --top N             Rows in one-shot output (default 10)
  --sort cpu|mem|energy
  --interval SECONDS  Process sampling interval (default 10)
  --energy=accurate   Use macOS Energy Impact instead of the CPU-time estimate.
                      Costs ~1s of CPU per 60s sample (~5x the default budget).
  -h, --help

Keys
  up/dn move   enter details   k kill   / filter   c m e sort   1-4 view   q quit

Notes
  Energy is estimated from CPU time by default; macOS's own Energy Impact
  costs ~1s of CPU per sample, which would make this tool a battery drain.
`;

/** One-shot, pipe-friendly output. Used when stdout is not a TTY. See I-22. */
async function oneShot(provider: MetricsProvider, o: Options): Promise<number> {
  // Two samples are required: CPU% is always a delta, never a lifetime average.
  await provider.processes();
  await new Promise((r) => setTimeout(r, 300));
  const [cpu, mem, batt, procs] = await Promise.all([
    provider.cpu(),
    provider.memory(),
    provider.battery(),
    provider.processes(),
  ]);
  const top = sortProcesses(procs.visible, o.sort).slice(0, o.top);

  if (o.json) {
    process.stdout.write(
      JSON.stringify(
        {
          cpu: { system: cpu.system, perCore: cpu.perCore, loadAvg: cpu.loadAvg },
          memory: mem,
          battery: batt,
          processes: top.map((p) => ({
            pid: p.pid,
            name: processName(p.command),
            command: p.command,
            user: p.user,
            cpuPercent: p.cpuPercent,
            rssBytes: p.rssBytes,
            energy: p.energy,
          })),
          others: procs.others,
          total: procs.total,
          energyAccurate: procs.energyAccurate,
        },
        null,
        2,
      ) + '\n',
    );
    return 0;
  }

  const lines = [
    `cpu ${cpu.system.toFixed(1)}%  mem ${((mem.usedBytes / mem.totalBytes) * 100).toFixed(1)}%  battery ${batt.percent}%${batt.charging ? ' charging' : ''}`,
    '',
    'PID     CPU%     MEM  NAME',
  ];
  for (const p of top) {
    lines.push(
      `${String(p.pid).padEnd(7)} ${percent(p.cpuPercent).padStart(5)}  ${bytes(p.rssBytes).padStart(6)}  ${processName(p.command)}`,
    );
  }
  process.stdout.write(lines.join('\n') + '\n');
  return 0;
}

async function main(): Promise<void> {
  const o = parseArgs(process.argv.slice(2));

  if (o.help) {
    process.stdout.write(HELP);
    process.exit(0);
  }

  let provider: MetricsProvider;
  if (o.mock) {
    provider = new MockProvider();
  } else if (process.platform === 'darwin') {
    provider = new DarwinProvider({ accurateEnergy: o.accurateEnergy });
  } else {
    // I-24: name the cause and the remedy rather than failing obscurely.
    process.stderr.write(
      `useful-system-monitor: only macOS is supported today (this is ${process.platform}).\n` +
        '        Run `useful-system-monitor --mock` to see the interface with scripted data.\n',
    );
    process.exit(1);
  }

  /*
   * I-22: no TUI unless we have a real terminal on BOTH ends.
   *
   * stdout alone is not enough. Ink's useInput needs raw mode on stdin, and
   * when stdin is a pipe or /dev/null (`useful-system-monitor < /dev/null`, or the process
   * backgrounded from a script) it throws "Raw mode is not supported" and dies
   * with a React stack trace. Falling back to one-shot output is both more
   * useful and more composable.
   */
  const interactive = Boolean(process.stdout.isTTY) && Boolean(process.stdin.isTTY);
  if (!interactive || o.json) {
    if (process.stdout.isTTY && !process.stdin.isTTY && !o.json) {
      // I-24: say why the dashboard did not appear, and how to get it.
      process.stderr.write(
        'useful-system-monitor: stdin is not a terminal, so the interactive dashboard is unavailable.\n' +
          '        Showing a one-shot summary. Run it directly from a shell for the TUI.\n',
      );
    }
    const code = await oneShot(provider, o);
    process.exit(code);
  }

  const tiers: Tiers = o.interval
    ? {
        ...DEFAULT_TIERS,
        // The CPU tier drives the render rate, which dominates cost, so
        // --interval has to move it too or the flag cannot buy responsiveness.
        cpu: o.interval * 1000,
        processes: o.interval * 1000,
        memory: o.interval * 1000,
      }
    : DEFAULT_TIERS;

  const mock = provider instanceof MockProvider ? provider : null;
  const { waitUntilExit } = render(
    <App
      provider={provider}
      tiers={tiers}
      demo={o.mock}
      killFn={mock ? () => {} : undefined}
      onKilled={mock ? (pid) => mock.simulateKill(pid) : undefined}
    />,
    { exitOnCtrlC: true },
  );
  await waitUntilExit();
}

main().catch((err: unknown) => {
  // I-24: errors to stderr, non-zero exit.
  process.stderr.write(`useful-system-monitor: ${err instanceof Error ? err.message : String(err)}\n`);
  process.exit(1);
});
