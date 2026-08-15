import { Box, Text, useApp, useInput } from 'ink';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { age, clockTime, duration } from './core/format.js';
import { sortProcesses, type SortKey } from './core/scoring.js';
import type { ProcessSample } from './core/types.js';
import { stepView, VIEW_KEYS, type View } from './core/views.js';
import { truncate } from './core/width.js';
import { stepCostNote, stepLabel, WORKING_SET_STEPS } from './core/workingSet.js';
import { useProcessHistory } from './hooks/useProcessHistory.js';
import { useSampler } from './hooks/useSampler.js';
import { useTerminalSize } from './hooks/useTerminalSize.js';
import { checkKill, processName, type GuardContext, type LiveIdentity } from './kill/guards.js';
import { sendSignal, type KillFn } from './kill/signal.js';
import type { MetricsProvider, Tiers } from './providers/types.js';
import { BatteryView } from './ui/BatteryView.js';
import { CpuView, MemView } from './ui/DetailViews.js';
import { DiskView } from './ui/DiskView.js';
import { KillModal } from './ui/KillModal.js';
import { Overview } from './ui/Overview.js';
import { ProcessDetail } from './ui/ProcessDetail.js';
import { MIN_TABLE_WIDTH, ProcessTable } from './ui/ProcessTable.js';
import { theme } from './ui/theme.js';
import { ViewTabs } from './ui/ViewTabs.js';

const BRAND = 'useful-system-monitor';
/** "20:30:36" — clockTime is fixed width, so the header can budget around it. */
const CLOCK_WIDTH = 8;

/*
 * The smallest terminal the dashboard will draw into.
 *
 * Width: the narrowest process table that can still fit a name, plus the two
 * columns app.tsx keeps as a margin. Height: the overview's fixed chrome with
 * both the cards and the core strip given up — see OVERVIEW_FIXED — plus a row
 * of headroom, so the minimum is not also the point at which the table is empty.
 */
export const MIN_COLUMNS = MIN_TABLE_WIDTH + 2;
export const MIN_ROWS = 10;

/**
 * Who the PID belongs to *now*, for the PID-reuse guard. See I-16.
 *
 * Prefers a fresh read from the provider over the last sample, because the
 * sample can be a whole tier old — and a PID recycled inside that window still
 * carries the old process's start time in it, which is precisely the case the
 * guard is supposed to catch. The sample is the fallback for providers that
 * cannot read one process on demand; when neither can answer, the result says
 * so and `checkKill` refuses rather than guessing.
 */
