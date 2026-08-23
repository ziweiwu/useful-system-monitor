#!/bin/bash
# Exercise the built CLI the way a user's shell would.
#
# The test suite imports modules; this is the only check that runs `dist/cli.js`
# as a program, so it is where a stale flag, a bad shebang or a version that
# cannot find its own package.json shows up. CI and the release workflow both
# run it, because a release that skips it can publish while CI is red — which
# is exactly what happened when a flag was removed and this file still used it.
set -euo pipefail

CLI="${1:-dist/cli.js}"

fail() {
  echo "smoke: $1" >&2
  exit 1
}

node "$CLI" --help > /dev/null || fail "--help did not exit 0"

# The version is read from package.json at runtime; this is the only place that
# runs against a real build tree, where that resolution can actually break.
want=$(node -p "require('./package.json').version")
got=$(node "$CLI" --version)
[ "$got" = "$want" ] || fail "--version said $got, package.json says $want"

# Not a TTY here, so it must degrade to one-shot output (I-22) rather than
# trying to start the dashboard.
node "$CLI" --json > /tmp/usm-smoke.json
node -e "
  const j = require('/tmp/usm-smoke.json');
  if (!j.processes?.length) throw new Error('no processes in JSON output');
  if (!j.version) throw new Error('no version in JSON output');
  if (!j.cpu || !j.memory || !j.disk || !j.battery) throw new Error('a panel is missing from JSON output');
  console.log('smoke: json ok —', j.processes.length, 'processes, version', j.version);
"

# I-24: an option it does not understand is an error, not a no-op.
if node "$CLI" --nonsense > /dev/null 2>&1; then
  fail "an unknown option should have exited non-zero"
fi

# I-10b: the built binary must actually load React's production build.
#
# NODE_ENV is conventionally unset in a user's shell, and under the development
# build React writes ~40 User Timing entries per commit into a buffer Node never
# caps — which is what killed a three-day-old dashboard at a 4 GB heap.
#
# This asks the only question that matters, of a real run: which React came off
# disk? Nothing else here can. The first attempt at this fix set NODE_ENV in the
# first import of the entry point, which reads correctly and passes any check
# made against the source — and shipped a binary that still loaded
# react.development.js, because tsc hoists its own jsx-runtime import above
# every hand-written one, and because Node evaluates CJS dependencies while
# linking the module graph, before any ESM body in it.
#
# The preload below deliberately imports nothing from react: one that did would
# load it ahead of the launcher and manufacture the failure it reports. It also
# counts only modules that were actually *evaluated* — see the comment inside.
probe=$(mktemp -t usm-probe-XXXXXX).mjs
cat > "$probe" <<'PROBE'
import { createRequire } from 'node:module';
const require = createRequire(import.meta.url);
process.on('exit', () => {
  const dev = [];
  for (const [k, m] of Object.entries(require.cache)) {
    // `m.loaded` is the whole point: Node registers a cache entry for BOTH
    // branches of react's `if (NODE_ENV === 'production')` without evaluating
    // the one it did not take. Keying on the entry's existence reports a
    // development React on a build that never ran a line of it.
    if (!m.loaded) continue;
    if (/node_modules\/(react|react-reconciler|scheduler)\/cjs\/.*\.development\.js$/.test(k)) {
      dev.push(k.split('/node_modules/')[1]);
    }
  }
  process.stderr.write('NODE_ENV=' + String(process.env.NODE_ENV) + '\n');
  for (const k of dev) process.stderr.write('DEV ' + k + '\n');
});
PROBE
probe_out=$(env -u NODE_ENV node --import "file://$probe" "$CLI" --json 2>&1 >/dev/null)
rm -f "$probe"

echo "$probe_out" | grep -q '^NODE_ENV=production$' \
  || fail "the CLI ran with NODE_ENV=$(echo "$probe_out" | sed -n 's/^NODE_ENV=//p'); React would load its development build"

if echo "$probe_out" | grep -q '^DEV '; then
  echo "$probe_out" | grep '^DEV ' >&2
  fail "the CLI loaded React's development build — the User Timing leak of I-10b is back"
fi

# The other half: NODE_ENV=production is only safe if the build emitted the
# production JSX runtime. tsc does; tsx/esbuild emits jsxDEV, whose production
# counterpart exports `jsxDEV: undefined` — pointing one at the other renders a
# blank screen with no error at all.
if grep -rlq 'from "react/jsx-dev-runtime"' "$(dirname "$CLI")"; then
  fail "the build emitted the development JSX runtime; with NODE_ENV=production it renders nothing"
fi
echo "smoke: prod-env ok — production React, production JSX runtime, no dev modules loaded"

echo "smoke: PASS"
