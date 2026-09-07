import { createRequire } from 'node:module';
import { render } from 'ink';
import { bytes, percent } from './core/format.js';
import { parseArgs, type Options } from './core/options.js';
import { sortProcesses } from './core/scoring.js';
import { DarwinProvider } from './providers/darwin/provider.js';
import { MockProvider } from './providers/mock/provider.js';
import { DEFAULT_TIERS, type MetricsProvider, type Tiers } from './providers/types.js';
import type { ProcessSample } from './core/types.js';
import { processName } from './kill/guards.js';
import { App } from './app.js';

/* Read at runtime rather than baked in at build time, so the version can never
   disagree with the package it was installed from. `../package.json` resolves
   from both `src/main.tsx` and the compiled `dist/main.js`. */
const VERSION = (createRequire(import.meta.url)('../package.json') as { version: string }).version;

const HELP = `useful-system-monitor — see what's using up your Mac, from the terminal

Usage
  useful-system-monitor [options]

  With no options it opens the dashboard. When stdin or stdout is not a
  terminal it prints a one-shot summary instead, so it composes in pipes,
  scripts and cron.

Options
  --json              One-shot JSON to stdout instead of the dashboard
  --interval SECONDS  Dashboard refresh, 1 or more (default 10). Lower is more
                      responsive and costs more CPU; no effect one-shot.
  --energy=accurate   Use macOS Energy Impact instead of the CPU-time estimate.
                      Costs ~1s of CPU per 60s sample (~5x the default budget).
  --mock              Run with scripted data, without touching the system
  -h, --help          Show this help
  --version           Print the version

  There is no row-count or sort option: the dashboard sorts with c, m and e,
  and --json hands over the whole working set so head and jq can do the rest.

Keys (dashboard)
  left/right   move between the five screens; 1-5 jump straight to one
  up/dn        pick a process     +/-     show more or fewer rows
  enter        details            k       close the selected app
  /            search             c m e   sort by cpu, memory, energy
  r            refresh now        q       quit

Examples
  useful-system-monitor                        open the dashboard
  useful-system-monitor --json | jq .cpu       the numbers, for a script
  useful-system-monitor | head -8              the busiest few, as text
  useful-system-monitor --interval 2           refresh faster to catch a spike

Exit status
  0  success
  1  could not run — unsupported platform, or a collector failed
  2  bad usage — unknown option, or a value that cannot be used

Notes
  Energy is estimated from CPU time by default; macOS's own Energy Impact
  costs ~1s of CPU per sample, which would make this tool a battery drain.

  Colour follows NO_COLOR and the terminal's capabilities. The dashboard is
  laid out for 80x24 and adapts down to 50x10, dropping detail rather than
  overflowing; below 50x10 it says so instead of drawing a broken frame.
`;

/** One-shot, pipe-friendly output. Used when stdout is not a TTY. See I-22. */
/* Rows of text, before piping. Enough to answer "what is busy right now" at a
   glance; `head` trims it further and --json ignores it entirely. */
const TEXT_ROWS = 10;

/* Long enough that ps's centisecond CPU column still quantises finely, short
   enough that the summary is not noticeably slow. Mirrors PRIMING_DELAY in the
   Rust build. */
const PRIMING_DELAY_MS = 300;
const MS_PER_SEC = 1000;

/** One sample of everything, primed so CPU% is a real delta. */
async function sampleAll(provider: MetricsProvider) {
  // Two samples are required: CPU% is always a delta, never a lifetime average.
  await provider.processes();
  await new Promise((r) => setTimeout(r, PRIMING_DELAY_MS));
  const [cpu, mem, disk, batt, procs] = await Promise.all([
    provider.cpu(),
    provider.memory(),
    provider.disk(),
    provider.battery(),
    provider.processes(),
  ]);
  return { cpu, mem, disk, batt, procs };
}

type Sample = Awaited<ReturnType<typeof sampleAll>>;

