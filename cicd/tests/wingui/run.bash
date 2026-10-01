#!/usr/bin/env bash

#  shellcheck disable=2016  ## 'Expressions don't expand in single quotes.' The PowerShell being generated needs literal '$'.

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
##		WINGUI_EXE names a binary to send instead.
##	- Exit: 0 pass or skipped, 1 a scenario failed.
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

set -euo pipefail
meDir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use
root="$(cd "${meDir}/../../.." && pwd)"
winRemote="${WINGUI_WIN_REMOTE:-${root}/cicd/utility/win-remote.bash}"
shotDir="${root}/cicd/artifacts/wingui"
##	These boxes are shared, so a run may not use a fixed folder or task name -
##	two at once would read each other's answers and cancel each other's tasks.
token="$(date +%Y%m%d-%H%M%S)-$$"
remoteDir="C:\\ProgramData\\silkrig\\run-${token}"

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

##	The harness ships itself rather than coming from the remote clone, which is
##	pinned to origin/dev - otherwise every edit here would need a push before it
##	could be run once.
bundle="$(mktemp --suffix=.tgz)"
launcher=""; sweep=""
##	install.ps1 goes along for the scenario that checks its PATH change.
tar czf "${bundle}" -C "${meDir}" --exclude=run.bash --exclude=harness-test.bash . -C "${root}" install.ps1
trap 'rm -f "${bundle}" "${launcher}" "${sweep}"' EXIT

##	Nothing to build for when every box is off.
if ! "${winRemote}" "${host[@]}" --optional hosts 2>/dev/null | grep -q ' up '; then
	echo "wingui: no box up, skipped"; exit 0
fi

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
"${winRemote}" "${host[@]}" --optional push "${exe}" "${remoteDir}\\silkterm.exe" >/dev/null

fRun() {
	local scenario="$1"
	launcher="$(mktemp --suffix=.ps1)"
	{
		printf '$ErrorActionPreference = "Stop"\n'
		printf '. "$PSScriptRoot\\_env.ps1"\n'
		printf '$scenario = "%s"\n' "${scenario}"
		printf '$dir = "%s"\n' "${remoteDir}"
		printf '$b64 = @"\n%s\n"@\n' "$(base64 -w120 "${bundle}")"
		cat <<'PS'
$work = Join-Path $dir "wingui"
$out  = Join-Path $dir "out"
Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $work, $out | Out-Null
##	The scenario runs as the console user, who is usually not the account that made
##	this directory - and ProgramData only lets a creator write its own files.
icacls "C:\ProgramData\silkrig" /grant "*S-1-5-32-545:(OI)(CI)M" /T /Q 2>&1 | Out-Null
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

##	An interactive-token task is the one route into that session that needs no
##	stored password: it runs as that user, on their desktop.
$pwsh = (Get-Command pwsh).Source
$name = "silkrig-" + (Split-Path $dir -Leaf)
$me   = "$env:COMPUTERNAME\$who"
$arg  = "-NoProfile -STA -ExecutionPolicy Bypass -File `"$work\_run.ps1`" -Scenario $scenario -Exe `"$exe`" -OutDir `"$out`""
$act  = New-ScheduledTaskAction -Execute $pwsh -Argument $arg
$pri  = New-ScheduledTaskPrincipal -UserId $me -LogonType Interactive
##	Always clear the last answer first. A result file left by the scenario before
##	this one is indistinguishable from this one finishing instantly, and the poll
##	below would take it, print it, and unregister the task mid-run.
$res  = Join-Path $out "result.txt"
Remove-Item $res -Force -ErrorAction SilentlyContinue
try {
	Register-ScheduledTask -TaskName $name -Action $act -Principal $pri -Force | Out-Null
	"running as $me in session $consoleId"
	Start-ScheduledTask -TaskName $name
	for ($i = 0; $i -lt 480; $i++) { if (Test-Path $res) { break }; Start-Sleep -Milliseconds 500 }
} finally {
	Unregister-ScheduledTask -TaskName $name -Confirm:$false -ErrorAction SilentlyContinue
	##	Only what this run started. A SilkTerm already on the box is somebody's.
	& (Join-Path $work "_stop.ps1") -List (Join-Path $out "started.txt")
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
	"${winRemote}" "${host[@]}" --optional run "${launcher}" 2>&1
}

failed=0
for scenario in "${scenarios[@]}"; do
	echo "== wingui: ${scenario}"
	if ! fRun "${scenario}" | sed 's/^/  /'; then failed=1; fi
done

##	Shots are the whole point of a graphical test, so bring them home.
if ((! keep)); then
	mkdir -p "${shotDir}"
	"${winRemote}" "${host[@]}" --optional pull "${remoteDir}\\out\\shots" "${shotDir}" >/dev/null 2>&1 || true
	find "${shotDir}" -name '*.png' -printf '  shot %P\n' 2>/dev/null | sort || true
	##	Take the run's folder away with it, so a shared box does not accumulate them.
	sweep="$(mktemp --suffix=.ps1)"
	printf 'Remove-Item -Recurse -Force "%s" -ErrorAction SilentlyContinue\n' "${remoteDir}" > "${sweep}"
	"${winRemote}" "${host[@]}" --optional run "${sweep}" >/dev/null 2>&1 || true
	rm -f "${sweep}"
fi

((failed == 0))

##	Script history:
##		- 20260908: Created.
##		- 20260910: holds the boxes for the whole run.
##		- 20260918: sends a binary built from the tree under test, and stops only what it started.
