# SYSMON invariants

The contract this app is built against. Every invariant is numbered and has at
least one test whose name starts with its number, so the mapping is mechanically
checkable:

```
npx vitest run -t "I-16"
```

Phase status: **Phases 0–3 complete** — scaffold, mock UI, live macOS
collectors, and the kill path. All invariants below are enforced and tested.

## Sampling correctness

| # | Invariant | Where |
|---|---|---|
| I-1 | Process CPU% is always a delta over a known wall-clock window, never a lifetime average. First observation yields `null`, rendered `—`, never `0` | `core/deltas.ts` · `test/deltas.test.ts` |
| I-2 | `0 ≤ core CPU% ≤ 100`. Per-process CPU% is in `[0, 100 × ncpu]`, normalised only at render | `core/deltas.ts` · property test |
| I-3 | Cumulative counters are non-decreasing per PID. A decrease means PID reuse → discard the delta rather than emit a negative rate | `core/deltas.ts` · `test/deltas.test.ts` |
| I-4 | Each panel is internally atomic. Panels on different tiers are deliberately *not* globally atomic, so each shows its own sample age | `hooks/useSampler.ts` · `app.tsx` |
| I-4b | The delta map tracks **every** PID, not just the working set, so a process entering the top 50 already has a real CPU% | `core/deltas.ts` |
| I-5 | Memory reconciles: `used + free == total`, via `vm_stat`. `os.freemem()` is rejected — it read 170 MB on a machine with >1 GB free. "Used" excludes reclaimable inactive pages | `providers/darwin/parse.ts` · `test/parse.test.ts` |
| I-6 | System and per-core CPU come from `os.cpus()` only, never from summing process rows: `kernel_task` (PID 0) is invisible to `ps` | `core/deltas.ts` |

## Liveness and cost

| # | Invariant | Where |
|---|---|---|
| I-7 | Sampling never blocks input or render | `hooks/useSampler.ts` |
| I-8 | At most one run per collector is in flight; an overrun skips the next tick rather than queueing it | `hooks/useSampler.ts` |
| I-9 | Collector cost < 1% of one core; **measured 0.31%**. Whole-app cost at the 10s default is **1.31%** (1.00% render + 0.31% collectors) | `test/cost.test.ts` (collectors) · `npm run verify:selfcost` (whole app) |
| I-9b | SYSMON must not appear in its own top-20 energy consumers while idle. **Verified: absent, or ranked ~#26–28 of ~40** | `npm run verify:selfcost` |
| I-9c | Widening the working set with `+` adds no rendered rows (the table is windowed, I-26). Measured per `processes()` call, interleaved A/B: cap 50 **27.3ms**, 150 **25.1ms**, 300 **26.5ms** — free, within noise. `all` is **90.4ms (+0.63% of one core at the 10s tier)** because the static `ps` then refetches every tick; the UI labels that step with its cost | `npm run verify:workingset` |
| I-10 | History lives in fixed-capacity ring buffers, and per-process history only for the working set, evicted on exit | `core/ring.ts` · `hooks/useProcessHistory.ts` · `test/ring.test.ts` · `test/history.test.ts` |
| I-11 | A collector failure degrades only its own panel; it never crashes the app or blanks other panels | `hooks/useSampler.ts` · `ui/Gauge.tsx` |

## Kill safety

| # | Invariant | Where |
|---|---|---|
| I-12 | Never signal PID ≤ 1 | `kill/guards.ts` · `test/guards.test.ts` |
| I-13 | Never signal our own PID or any ancestor (walks the PPID chain, cycle-safe). The parent map covers **every** PID, not just the working set — ancestors are usually idle shells outside the top 50 | `kill/guards.ts` · `test/parse.test.ts` |
| I-14 | A denylist of critical processes is refused outright, with the consequence explained | `kill/guards.ts` |
| I-15 | Every kill is confirmed by name; SIGKILL needs a second, distinct keypress | `ui/KillModal.tsx` · `app.tsx` |
| I-16 | The target is bound to `(pid, startTime)`. If the PID was recycled between selection and confirmation, abort | `kill/guards.ts` · `test/guards.test.ts` |
| I-17 | `ESRCH` is success (already gone). `EPERM` surfaces a remedy and never auto-escalates to sudo | `kill/signal.ts` · `test/signal.test.ts` |

Every refusal path is tested to emit **no signal at all**, not merely to return
an error.

## Rendering and CLI citizenship

