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
  development one leaks (I-10b: 223 KB/render, ~4 GB and a fatal mark-compact
  in ~3.5 days, against 3.9 KB/render fixed). Set `NODE_ENV` yourself to
  override either.
- **A build that loads is not a build that mounts.** One drew six bytes, exited
  0, and passed every check — the suite renders `App` under a test renderer and
  `verify:smoke` runs `--json`, which returns before `render()`. A heap check
  called it flat, because an app that never redraws never allocates. That is
  what `verify:tui` exists for (I-22b).
- **No row may overflow its box.** ink's `wrap-text.js` caches every string it
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
