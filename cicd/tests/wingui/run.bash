#!/usr/bin/env bash

#  shellcheck disable=2016  ## 'Expressions don't expand in single quotes.' The PowerShell being generated needs literal '$'.
#  shellcheck disable=2001  ## 'See if you can use ${variable//search/replace} instead.' Complains about good uses of sed.

##	- Purpose:
##		Run a graphical scenario against a real Windows desktop and bring back the
##		verdict and the screenshots. Nothing here can be done from an ssh session
##		on its own: that opens in session 0, which has no desktop, so the scenario
##		is handed to an interactive scheduled task in the console session instead.
##	- Syntax:
##		run.bash [--host <name>] [--keep] [<scenario> ...]
##		With no scenario it runs 'smoke'. Shots come back under cicd/artifacts/wingui.
##		The binary is cross-built here from the tree under test and sent along, so
##		the result is for this commit and not whatever was last built on the box.
##		WINGUI_EXE names a binary to send instead. WINGUI_EXTRA lists more files,
##		space separated, that go into the run's folder under their own names.
##	- Exit: 0 pass or skipped, 1 a scenario failed.
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

set -euo pipefail
meDir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "${meDir}/../../.." && pwd)"
winRemote="${WINGUI_WIN_REMOTE:-${root}/cicd/utility/win-remote.bash}"
shotDir="${root}/cicd/artifacts/wingui"

origArgs=("$@")
host=(); keep=0
while (($#)); do case "$1" in
	--host) host=(--host "${2:-}"); shift 2 ;;
	--keep) keep=1; shift ;;
	-h|--help) grep -E '^##' "$0" | sed 's/^##\t\?//'; exit 0 ;;
	*) break ;;
