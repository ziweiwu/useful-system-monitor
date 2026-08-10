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

echo "smoke: PASS"
