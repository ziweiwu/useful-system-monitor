# Changelog

Notable changes per release. Versions follow [semver](https://semver.org): the
public surface is the command line, the JSON shape, and the keys.

## 0.9.0

### Fixed

- **The refresh key was documented nowhere.** `r` forces a sample without
  waiting for the next tier — up to five minutes on the disk tier — and it is
  the remedy for every "data unavailable" panel. It appeared in no footer
  legend, not in `--help`, and not in the README, while a guard message already
  instructed the user to "Press r to refresh". An accelerator documented
  nowhere is an accelerator nobody has. Now in all three, with the legend
  coming in two lengths so adding it does not truncate `q quit` off the end at
  80 columns. `test/keys-documented.test.ts` checks the keymap against its own
  documentation so the next key cannot go missing the same way.
- **Four dead-end error messages.** "CPU data unavailable.", and the memory,
  battery and disk equivalents, stated a cause and no remedy — unlike every
  other error in the app, which names one. They now say what to press.

- **`truncate` and `padEnd` overran their budget on emoji with a variation
  selector.** They measured a character at a time, which cannot see that U+FE0F
  widens the character *before* it: `⚠` measures 1 alone and the selector 0
  alone, but `⚠️` occupies two cells. So `truncate('⚠️abc', 3)` returned four
  cells and `padEnd` padded one cell too far, overflowing the row it was drawn
  in — and macOS application names really do contain emoji. `displayWidth` had
  the rule right; the bug was that the rule existed twice. Both now consume one
  shared generator. 33 measured cases fixed.
- **The kill key acted on an invisible target.** The footer offered
  "up/dn move  enter info  k kill" on all five screens, but only the overview
  and battery screens show *which* process is selected — so pressing `k` on the
  CPU, memory or disk screen opened a confirmation for a process that screen
  was not displaying. The confirmation names its target, so nothing could be
  killed blind, but a destructive key whose subject is invisible is precisely
  the failure the footer exists to prevent. Row actions are now limited to the
  screens that show a cursor, and the footer names only what the current screen
  offers.
- **The mock's memory figures did not add up.** `usedBytes` swung on its own
  while the four component figures were constants, so the memory screen showed
  a headline of 13.4G above bars summing to 12.3G. The real collector maintains
  the identity (I-5: used is wired + active + compressed); the fixture did not,
  so reviewing that screen meant first ruling out the fixture — exactly the tax
  a mock exists to remove. `used` is now derived from the breakdown.
- **Growing the terminal could make a list disappear.** An optional note below
  the disk and battery lists cost two rows and switched on the moment it became
  affordable — and a two-row section cannot switch on without the list beneath
  it losing a row, because it arrives one row later than the row that paid for
  it. At 70 columns the disk screen showed one volume at 13 rows and none at
  14, leaving a heading, a roll-up and a note *about* the volumes it had just
  stopped showing. Notes now cost one row and buy their blank separator only
  out of slack, so every list grows monotonically with the terminal. As a side
  effect the disk screen now fits all volumes two rows earlier than before.
- **Pasting into the filter ran the characters as commands.** A terminal
  delivers a burst of keys as one chunk and `useInput` handles every key in it
  against the state of the render that created it, so `/` set filter mode and
  the characters after it still saw `false`: pasting "chrome" silently
  re-sorted by energy, and pasting "book" opened the kill confirmation, because
  `k` is the kill key. Filter mode is now read through a ref the handler
  updates synchronously. Deliberately only that mode — for the kill and detail
  modes the staleness fails *safe*, and making them synchronous would let one
  pasted chunk both open a confirmation and answer it.
- **The "… N others" roll-up ignored the active filter.** It is the tail of the
  working-set cap, which has nothing to do with a search, so filtering for
  "chrome" printed "… 774 others" under two matches — implying 774 more Chrome
  processes — and filtering for something absent printed "no matches" with a
  roll-up of 775 in the very next line. Under a filter it now reports the one
  thing that is both true and useful: how many processes were outside the
  searched set, and that `+` widens it.
- **`theme.dim` failed WCAG 1.4.3.** Measured 3.39:1 on black and 2.76:1 on a
  Tokyo-Night background against a 4.5:1 requirement, while carrying 64 pieces
  of real text. Worse, the PID and USER cells were the only ones that did not
  brighten on the selected row, leaving the PID at 1.97:1 on the selection
  background — on the row the user is about to press `k` against. The palette
  is corrected to 5.60:1 / 4.56:1, borders from 2.49:1 to 3.71:1 for 1.4.11's
  3:1, and both cells are selection-aware. `test/contrast.test.ts` now measures
  this rather than trusting it.
- `scripts/screenshot.tsx` set `FORCE_COLOR` in the file, where it has no
  effect — Ink and chalk resolve colour support at import time, and ESM
  evaluates imports before a module's own statements. Every frame it printed
  was silently colourless, which is a poor property for the tool used to review
  colour. Moved to the npm script (measured: 0 escape sequences before, 20
  after).

### Added

- `test/selection.test.tsx` pins I-21 end to end: the cursor stays on the same
  PID across four re-sorts and across widening and narrowing the working set,
  exactly one cursor is ever drawn through sort/filter/detail sequences, and a
  filter matching nothing shows "no matches" with no roll-up beneath it. The
  selected row *is* the kill target, so a re-sort sliding a different process
  under the cursor is the failure this rules out.
- `test/storms.test.tsx` pins three properties that had no coverage: a burst of
  forty resizes with no settle between them still lands on a coherent frame; 60
  presses of the refresh key produce a handful of collector runs rather than 60
  (I-8 — overruns skip, they do not queue; measured 6); and `parseTopPower`
  survives empty, header-only, truncated, negative, absurd and comma-decimal
  `top` output while still reading the *last* sample block, since `top -l 2`
  prints an all-zero block first.
- Control characters are replaced with a visible `·` where external text enters
  (`parsePsStatic`, `commandLine`) and again in `processName`, the funnel every
  rendered name passes through. Defence in depth rather than a live fix: a
  process names itself, and `exec -a $'malware\rSafari'` would otherwise draw as
  `Safari` — `displayWidth` counts a control byte as zero cells while the
  terminal still acts on it, so a carriage return returns the cursor to column
  zero and the rest of the row overwrites its own start, hiding the real name
  and the PID beside it. macOS `ps` escapes every control byte today (verified
  with `od -c`: zero raw 0x0D bytes), so this is not reachable through the
  current collector — but that escaping is an undocumented implementation
  detail, and the `MetricsProvider` interface is implementable by anything.
- `npm run verify:layout` sweeps every screen and mode across a grid of terminal
  sizes and fails on any frame taller or wider than its terminal, and
  `npm run qa:fuzz` drives random keys at random sizes from a seed. Both were
  throwaway probes during the 0.8.x bug hunt; the fuzzer is what found the
  detail-panel-plus-kill-confirmation stacking, which the size sweep could not
  see, because it sends keys in bursts that land in one chunk. Keeping them
  means the next regression is caught by a command rather than by rewriting the
  harness. Each prints a seed to reproduce with.

## 0.8.1

### Fixed

- The v0.8.0 release never published: a test asserted that the host had swap
  configured, which is true of a laptop and false of the CI runner — an
  environment fact dressed up as a property of the code. It now asserts what
  the fix actually guarantees (a `.` decimal separator under a comma locale)
  and that the parse agrees with whatever number is printed, so it holds on a
  machine with no swap at all. Everything below shipped in this release.

## 0.8.0

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
