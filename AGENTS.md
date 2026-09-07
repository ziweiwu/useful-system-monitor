# Working on useful-system-monitor

Instructions for Claude Code and any other agent working in this repository.
Read this before changing anything.

## What this is

A macOS terminal dashboard — CPU, memory, disk and battery, the processes
using the most of each, and a kill path. Ink and React, shipped as
`dist/cli.js`.

It watches a machine while the user works, so its own cost is a feature, not an
afterthought. Collector cost is **0.31% of one core**, whole-app **1.31%** at
the 10s default (I-9). Quote real numbers or say nothing.

## The invariant contract

`INVARIANTS.md` is where the behaviour this app promises is written down: **37
numbered invariants**, each with at least one test whose name starts with its
number, so the mapping is mechanically checkable.

```sh
npx vitest run -t "I-16"     # runs exactly that invariant's guard
```

27 of the 37 are cited by name in `test/`; the rest are carried by a `verify:`
script, because the claim is a measurement rather than an assertion — I-9b
("must not appear in its own top-20 energy consumers") cannot be asserted
against a renderer.

When you add behaviour worth relying on, add a numbered invariant and a test
carrying its number. When you change behaviour, update the invariant in the
same commit.

## Before you say it works

```sh
npm install
npm test
npm run mock                 # work on the interface without touching your system
npm run verify:longrun       # heap must stay flat across a long run (I-10b)
npm run verify:tui           # the built binary really mounts and draws (I-22b)
```

If you did not run them, say so explicitly rather than implying success.

`verify:longrun` forces `FORCE_COLOR=3` on itself: ink caches layout results
keyed on the *decorated* string, so measuring without colour reports about a
quarter of what a real terminal costs.

`verify:tui` starts the built binary with `scripts/tty-shim.mjs`, which makes a
pipe look like a terminal so the dashboard path runs without a pty; `TUI_COLS`
and `TUI_ROWS` set the size it reports, and are read by nothing else.

Both also force ink's **interactive** rendering, which it otherwise switches off
whenever `CI` or `CONTINUOUS_INTEGRATION` is set — and a non-interactive ink
writes only the final frame at unmount. Unforced, `verify:longrun` drew 1 frame
per 3,000 ticks on a runner and `verify:tui`'s child drew nothing at all, while
the heap number stayed normal because the app really was rendering. If you make
a harness that mounts the dashboard, force it there too, and give it a
frames-drawn floor: the heap alone cannot tell a flat app from a quiet one.

## Things that have already bitten

- **`bin` points at compiled output.** `npm link` and the global install run
  `dist/`, so source edits do nothing until `npm run build`.
- **`npm start` and `npm run mock` run React's development build**, so you get
  its warnings; the compiled binary forces the production build, because the
  development one leaks (I-10b: **173.7 KB/render measured**, ~4 GB and a
  fatal mark-compact in ~3 days, against **0.6 KB/render** fixed). Set
  `NODE_ENV` yourself to override either. The leak is React's development
  reconciler narrating every render to the Performance Timeline, which Node
  buffers and never evicts — so `src/cli.ts` sets `NODE_ENV` and only then
  reaches the app through a *dynamic* import. It is deliberately `.ts` and
  imports nothing at all: a static import there undoes the fix, and in a `.tsx`
  file JSX counts, because tsc hoists its own `react/jsx-runtime` import above
  every hand-written one. See `core/prod-env.ts`.
- **A build that loads is not a build that mounts.** One drew six bytes, exited
  0, and passed every check — the suite renders `App` under a test renderer and
  `verify:smoke` runs `--json`, which returns before `render()`. A heap check
  called it flat, because an app that never redraws never allocates. That is
  what `verify:tui` exists for (I-22b).
- **No row may overflow its box.** ink's own `build/wrap-text.js` (in
  `node_modules`, not this repo) caches every string it
  has had to wrap and never evicts, so a line keyed on a value that drifts all
  day grows the heap forever. Fit with `fitter`, not `wrap="truncate"`.
- **Collectors spawn in a locale they can parse** — `LC_TIME` and `LC_NUMERIC`
  pinned to `C`, `LC_ALL` removed, `LC_CTYPE` left alone so non-ASCII process
  names survive (I-28). `ps -o lstart` and `sysctl vm.swapusage` format through
  them.
