#!/usr/bin/env bash

#  shellcheck disable=2016  ## 'Expressions don't expand in single quotes.' The PowerShell being matched needs literal '$'.

##	- Purpose:
##		Four things the Windows scenario harness got wrong, checked without a box.
##		It ran whatever binary the box last built, so a result could be for an
##		older commit. Its cleanup stopped every process named silkterm, on
##		boxes other people use. It kept its files outside the temp folder. And it
##		left run folders behind, here and on the box, and every script it sent.
##	- Test ID: dct7r
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
fNot(){ ! "${@}"; }

work="$(mktemp -d "${TMPDIR:-/tmp}/silk-wingui.XXXXXX")"
## The list of runs not yet swept, here rather than in this tree's artifacts.
export WINGUI_RUNS="${work}/runs.txt"
declare -a started=()
fEnd(){ local -r rc=$?; local p; for p in "${started[@]}"; do kill "${p}" 2>/dev/null || true; done; rm -rf "${work}"; fTestDir_End "${rc}"; }
trap fEnd EXIT

## The binary. A stand-in win-remote records what it is asked and keeps the
## launcher, and the binary is a file named here, so nothing is built or sent.
cat > "${work}/win-remote" <<'STUB'
#!/usr/bin/env bash
stubHost=box
while [[ "${1:-}" == --* ]]; do [[ "$1" == --host ]] && { stubHost="$2"; shift; }; shift; done
echo "$*" >> "${STUB_LOG}"
case "${1:-}" in
	hold) shift; WINRIG_HELD=1 exec "$@" ;;
	push) [[ -z "${STUB_PUSHFAIL:-}" ]] || exit 1 ;;
	hosts)
		if [[ -n "${STUB_HOSTS:-}" ]]; then printf '%s\n' "${STUB_HOSTS}"
		elif [[ -n "${STUB_DOWN:-}" ]]; then echo "box      down  192.0.2.1"; else echo "box      up    192.0.2.1"; fi ;;
	run)
		if grep -qE $'^fStage\r?$' "$2"; then
			cp "$2" "${STUB_DIR}/stage.ps1"
			[[ -n "${STUB_NOSTAGE:-}" ]] && { echo "test run folder: refused"; exit 1; }
			[[ " ${STUB_NOSESSION:-} " == *" ${stubHost} "* ]] && { printf 'NOSESSION nobody is logged on at the console\r\n'; exit 0; }
			echo "RUNFOR wintest"
			printf 'RUNDIR %s\r\n' 'C:\Users\wintest\AppData\Local\Temp\test_silkterm_20260101-00000000'
			printf 'RUNTOKEN %s\r\n' '4242-1234567890'
		elif grep -qE "^'SWEPT'" "$2"; then
			kind=sweep; [[ "$2" == *_old.ps1 ]] && kind=old
			cp "$2" "${STUB_DIR}/${kind}-$(date +%s%N).ps1"
			[[ -n "${STUB_SILENT:-}" ]] || { echo "${STUB_SWEPT:-REMOVED}"; echo SWEPT; }
		else
			cp "$2" "${STUB_DIR}/launcher-$(date +%s%N).ps1"; echo "VERDICT pass"
			echo "${stubHost} $(sed -n 's/^\$scenario = "\(.*\)"\r\{0,1\}$/\1/p' "$2")" >> "${STUB_DIR}/ran"
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

## What the run sends lands in the box's jobs folder under its own name, so each
## script is named for the run, and the sweep takes them by that name, itself too.
runId="$(sed -n 's/^run .*silkrig_\([0-9]*-[0-9]*\)_stage\.ps1$/\1/p' "${work}/swept/calls" | head -1)"
fNamed(){ grep -qE "^run .*/silkrig_${runId}_stage\.ps1$" "${1}" && grep -qE "^run .*/silkrig_${runId}_smoke\.ps1$" "${1}" && grep -qE "^run .*/silkrig_${runId}_sweep\.ps1$" "${1}" ;}
fCheck "the scripts sent are named for the run" bash -c '[[ -n "$1" ]]' _ "${runId}"
fCheck "the stage, each scenario and the sweep" fNamed "${work}/swept/calls"
fCheck "the sweep takes them from its own folder by that name" grep -qFx "Get-ChildItem -LiteralPath \$PSScriptRoot -File -Filter 'silkrig_${runId}_*.ps1' | Remove-Item -Force -ErrorAction SilentlyContinue" "${sweep:-/dev/null}"
fCheck "and the run, once swept, is off the list" bash -c '! grep -qF "$1" "$2" 2>/dev/null' _ "${runId}" "${WINGUI_RUNS}"

