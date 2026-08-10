# Changelog

Notable changes per release. Versions follow [semver](https://semver.org): the
public surface is the command line, the JSON shape, and the keys.

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