- **The suite runs at a 30s timeout, not vitest's 5s.** At load average 125 and
  again at 343 the default failed a *different* handful of tests each run, all
  of them passing alone. See the comment in `vitest.config.ts` before lowering
  it.

If you touch anything that reads from the system, add a sample of the real
command output to `test/fixtures/`.

## The Rust port

**This is what `bin` points at.** As of 0.10.0 the published command is the
compiled `sysmon` binary, delivered through four per-platform npm packages that
the main package declares as `optionalDependencies` and `scripts/npm-launcher.mjs`
resolves at run time. The TypeScript implementation is still here and still
tested; it is no longer what a user runs. Plan and phasing:
`~/.claude/plans/mellow-meandering-river.md`. Deferred behaviour changes:
`POST-CUTOVER.md`.

Needs a Rust toolchain: `rust-toolchain.toml` pins stable with `rustfmt` and
`clippy`, and the workspace sets `rust-version = "1.85"`.

**The version lives in two files and they must agree.** `package.json` is what
the registry, the tag check and the launcher read; `crates/sysmon/Cargo.toml` is
what `--version` actually prints, because it comes from the binary. `npm run
verify:versions` compares them and the four `optionalDependencies` pins, and CI
runs it — a release that bumped one and not the other would publish a package
whose `--version` disagreed with its own metadata (I-25).

```sh
npm run verify:rust          # fmt + clippy + cargo test  (232 tests)
npm run verify:diff          # Rust --json vs. the shipping Node build
npm run verify:tui:rust      # the built binary really mounts and draws (I-22b)
npm run verify:longrun:rust  # RSS flat across a long run (I-10b)
npm run qa:fuzz:rust         # seeded keyboard fuzzing, ~4,500 steps/s
npm run check:linux          # clippy against x86_64-unknown-linux-gnu
npm run verify:versions      # package.json, the crate and the four pins agree
npm run build:npm-packages   # assemble the per-platform packages from target/
```

CI runs `verify:versions` with the other cheap checks, a `rust` job (fmt,
clippy, test, the pty harness and the fuzzer) on macOS **and** Linux, and a
`cross-build` job that builds all four release binaries on every push — so a
target that stops cross-compiling is caught by the commit that broke it rather
than by the release that needed it. The release workflow builds those four on
their own runners, publishes the platform packages **first**, then the main
one: the main package pins them at an exact version, so the other order leaves
a window where an install resolves a launcher with no binary behind it.

`~/.cargo/bin` is not on `PATH` in a non-interactive shell, which is why the npm
scripts prepend it.

### What works

`sysmon` builds on macOS and Linux and does everything the TypeScript build
does: the five-screen dashboard, the process table, the detail panel, the kill
path, `--json`, the one-shot text path, `--interval`, `--mock`, `--help`,
`--version`. `--energy=accurate` is macOS-only and is **rejected with exit 2**
on Linux rather than silently serving the estimate — one column, one unit
(I-1b, I-24).

Measured against the TypeScript build: **~4.3 MB RSS against ~60 MB**, and
−0.16 KB/render against 173.7 KB/render before the leak fix.

### Layout

A three-crate workspace, and the split is load-bearing rather than tidy.

- **`sysmon-core`** — pure. No ratatui, crossterm, `std::process` or threads.
  That absence is what makes `plan.rows_used() <= rows` a property a unit test
  can sweep in milliseconds instead of a 1,260-frame render.
- **`sysmon`** — the binary. `render::build` is a **pure function of
  `(data, ui, size)`**, so the whole layout sweep, the golden frames and the
  fuzzer need no terminal; the pty harness only answers the one question a pure
  test cannot, which is whether the built binary mounts.
- **`sysmon-harness`** — dev-only: `diff`, `verify_tui`, `verify_longrun`,
  `qa_fuzz`.

### Two rules that are not style

**Nothing statically imported by `crates/sysmon/src/collect/mod.rs`'s platform
seam may assume a platform**, and `#[cfg]` covers both. `cargo check
--target x86_64-unknown-linux-gnu` is the only thing that catches a break, and
`npm run check:linux` runs it.