## A run killed before its sweep: the next run on that box sweeps it first, its
## folder through the token it was given and its scripts by name, and takes it
## off the list. Another box's stays.
killedDir='C:\Users\wintest\AppData\Local\Temp\test_silkterm_20251231-00000000'
mkdir "${work}/killed"
printf 'box\t20251231000000-1\t\t\nbox\t20251231000000-1\t%s\t77-88\nother\t20251231000000-2\t\t\n' "${killedDir}" >"${work}/killed/runs.txt"
rc=0
out="$(WINGUI_RUNS="${work}/killed/runs.txt" STUB_LOG="${work}/killed/calls" STUB_DIR="${work}/killed" WINRIG_HELD=1 \
	WINGUI_WIN_REMOTE="${work}/win-remote" WINGUI_EXE="${work}/silkterm.exe" "${meDir}/run.bash" smoke 2>&1)" || rc=$?
old="$(find "${work}/killed" -name 'old-*.ps1' | head -1)"
fCheck "the next run on the box sweeps a killed run's leftovers first" bash -c '[[ "$(grep "^run " "$1" | head -1)" == *_old.ps1 ]]' _ "${work}/killed/calls"
fCheck "its folder only through the token it was given" grep -qFx "if (fTestDir_Remove -Dir '${killedDir}' -Token '77-88') { 'REMOVED' } else { 'NOT REMOVED' }" "${old:-/dev/null}"
fCheck "and its scripts by their name" grep -qF -- "-Filter 'silkrig_20251231000000-1_*.ps1'" "${old:-/dev/null}"
fCheck "not another box's" fNot grep -qF '20251231000000-2' "${old:-/dev/null}"
fCheck "then it is off the list, and the other box's run is not" test "$(cat "${work}/killed/runs.txt")" = $'other\t20251231000000-2\t\t'
fCheck "and the run went on" test "${rc}" -eq 0

## A box that never answers the sweep keeps the run on the list for next time.
mkdir "${work}/silent"
rc=0
out="$(WINGUI_RUNS="${work}/silent/runs.txt" STUB_SILENT=1 STUB_LOG="${work}/silent/calls" STUB_DIR="${work}/silent" WINRIG_HELD=1 \
	WINGUI_WIN_REMOTE="${work}/win-remote" WINGUI_EXE="${work}/silkterm.exe" "${meDir}/run.bash" smoke 2>&1)" || rc=$?
fCheck "a sweep the box never answered leaves the run on the list" grep -qF "${runDir}" "${work}/silent/runs.txt"
## --keep keeps everything, so nothing is written down to sweep later.
mkdir "${work}/kept"
out="$(WINGUI_RUNS="${work}/kept/runs.txt" STUB_LOG="${work}/kept/calls" STUB_DIR="${work}/kept" WINRIG_HELD=1 \
	WINGUI_WIN_REMOTE="${work}/win-remote" WINGUI_EXE="${work}/silkterm.exe" "${meDir}/run.bash" --keep smoke 2>&1)" || true
fCheck "--keep writes nothing down" test ! -e "${work}/kept/runs.txt"

rc=0
: > "${work}/calls-nostage"
out="$(STUB_NOSTAGE=1 STUB_LOG="${work}/calls-nostage" STUB_DIR="${work}" WINRIG_HELD=1 WINGUI_WIN_REMOTE="${work}/win-remote" \
	WINGUI_EXE="${work}/silkterm.exe" "${meDir}/run.bash" --keep smoke 2>&1)" || rc=$?
fCheck "a box that makes no run folder fails the run" test "${rc}" -ne 0
fCheck "without sending or running a scenario" bash -c '! grep -qE "^push |^run .*launcher" "$1"' _ "${work}/calls-nostage"
mkdir "${work}/nostage"
out="$(STUB_NOSTAGE=1 STUB_LOG="${work}/nostage/calls" STUB_DIR="${work}/nostage" WINRIG_HELD=1 WINGUI_WIN_REMOTE="${work}/win-remote" \
	WINGUI_EXE="${work}/silkterm.exe" "${meDir}/run.bash" smoke 2>&1)" || true
nsweep="$(find "${work}/nostage" -name 'sweep-*.ps1' | head -1)"
fCheck "and still has the stage script swept, with no folder to remove" bash -c '[[ -n "$1" ]] && grep -qF -- "-Filter '"'"'silkrig_" "$1" && ! grep -q "^if (fTestDir_Remove" "$1"' _ "${nsweep}"