| # | Invariant | Where |
|---|---|---|
| I-18 | Render is a pure function of `(Snapshot, UiState)`; no I/O in components | `ui/*` |
| I-19 | Layout never overflows terminal width. Widths use display-width, not `String.length` | `core/width.ts` · `test/width.test.ts` · `test/ui.test.tsx` |
| I-20 | Sort is stable and total — ties broken by PID — so rows do not jitter at equal CPU | `core/scoring.ts` · `test/scoring.test.ts` |
| I-21 | Selection is keyed by **PID, not row index**. A re-sort under the cursor must never move the kill target | `app.tsx` · `test/ui.test.tsx` |
| I-22 | No TUI when stdout is not a TTY; one-shot text or `--json` instead | `cli.tsx` |
| I-23 | Colour degrades by capability and honours `NO_COLOR`. Colour is only ever *redundant* encoding | `ui/theme.ts` |
| I-24 | Data to stdout, errors to stderr, exit 0/non-zero. Errors state cause *and* remedy. An unknown option or an unusable value is an error with its own exit status (2), **never ignored** — ignoring them returned rows in no order (`--sort bogus`) and spun the render loop at ~1000Hz (`--interval -5`) | `cli.tsx` · `core/options.ts` · `test/options.test.ts` |
| I-25 | Every interactive action has a non-interactive equivalent, and the build identifies itself to both a human and a script | `cli.tsx` (`--json`, `--top`, `--sort`, `--version`, and `version` in the JSON) |
| I-26 | The process table is a scrolling window that always contains the selection, and **no mode on any screen ever exceeds the terminal height**. Lists whose length comes from the machine — cores, mounted volumes, top processes — are budgeted and roll up what will not fit | `app.tsx` · `core/rows.ts` · `ui/*` · `test/rows.test.ts` · `test/scroll.test.ts` · `test/ui.test.tsx` |
| I-27 | Every screen is reachable by walking left/right as well as by its number key, the strip on screen names the current one, and the arrows wrap rather than dead-ending | `core/views.ts` · `ui/ViewTabs.tsx` · `test/views.test.ts` · `test/ui.test.tsx` |

## The invariants worth the extra words

**I-16 (PID reuse).** PIDs are recycled. Between selecting a row and confirming
the kill, the process can exit and an unrelated one can inherit its PID. Binding
the target to `(pid, startTime)` and re-checking at signal time is the only thing
standing between a confirmation dialog and killing the wrong program.

**I-21 (selection by PID).** The selected row *is* the kill target. If selection
were an index, a re-sort arriving between keypress and confirmation would slide a
different process under the cursor. Keying on PID makes that impossible.

**I-26 (windowed table).** The table used to render a fixed `slice(0, rows)`
while the cursor was free to move to the end of the list, so holding *down* walked
the selection straight off the bottom of the screen and the cursor vanished —
with `enter` and `k` still acting on a row nobody could see. The window is now
derived during render from the selected index, not stored, so a re-sort, a
filter, a resize or a `+` expansion can never strand the cursor off-screen even
for one frame. The same budget fix keeps the frame inside the terminal: the
column header and the "… N others" roll-up are two lines the table prints around
its rows, and leaving them uncounted pushed the frame two lines past the bottom,
scrolling the app's own header away.

The same rule now covers every mode and every screen, because three more ways to
overrun the terminal were found the same way:

- **The kill confirmation was an overlay, not a mode.** Stacked below a
  full-height table it drew 40 lines into a 24-row terminal, scrolling away the
  header and the cards at the moment the user is being asked to confirm
  something irreversible. It replaces the dashboard now, exactly as the detail
  panel does.
- **Machine-sized lists were unbudgeted.** The CPU screen drew one bar per core
  unconditionally (a 24-core Mac overran an 80x24 terminal by seven lines), the
  disk screen one row per mount, and the core strip one cell per core across a
  fixed 80 columns. Each now takes a row budget and rolls the remainder into a
  count — a list that stops silently is indistinguishable from a machine with
  nothing more to show.
- **The layout and the renderer disagreed about the terminal width.** Ink falls
  back to 80x24 when `stdout.columns` is 0 — a pty with no size set, several CI
  runners — while this app fell back to 100x30, so it laid out 20 columns wider
  than the frame Ink drew and the fourth card came out clipped and ragged. Both
  now fall back to 80x24. Laying out narrower than the renderer wastes space;
  laying out wider corrupts the frame.

**I-27 (five screens, one strip).** Five screens behind number keys documented
only in a footer that truncates at 80 columns is a feature nobody finds. The tab
strip says which screen you are on and that the other four exist; left/right
walk them and wrap, so neither arrow is ever a dead key. The strip and the
keymap read the same `VIEW_ORDER`, so a label can never open a different screen.
