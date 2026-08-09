import { Box, Text, useApp, useInput } from 'ink';
import { useCallback, useEffect, useMemo, useState } from 'react';
import { age, clockTime, duration } from './core/format.js';
import { sortProcesses, type SortKey } from './core/scoring.js';
import type { ProcessSample } from './core/types.js';
import { truncate } from './core/width.js';
import { stepCostNote, stepLabel, WORKING_SET_STEPS } from './core/workingSet.js';
import { useProcessHistory } from './hooks/useProcessHistory.js';
import { useSampler } from './hooks/useSampler.js';
import { useTerminalSize } from './hooks/useTerminalSize.js';
import { checkKill, processName, type GuardContext } from './kill/guards.js';
import { sendSignal, type KillFn } from './kill/signal.js';
import type { MetricsProvider, Tiers } from './providers/types.js';
import { BatteryView } from './ui/BatteryView.js';
import { CpuView, MemView } from './ui/DetailViews.js';
import { KillModal } from './ui/KillModal.js';
import { Overview } from './ui/Overview.js';
import { ProcessDetail } from './ui/ProcessDetail.js';
import { ProcessTable } from './ui/ProcessTable.js';
import { theme } from './ui/theme.js';

type View = 'overview' | 'cpu' | 'memory' | 'battery';

const VIEW_KEYS: Record<string, View> = {
  '1': 'overview',
  '2': 'cpu',
  '3': 'memory',
  '4': 'battery',
};

export interface AppProps {
  provider: MetricsProvider;
  tiers: Tiers;
  /** Injected so the mock can simulate kills and tests can assert on them. */
  killFn?: KillFn;
  onKilled?: (pid: number) => void;
  demo?: boolean;
}