## A box nobody is logged on at, such as a VM just started, has no desktop for
## a scenario. It was run anyway, as the ssh account, and waited out 4 minutes
## for a task that never started. Now it's a skip that says why.
mkdir "${work}/nosession"
rc=0
out="$(STUB_NOSESSION=box STUB_LOG="${work}/nosession/calls" STUB_DIR="${work}/nosession" WINRIG_HELD=1 \
	WINGUI_WIN_REMOTE="${work}/win-remote" WINGUI_EXE="${work}/silkterm.exe" "${meDir}/run.bash" smoke 2>&1)" || rc=$?
fCheck "a box nobody is logged on at is a skip that says why" grep -qFx "wingui: box: nobody is logged on at the console, skipped" <<< "${out}"
fCheck "which doesn't fail the run" test "${rc}" -eq 0
fCheck "and sends and runs nothing there" bash -c '! grep -qE "^push |^run .*_smoke\.ps1" "$1"' _ "${work}/nosession/calls"
fCheck "but still sweeps the stage script" test "$(fSweeps "${work}/nosession")" -eq 1
fCheck "a scenario stops if nobody is on the console by the time it runs" grep -qF '"VERDICT fail nobody is logged on at the console now"' "${launcher}"

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
	fGone(){ ! kill -0 "${1}" 2>/dev/null ;}
	fCheck "a process the run started is stopped" fGone "${ours}"
	fCheck "and what it started in turn" fGone "${kid:-0}"
	fCheck "a silkterm the run did not start keeps running" kill -0 "${theirs}"
	fNoNameKill(){ ! grep -qF -- '-Name silkterm' "${meDir}/_run.ps1" "${meDir}/run.bash" ;}
	fCheck "neither cleanup stops processes by name" fNoNameKill

	## A listed pid handed on to a process this user cannot read. Its start time
	## comes back empty, and on vm925w the stop threw on it and the verdict was lost.
	cat > "${work}/unreadable.ps1" <<PS
function Get-Process { [CmdletBinding()] param([int] \$Id)
	if (\$PSBoundParameters.ContainsKey('Id')) { [pscustomobject]@{ Id = \$Id; StartTime = \$null; Parent = \$null } } }
\$ErrorActionPreference = "Stop"
Set-Content -Path "${work}/unreadable.txt" -Value "4 638900000000000000"
& "${meDir}/_stop.ps1" -List "${work}/unreadable.txt"
"stopped"
PS
	fUnreadable(){ [[ "$(pwsh -NoProfile -File "${work}/unreadable.ps1" 2>&1)" == stopped ]] ;}
	fCheck "a listed process whose start time cannot be read is passed over" fUnreadable

	## And whatever the cleanup does, the verdict still gets written. The far
	## side reads a missing one as a session that never answered.
	mkdir -p "${work}/stopthrows"
	cp "${meDir}/_run.ps1" "${work}/stopthrows/"
	: > "${work}/stopthrows/_lib.ps1"
	echo 'throw "cleanup broke"' > "${work}/stopthrows/_stop.ps1"
	pwsh -NoProfile -File "${work}/stopthrows/_run.ps1" -Scenario none -Exe none \
		-OutDir "${work}/stopthrows/out" -RunDir "${work}/stopthrows" >/dev/null 2>&1 || true
	fCheck "a cleanup that throws still leaves a verdict" grep -q '^VERDICT ' "${work}/stopthrows/out/result.txt"
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

## Who holds the console: nobody at a logon screen, never the ssh account. A box
## that logs on by itself is waited for, and one that doesn't isn't. The wait is
## long on a session that just started and short on an older one.
if command -v pwsh >/dev/null 2>&1; then
	cat > "${work}/console.ps1" <<PS
