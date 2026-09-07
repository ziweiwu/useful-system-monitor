# Post-cutover

Changes deliberately **not** made during the Rust port, because the first Rust
release is a parity port and the differential harness is the schedule's brake:
anything that widens the diff against the shipping Node build for reasons
unrelated to the port makes parity harder to prove and takes longer to land.

Each item says what to change and how it will be verified. Apply them to **both**
implementations at once while both still ship, or to Rust alone once Node is
retired.

## Width: combining marks outside U+0300–U+036F are charged a cell

`src/core/width.ts` (and its Rust port) treat only the Latin combining
diacriticals as zero-width. Hebrew points, Arabic marks and Indic matras are
counted as one cell each while a terminal draws them as none, so a process name
or path in those scripts measures wider than it renders and its column pads
short and drifts left.

Measured against `unicode-width` 0.2.0: **10,530 non-control code points** differ,
and this is the bulk of them. It never overflows a row, which is why
`verify:layout` never caught it — the sweep only fails on overflow.

*Verify:* update `EXPECTED_TABLE_DIVERGENCE` in
`crates/sysmon-core/tests/width_vs_unicode_width.rs`, regenerate the golden
corpus (`npm run gen:width-corpus`), and add a case to `test/width.test.ts`
with a Hebrew or Devanagari name.

## Overview: growing the terminal can shrink the table and hide a section

Found by the Rust port's layout sweep, which asserts `rows_used() <= rows` over
every height rather than the 14x9 grid `verify:layout` samples.

`app.tsx` runs two independent affordability checks in sequence — cards, then the
core strip — and calls it a priority order. It is not one, and the result is not
monotonic in terminal height:

```
rows   cards  cores  table
  19   false  true       8
  20   true   false      3   <- the core strip VANISHES as the terminal grows
  21   true   false      4
  22   true   true       3   <- and the table shrinks again when it returns
```

So dragging the window one row taller can remove a whole section and cut the
process table from eight rows to three. Nothing overflows, which is exactly why
the sweep never caught it: `verify:layout` only fails on overflow.

This is the same shape as the rule `DiskView` and `BatteryView` each carry a
comment about having fixed — "a note costs a row but its separator waits for
slack, or the list loses a row when the terminal grows". The overview never got
the same treatment.

*Fix:* make the two sections one priority-ordered allocation with hysteresis, so
a section never disappears as space increases and the table is monotonic in
height. The Rust `layout::overview` module is already shaped for this — it is a
pure function returning a plan, so the fixed version can be property-tested for
monotonicity directly.

*Verify:* `i26_the_overview_section_toggling_is_pinned_as_it_ships` in
`crates/sysmon-core/tests/layout.rs` records the current behaviour; replace it
with a monotonicity property, and add the matching case to `test/ui.test.tsx`.

## Panels: keep the last-good value alongside the error

Today a collector failure *replaces* the panel data, so the panel blanks. The
Rust `Panel<T>` is designed to hold `value` and `error` together, which is
strictly better UX — show the last-good number, aged, with an error annotation.
Held back only because it changes rendering and so widens the differential diff.

*Verify:* `test/degrade.test.tsx`'s Rust equivalent, plus a golden frame.

## Native collectors instead of seven shell-outs

`libproc` for the process list and static columns, `mach`/`host_statistics64`
for memory, `statvfs` for disks, IOKit for battery. This is where the remaining
0.31% of one core lives, and the reason to build the harness properly.

Two things do **not** have a native path: `top -stats power` (Energy Impact is a
private Apple algorithm) and the `ioreg AppleSmartBattery` detail fields, which
need raw IOKit FFI rather than a crate.

The big win is `proc_pidinfo(PROC_PIDTBSDINFO)`: it returns a raw `time_t` start
time and a uid, which removes `lstart` string parsing and with it the entire
`LC_TIME` failure mode (I-28) — the one that once silently disabled the kill
path machine-wide.

*Verify:* the same differential harness with `SYSMON_SOURCE=shell|native`, which
is exactly what it was built for.