export function App({ provider, tiers, killFn, onKilled, demo }: AppProps) {
  const { exit } = useApp();
  const { columns, rows } = useTerminalSize();
  /* Index into WORKING_SET_STEPS. `+` widens the set, `-` narrows it. */
  const [wsStep, setWsStep] = useState(0);
  const workingSetSize = WORKING_SET_STEPS[wsStep]!;
  const { snapshot, histories, refresh } = useSampler(provider, tiers, workingSetSize);

  const [view, setView] = useState<View>('overview');
  const [sortKey, setSortKey] = useState<SortKey>('cpu');
  const [filter, setFilter] = useState('');
  const [filterMode, setFilterMode] = useState(false);
  /** Selection is keyed by PID, never by row index. See I-21. */
  const [selectedPid, setSelectedPid] = useState<number | null>(null);
  const [killTarget, setKillTarget] = useState<ProcessSample | null>(null);
  const [armedKill, setArmedKill] = useState(false);
  const [toast, setToast] = useState<{ text: string; bad: boolean } | null>(null);
  const [detailPid, setDetailPid] = useState<number | null>(null);
  const [commandLine, setCommandLine] = useState<string | null>(null);
  /** First visible row. Derived state; `viewOffset` below is the truth. */
  const [scrollTop, setScrollTop] = useState(0);

  /*
   * The clock and the sample-age readouts derive from the newest panel sample
   * rather than their own 1s interval.
   *
   * A second timer meant ~1.8 renders/s instead of ~0.5, and since a render
   * costs far more than any collector, the clock tick was one of the most
   * expensive things this app did. Ages still advance, because the CPU panel
   * updates on its own tier.
   */
  const now = useMemo(() => {
    const stamps = [snapshot.cpu, snapshot.memory, snapshot.disk, snapshot.battery, snapshot.processes]
      .map((p) => (p.status === 'ok' || p.status === 'error' ? p.sampledAt : 0));
    const newest = Math.max(0, ...stamps);
    // Before the first sample lands there is nothing to derive from.
    return newest || Date.now();
  }, [snapshot]);

  useEffect(() => {
    if (!toast) return;
    const t = setTimeout(() => setToast(null), 4000);
    return () => clearTimeout(t);
  }, [toast]);

  const procData = snapshot.processes.status === 'ok' ? snapshot.processes.data : null;
  const history = useProcessHistory(procData);

  const filtered = useMemo(() => {
    if (!procData) return [];
    const q = filter.trim().toLowerCase();
    const list = q
      ? procData.visible.filter(
          (p) => processName(p.command).toLowerCase().includes(q) || String(p.pid).includes(q),
        )
      : procData.visible;
    return sortProcesses(list, sortKey);
  }, [procData, filter, sortKey]);

  // I-21: keep the selection pinned to a PID across re-sorts; only fall back to
  // the first row when the selected process is genuinely gone.
  useEffect(() => {
    if (filtered.length === 0) return;
    if (selectedPid === null || !filtered.some((p) => p.pid === selectedPid)) {
      setSelectedPid(filtered[0]!.pid);
    }
  }, [filtered, selectedPid]);

  /*
   * The detail panel is a mode, not an overlay: it replaces the cards and the
   * table rather than stacking below them. Rendering both put 93 lines into a
   * 24-row terminal, which scrolled the header and every card off the top.
   */
  const detailProc =
    detailPid === null ? null : (filtered.find((x) => x.pid === detailPid) ?? null);

  useEffect(() => {
    // The process can exit, or drop out of the working set, while its detail is
    // open. Falling back to the table beats rendering an empty screen.
    if (detailPid !== null && !detailProc) setDetailPid(null);
  }, [detailPid, detailProc]);

  const guardCtx: GuardContext = useMemo(
    // I-13: the full map, so ancestors outside the top 50 are still found.
    () => ({ selfPid: process.pid, parents: procData?.parents ?? new Map() }),
    [procData],
  );

  useEffect(() => {
    if (detailPid === null) {
      setCommandLine(null);
      return;
    }
    let cancelled = false;
    setCommandLine(null);
    void provider.commandLine?.(detailPid).then((c) => {
      if (!cancelled) setCommandLine(c);
    });
    return () => {
      cancelled = true;
    };
  }, [detailPid, provider]);

  // Computed once so the modal, the keymap and the footer cannot disagree about
  // whether this kill is permitted.
  const killCheck = useMemo(
    () => (killTarget ? checkKill(killTarget, guardCtx) : null),
    [killTarget, guardCtx],
  );

  const move = useCallback(
    (delta: number) => {
      if (filtered.length === 0) return;
      const idx = filtered.findIndex((p) => p.pid === selectedPid);
      const next = Math.max(0, Math.min(filtered.length - 1, (idx < 0 ? 0 : idx) + delta));
      setSelectedPid(filtered[next]!.pid);
    },
    [filtered, selectedPid],
  );

  const doKill = useCallback(
    (target: ProcessSample, signal: 'SIGTERM' | 'SIGKILL') => {
      const live = procData?.visible.find((p) => p.pid === target.pid);
      const outcome = sendSignal(target, signal, guardCtx, {
        // I-16: compare against the start time observed right now.
        liveStartTime: live?.startTime,
        kill: killFn,
      });
      setKillTarget(null);
      setArmedKill(false);
      if (outcome.ok) {
        onKilled?.(target.pid);
        setToast({
          text: `${signal} sent to ${processName(target.command)} (${target.pid})${outcome.note ? ` — ${outcome.note}` : ''}`,
          bad: false,
        });
        refresh();
      } else if ('refusal' in outcome) {
        setToast({ text: outcome.refusal.message, bad: true });
      } else {
        setToast({ text: outcome.error, bad: true });
      }
    },
    [guardCtx, killFn, onKilled, procData, refresh],
  );

  useInput((input, key) => {
    if (filterMode) {
      if (key.escape) {
        setFilterMode(false);
        setFilter('');
      } else if (key.return) {
        setFilterMode(false);
      } else if (key.backspace || key.delete) {
        setFilter((f) => f.slice(0, -1));
      } else if (input && !key.ctrl && !key.meta) {
        setFilter((f) => f + input);
      }
      return;
    }

    if (killTarget) {
      if (key.escape) {
        setKillTarget(null);
        setArmedKill(false);
      } else if (killCheck?.allowed) {
        // Signal keys are inert on a refused target; esc is the only way out.
        if (input === 't') {
          doKill(killTarget, 'SIGTERM');
        } else if (input === 'k') {
          // I-15: SIGKILL needs a second, distinct press.
          if (armedKill) doKill(killTarget, 'SIGKILL');
          else setArmedKill(true);
        }
      }
      return;
    }

    if (detailPid !== null) {
      if (key.escape || key.return) {
        setDetailPid(null);
      } else if (input === 'k') {
        const target = filtered.find((p) => p.pid === detailPid);
        if (target) {
          setDetailPid(null);
          setKillTarget(target);
          setArmedKill(false);
        }
      } else if (input === 'q') {
        exit();
      }
      return;
    }

    if (input === 'q' || (key.ctrl && input === 'c')) {
      exit();
      return;
    }
    if (VIEW_KEYS[input]) {
      setView(VIEW_KEYS[input]!);
      return;
    }
    // Arrows move; `k` is reserved for kill. Binding `k` to vim-up as well
    // would make the single most destructive action ambiguous.
    if (key.upArrow) move(-1);
    else if (key.downArrow) move(1);
    else if (key.pageUp) move(-10);
    else if (key.pageDown) move(10);
    else if (input === 'c') setSortKey('cpu');
    else if (input === 'm') setSortKey('mem');
    else if (input === 'e') setSortKey('energy');
    else if (input === 'r') refresh();
    // `=` is the same physical key as `+`, so expanding does not need shift.
    else if (input === '+' || input === '=')
      setWsStep((i) => Math.min(WORKING_SET_STEPS.length - 1, i + 1));
    else if (input === '-' || input === '_') setWsStep((i) => Math.max(0, i - 1));
    else if (input === '/') setFilterMode(true);
    else if (key.return) {
      if (selectedPid !== null) setDetailPid(selectedPid);
    } else if (input === 'k') {
      const target = filtered.find((p) => p.pid === selectedPid);
      if (target) {
        setKillTarget(target);
        setArmedKill(false);
      }
    }
  });

  /*
   * Everything on screen that is not a process row: the app header, the four
   * cards, the core strip, the status line, the footer and their margins — plus
   * the two lines ProcessTable prints around its rows (the column header and
   * the "… N others" roll-up). Those last two used to be uncounted, so the
   * frame ran two lines past the terminal and scrolled its own header away.
   */
  const CHROME_ROWS = 18;
  /* The detail panel's own fixed height: borders, title, five fields, the two
     block labels, three sparklines, the key hints, and their margins — plus the
     app header and footer around it. Whatever is left goes to the blocks. */
  const DETAIL_CHROME_ROWS = 22;
  /* At the documented 80x24 minimum the panel is one line taller than the
     screen even with single-line blocks, so the gap below the header is the
     first thing to go. */
  const detailGap = rows >= 26 ? 1 : 0;
  const tableRows = Math.max(3, rows - CHROME_ROWS - (toast ? 2 : 0));
  const width = Math.max(60, columns - 2);

  /*
   * I-26: the visible window always contains the selection.
   *
   * Derived during render rather than stored, so a re-sort, a filter change, a
   * resize or a `+` expansion can never leave the cursor stranded off-screen
   * for a frame. `scrollTop` only remembers where the window was, so ordinary
   * up/down movement inside the window does not drag the list around.
   */
  const selIndex = filtered.findIndex((p) => p.pid === selectedPid);
  const viewOffset = useMemo(() => {
    const max = Math.max(0, filtered.length - tableRows);
    const cur = Math.min(Math.max(0, scrollTop), max);
    if (selIndex < 0) return cur;
    if (selIndex < cur) return selIndex;
    if (selIndex >= cur + tableRows) return Math.max(0, selIndex - tableRows + 1);
    return cur;
  }, [scrollTop, selIndex, filtered.length, tableRows]);

  useEffect(() => {
    if (viewOffset !== scrollTop) setScrollTop(viewOffset);
  }, [viewOffset, scrollTop]);

  const totalEnergy =
    (procData?.visible.reduce((s, p) => s + (p.energy ?? 0), 0) ?? 0) +
    (procData?.others.energy ?? 0);
  const batt = snapshot.battery.status === 'ok' ? snapshot.battery.data : null;

  const ages =
    `cpu ${snapshot.cpu.status === 'ok' ? age(snapshot.cpu.sampledAt, now) : '—'}` +
    ` · proc ${snapshot.processes.status === 'ok' ? age(snapshot.processes.sampledAt, now) : '—'}` +
    ` · batt ${snapshot.battery.status === 'ok' ? age(snapshot.battery.sampledAt, now) : '—'}`;

  return (
    <Box flexDirection="column" width={width}>
      {/* Header */}
      <Box justifyContent="space-between">
        <Text>
          <Text bold color={theme.mem}>
            useful-system-monitor{' '}
          </Text>
          <Text color={theme.dim}>
            {/* host() is async, so the first frame has no hardware info yet.
                Show that plainly rather than the placeholder "unknown · 1
                cores", which is a visible falsehood even for one frame. */}
            {snapshot.host.cores > 0 && snapshot.host.model !== 'unknown'
              ? `${snapshot.host.model} · ${snapshot.host.cores} cores · up ${duration(snapshot.host.uptimeSec)}`
              : 'detecting hardware…'}
            {demo ? '  [MOCK DATA]' : ''}
          </Text>
        </Text>
        <Text color={theme.dim}>{clockTime(now)}</Text>
      </Box>

      <Box marginTop={1} flexDirection="column">
        {detailProc ? null : view === 'overview' ? (
          <Overview snapshot={snapshot} histories={histories} width={width} />
        ) : null}
        {!detailProc && view === 'cpu' && (
          <CpuView snapshot={snapshot} histories={histories} width={width} />
        )}
        {!detailProc && view === 'memory' && (
          <MemView snapshot={snapshot} histories={histories} width={width} />
        )}
        {!detailProc && view === 'battery' && (
          <BatteryView
            snapshot={snapshot}
            histories={histories}
            width={width}
            selectedPid={selectedPid}
          />
        )}
      </Box>

      {view === 'overview' && !detailProc && (
        <Box flexDirection="column" marginTop={1}>
          <Text color={theme.dim}>
            {procData
              ? filtered.length === 0
                ? 'no matches'
                : `${viewOffset + 1}-${Math.min(viewOffset + tableRows, filtered.length)} of ${filtered.length}` +
                  ` · top ${stepLabel(workingSetSize)} of ${procData.total}${stepCostNote(workingSetSize)}`
              : 'sampling…'}
            {' · sort '}
            <Text color={theme.mem}>{sortKey}</Text>
            {' · filter '}
            {filterMode ? (
              <Text color={theme.cpuMid}>{filter || '…'}▏</Text>
            ) : (
              <Text color={filter ? theme.cpuMid : theme.dim}>{filter || '(none)'}</Text>
            )}
            {'      '}
            {ages}
          </Text>
          {procData && (
            <ProcessTable
              processes={filtered}
              others={procData.others}
              selectedPid={selectedPid}
              width={width}
              rows={tableRows}
              offset={viewOffset}
              canExpand={wsStep < WORKING_SET_STEPS.length - 1}
              totalEnergy={totalEnergy}
              totalWatts={batt?.watts ?? null}
              energyAccurate={procData.energyAccurate}
            />
          )}
        </Box>
      )}

      {detailProc && (
        <Box marginTop={detailGap}>
          <ProcessDetail
            p={detailProc}
            history={history.get(detailProc.pid)}
            commandLine={commandLine}
            width={width}
            /* The panel is fixed-height apart from the two wrapped blocks, so
               those absorb whatever the terminal cannot fit. See I-26. */
            maxLinesPerBlock={Math.max(
              1,
              Math.floor((rows - DETAIL_CHROME_ROWS - detailGap - (detailProc.protected ? 2 : 0)) / 2),
            )}
          />
        </Box>
      )}

      {killTarget && (
        <Box marginTop={1}>
          <KillModal
            target={killTarget}
            check={killCheck ?? checkKill(killTarget, guardCtx)}
            armedKill={armedKill}
            totalEnergy={totalEnergy}
            totalWatts={batt?.watts ?? null}
          />
        </Box>
      )}

      {toast && (
        <Box marginTop={1}>
          <Text color={toast.bad ? theme.danger : theme.cpuLow} wrap="truncate">
            {toast.bad ? '! ' : '✓ '}
            {truncate(toast.text, width - 4)}
          </Text>
        </Box>
      )}

      {/* Footer */}
      <Box marginTop={1}>
        {/* I-19: the key hints are the widest fixed string in the app, so they
            are truncated rather than allowed to wrap at narrow widths. */}
        <Text color={theme.dim} wrap="truncate">
          {filterMode
            ? 'type to filter   enter apply   esc clear'
            : detailPid !== null
              ? 'k kill   esc back'
              : killTarget
              ? killCheck?.allowed
                ? 't SIGTERM   k SIGKILL (twice)   esc cancel'
                : 'esc back'
              : 'up/dn move  +/- rows  enter info  k kill  / filter  c m e sort  1-4 view  q quit'}
        </Text>
      </Box>
    </Box>
  );
}
