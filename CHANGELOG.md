# Changelog

Notable changes per release. Versions follow [semver](https://semver.org): the
public surface is the command line, the JSON shape, and the keys.

## Unreleased

### Fixed

- **The PID-reuse guard (I-16) silently did nothing in the case it exists for.**
  The kill path compared the target's start time against the *last process
  sample*, which is two problems. It looked the target up in the top-50 working
  set, and a recycled PID belongs to a brand-new process — no CPU history, a
  small RSS — so it is almost never in that set; when the lookup missed, the
  parameter was `undefined`, which meant "not checked" and the comparison was
  skipped entirely rather than failed. And even on a hit, the sample can be a
  whole refresh interval old, so a PID recycled inside that window still
  carried the previous process's start time. The identity is now read at signal
  time (`ps -o lstart= -p PID`, ~15 ms), and "could not verify" is a distinct,
  refusing state rather than a synonym for "fine".
- **The ancestor guard (I-13) failed open whenever the process sample was
  missing.** Its parent map comes from that sample, and the map is empty when
  the collector errors — a `ps` timeout under load is enough — while the
  confirmation panel stays open across it. An empty map read as "no ancestors",
  so the rule that stops you killing the shell you are sitting in stopped
  applying. It is now read as "no answer", and refused.
- **`--interval` above 24.8 days spun the render loop at ~1000 Hz.**
  `setInterval` takes a signed 32-bit millisecond count, so a larger delay is
  silently clamped to 1 ms: `--interval 3000000` measured 265 fires in 300 ms.
  That is the same failure the lower bound was added to prevent, reached from
  the other end. The accepted range is now 1 to 2147483 seconds.
- **A non-finite sample silently collapsed bars and sparklines.** `barCells`
  and `sparkline` are pure width arithmetic and NaN passes straight through
  them — `'█'.repeat(NaN)` is `''` — so one bad value drew a 10-cell bar as 0
  cells and a 5-cell sparkline as 4 characters, breaking the layout with no
  error anywhere. Both clamp non-finite input now, and the two ratios that
  could produce it (the overview's disk gauge, and the values pushed into the
  history rings) guard their denominators, so a NaN cannot lodge in a ring and
  poison the scale of every sparkline drawn from it.
- **The detail panel and the kill confirmation could be on screen at once.**
  They are described as mutually exclusive modes but were two independent
  conditionals, and `useInput` reads its state from the render that created it —
  so two keys arriving in one chunk (enter then k, which is how you would
  naturally kill something you have just inspected) were both handled against
  the same stale closure and opened both. The frame then stacked a detail
  panel, a confirmation dialog and a toast — 28 lines into a 19-row terminal —
  with the footer offering "esc back" over a confirmation for an irreversible
  action. Found by fuzzing keypresses; input already resolved kill-first, and
  now the render and the footer do too.
- **A failing `host()` took the whole app down.** It is the one collector not
  driven by `poll()`, so nothing caught its rejection, and an unhandled
  rejection terminates the process — the exact failure I-11 exists to rule out.
  `commandLine()` had the same gap. Both degrade to their placeholder now
  ("detecting hardware…", "loading…") instead of crashing.
- **Per-process history was keyed by PID alone**, so a PID recycled while still
  in the working set inherited the dead process's rings and the detail panel
  drew another program's CPU and memory history under this one's name. The
  rings are keyed by `(pid, startTime)` now, like every other identity here.
- **Process names showed as `pid 1234` on any Mac not set to English.**
  Collectors inherited the user's locale, and `ps -o lstart` formats the start
  time through `strftime`: a German Mac printed `Mi. 12 Aug. 19:39:58 2026`, a
  Chinese one `三  8月/12 19:39:58 2026`, a British one `Wed 12 Aug ...` — none
  of which matched the C-locale form the parser reads, so no row got a name, a
  user or a start time. Because a process this tool cannot name is treated as
  protected, **the kill path was silently disabled for the whole machine**.
  Collectors now spawn with `LC_TIME` and `LC_NUMERIC` pinned to `C` and
  `LC_ALL` removed — it outranks the categories, so overriding them while it
  survived would have done nothing. `LC_CTYPE` is left alone so non-ASCII
  process names are still returned as UTF-8. The parser also recovers a name
  from a localised date it cannot otherwise read. See I-28.
- **Swap read 0 B in every comma-decimal locale**, from the same inheritance:
  `sysctl vm.swapusage` printed `total = 1024,00M` and the parser stopped at
  the comma. It now accepts either separator, on top of the pinned locale.
- **The dashboard corrupted terminals narrower than 73 columns or shorter than
  22 rows.** Several places floored a width instead of fitting one, which is
  how a layout ends up wider than the frame it is drawn into:
  - every process row was one cell wider than its box below 73 columns, so Ink
    wrapped each one and the table cost twice its budgeted height. The USER
    column, then the energy column, are given up now — the name, which no other
    screen can supply, is the last to go.
  - the header's hardware line pushed the clock out and became two lines,
    which put every row budget below it off by one.
  - the four overview cards had a 14-column floor each, so they asked for 59
    columns on a 60-column terminal. Cards that do not fit are dropped.
  - the table's row budget was `max(3, rows - 19)`; a floor is not a fit. The
    cards and then the core strip are dropped instead, so the process list —
    the only thing that answers "what is using up my Mac" — survives longest.
  - the memory, disk and battery screens drew fixed sections whose height they
    had counted but never checked; the kill confirmation was a hardcoded
    64-column box 14 rows tall. Both now size to the terminal, and the kill
    panel keeps the process's name and the cancel key at every size.
  Below 50x10 the app says the terminal is too small rather than drawing a
  broken frame. See I-19, I-26.
- **A short terminal plus a kill toast could spin the render loop.** "The
  visible window contains the selection" (I-26) has no solution for a window of
  zero rows, and the offset is a fixpoint written back into state, so it
  alternated between two values on every render — React's "Maximum update depth
  exceeded", an infinite loop rather than a layout glitch. The table is dropped
  entirely rather than drawn with no rows, and the window used for the scroll
  arithmetic is never zero.
- **The first ten seconds after launch showed no CPU data at all.** Every rate
  here is a delta, and the second sample — the first that can show a number —
  waited out the full refresh interval, so the dashboard opened with every
  CPU% reading `—` and the gauge reading 0.0%. The two delta-based collectors
  now take one extra sample 700 ms after launch. It is one extra `ps` per
  launch, not a faster tier. See I-29.

## 0.7.0

### Changed

- Requires Node 22 or newer. Node 20 reached end of life on 2026-04-30, so
  `engines: ">=20"` was a promise about an unsupported runtime.
- CI now runs on both Node lines that are still supported — 24, the current
  LTS, and 22, in maintenance until April 2027 — so the `engines` range is
  tested rather than asserted. Releases are published from 24.
- `@types/node` tracks 22, the *lowest* supported runtime, so a Node 24-only
  API cannot slip in and typecheck.
- `actions/checkout` and `actions/setup-node` moved to v7; v4 ran on the
  deprecated Node 20 action runtime.

### Fixed

- CI's smoke test still passed `--top`, removed in 0.6.0, so `main` went red the
  moment that release landed. The smoke test now lives in `scripts/smoke.sh`,
  runs from both CI and the release workflow, and checks the things only a real
  build can get wrong: `--version` matching the package it shipped in, `--json`
  degrading correctly off a TTY, and an unknown option exiting non-zero.

## 0.6.0

### Removed

- `--top` and `--sort`. Both only shaped one-shot output, which `head` and `jq`
  already do better; `--json` now hands over the whole working set instead of
  guessing how much of it you wanted. Text output is the busiest 10 by CPU.
- `-v` and `-V`. `--version` is the one spelling.
- The upper bounds on `--interval` and `--top`. Nothing motivated them, and
  `--interval 7200` for a lazy background pane is a fair thing to want. The
  floor of 1s stays: that is the one that stops a negative value reaching
  `setInterval`.

## 0.5.0

### Added

- `--version` / `-v`, and a `version` field in `--json` output, so a bug report
  or a script can say which build produced the numbers.
- `--help` now covers exit statuses, examples, which options apply to the
  dashboard versus one-shot output, `NO_COLOR`, and the 80x24 minimum.

### Fixed

- Unknown options and unusable values are now errors with an exit status of 2,
  rather than being ignored. Previously `--jsonn` silently produced text for a
  script that asked for JSON, `--sort bogus` returned rows in **no order at
  all**, `--top -5` dropped the last five rows, and `--interval -5` reached
  `setInterval`, which clamps to 1 ms and spun the render loop at ~1000 Hz.
- `--top N` above 50 returned at most 50 rows, because the one-shot path asked
  for the default working set regardless of what was requested.

## 0.4.0

### Added

- Left and right arrows move between the five screens, wrapping at both ends,
  and a strip under the header names all five and marks the current one.

### Fixed

- The kill confirmation drew below a full-height table — 40 lines into a 24-row
  terminal — scrolling the header and cards away mid-confirmation. It is now a
  mode that replaces the dashboard.
- Per-core bars, volume rows and top-process lists are budgeted against the
  terminal height and roll up the remainder; a 24-core Mac used to overrun the
  CPU screen by seven lines.
- The detail panel did not budget the warning line a protected process adds, so
  it was two rows too tall at 80x24.
- Terminal size fell back to 100x30 where Ink falls back to 80x24, so on a pty
  that reports no size the layout was 20 columns wider than the rendered frame
  and the fourth card came out clipped.
- The overview status line wrapped at 80 columns into an unbudgeted row.

## 0.3.0

### Added

- A disk screen listing every mounted volume: how full each is, how much is
  left, and which are network shares, with a warning past 90%. The internal
  disk appears once rather than once per APFS volume sharing its pool.
- `disk` is included in `--json` and one-shot text output.

## 0.2.1

### Fixed

- The process detail is a mode rather than an overlay; rendering it below the
  table put 93 lines into a 24-row terminal.

## 0.2.0

### Added

- The process table scrolls, always keeping the selection on screen, and `+`/`-`
  widen the working set beyond the default 50 processes.

## 0.1.x

- First releases: CPU, memory, disk and battery at a glance, per-process
  attribution, and a guarded kill path.
