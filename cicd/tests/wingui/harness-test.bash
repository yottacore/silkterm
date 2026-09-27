#!/usr/bin/env bash

#  shellcheck disable=2016  ## 'Expressions don't expand in single quotes.' The PowerShell being matched needs literal '$'.

##	- Purpose:
##		Two things the Windows scenario harness got wrong, checked without a box.
##		It ran whatever binary the box last built, so a result could be for an
##		older commit. And its cleanup stopped every process named silkterm, on
##		boxes other people use.
##	- Test ID: EqH4isr
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

set -euo pipefail
meDir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "${meDir}/../../.." && pwd)"

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }

work="$(mktemp -d "${TMPDIR:-/tmp}/silk-wingui.XXXXXX")"
declare -a started=()
fEnd(){ local p; for p in "${started[@]}"; do kill "${p}" 2>/dev/null || true; done; rm -rf "${work}"; }
trap fEnd EXIT

## The binary. A stand-in win-remote records what it is asked and keeps the
## launcher, and the binary is a file named here, so nothing is built or sent.
cat > "${work}/win-remote" <<'STUB'
#!/usr/bin/env bash
while [[ "${1:-}" == --* ]]; do [[ "$1" == --host ]] && shift; shift; done
echo "$*" >> "${STUB_LOG}"
case "${1:-}" in
	hosts) if [[ -n "${STUB_DOWN:-}" ]]; then echo "box      down  192.0.2.1"; else echo "box      up    192.0.2.1"; fi ;;
	run) cp "$2" "${STUB_DIR}/launcher-$(date +%s%N).ps1"; echo "VERDICT pass" ;;
esac
STUB
chmod +x "${work}/win-remote"
printf 'MZ stand-in\n' > "${work}/silkterm.exe"
out="$(STUB_LOG="${work}/calls" STUB_DIR="${work}" WINRIG_HELD=1 WINGUI_WIN_REMOTE="${work}/win-remote" \
	WINGUI_EXE="${work}/silkterm.exe" "${meDir}/run.bash" --keep smoke 2>&1)" || true
launcher="$(find "${work}" -name 'launcher-*.ps1' | head -1)"
commit="$(git -C "${root}" rev-parse --short HEAD)"
fSent(){ grep -qF "push ${work}/silkterm.exe" "${work}/calls" 2>/dev/null ;}
fRunsSent(){ [[ -n "${launcher}" ]] && grep -qF 'Join-Path $dir "silkterm.exe"' "${launcher}" && ! grep -qF 'target\release' "${launcher}" ;}
fNamesCommit(){ grep -qF "testing ${commit}" <<< "${out}" ;}
fCheck "the binary under test is sent to the box" fSent
fCheck "the scenario runs the binary sent, not the box's own build" fRunsSent
fCheck "the result names the commit tested" fNamesCommit

## A box that is off is a skip: the run passes, and nothing is sent or run.
rc=0
out="$(STUB_DOWN=1 STUB_LOG="${work}/calls-down" STUB_DIR="${work}" WINRIG_HELD=1 WINGUI_WIN_REMOTE="${work}/win-remote" \
	WINGUI_EXE="${work}/silkterm.exe" "${meDir}/run.bash" smoke 2>&1)" || rc=$?
fCheck "with no box up, the run passes" test "${rc}" -eq 0
fCheck "and says it skipped" grep -qFx "wingui: no box up, skipped" <<< "${out}"
fCheck "without sending or running anything" bash -c '! grep -qE "^(push|run) " "$1"' _ "${work}/calls-down"

## The cleanup. Two processes named silkterm: one a run started, with a child of
## its own, and one somebody else's. Only the first two may stop.
if command -v pwsh >/dev/null 2>&1; then
	mkdir -p "${work}/ours" "${work}/theirs"
	cp /bin/sh "${work}/ours/silkterm"
	cp /bin/sleep "${work}/theirs/silkterm"
	printf 'sleep 60 &\nwait\n' > "${work}/kid.sh"
	"${work}/theirs/silkterm" 60 & theirs=$!; started+=("${theirs}")
	## fStartSilk and fTrack as they stand in _lib.ps1, so their record is the one tested.
	cat > "${work}/start.ps1" <<PS
\$ast = [System.Management.Automation.Language.Parser]::ParseFile("${meDir}/_lib.ps1", [ref]\$null, [ref]\$null)
foreach (\$name in 'fStartSilk', 'fTrack') {
	\$fn = \$ast.Find({ param(\$n) \$n -is [System.Management.Automation.Language.FunctionDefinitionAst] -and \$n.Name -eq \$name }, \$true)
	Invoke-Expression \$fn.Extent.Text
}
\$script:startedList = "${work}/started.txt"
\$p = fStartSilk "${work}/ours/silkterm" @("${work}/kid.sh") @{}
\$p.Id
PS
	## to a file, since a capture would wait on the started process's stdout
	pwsh -NoProfile -File "${work}/start.ps1" > "${work}/ours.pid"
	ours="$(tr -d '[:space:]' < "${work}/ours.pid")"; started+=("${ours}")
	kid=""
	for _ in $(seq 50); do kid="$(pgrep -P "${ours}" || true)"; [[ -n "${kid}" ]] && break; sleep 0.1; done
	started+=("${kid}")
	pwsh -NoProfile -File "${meDir}/_stop.ps1" -List "${work}/started.txt"
	sleep 0.3
	fGone(){ ! kill -0 "$1" 2>/dev/null ;}
	fCheck "a process the run started is stopped" fGone "${ours}"
	fCheck "and what it started in turn" fGone "${kid:-0}"
	fCheck "a silkterm the run did not start keeps running" kill -0 "${theirs}"
	fNoNameKill(){ ! grep -qF -- '-Name silkterm' "${meDir}/_run.ps1" "${meDir}/run.bash" ;}
	fCheck "neither cleanup stops processes by name" fNoNameKill
else
	echo "  skip the cleanup half: pwsh not found"
fi

((failures == 0)) || { echo "${failures} check(s) failed"; exit 1; }
echo "all passed"

##	Script history:
##		- 20260918: Created.
##		- 20260926: A box that is down is a skip.
