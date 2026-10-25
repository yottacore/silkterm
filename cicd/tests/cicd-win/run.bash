#!/usr/bin/env bash

##	- Purpose:
##		Pieces of cicd-win.ps1 that need no Windows box, run here: it keeps only
##		the newest run logs, which it once never pruned, and it maps the build
##		box's paths out of release builds and fails on one left behind. The map
##		is a TOML file, so a folder name holding a quote or a backslash has to
##		come through escaped. The stash it takes before a pull says whether it
##		stashed anything, and nothing else.
##	- Test ID: Er2UgYE
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

set -euo pipefail
meDir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use

if ! command -v pwsh >/dev/null 2>&1; then
	echo "  skip cicd-win.ps1 pieces (no pwsh)"
	exit 0
fi

work="$(mktemp -d "${TMPDIR:-/tmp}/silk-cicdwin.XXXXXX")"
trap 'rc=$?; rm -rf "${work}"; fTestDir_End "${rc}"' EXIT

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }

rc=0
out="$(pwsh -NoProfile -NonInteractive -File "${meDir}/_lift.ps1" -Work "${work}" 2>&1)" || rc=$?
grep -v '^[{]' <<<"${out}" || true
fCheck "the lifted pieces all passed" test "${rc}" -eq 0

fMapReads(){ python3 - "$(grep '^[{]' <<<"${out}" | tail -1)" <<'PY'
import json, sys, tomllib
got = json.loads(sys.argv[1])
with open(got["cfg"], "rb") as f:
	flags = tomllib.load(f)["target"]["cfg(all())"]["rustflags"]
sys.exit(0 if flags == got["want"] else 1)
PY
}
fCheck "the path map reads back as the paths it was given" fMapReads

if ((failures)); then echo "${failures} failed"; exit 1; fi
echo "all passed"

##	History:
##		- 20260926 JC: Created.
##		- 20261006 JC: The stash before a pull.
