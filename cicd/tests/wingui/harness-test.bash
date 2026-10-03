#!/usr/bin/env bash

#  shellcheck disable=2016  ## 'Expressions don't expand in single quotes.' The PowerShell being matched needs literal '$'.

##	- Purpose:
##		Four things the Windows scenario harness got wrong, checked without a box.
##		It ran whatever binary the box last built, so a result could be for an
##		older commit. Its cleanup stopped every process named silkterm, on
##		boxes other people use. It kept its files outside the temp folder. And it
##		left run folders behind, here and on the box.
##	- Test ID: EqH4isr
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

set -euo pipefail
meDir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use
root="$(cd "${meDir}/../../.." && pwd)"

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }

work="$(mktemp -d "${TMPDIR:-/tmp}/silk-wingui.XXXXXX")"
declare -a started=()
fEnd(){ local -r rc=$?; local p; for p in "${started[@]}"; do kill "${p}" 2>/dev/null || true; done; rm -rf "${work}"; fTestDir_End "${rc}"; }
trap fEnd EXIT

## The binary. A stand-in win-remote records what it is asked and keeps the
## launcher, and the binary is a file named here, so nothing is built or sent.
cat > "${work}/win-remote" <<'STUB'
#!/usr/bin/env bash
while [[ "${1:-}" == --* ]]; do [[ "$1" == --host ]] && shift; shift; done
echo "$*" >> "${STUB_LOG}"
case "${1:-}" in
	hold) shift; WINRIG_HELD=1 exec "$@" ;;
	push) [[ -z "${STUB_PUSHFAIL:-}" ]] || exit 1 ;;
	hosts) if [[ -n "${STUB_DOWN:-}" ]]; then echo "box      down  192.0.2.1"; else echo "box      up    192.0.2.1"; fi ;;
	run)
		if grep -qE $'^fStage\r?$' "$2"; then
			cp "$2" "${STUB_DIR}/stage.ps1"
			[[ -n "${STUB_NOSTAGE:-}" ]] && { echo "test run folder: refused"; exit 1; }
			echo "RUNFOR wintest"
			printf 'RUNDIR %s\r\n' 'C:\Users\wintest\AppData\Local\Temp\test_silkterm_20260101-00000000'
			printf 'RUNTOKEN %s\r\n' '4242-1234567890'
		elif grep -qE '^if \(fTestDir_Remove -Dir' "$2"; then
			cp "$2" "${STUB_DIR}/sweep-$(date +%s%N).ps1"; echo "${STUB_SWEPT:-REMOVED}"
		else
			cp "$2" "${STUB_DIR}/launcher-$(date +%s%N).ps1"; echo "VERDICT pass"
		fi ;;
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