/** The `--json` document. This shape is a public interface — see INVARIANTS. */
function jsonDocument({ cpu, mem, disk, batt, procs }: Sample, top: ProcessSample[]): string {
  return (
    JSON.stringify(
      {
        /* Named so a consumer can tell which shape it is reading. */
        version: VERSION,
        cpu: { system: cpu.system, perCore: cpu.perCore, loadAvg: cpu.loadAvg },
        memory: mem,
        disk,
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
    ) + '\n'
  );
}

function textReport({ cpu, mem, disk, batt }: Sample, top: ProcessSample[]): string {
  const lines = [
    `cpu ${cpu.system.toFixed(1)}%  mem ${((mem.usedBytes / mem.totalBytes) * 100).toFixed(1)}%` +
      `  disk ${((disk.usedBytes / disk.totalBytes) * 100).toFixed(0)}%` +
      `  battery ${batt.percent}%${batt.charging ? ' charging' : ''}`,
    '',
    'PID     CPU%     MEM  NAME',
  ];
  for (const p of top) {
    lines.push(
      `${String(p.pid).padEnd(7)} ${percent(p.cpuPercent).padStart(5)}  ${bytes(p.rssBytes).padStart(6)}  ${processName(p.command)}`,
    );
  }
  return lines.join('\n') + '\n';
}

async function oneShot(provider: MetricsProvider, options: Options): Promise<number> {
  const sample = await sampleAll(provider);
  /* JSON gets the whole working set: a consumer that wants ten rows sorted by
     memory has jq, and guessing on its behalf is what --top and --sort were. */
  const ranked = sortProcesses(sample.procs.visible, 'cpu');
  const top = options.json ? ranked : ranked.slice(0, TEXT_ROWS);
  process.stdout.write(options.json ? jsonDocument(sample, top) : textReport(sample, top));
  return 0;
}

/** The provider this platform can offer, or an explanation and an exit. */
function chooseProvider(options: Options): MetricsProvider {
  if (options.mock) return new MockProvider();
  if (process.platform === 'darwin') {
    return new DarwinProvider({ accurateEnergy: options.accurateEnergy });
  }
  // I-24: name the cause and the remedy rather than failing obscurely.
  process.stderr.write(
    `useful-system-monitor: only macOS is supported today (this is ${process.platform}).\n` +
      '        Run `useful-system-monitor --mock` to see the interface with scripted data.\n',
  );
  process.exit(1);
}

/** `--interval` moves the three fast tiers together. */
function tiersFor(options: Options): Tiers {
  if (!options.interval) return DEFAULT_TIERS;
  const ms = options.interval * MS_PER_SEC;
  return {
    ...DEFAULT_TIERS,
    // The CPU tier drives the render rate, which dominates cost, so --interval
    // has to move it too or the flag cannot buy responsiveness.
    cpu: ms,
    processes: ms,
    memory: ms,
  };
}

/** The two options that print and exit rather than sampling anything. */
function printAndExit(options: Options): void {
  if (options.help) {
    process.stdout.write(HELP);
    process.exit(0);
  }
  if (options.version) {
    process.stdout.write(`${VERSION}\n`);
    process.exit(0);
  }
}

/*
 * I-22: no TUI unless we have a real terminal on BOTH ends.
 *
 * stdout alone is not enough. Ink's useInput needs raw mode on stdin, and when
 * stdin is a pipe or /dev/null (`useful-system-monitor < /dev/null`, or the
 * process backgrounded from a script) it throws "Raw mode is not supported" and
 * dies with a React stack trace. Falling back to one-shot output is both more
 * useful and more composable.
 */
function wantsDashboard(options: Options): boolean {
  const stdoutTty = Boolean(process.stdout.isTTY);
  const stdinTty = Boolean(process.stdin.isTTY);
  if (stdoutTty && stdinTty && !options.json) return true;
  if (stdoutTty && !stdinTty && !options.json) {
    // I-24: say why the dashboard did not appear, and how to get it.
    process.stderr.write(
      'useful-system-monitor: stdin is not a terminal, so the interactive dashboard is unavailable.\n' +
        '        Showing a one-shot summary. Run it directly from a shell for the TUI.\n',
    );
  }
  return false;
}

async function main(): Promise<void> {
  const parsed = parseArgs(process.argv.slice(2));
  if (!parsed.ok) {
    // I-24: cause and remedy, to stderr, with a usage exit code of its own so
    // a script can tell a typo apart from a machine it could not read.
    process.stderr.write(
      `useful-system-monitor: ${parsed.error}\n` +
        '        Run `useful-system-monitor --help` for the full list of options.\n',
    );
    process.exit(2);
  }
  const o = parsed.options;

  printAndExit(o);

  const provider = chooseProvider(o);
  if (!wantsDashboard(o)) {
    process.exit(await oneShot(provider, o));
  }

  const tiers = tiersFor(o);

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