\$ast = [System.Management.Automation.Language.Parser]::ParseFile("${meDir}/_stage.ps1", [ref]\$null, [ref]\$null)
foreach (\$name in 'fConsoleUserIn', 'fWaitConsoleUser', 'fConsoleWait') {
	\$fn = \$ast.Find({ param(\$n) \$n -is [System.Management.Automation.Language.FunctionDefinitionAst] -and \$n.Name -eq \$name }, \$true)
	Invoke-Expression \$fn.Extent.Text
}
Set-StrictMode -Version Latest
\$env:USERNAME = 'sshacct'
\$head = ' USERNAME              SESSIONNAME        ID  STATE   IDLE TIME  LOGON TIME'
"in:" + (fConsoleUserIn @(\$head, '>wintest               console             1  Active      none   10/25/2026 9:00 AM') 1)
"other:" + (fConsoleUserIn @(\$head, ' wintest                                   2  Disc        none   10/25/2026 9:00 AM') 1)
"none:" + (fConsoleUserIn @() 1)
\$script:calls = 0
function fConsoleUser { \$script:calls++; if (\$script:calls -ge 3) { 'wintest' } }
function fLogsOnByItself { \$false }
"nowait:" + (fWaitConsoleUser 5 1) + ":" + \$script:calls
\$script:calls = 0
function fLogsOnByItself { \$true }
"waited:" + (fWaitConsoleUser 5 0) + ":" + \$script:calls
function fConsoleUser { }
\$t = [DateTime]::UtcNow
"gaveup:" + (fWaitConsoleUser 1 0) + ":" + [int]([DateTime]::UtcNow - \$t).TotalSeconds
"fresh:" + (fConsoleWait 30) + ":" + (fConsoleWait 599)
"older:" + (fConsoleWait 600) + ":" + (fConsoleWait ([int]::MaxValue))
PS
	mapfile -t con < <(pwsh -NoProfile -File "${work}/console.ps1" 2>&1)
	fCheck "the console user is read from the console's own session" test "${con[0]:-}" = 'in:wintest'
	fCheck "a user on another session isn't it" test "${con[1]:-}" = 'other:'
	fCheck "with nobody logged on it's nobody, not the ssh account" test "${con[2]:-}" = 'none:'
	fCheck "a box that doesn't log on by itself isn't waited for" test "${con[3]:-}" = 'nowait::1'
	fCheck "one that does is waited for until someone's on" test "${con[4]:-}" = 'waited:wintest:3'
	fCheck "for a while, and then it's nobody" test "${con[5]:-}" = 'gaveup::1'
	fCheck "a console session under 10 minutes old gets 3 minutes" test "${con[6]:-}" = 'fresh:180:180'
	fCheck "an older one, or one whose age can't be read, gets 10 s" test "${con[7]:-}" = 'older:10:10'
	fCheck "the stage waits by the session's age" grep -qE $'^\t\\$wait = fConsoleWait \\(fConsoleAge\\)\r?$' "${meDir}/_stage.ps1"
	fCheck "and the age is read after the console id's type is loaded" grep -qE $'^\t\t\\$id = fConsoleId\r?$' "${meDir}/_stage.ps1"
	fCheck "the stage says NOSESSION rather than make a folder for nobody" grep -qE $'^\t\tif \\(fLogsOnByItself\\) \\{ "NOSESSION ' "${meDir}/_stage.ps1"
else
	echo "  skip the console user half: pwsh not found"
fi
## Where each scenario runs. The box listed first takes what it can, and one
## marked 'Box: gpu' goes only to a box tagged gpu. Each runs once.
fRoute(){  ## fRoute <dir> <hosts lines> <held: 1 or ''>: sets rc and out
	local -r dir="${1}" hosts="${2}" held="${3}"
	mkdir -p "${dir}"
	rc=0; out="$(STUB_HOSTS="${hosts}" STUB_LOG="${dir}/calls" STUB_DIR="${dir}" WINRIG_HELD="${held}" SILK_CICD_SKIPS="${dir}/skips" \
		WINGUI_RUNS="${dir}/runs.txt" WINGUI_WIN_REMOTE="${work}/win-remote" WINGUI_EXE="${work}/silkterm.exe" \
		"${meDir}/run.bash" smoke perfladder 2>&1)" || rc=$?
}
bothUp=$'boxb     up    192.0.2.2\nboxg     up    192.0.2.3 gpu'
fRoute "${work}/route-both" "${bothUp}" 1
fCheck "perfladder is marked as needing a GPU box" grep -qE '^##[[:space:]]+Box: gpu'$'\r''?$' "${meDir}/perfladder.ps1"
fCheck "with both boxes up, the first takes the plain scenario" test "$(cat "${work}/route-both/ran" 2>/dev/null)" = $'boxb smoke\nboxg perfladder'
fCheck "and the GPU box only what needs it, with nothing skipped" fNot grep -q '^SKIP' <<< "${out}"
fCheck "so the run's skip list gets nothing" test ! -s "${work}/route-both/skips"
fRoute "${work}/route-nogpu" $'boxb     up    192.0.2.2\nboxg     down  192.0.2.3 gpu' ""
fCheck "with the GPU box down, the plain scenario still runs" test "$(cat "${work}/route-nogpu/ran" 2>/dev/null)" = "boxb smoke"
fCheck "and the GPU one is a SKIP that says why" grep -qFx "SKIP wingui: perfladder, no box tagged gpu answered" <<< "${out}"
fCheck "which doesn't fail the run" test "${rc}" -eq 0
fCheck "and goes on the run's skip list too, for the summary at the end" \
	test "$(cat "${work}/route-nogpu/skips" 2>/dev/null)" = "SKIP wingui: perfladder, no box tagged gpu answered"