## Where it all goes: one run folder in the console user's temp folder, made
## on the box, since that user's temp folder is not the ssh account's.
runDir='C:\Users\wintest\AppData\Local\Temp\test_silkterm_20260101-00000000'
fStaged(){ [[ -f "${work}/stage.ps1" ]] && grep -q '^function fTestDir_Make' "${work}/stage.ps1" ;}
fSentThere(){ grep -qFx "push ${work}/silkterm.exe ${runDir}\silkterm.exe" "${work}/calls" ;}
fRunsThere(){ [[ -n "${launcher}" ]] && grep -qFx "\$dir = '${runDir}'" "${launcher}" && grep -qF -- '-RunDir `"$dir`"' "${launcher}" ;}
fScratchThere(){ grep -qE $'^\\$env:TEMP = \\$RunDir\r?$' "${meDir}/_run.ps1" && grep -qE $'^\\$env:TMP = \\$RunDir\r?$' "${meDir}/_run.ps1" ;}
fNoOtherFolder(){ ! grep -qi 'programdata' "${launcher}" "${work}/stage.ps1" "${meDir}"/*.ps1 ;}
fChecksUser(){ grep -qFx "\$runFor = 'wintest'" "${launcher}" ;}
fCheck "the run folder is made on the box by the same rule as every test run" fStaged
fCheck "the binary goes in it" fSentThere
fCheck "the scenario runs there" fRunsThere
fCheck "and points its temp folder there, for its own scratch and the app's" fScratchThere
fCheck "a scenario stops if the console changed hands since the folder was made" fChecksUser
fCheck "nothing the harness sends uses a folder outside it" fNoOtherFolder

## The run folder on the box goes once the run is over, pass or fail, and only
## by the helper's checks against the stage's token. --keep keeps it. The local
## run folder goes too, so the first pass, before the hold, must not make one.
fSweeps(){ find "${1}" -maxdepth 1 -name 'sweep-*.ps1' | wc -l ;}
fCheck "--keep leaves the folder on the box" test "$(fSweeps "${work}")" -eq 0
mkdir "${work}/swept" "${work}/swept-base"
rc=0
out="$(env -u SILKTERM_TEST_DIR TMPDIR="${work}/swept-base" STUB_LOG="${work}/swept/calls" STUB_DIR="${work}/swept" \
	WINGUI_WIN_REMOTE="${work}/win-remote" WINGUI_EXE="${work}/silkterm.exe" "${meDir}/run.bash" smoke 2>&1)" || rc=$?
sweep="$(find "${work}/swept" -name 'sweep-*.ps1' | head -1)"
fSweepsOurs(){ [[ -n "${sweep}" ]] && grep -qFx "if (fTestDir_Remove -Dir '${runDir}' -Token '4242-1234567890') { 'REMOVED' } else { 'NOT REMOVED' }" "${sweep}" ;}
fCheck "a run that passes sweeps the box's run folder with the stage's token" fSweepsOurs
fCheck "through the helper's own checks" grep -q '^function fTestDir_Remove' "${sweep:-/dev/null}"
fCheck "and leaves no run folder in the local temp dir" test "${rc}" -eq 0 -a -z "$(ls -A "${work}/swept-base")"
mkdir "${work}/pushfail"
rc=0
out="$(STUB_PUSHFAIL=1 STUB_LOG="${work}/pushfail/calls" STUB_DIR="${work}/pushfail" WINRIG_HELD=1 \
	WINGUI_WIN_REMOTE="${work}/win-remote" WINGUI_EXE="${work}/silkterm.exe" "${meDir}/run.bash" smoke 2>&1)" || rc=$?
fCheck "a box the binary cannot be sent to fails the run" test "${rc}" -ne 0
fCheck "and still has its run folder swept" test "$(fSweeps "${work}/pushfail")" -eq 1
mkdir "${work}/refused"
out="$(STUB_SWEPT=$'WARNING: test run folder: left it in place: no owner mark\nNOT REMOVED' STUB_LOG="${work}/refused/calls" \
	STUB_DIR="${work}/refused" WINRIG_HELD=1 WINGUI_WIN_REMOTE="${work}/win-remote" WINGUI_EXE="${work}/silkterm.exe" \
	"${meDir}/run.bash" smoke 2>&1)" || true
fCheck "a sweep that removed nothing says so, and why" bash -c 'grep -qFx "wingui: box: run folder not removed:" <<< "$1" && grep -qF "no owner mark" <<< "$1"' _ "${out}"

rc=0
: > "${work}/calls-nostage"
out="$(STUB_NOSTAGE=1 STUB_LOG="${work}/calls-nostage" STUB_DIR="${work}" WINRIG_HELD=1 WINGUI_WIN_REMOTE="${work}/win-remote" \
	WINGUI_EXE="${work}/silkterm.exe" "${meDir}/run.bash" --keep smoke 2>&1)" || rc=$?
fCheck "a box that makes no run folder fails the run" test "${rc}" -ne 0
fCheck "without sending or running a scenario" bash -c '! grep -qE "^push |^run .*launcher" "$1"' _ "${work}/calls-nostage"

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

## A user's temp folder from what the registry holds, unexpanded, against
## their profile and never the ssh account's.
if command -v pwsh >/dev/null 2>&1; then
	cat > "${work}/usertemp.ps1" <<PS
\$ast = [System.Management.Automation.Language.Parser]::ParseFile("${meDir}/_stage.ps1", [ref]\$null, [ref]\$null)
\$fn = \$ast.Find({ param(\$n) \$n -is [System.Management.Automation.Language.FunctionDefinitionAst] -and \$n.Name -eq 'fUserTemp' }, \$true)
Invoke-Expression \$fn.Extent.Text
\$env:USERPROFILE = 'C:\Users\sshacct'
\$env:LOCALAPPDATA = 'C:\Users\sshacct\AppData\Local'
fUserTemp '%USERPROFILE%\AppData\Local\Temp' 'C:\Users\wintest'
fUserTemp '%LocalAppData%\Temp' 'C:\Users\wintest'
fUserTemp '' 'C:\Users\wintest'
fUserTemp 'D:\scratch' 'C:\Users\wintest'
PS
	mapfile -t temps < <(pwsh -NoProfile -File "${work}/usertemp.ps1")
	fCheck "a user's temp folder is under their own profile" test "${temps[0]:-}" = 'C:\Users\wintest\AppData\Local\Temp'
	fCheck "also when written against their local app data" test "${temps[1]:-}" = 'C:\Users\wintest\AppData\Local\Temp'
	fCheck "and the Windows default when the registry has none" test "${temps[2]:-}" = 'C:\Users\wintest\AppData\Local\Temp'
	fCheck "a folder set outright is kept" test "${temps[3]:-}" = 'D:\scratch'
else
	echo "  skip the temp folder half: pwsh not found"
fi

((failures == 0)) || { echo "${failures} check(s) failed"; exit 1; }
echo "all passed"

##	Script history:
##		- 20260918: Created.
##		- 20260926: A box that is down is a skip.
##		- 20261002: The run folder is in the console user's temp folder.
##		- 20261002: Run folders are removed, here and on the box.