esac; done
scenarios=("$@"); ((${#scenarios[@]})) || scenarios=(smoke)

[[ -x "${winRemote}" ]] || { echo "wingui: no win-remote.bash, skipped"; exit 0; }
##	Keep the boxes for the whole run, or another session can get in between a
##	scenario and fetching its shots.
[[ -n "${WINRIG_HELD:-}" ]] || exec "${winRemote}" "${host[@]}" --optional hold "$0" "${origArgs[@]}"
##	Only after the exec, which runs no trap, so the held pass makes the run
##	folder and removes it.
# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use

##	The harness ships itself rather than coming from the remote clone, which is
##	pinned to origin/dev - otherwise every edit here would need a push before it
##	could be run once.
bundle="$(mktemp --suffix=.tgz)"
launcher=""; sweep=""; stage=""
##	install.ps1 goes along for the scenario that checks its PATH change.
tar czf "${bundle}" -C "${meDir}" --exclude=run.bash --exclude=harness-test.bash --exclude=_stage.ps1 . -C "${root}" install.ps1
fEnd(){ local -r rc=$?; rm -f "${bundle}" "${launcher}" "${sweep}" "${stage}"; fTestDir_End "${rc}"; }
trap fEnd EXIT

##	Nothing to build for when every box is off.
declare -a boxes=()
while read -r box state _; do
	if [[ "${state}" == "up" ]]; then boxes+=("${box}"); fi
done < <("${winRemote}" "${host[@]}" --optional hosts 2>/dev/null || true)
if ((! ${#boxes[@]})); then echo "wingui: no box up, skipped"; exit 0; fi

commit="$(git -C "${root}" rev-parse --short HEAD)"
[[ -z "$(git -C "${root}" status --porcelain 2>/dev/null)" ]] || commit+="+uncommitted"
exe="${WINGUI_EXE:-}"
if [[ -z "${exe}" ]]; then
	targetDir="${CARGO_TARGET_DIR:-target}"
	[[ "${targetDir}" == /* ]] || targetDir="${root}/${targetDir}"
	exe="${targetDir}/x86_64-pc-windows-gnu/release/silkterm.exe"
	##	A fat-LTO rustc crash here is usually a flake, so one retry.
	build=(cargo build --release --target x86_64-pc-windows-gnu)
	( cd "${root}/source" && PATH="${HOME}/.cargo/bin:${PATH}" "${build[@]}" >/dev/null 2>&1 ) \
		|| ( cd "${root}/source" && PATH="${HOME}/.cargo/bin:${PATH}" "${build[@]}" ) \
		|| { echo "wingui: the Windows build failed"; exit 1; }
fi
[[ -f "${exe}" ]] || { echo "wingui: no binary at ${exe}"; exit 1; }
echo "wingui: testing ${commit}, $(basename "${exe}") $(stat -c %s "${exe}") bytes"

##	PowerShell single quotes, so a path is taken as written.
fQuote(){ local -r text="${1//\'/\'\'}"; printf "'%s'" "${text}" ;}

fRun() {
	local box="$1" scenario="$2"
	launcher="$(mktemp --suffix=.ps1)"
	{
		printf '$ErrorActionPreference = "Stop"\n'
		printf '. "$PSScriptRoot\\_env.ps1"\n'
		printf '$scenario = "%s"\n' "${scenario}"
		printf '$dir = %s\n' "$(fQuote "${runDir}")"
		printf '$runFor = %s\n' "$(fQuote "${runFor}")"
		printf '$b64 = @"\n%s\n"@\n' "$(base64 -w120 "${bundle}")"
		cat <<'PS'
$work = Join-Path $dir "wingui"
$out  = Join-Path $dir "out"
Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $work, $out | Out-Null
$tgz = Join-Path $dir "wingui.tgz"
[IO.File]::WriteAllBytes($tgz, [Convert]::FromBase64String(($b64 -replace '\s', '')))
tar.exe -xzf $tgz -C $work
Remove-Item $tgz -Force

##	Sent along by run.bash. The clone's own build is from whenever it was last made.
$exe = Join-Path $dir "silkterm.exe"

##	Whoever holds the console is who the scenario has to run as - not whoever ssh
##	logged in as, which may have been pushed off it.
Add-Type -Namespace Con -Name W -MemberDefinition @"
[DllImport("kernel32.dll")] public static extern uint WTSGetActiveConsoleSessionId();
"@
$consoleId = [Con.W]::WTSGetActiveConsoleSessionId()
$who = $env:USERNAME
foreach ($l in (quser 2>$null | Select-Object -Skip 1)) {
	if ($l -match '^\s*>?(\S+)\s+.*?(\d+)\s+(Active|Disc)\b' -and [int]$Matches[2] -eq $consoleId) { $who = $Matches[1] }
}
##	The run's folder is in that user's temp folder, which nobody else can use.
if ($who -ne $runFor) { "VERDICT fail the console is now $who's, and the run folder is in ${runFor}'s temp folder"; exit 1 }

##	An interactive-token task is the one route into that session that needs no
##	stored password: it runs as that user, on their desktop.
$pwsh = (Get-Command pwsh).Source
$name = "silkrig-" + (Split-Path $dir -Leaf)
$me   = "$env:COMPUTERNAME\$who"
$arg  = "-NoProfile -STA -ExecutionPolicy Bypass -File `"$work\_run.ps1`" -Scenario $scenario -Exe `"$exe`" -OutDir `"$out`" -RunDir `"$dir`""
$act  = New-ScheduledTaskAction -Execute $pwsh -Argument $arg
$pri  = New-ScheduledTaskPrincipal -UserId $me -LogonType Interactive
##	Always clear the last answer first. A result file left by the scenario before
##	this one is indistinguishable from this one finishing instantly, and the poll
##	below would take it, print it, and unregister the task mid-run.
$res  = Join-Path $out "result.txt"
Remove-Item $res -Force -ErrorAction SilentlyContinue
##	The last scenario's pids were stopped already. Left in, they get looked up
##	again after Windows has handed them on to other processes.
$startedList = Join-Path $out "started.txt"
Remove-Item $startedList -Force -ErrorAction SilentlyContinue
try {
	Register-ScheduledTask -TaskName $name -Action $act -Principal $pri -Force | Out-Null
	"running as $me in session $consoleId"
	Start-ScheduledTask -TaskName $name
	for ($i = 0; $i -lt 480; $i++) { if (Test-Path $res) { break }; Start-Sleep -Milliseconds 500 }
} finally {
	Unregister-ScheduledTask -TaskName $name -Confirm:$false -ErrorAction SilentlyContinue
	##	Only what this run started. A SilkTerm already on the box is somebody's.
	try { & (Join-Path $work "_stop.ps1") -List $startedList }
	catch { "  note the cleanup threw: $($_.Exception.Message)" }
}
if (-not (Test-Path $res)) { "VERDICT fail the session never answered"; exit 1 }
$said = (Get-Content $res | Where-Object { $_ -like "SCENARIO *" }) -replace '^SCENARIO ', ''
if ($said -ne $scenario) { "VERDICT fail the answer is from '$said', not '$scenario'"; exit 1 }
Get-Content $res | Where-Object { $_ -notlike "SCENARIO *" }
Get-ChildItem (Join-Path $out "shots") -Filter *.png -ErrorAction SilentlyContinue |
	ForEach-Object { "  shot $($_.Name) $($_.Length)" }
##	Read the VERDICT line by name. It used to be the first line and is not any
##	more, and taking line one instead quietly stopped every failure propagating.
$line = (Get-Content $res | Where-Object { $_ -like "VERDICT *" } | Select-Object -First 1)
if (-not $line) { "VERDICT fail no verdict line in the result"; exit 1 }
if ($line -like "VERDICT fail*") { exit 1 }
PS
	} > "${launcher}"
	"${winRemote}" --host "${box}" --optional run "${launcher}" 2>&1
}

##	One box at a time, since each run folder is in its own box's console user's
##	temp folder, made there by _stage.ps1 and read back.
fBox(){
	local -r box="$1"
	local said scenario
	local -i boxFailed=0
	stage="$(mktemp --suffix=.ps1)"
	cat "${meDir}/../_testdir.ps1" "${meDir}/_stage.ps1" > "${stage}"
	said="$("${winRemote}" --host "${box}" --optional run "${stage}" 2>&1 | tr -d '\r' || true)"
	runDir="$(sed -n 's/^RUNDIR //p' <<< "${said}")"
	runFor="$(sed -n 's/^RUNFOR //p' <<< "${said}")"
	runToken="$(sed -n 's/^RUNTOKEN //p' <<< "${said}")"
	if [[ -z "${runDir}" || -z "${runFor}" || -z "${runToken}" ]]; then
		if grep -q 'skipped' <<< "${said}"; then echo "wingui: ${box} went away, skipped"; return 0; fi
		sed 's/^/  /' <<< "${said}"
		echo "wingui: ${box}: no run folder was made"
		return 1
	fi
	echo "wingui: ${box}: ${runDir}"
	##	Checked here, since errexit is off inside a function called with ||. A
	##	scenario with no binary would only skip.
	local extra extraFailed=0
	for extra in ${WINGUI_EXTRA:-}; do
		"${winRemote}" --host "${box}" --optional push "${extra}" "${runDir}\\$(basename "${extra}")" >/dev/null || extraFailed=1
	done
	if ! "${winRemote}" --host "${box}" --optional push "${exe}" "${runDir}\\silkterm.exe" >/dev/null || ((extraFailed)); then
		echo "wingui: ${box}: sending the binary failed"
		boxFailed=1
	else
		for scenario in "${scenarios[@]}"; do
			echo "== wingui: ${scenario}"
			if ! fRun "${box}" "${scenario}" | sed 's/^/  /'; then boxFailed=1; fi
		done
		##	Shots are the whole point of a graphical test, so bring them home.
		if ((! keep)); then
			mkdir -p "${shotDir}"
			"${winRemote}" --host "${box}" --optional pull "${runDir}\\out\\shots" "${shotDir}" >/dev/null 2>&1 || true
		fi
	fi
	##	Take the run's folder away, pass or fail, so a shared box does not pile
	##	them up. Only the folder the stage made and marked.
	if ((! keep)); then
		sweep="$(mktemp --suffix=.ps1)"
		{
			cat "${meDir}/../_testdir.ps1"
			printf "\nif (fTestDir_Remove -Dir %s -Token %s) { 'REMOVED' } else { 'NOT REMOVED' }\n" "$(fQuote "${runDir}")" "$(fQuote "${runToken}")"
		} > "${sweep}"
		said="$("${winRemote}" --host "${box}" --optional run "${sweep}" 2>&1 | tr -d '\r' || true)"
		if ! grep -qx 'REMOVED' <<< "${said}"; then
			echo "wingui: ${box}: run folder not removed:"
			sed 's/^/  /' <<< "${said}"
		fi
	fi
	return "${boxFailed}"
}

failed=0
runDir=""; runFor=""; runToken=""
for box in "${boxes[@]}"; do
	fBox "${box}" || failed=1
done
if ((! keep)); then find "${shotDir}" -name '*.png' -printf '  shot %P\n' 2>/dev/null | sort || true; fi

((failed == 0))

##	Script history:
##		- 20260908: Created.
##		- 20260910: holds the boxes for the whole run.
##		- 20260918: sends a binary built from the tree under test, and stops only what it started.
##		- 20261002: stages in the console user's temp folder, one box at a time.
##		- 20261002: removes the run folder on the box only when it has the run's mark, also after a failed send.
##		- 20261003: WINGUI_EXTRA.