fRoute "${work}/route-gpuonly" $'boxb     down  192.0.2.2\nboxg     up    192.0.2.3 gpu' 1
fCheck "with only the GPU box up, it runs both" test "$(cat "${work}/route-gpuonly/ran" 2>/dev/null)" = $'boxg smoke\nboxg perfladder'
STUB_NOSESSION=boxg fRoute "${work}/route-nosession" "${bothUp}" 1
fCheck "a GPU box nobody is logged on at runs nothing" test "$(cat "${work}/route-nosession/ran" 2>/dev/null)" = "boxb smoke"
fCheck "and its SKIP line gives that box's reason" grep -qFx "SKIP wingui: perfladder, no box tagged gpu answered (boxg: nobody is logged on at the console)" <<< "${out}"
fCheck "which doesn't fail the run either" test "${rc}" -eq 0
fRoute "${work}/route-none" $'boxb     down  192.0.2.2\nboxg     down  192.0.2.3 gpu' ""
fCheck "with no box up, each scenario is a SKIP" test "$(grep -c '^SKIP wingui: ' <<< "${out}")" -eq 2 -a "${rc}" -eq 0
fCheck "and both are on the run's skip list" test "$(grep -c '^SKIP wingui: ' "${work}/route-none/skips" 2>/dev/null)" = 2
## A scenario is read the same whichever line ends the checkout gave it, since
## .gitattributes makes them CRLF and an editor can leave one LF.
mkdir -p "${work}/eol"
eval "$(grep -E '^fGpuOnly\(\)' "${meDir}/run.bash")"
printf '##\tTest ID: x\r\n##\tBox: gpu\r\n\r\nfOne\r\n' > "${work}/eol/gpucrlf.ps1"
printf '##\tTest ID: x\n##\tBox: gpu\n\nfOne\n' > "${work}/eol/gpulf.ps1"
printf '##\tTest ID: x\r\n\r\nfOne\r\n' > "${work}/eol/plaincrlf.ps1"
fEol(){ local meDir="${work}/eol"; fGpuOnly "${1}" ;}  ## run.bash's own test, against the files here
fCheck "a 'Box: gpu' line with CRLF ends marks the scenario" fEol gpucrlf
fCheck "and with LF ends" fEol gpulf
fCheck "and a scenario without one isn't marked" fNot fEol plaincrlf
mkdir -p "${work}/route-nolist"
rc=0; out="$(STUB_HOSTS=$'boxb     down  192.0.2.2' STUB_LOG="${work}/route-nolist/calls" STUB_DIR="${work}/route-nolist" \
	WINGUI_RUNS="${work}/route-nolist/runs.txt" WINGUI_WIN_REMOTE="${work}/win-remote" WINGUI_EXE="${work}/silkterm.exe" \
	SILK_CICD_SKIPS="" "${meDir}/run.bash" smoke 2>&1)" || rc=$?
fCheck "with no skip list named, the SKIP line is still printed" test "$(grep -c '^SKIP wingui: smoke' <<< "${out}")" -eq 1 -a "${rc}" -eq 0

((failures == 0)) || { echo "${failures} check(s) failed"; exit 1; }
echo "all passed"

##	Script history:
##		- 20260918: Created.
##		- 20260926: A box that is down is a skip.
##		- 20261002: The run folder is in the console user's temp folder.
##		- 20261002: Run folders are removed, here and on the box.
##		- 20261019: So are the scripts it sends, and a killed run's, by the next run.
##		- 20261025: A box nobody is logged on at is a skip.
##		- 20261025: Which box each scenario runs on.
##		- 20261025: The SKIP lines on the run's skip list.
##		- 20261025: The 'Box: gpu' line with CRLF ends.
##		- 20261028: The console wait goes by how old the session is.