async function liveIdentity(
  provider: MetricsProvider,
  sampled: ProcessSample | undefined,
  pid: number,
): Promise<LiveIdentity> {
  if (provider.identity) {
    try {
      const id = await provider.identity(pid);
      return id ? { known: true, startTime: id.startTime } : { known: false, gone: true };
    } catch {
      return { known: false, gone: false };
    }
  }
  return sampled ? { known: true, startTime: sampled.startTime } : { known: false, gone: false };
}

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
  /*
   * Filter mode is mirrored in a ref that the key handler reads and writes
   * synchronously.
   *
   * `useInput` closes over the state of the render that created it, and a
   * terminal delivers a burst of keys — a paste, or a fast typist — as one
   * chunk, so every key in that chunk is handled against the same closure.
   * `/` would set filterMode, and the characters after it in the same chunk
   * would still see `false` and fall through to the main keymap: pasting
   * "chrome" silently re-sorted by energy instead of filtering, and pasting
   * "book" opened the kill confirmation, because `k` is the kill key.
   *
   * Deliberately only this mode. The kill and detail modes are left reading
   * possibly-stale state, because there the staleness fails *safe* — keys fall
   * through to the main keymap and do something harmless — whereas making them
   * synchronous would let a single pasted chunk both open a confirmation and
   * answer it. I-15 wants a second, distinct keypress, and one chunk is not
   * two keypresses.
   */
  const filterModeRef = useRef(false);
  const enterFilterMode = useCallback((on: boolean) => {
    filterModeRef.current = on;
    setFilterMode(on);
  }, []);
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

  /* The two modes that take over the screen: the detail panel and the kill
     confirmation. Both replace the dashboard rather than stacking under it. */
  const dashboardHidden = detailProc !== null || killTarget !== null;

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
    /* I-11 again: a provider that rejects here must cost the detail panel its
       argv line, not take the app down. The panel renders null as "loading…". */
    void provider
      .commandLine?.(detailPid)
      .then((c) => {
        if (!cancelled) setCommandLine(c);
      })
      .catch(() => {
        if (!cancelled) setCommandLine(null);
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

  /* One signal at a time: `doKill` awaits a fresh identity read, and without
     this a second keypress during that await would send a second signal. */
  const killing = useRef(false);

  const doKill = useCallback(
    async (target: ProcessSample, signal: 'SIGTERM' | 'SIGKILL') => {
      if (killing.current) return;
      killing.current = true;
      /* try/finally, not a bare reset: if anything in here threw, the flag
         would stay set and the kill key would stop working for the rest of the
         session — a silent, permanent failure of the one destructive action. */
      let outcome;
      try {
        // I-16: the identity observed right now, not the one in the last sample.
        const live = await liveIdentity(
          provider,
          procData?.visible.find((p) => p.pid === target.pid),
          target.pid,
        );
        outcome = sendSignal(target, signal, guardCtx, { live, kill: killFn });
      } finally {
        killing.current = false;
      }
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
    [guardCtx, killFn, onKilled, procData, provider, refresh],
  );

  useInput((input, key) => {
    if (filterModeRef.current) {
      if (key.escape) {
        enterFilterMode(false);
        setFilter('');
      } else if (key.return) {
        enterFilterMode(false);
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
          void doKill(killTarget, 'SIGTERM');
        } else if (input === 'k') {
          // I-15: SIGKILL needs a second, distinct press.
          if (armedKill) void doKill(killTarget, 'SIGKILL');
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
    //
    // Left/right walk the tab strip and wrap at both ends (I-27). They are free
    // to take: up/down own the row cursor, and nothing on any screen is
    // horizontally scrollable.
    if (key.leftArrow) setView((v) => stepView(v, -1));
    else if (key.rightArrow) setView((v) => stepView(v, 1));
    else if (key.upArrow) move(-1);
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
    else if (input === '/') enterFilterMode(true);
    else if (key.return) {
      if (selectedPid !== null) {
        setKillTarget(null);
        setDetailPid(selectedPid);
      }
    } else if (input === 'k') {
      const target = filtered.find((p) => p.pid === selectedPid);
      if (target) {
        setDetailPid(null);
        setKillTarget(target);
        setArmedKill(false);
      }
    }
  });

  /*
   * The overview's row budget, spent in priority order.
   *
   * OVERVIEW_FIXED is everything that is on screen no matter how short the
   * terminal is: the app header, the tab strip, the status line, the table's
   * own column header and "… N others" roll-up, the footer, and their margins.
   * The cards and the core strip are not in it, because at some height they
   * stop being affordable and the process table — the only part that answers
   * "what is using up my Mac" — has to win.
   *
   * This used to be one constant, 19, with `Math.max(3, rows - 19)` under it.
   * A floor is not a fit: on a terminal shorter than 22 rows the table drew
   * three rows into a budget of one and the frame scrolled its own header
   * away. Dropping a section is the only way to actually get shorter. See I-26.
   */
  const OVERVIEW_FIXED = 7;
  /** The table's own column header and its "… N others" roll-up. */
  const TABLE_CHROME = 2;
  const CARDS_ROWS = 8;
  /** The core strip plus the blank line above it. */
  const CORES_ROWS = 2;
  /** Below this the table costs more in headers than it returns in rows. */
  const MIN_TABLE_ROWS = 3;
  /* Cards go before the core strip: the strip is ten glyphs of the same number
     the CPU card already shows, and the CPU screen draws it per core in full. */
  let spare = rows - OVERVIEW_FIXED - (toast ? 2 : 0);
  const showCards = spare - CARDS_ROWS >= TABLE_CHROME + MIN_TABLE_ROWS;
  if (showCards) spare -= CARDS_ROWS;
  const showCores = spare - CORES_ROWS >= TABLE_CHROME + MIN_TABLE_ROWS;
  if (showCores) spare -= CORES_ROWS;
  /*
   * The table's two chrome lines are charged only when it is drawn, so that a
   * toast — which costs two rows and appears exactly when a kill has just
   * happened on a short terminal — can push the table out entirely instead of
   * leaving it with a header, a roll-up and no rows.
   *
   * `tableRows` is never 0 while the table renders: a window of zero rows
   * cannot contain the selection, which makes `viewOffset` below unsatisfiable.
   */
  const showTable = spare >= TABLE_CHROME + 1;
  const tableRows = showTable ? spare - TABLE_CHROME : 0;
  /*
   * I-19: never wider than the frame Ink actually draws.
   *
   * This was `Math.max(60, columns - 2)`, which on a terminal narrower than 62
   * laid the whole app out wider than the terminal — the exact failure
   * useTerminalSize's own comment warns about. Below MIN_COLUMNS the app says
   * so instead (see `tooSmall`), so nothing here has to cope with a width that
   * cannot hold a row.
   */
  const width = Math.max(1, columns - 2);
  /*
   * Rows a full-screen view may draw into: everything except the header, the
   * tab strip, the blank line above and below the content, and the footer.
   *
   * The four detail screens all render at least one list that grows with the
   * machine (cores, volumes, top processes), so each is handed this budget and
   * rolls up whatever does not fit. Without it a 16-core Mac overflowed the CPU
   * screen by seven lines on an 80x24 terminal. See I-26.
   */
  const viewRows = Math.max(4, rows - 5 - (toast ? 2 : 0));

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
    /*
     * The window is at least one row here even when the table is not drawn.
     *
     * "The window contains the selection" has no solution for a window of zero
     * rows, and this is a fixpoint: the effect below writes the result back to
     * `scrollTop`, which feeds straight back in. At `tableRows === 0` it
     * alternated between `selIndex` and `selIndex + 1` on every render, which
     * React reports as "Maximum update depth exceeded" — an infinite render
     * loop, not a layout glitch. The old `Math.max(3, …)` row floor hid this.
     */
    const windowRows = Math.max(1, tableRows);
    const max = Math.max(0, filtered.length - windowRows);
    const cur = Math.min(Math.max(0, scrollTop), max);
    if (selIndex < 0) return cur;
    if (selIndex < cur) return selIndex;
    if (selIndex >= cur + windowRows) return Math.max(0, selIndex - windowRows + 1);
    return cur;
  }, [scrollTop, selIndex, filtered.length, tableRows]);

  useEffect(() => {
    if (viewOffset !== scrollTop) setScrollTop(viewOffset);
  }, [viewOffset, scrollTop]);

  const totalEnergy =
    (procData?.visible.reduce((s, p) => s + (p.energy ?? 0), 0) ?? 0) +
    (procData?.others.energy ?? 0);
  const batt = snapshot.battery.status === 'ok' ? snapshot.battery.data : null;

  /*
   * I-26: the one size where the honest answer is "not here".
   *
   * Every screen degrades — cards drop, columns drop, lists roll up — but the
   * process table still needs room to name a process, and the overview still
   * needs its header, status line and footer. Below that, a frame can only be
   * drawn by overflowing, which corrupts the terminal rather than merely
   * looking cramped. Saying so in one line is the only non-destructive option,
   * and it tells the user the thing they can act on: the size to grow to.
   */
  const tooSmall = columns < MIN_COLUMNS || rows < MIN_ROWS;
  if (tooSmall) {
    return (
      <Box flexDirection="column" width={Math.max(1, columns)}>
        <Text color={theme.danger} wrap="truncate">
          {truncate(`terminal too small: ${columns}x${rows}`, Math.max(1, columns))}
        </Text>
        <Text color={theme.dim} wrap="truncate">
          {truncate(`needs at least ${MIN_COLUMNS}x${MIN_ROWS}`, Math.max(1, columns))}
        </Text>
      </Box>
    );
  }

  /* host() is async, so the first frame has no hardware info yet. Say that
     plainly rather than show the placeholder "unknown · 1 cores", which is a
     visible falsehood even for one frame. */
  const hardwareLine =
    (snapshot.host.cores > 0 && snapshot.host.model !== 'unknown'
      ? `${snapshot.host.model} · ${snapshot.host.cores} cores · up ${duration(snapshot.host.uptimeSec)}`
      : 'detecting hardware…') + (demo ? '  [MOCK DATA]' : '');

  const ages =
    `cpu ${snapshot.cpu.status === 'ok' ? age(snapshot.cpu.sampledAt, now) : '—'}` +
    ` · proc ${snapshot.processes.status === 'ok' ? age(snapshot.processes.sampledAt, now) : '—'}` +
    ` · batt ${snapshot.battery.status === 'ok' ? age(snapshot.battery.sampledAt, now) : '—'}`;

  return (
    <Box flexDirection="column" width={width}>
      {/* Header */}
      <Box justifyContent="space-between">
        {/*
         * I-19: truncated against a measured budget, not by `wrap` alone.
         *
         * `wrap="truncate"` clips this Text to its own box, and under
         * space-between that box is as wide as the string wants to be — so
         * once the hardware line grew past the terminal it pushed the clock
         * out, Ink wrapped the row, and the header silently became two lines.
         * Every row budget below counts it as one, so the whole frame ran a
         * line past the bottom of the terminal on any narrow window.
         */}
        <Text wrap="truncate">
          <Text bold color={theme.mem}>
            {BRAND}{' '}
          </Text>
          <Text color={theme.dim}>
            {truncate(hardwareLine, Math.max(0, width - BRAND.length - 1 - CLOCK_WIDTH - 1))}
          </Text>
        </Text>
        <Text color={theme.dim}>{clockTime(now)}</Text>
      </Box>

      {/* Hidden in the two modes below, where the number and arrow keys are
          inert and a strip of screen names would only mislead. */}
      {!dashboardHidden && <ViewTabs active={view} width={width} />}

      <Box marginTop={1} flexDirection="column">
        {dashboardHidden ? null : view === 'overview' ? (
          <Overview
            snapshot={snapshot}
            histories={histories}
            width={width}
            showCards={showCards}
            showCores={showCores}
          />
        ) : null}
        {!dashboardHidden && view === 'cpu' && (
          <CpuView snapshot={snapshot} histories={histories} width={width} maxRows={viewRows} />
        )}
        {!dashboardHidden && view === 'memory' && (
          <MemView snapshot={snapshot} histories={histories} width={width} maxRows={viewRows} />
        )}
        {!dashboardHidden && view === 'battery' && (
          <BatteryView
            snapshot={snapshot}
            histories={histories}
            width={width}
            maxRows={viewRows}
            selectedPid={selectedPid}
          />
        )}
        {!dashboardHidden && view === 'disk' && (
          <DiskView
            snapshot={snapshot}
            histories={histories}
            width={width}
            maxRows={viewRows}
            now={now}
          />
        )}
      </Box>

      {view === 'overview' && !dashboardHidden && (
        <Box flexDirection="column" marginTop={1}>
          {/* I-19: this line is the widest variable string on the screen, and
              CHROME_ROWS budgets exactly one row for it. */}
          <Text color={theme.dim} wrap="truncate">
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
          {procData && showTable && (
            <ProcessTable
              processes={filtered}
              others={procData.others}
              selectedPid={selectedPid}
              width={width}
              rows={tableRows}
              offset={viewOffset}
              canExpand={wsStep < WORKING_SET_STEPS.length - 1}
              filterActive={filter.trim().length > 0}
              totalEnergy={totalEnergy}
              totalWatts={batt?.watts ?? null}
              energyAccurate={procData.energyAccurate}
            />
          )}
        </Box>
      )}

      {/*
        * I-16 aside, these are the two modes that take over the screen, and
        * exactly one of them may be drawn.
        *
        * They used to be two independent conditionals, and `useInput` reads
        * `killTarget` and `detailPid` from the render that created it — so two
        * keys arriving in one chunk (enter then k, which is how you would
        * naturally kill something you just inspected) were both handled
        * against the same stale closure and set both. The frame then stacked a
        * detail panel, a kill confirmation and a toast: 28 lines into a 19-row
        * terminal, with the footer offering "esc back" over a confirmation
        * dialog. Input already resolves kill-first, so the render does too.
        */}
      {killTarget === null && detailProc && (
        <Box>
          <ProcessDetail
            p={detailProc}
            history={history.get(detailProc.pid)}
            commandLine={commandLine}
            width={width}
            /* Same budget as a detail screen: both replace the dashboard. */
            maxRows={viewRows}
          />
        </Box>
      )}

      {/* A mode, not an overlay — same reason as the detail panel. Stacked
          below a full-height table it pushed the frame 16 lines past a 24-row
          terminal, scrolling away the header and the cards at the exact moment
          the user is being asked to confirm something irreversible. */}
      {killTarget && (
        <Box marginTop={1}>
          <KillModal
            target={killTarget}
            check={killCheck ?? checkKill(killTarget, guardCtx)}
            armedKill={armedKill}
            totalEnergy={totalEnergy}
            totalWatts={batt?.watts ?? null}
            width={width}
            /* Same budget as a detail screen: both replace the dashboard. */
            maxRows={viewRows}
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
            : killTarget
              ? killCheck?.allowed
                ? 't SIGTERM   k SIGKILL (twice)   esc cancel'
                : 'esc back'
              : detailPid !== null
                ? 'k kill   esc back'
                : 'up/dn move  +/- rows  enter info  k kill  / filter  c m e sort  q quit'}
        </Text>
      </Box>
    </Box>
  );
}