**`unsafe_code` is `deny`, not `forbid`**, so exactly two modules can opt out
with a comment saying why: `crates/sysmon/src/collect/cpu.rs` (mach
`host_processor_info`, which
has no safe wrapper and is the only way to read per-core ticks without spawning
`top`) and the `statvfs`/`sysconf` calls in `crates/sysmon/src/collect/linux.rs`.
Everything else
is safe code over plain data.

### The technique that has paid for itself

Four **golden corpora**, generated from the TypeScript and replayed in Rust,
because in each case the obvious port was wrong in a way no amount of reasoning
would have caught:

| corpus | cases | what it caught |
|---|---|---|
| `crates/sysmon-core/tests/fixtures/width-corpus.json` | 1,147 | the U+FE0F retroactive-widening rule |
| `crates/sysmon-core/tests/fixtures/lstart-corpus.json` | 413 | V8's `Date.parse` accepts de_DE and en_GB, not just C |
| `crates/sysmon-core/tests/fixtures/format-corpus.json` | 12,403 | `toFixed` rounds half **up**, `{:.1}` half to **even** — 1280 B is `1.3K`, not `1.2K` |
| `crates/sysmon-core/tests/fixtures/options-corpus.json` | 135 | `Number("")` is 0, `Number("0x10")` is 16, and Rust accepts `inf` where JS does not |

Regenerate only when the TypeScript behaviour changes deliberately: a diff in
one of these files is a behaviour change, not a refactor.

### Environment the harnesses read

None of these are read by the app — only by the checks, and only to make a long
run short enough to sit through:

| variable | read by | effect |
|---|---|---|
| `RUN_MS` | `npm run verify:longrun`, `verify:selfcost` | how long the run lasts, in ms |
| `TUI_COLS` / `TUI_ROWS` | `scripts/tty-shim.mjs` | the size the shimmed pty reports |
| `DUMP` | `frame_diff` | print every chrome line, node beside rust |
| `DUMPALL` | `frame_diff` | print every line of one screen; the value is the screen key, `1`–`5` |
| `FORCE_COLOR=3` | `verify:longrun` | required, not optional — ink keys its layout cache on the *decorated* string, so measuring without colour under-reports by ~4x |

### Things that have already bitten, in Rust

- **A pty with no size draws nothing.** `script -q /dev/null` gives the child a
  0x0 winsize, the draw area is empty, and the app looks dead while being
  perfectly healthy. Use the `portable-pty` harness, which sets a real size.
- **A lone ESC followed immediately by another byte is `Alt+<key>`.** The
  harness sends escape with a gap, the way a keyboard does.
- **crossterm delivers discrete key events; Ink coalesced a burst into one
  string.** A pasted `"kk"` matched no Ink binding and did nothing, so the
  second-press rule for SIGKILL was protected *by accident*. In Rust it has to
  be stated: batch identity, a settle time, and a confirm that is gated on the
  confirmation being in the **last frame actually drawn**.
- **`--mock` must not reach `kill(2)`.** The scripted PIDs are small integers
  and some of them are real processes, so mock mode swaps in a killer that
  signals nothing. A mode advertised as safe to try has to be safe to try.

### Measured against the TypeScript build

Interleaved on one machine, medians, via `npm run bench`:

| | node | rust | |
|---|---:|---:|---|
| startup (`--version`) | 185 ms | 3 ms | **60x** |
| one-shot `--json`, wall clock | 614 ms | 465 ms | 1.3x |
| one-shot `--json`, peak RSS | 90.8 MB | 6.9 MB | **13x** |
| dashboard idle, RSS | 85.9 MB | 3.7 MB | **23x** |
| dashboard idle, CPU | 0.58% of a core | 0.03% | **17x** |
| shipped size | 107.8 MB, 5,753 files | 3.6 MB, 1 file | **30x** |
| runtime required | node >= 22 | none | |

The one-shot wall clock understates the gap: both builds sleep 300 ms priming
the CPU delta, which is most of what is left after startup.

### Frame parity (M11), honestly

`npm run verify:frames` drives both builds through their own pty with `--mock`
and compares the **cell grids**. Current state, from 4% chrome and 53% overall
when the comparison was first run:

| screen | chrome lines identical | whole screen |
|---|---|---|
| overview | **5/5** | 84.0% |
| cpu | 2/4 | 79.1% |
| memory | 2/4 | 81.5% |
| battery | 2/4 | 82.3% |
| disk | 2/4 | 75.8% |
| **all** | **13/21 (62%)** | **80.6%** |

The remaining ~20% is **mock data**, and no renderer edit closes it: the two
mocks are different implementations, so the process names and values were never
going to match. Closing it needs the recorded mock trace from the plan — dump N
ticks of the TypeScript mock to a file and have both builds replay it, so the
renderer is the only variable. Until then parity is measured, not claimed.

`frame_diff` earned itself several times over. It caught, in order:

1. **The process table's column order.** The width arithmetic computes the name
   column *last* and the renderer draws it *second*, so the Rust table had PID,
   CPU, MEM, ENERGY, USER, NAME where the original has PID, PROCESS, CPU, MEM,
   POWER, USER. All 232 tests passed; only reading the frames found it.
2. A card interior of `width - 2` where Ink's `paddingX={1}` makes it
   `width - 4`, which shifted every card in the row.
3. Three missing one-row margins — above the cards, above the status line, and
   above every detail screen — each of which offset everything below it by one.
4. A one-space tab separator where the original has two, because the spaces
   belong to the inactive tabs rather than sitting between them.
5. `--mock` not saying `[MOCK DATA]` in the header, which is a real feature: a
   dashboard of invented numbers that looks exactly like one of real numbers is
   a trap, and screenshots outlive the terminal they were taken in.

### npm packaging (M12)

`npm run build:npm-packages` assembles one package per platform — the
esbuild/swc/biome pattern — and `scripts/npm-launcher.mjs` resolves whichever
one npm installed. Both macOS binaries build here; the Linux pair needs a Linux
runner. `test/npm-launcher.test.ts` covers resolution, argument pass-through,
exit-status forwarding and the two distinct failures (unsupported platform vs
`--omit=optional`).

**The `bin` still points at `dist/cli.js`.** Switching it is the cutover, and
the cutover waits on parity.

### Not done

- **M11 to completion**: the recorded mock trace, and the four detail screens.
- **The npm cutover**: flipping `bin`, publishing the platform packages, and
  the CI job that builds the Linux binaries.
- **Linux has no oracle.** The TypeScript build is macOS-only, so the
  differential harness cannot check the Linux collectors at all, and the
  fixtures in `test/fixtures/linux/` are written from the documented `proc(5)`
  formats rather than captured from a running machine. Replace them with real
  captures from more than one distro and kernel before calling Linux finished.

## The Stop hook

`.claude/gates.json` lists the checks a turn may not end without. The hook
itself lives in the `harness` plugin (`hooks/verify-gate.py`) and reads its
list from here — `watch` pathspecs, `gates` as `[name, argv]` pairs, and a
per-gate `timeout`. A repo with no `gates.json` gets nothing, which is what
keeps a Stop hook that runs a test suite from firing everywhere.

The list is `prepublishOnly`'s, unchanged: `lint`, `typecheck`, `test`,
`build`. **Measured 23.6s end to end.**

Two deliberate exclusions:

- **`test/ui.test.tsx` is excluded from the hook's `test` gate** — 90 tests,
  **110s of the suite's 134s** on its own, because each one mounts a real Ink
  app and waits for frames. Including it made the hook a 152s tax on every
  turn, and a slow Stop hook is one that gets deleted. The remaining 38 files
  and 355 tests run in 21s. CI runs the whole suite on both Node lines, so
  nothing is unguarded — it is guarded later.
- **The `verify:*` scripts are excluded entirely** — `longrun`, `tui`,
  `layout`, `selfcost` are tens of seconds to minutes each and need a real
  machine to measure against. CI carries them on a macOS runner.

`watch` entries are git pathspecs, not prefixes — git matches whole path
components, so the manifests are spelled out individually. A lockfile change is
the one most likely to break the build and the easiest to leave off the list.

## Commits

- Explain *why*, not just what. The body is where the reasoning goes.
- **Never add a `Co-Authored-By: Claude` or any AI-attribution trailer.**
- Commit or push only when asked.
- What changed between releases is in `CHANGELOG.md`.
