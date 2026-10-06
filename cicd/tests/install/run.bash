#!/usr/bin/env bash

##	- Purpose:
##		Hygiene the installers and the headless rig have to keep: no secret on a
##		command line, no plain-http redirect, no adopting a directory somebody
##		else made in a shared temp folder, no token file left behind, and a menu
##		launcher that still works from a path holding a space.
##		The last two run install.bash for real, against a stand-in release served
##		by a curl on PATH, in a scratch home and temp folder.
##	- Test ID: EpHW9fk
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

fAbsent(){ ! grep -Eq -e "${1}" -- "${2}" </dev/null; }
fPresent(){ grep -Eq -e "${1}" -- "${2}" </dev/null; }

fCheck "the token is never a curl argument" \
	fAbsent '(-H|--header)[= ]"?Authorization' "${root}/install.bash"
fCheck "https is pinned across redirects (curl)" \
	fPresent '\-\-proto-redir' "${root}/install.bash"
fCheck "https is pinned (wget)" \
	fPresent '\-\-https-only' "${root}/install.bash"
fCheck "the ps1 does not adopt an existing temp directory" \
	fAbsent 'New-Item -ItemType Directory -Force -Path \$tmpDir' "${root}/install.ps1"
## The line above only knows one spelling. This runs install.ps1's own step.
if command -v pwsh >/dev/null 2>&1; then
	fCheck "the ps1's temp folder step refuses a folder already there" \
		pwsh -NoProfile -File "${meDir}/tempdir.ps1" -Installer "${root}/install.ps1"
else
	echo "  skip the ps1's temp folder step (no pwsh)"
fi

## Everything below installs for real. One scratch tree, thrown away at the end.
work="$(mktemp -d)"
trap 'rc=$?; rm -rf "${work}"; fTestDir_End "${rc}"' EXIT

## A stand-in release: for each version, a program that records that it ran
## and a line naming its version, plus the checksums file the installer
## verifies it against. The release list the API serves is written per case.
## fReleaseElf makes each program a copy of sleep instead, since only a running
## binary, not a script, makes a copy over it fail with "text file busy".
fRelease() {
	local dir="$1" ver; shift
	mkdir -p "${dir}"
	for ver in "$@"; do
		printf '#!/bin/sh\n#ver %s\nprintf ran > "${HOME}/ran.txt"\n' "${ver}" > "${dir}/silkterm-${ver}-linux-x86_64"
		( cd "${dir}" && sha256sum "silkterm-${ver}-linux-x86_64" > "silkterm-${ver}-sha256sums.txt" )
	done
}
fReleaseElf() {
	local dir="$1" ver; shift
	mkdir -p "${dir}"
	for ver in "$@"; do
		{ cat "$(command -v sleep)"; printf '\n#ver %s\n' "${ver}"; } > "${dir}/silkterm-${ver}-linux-x86_64"
		( cd "${dir}" && sha256sum "silkterm-${ver}-linux-x86_64" > "silkterm-${ver}-sha256sums.txt" )
	done
}
relDir="${work}/release"
fRelease "${relDir}" 9.9.9
printf '[{"tag_name":"v9.9.9","draft":false,"prerelease":false}]\n' > "${relDir}/releases.json"

## A curl that serves it. Takes the URL, -o and -w, ignores the rest, and notes
## each --config so the token check can see the file was still passed that way.
## STUB_API_CODE is the status the API answers with.
stubDir="${work}/stub"
mkdir -p "${stubDir}"
cat > "${stubDir}/curl" <<'STUB'
#!/usr/bin/env bash
out=""; url=""; fmt=""
while [[ "$#" -gt 0 ]]; do
	case "$1" in
		-o)        out="$2"; shift 2 ;;
		-w)        fmt="$2"; shift 2 ;;
		--config)  echo "config $2" >>"${STUB_LOG}"; shift 2 ;;
		https://*) url="$1"; shift ;;
		*)         shift ;;
	esac
done
code=200
fServe() {
	case "${url}" in
		*/releases\?*) code="${STUB_API_CODE:-200}"; cat "${STUB_DIR}/releases.json" ;;
		*) cat "${STUB_DIR}/${url##*/}" ;;
	esac
}
if [[ -n "${out}" ]]; then fServe >"${out}"; else fServe; fi
[[ -z "${fmt}" ]] || printf '%s' "${code}"
STUB
chmod +x "${stubDir}/curl"

## Run an installer in a home and temp folder of its own. $1 is the home, the
## rest are extra environment, and what it printed goes to out.log there.
## INSTALLER is bash or ps1, and STUB_DIR, STUB_API_CODE and INSTALL_ARGS pick
## the case. Returns the installer's status; install.ps1 run as a script block
## has none, so there an "Error:" line is the failure.
pwshDir="$(dirname "$(readlink -f "$(command -v pwsh 2>/dev/null || echo /nonexistent)")")"
fInstall() {
	local home="$1" rc=0; shift
	local -a args=() run=()
	read -r -a args <<<"${INSTALL_ARGS:-}"
	if [[ "${INSTALLER:-bash}" = "ps1" ]]; then
		run=(pwsh -NoProfile -NonInteractive -File "${meDir}/stubrun.ps1" -Installer "${root}/install.ps1")
	else
		run=(bash "${root}/install.bash" --yes)
	fi
	mkdir -p "${home}" "${home}/.tmp"
	env -i PATH="${stubDir}:/usr/bin:/bin:${pwshDir}" HOME="${home}" TMPDIR="${home}/.tmp" \
		STUB_DIR="${STUB_DIR:-${relDir}}" STUB_API_CODE="${STUB_API_CODE:-200}" \
		STUB_LOG="${home}/.tmp/calls.log" "$@" \
		"${run[@]}" "${args[@]}" >"${home}/out.log" 2>&1 || rc=$?
	if [[ "${rc}" = "0" ]] && grep -q '^Error:' "${home}/out.log"; then rc=1; fi
	return "${rc}"
}
fInstalled(){ grep -aqFx "#ver ${2}" "${1}/.local/bin/silkterm" 2>/dev/null; }
fSaid(){ grep -qF -- "${2}" "${1}/out.log"; }
fNotSaid(){ ! grep -qF -- "${2}" "${1}/out.log"; }
fSaidRe(){ grep -qE -- "${2}" "${1}/out.log"; }

## The token used to be written into a fresh 0700 folder per API call, and the
## cleanup found nothing to remove because the function that made it ran in a
## command substitution.
tokenHome="${work}/tokenhome"
fInstall "${tokenHome}" GITHUB_TOKEN="sekrit-token-42" || true
fCheck "the token file does not outlast the run" \
	test -z "$(find "${tokenHome}/.tmp" -type f -not -name calls.log 2>/dev/null)"
fCheck "and the token was still passed in a file, not on a command line" \
	fPresent '^config ' "${tokenHome}/.tmp/calls.log"

## install.bash's own escaping rule, lifted out of the file so the test cannot
## drift from it, against the shared case list.
bad=0
lifted="$(sed -n '/^function fDesktopExec()/,/^}/p' "${root}/install.bash")"
if [[ -z "${lifted}" ]]; then
	echo "    install.bash has no fDesktopExec"
	bad=1
else
	eval "${lifted}"
	while IFS=$'\t' read -r path want; do
		case "${path}" in '' | '#'*) continue ;; esac
		got="$(fDesktopExec "${path}")"
		if [[ "${got}" != "${want}" ]]; then
			echo "    ${path}: wanted ${want}, got ${got}"
			bad=$((bad + 1))
		fi
	done < "${meDir}/desktop-exec-cases.txt"
fi
fCheck "install.bash escapes Exec the way both rule sets read it" test "${bad}" -eq 0

## A menu launcher from a home holding a space. The desktop entry format splits
## Exec at spaces, so an unquoted path gives an entry the desktop cannot load.
spacedHome="${work}/home dir"
fInstall "${spacedHome}" || true
entry="${spacedHome}/.local/share/applications/silkterm.desktop"
if command -v desktop-file-validate >/dev/null 2>&1; then
	fCheck "the launcher written from a spaced home is a valid entry" \
		desktop-file-validate "${entry}"
else
	echo "  skip desktop-file-validate (not installed)"
fi
if command -v gio >/dev/null 2>&1; then
	env -u DISPLAY HOME="${spacedHome}" gio launch "${entry}" >/dev/null 2>&1 || true
	## gio returns before the program has written anything.
	for _ in 1 2 3 4 5 6 7 8 9 10; do [[ -e "${spacedHome}/ran.txt" ]] && break; sleep 0.2; done
	fCheck "and it starts the installed program" test -e "${spacedHome}/ran.txt"
else
	echo "  skip gio launch (not installed)"
fi

## The cases both installers have to get right, run once for each. $1 is bash
## or ps1.
fCases() {
	local kind="$1" h caseDir rc pid
	local INSTALLER="${kind}"
	export INSTALLER
	## Which release: the highest version, not the first listed, with drafts
	## skipped and 1.0.0 above its own pre-releases.
	caseDir="${work}/${kind}-rel-order"
	fRelease "${caseDir}" 9.9.9 9.9.9-alpha.2 9.9.8 10.0.0
	printf '%s\n' '[{"tag_name":"v9.9.9-alpha.2","draft":false,"prerelease":true},{"tag_name":"v10.0.0","draft":true,"prerelease":false},{"tag_name":"v9.9.8","draft":false,"prerelease":false},{"tag_name":"v9.9.9","draft":false,"prerelease":false}]' > "${caseDir}/releases.json"
	h="${work}/${kind}-order-stable"
	STUB_DIR="${caseDir}" fInstall "${h}" || true
	fCheck "${kind}: stable takes the highest full release, skipping a draft" fInstalled "${h}" 9.9.9
	h="${work}/${kind}-order-dev"
	STUB_DIR="${caseDir}" INSTALL_ARGS="--release dev" fInstall "${h}" || true
	fCheck "${kind}: dev puts 1.0.0 above 1.0.0-alpha.2" fInstalled "${h}" 9.9.9
	## Windows PowerShell 5.1 hands the API's list over as one object. Read as a
	## list of one, every tag came at once and nothing could be picked.
	if [[ "${kind}" = "ps1" ]]; then
		h="${work}/${kind}-order-one-object"
		STUB_DIR="${caseDir}" fInstall "${h}" STUB_ONE_OBJECT=1 || true
		fCheck "${kind}: a release list handed over as one object still picks the highest" fInstalled "${h}" 9.9.9
	fi

	## Only pre-releases: stable says so and takes the newest, beta10 over beta3.
	caseDir="${work}/${kind}-rel-pre"
	fRelease "${caseDir}" 9.9.9-beta3 9.9.9-beta10
	printf '%s\n' '[{"tag_name":"v9.9.9-beta3","draft":false,"prerelease":true},{"tag_name":"v9.9.9-beta10","draft":false,"prerelease":true}]' > "${caseDir}/releases.json"
	h="${work}/${kind}-pre-only"
	STUB_DIR="${caseDir}" fInstall "${h}" || true
	fCheck "${kind}: with no full release, stable takes the newest pre-release" fInstalled "${h}" 9.9.9-beta10
	fCheck "${kind}: and says it did" fSaid "${h}" "No full release published yet"

	## An API failure is an error, never "no full release".
	caseDir="${work}/${kind}-rel-500"
	mkdir -p "${caseDir}"
	printf '{"message":"Server Error"}\n' > "${caseDir}/releases.json"
	h="${work}/${kind}-api-500"
	rc=0; STUB_DIR="${caseDir}" STUB_API_CODE=500 fInstall "${h}" || rc=$?
	fCheck "${kind}: a failed API call stops the install" test "${rc}" -ne 0
	fCheck "${kind}: and is not read as no full release" fNotSaid "${h}" "No full release"
	fCheck "${kind}: and names the status" fSaidRe "${h}" "Detail:.*500"

	caseDir="${work}/${kind}-rel-403"
	mkdir -p "${caseDir}"
	printf '{"message":"API rate limit exceeded for 192.0.2.1."}\n' > "${caseDir}/releases.json"
	h="${work}/${kind}-api-403"
	rc=0; STUB_DIR="${caseDir}" STUB_API_CODE=403 fInstall "${h}" || rc=$?
	fCheck "${kind}: a rate-limited API call stops the install" test "${rc}" -ne 0
	fCheck "${kind}: and gives the rate limit hint" fSaid "${h}" "rate limit is exhausted"

	## A re-run with the program current puts back a launcher that went missing,
	## and leaves the program alone.
	h="${work}/${kind}-rerun"
	fInstall "${h}" || true
	rm -f "${h}/.local/share/applications/silkterm.desktop"
	touch -d '2001-01-01' "${h}/.local/bin/silkterm"
	rc=0; fInstall "${h}" || rc=$?
	fCheck "${kind}: a re-run puts back a missing launcher" test -e "${h}/.local/share/applications/silkterm.desktop"
	fCheck "${kind}: without installing the program again" test "$(stat -c %Y "${h}/.local/bin/silkterm")" = "$(date -d '2001-01-01' +%s)"
	fCheck "${kind}: and says so" fSaid "${h}" "Put back what was missing"
	rc=0; fInstall "${h}" || rc=$?
	fCheck "${kind}: with nothing missing, a re-run does nothing" fSaid "${h}" "Already up to date"

	## An upgrade over a running copy. A copy over it fails on Linux.
	caseDir="${work}/${kind}-rel-busy"
	fReleaseElf "${caseDir}" 9.9.8 9.9.9
	printf '[{"tag_name":"v9.9.8","draft":false,"prerelease":false}]\n' > "${caseDir}/releases.json"
	h="${work}/${kind}-busy"
	STUB_DIR="${caseDir}" fInstall "${h}" || true
	"${h}/.local/bin/silkterm" 30 &
	pid=$!
	printf '[{"tag_name":"v9.9.9","draft":false,"prerelease":false}]\n' > "${caseDir}/releases.json"
	rc=0; STUB_DIR="${caseDir}" fInstall "${h}" || rc=$?
	kill "${pid}" 2>/dev/null || true
	wait "${pid}" 2>/dev/null || true
	fCheck "${kind}: an upgrade goes over a running copy" test "${rc}" -eq 0
	fCheck "${kind}: and installs the new one" fInstalled "${h}" 9.9.9
	fCheck "${kind}: leaving nothing staged behind" test -z "$(find "${h}/.local/bin" -name '.silkterm*' 2>/dev/null)"
}
fCases bash
if command -v pwsh >/dev/null 2>&1; then
	fCases ps1
else
	echo "  skip install.ps1 cases (no pwsh)"
fi

## An option with no value says which one, rather than exiting in silence.
for opt in --release --target; do
	h="${work}/bash-bare${opt}"
	rc=0; INSTALL_ARGS="${opt}" fInstall "${h}" || rc=$?
	fCheck "bash: ${opt} with no value fails" test "${rc}" -ne 0
	fCheck "bash: and names the option" fSaid "${h}" "${opt} needs a value"
done

## install.ps1 without -Yes, where it cannot ask. It used to say "Aborted" and
## succeed. Read-Host throws under -NonInteractive, and with a terminal on stdin
## nothing else gives it away, so that case runs under a pty.
if command -v pwsh >/dev/null 2>&1; then
	fAsked(){
		fSaid "${1}" "Error: there is no terminal here to ask for confirmation" &&
			fSaid "${1}" "Re-run with -Yes" && [[ ! -e "${1}/.local/bin/silkterm" ]]
	}
	h="${work}/ps1-no-yes-devnull"
	rc=0; INSTALLER=ps1 fInstall "${h}" STUB_NO_YES=1 </dev/null || rc=$?
	fCheck "ps1: no -Yes and no input fails, asks for -Yes and installs nothing" test "${rc}" -ne 0 -a -n "$(fAsked "${h}" && echo y)"
	if command -v script >/dev/null 2>&1; then
		h="${work}/ps1-no-yes-pty"
		mkdir -p "${h}/.tmp"
		env -i PATH="${stubDir}:/usr/bin:/bin:${pwshDir}" HOME="${h}" TMPDIR="${h}/.tmp" STUB_DIR="${relDir}" \
			STUB_LOG="${h}/.tmp/calls.log" STUB_NO_YES=1 \
			script -qec "pwsh -NoProfile -NonInteractive -File '${meDir}/stubrun.ps1' -Installer '${root}/install.ps1'" /dev/null \
			</dev/null >"${h}/out.log" 2>&1 || true
		fCheck "ps1: no -Yes under -NonInteractive on a terminal asks for -Yes and installs nothing" fAsked "${h}"
		fCheck "ps1: and does not claim to have aborted" fNotSaid "${h}" "Aborted"
	else
		echo "  skip install.ps1 on a terminal (no script)"
	fi
fi

## install.ps1 writes the same entry, and its Exec goes through the same rule.
## Its own block is lifted out of the file, so the two cannot drift apart.
if command -v pwsh >/dev/null 2>&1; then
	rc=0
	pwsh -NoProfile -File "${meDir}/desktop-entry.ps1" || rc=$?
	fCheck "install.ps1 writes a launcher that loads from a spaced path" test "${rc}" -eq 0
else
	echo "  skip install.ps1 launcher (no pwsh)"
fi

## The headless rig's run directory: predictable name, so it must refuse anything
## it does not own. Driven for real, in a sandbox of its own.
headless="${root}/cicd/utility/gui-headless.bash"
sandboxUser="silktest-$$-$RANDOM"
runDir="/tmp/cicd-gui-headless-${sandboxUser}"
elsewhere="$(mktemp -d)"
ln -s "${elsewhere}" "${runDir}"
rc=0
USER="${sandboxUser}" "${headless}" status >/dev/null 2>&1 || rc=$?
fCheck "a run directory that is a link is refused" test "${rc}" -ne 0
rm -f "${runDir}"; rm -rf "${elsewhere}"

## And an ordinary one of our own is fine, with the mode it should have.
mkdir -p "${runDir}"
rc=0
USER="${sandboxUser}" "${headless}" status >/dev/null 2>&1 || rc=$?
fCheck "one of our own is used" test "${rc}" -eq 0
fCheck "and is not readable by anyone else" test "$(stat -c %a "${runDir}")" = "700"
rm -rf "${runDir}"

## The rig's display: a number it did not start, a pid file whose pid is now
## something else, and a second run's stop. Each on a free number, with the
## sandbox's own run directory.
fFreeDisplay(){ local n; for n in $(seq "${1}" 299); do [[ -e "/tmp/.X${n}-lock" || -e "/tmp/.X11-unix/X${n}" ]] || { echo "${n}"; return 0; }; done; return 1; }
fGone(){ local _; for _ in {1..50}; do [[ -d "/proc/${1}" ]] || return 0; sleep 0.1; done; return 1; }
if command -v Xvfb >/dev/null && command -v xdpyinfo >/dev/null; then
	mkdir -p "${runDir}"; chmod 700 "${runDir}"
	fRig(){ USER="${sandboxUser}" CICD_HEADLESS_DISPLAY=":${1}" CICD_HEADLESS_SIZE=320x200x24 "${headless}" "${@:2}"; }

	## Taken: someone else's server already holds the number.
	n="$(fFreeDisplay 250)"
	Xvfb ":${n}" -screen 0 640x480x24 -nolisten tcp >/dev/null 2>&1 &
	foreign=$!
	for _ in {1..50}; do [[ -e "/tmp/.X${n}-lock" ]] && break; sleep 0.1; done
	rc=0; out="$(fRig "${n}" start 2>&1)" || rc=$?
	fCheck "a number another server holds is refused" test "${rc}" -ne 0
	fCheck "and not reported as started" bash -c '[[ "$1" != *Started* ]]' _ "${out}"
	fCheck "and the other server is left alone" test -d "/proc/${foreign}"
	kill "${foreign}" 2>/dev/null || true; fGone "${foreign}" || true

	## Stale: the pid file names a process that is not our server.
	n="$(fFreeDisplay 250)"
	sleep 60 &
	decoy=$!
	echo "${decoy}" >"${runDir}/xvfb-${n}.pid"
	fCheck "a stale pid file is not a running server" bash -c '[[ "$(USER="$1" CICD_HEADLESS_DISPLAY=":$2" "$3" status)" == no\ Xvfb* ]]' _ "${sandboxUser}" "${n}" "${headless}"
	fRig "${n}" stop >/dev/null 2>&1 || true
	fCheck "and stop leaves its process alone" test -d "/proc/${decoy}"
	kill "${decoy}" 2>/dev/null || true

	## Two runs: the one that started the server keeps it until it stops it.
	n="$(fFreeDisplay 250)"
	gate="$(mktemp -d)"
	bash -c 'USER="$1" CICD_HEADLESS_DISPLAY=":$2" "$3" start >/dev/null 2>&1; touch "$4/up"; while [[ ! -e "$4/go" ]]; do sleep 0.1; done; USER="$1" CICD_HEADLESS_DISPLAY=":$2" "$3" stop >/dev/null 2>&1' \
		_ "${sandboxUser}" "${n}" "${headless}" "${gate}" &
	firstRun=$!
	for _ in {1..100}; do [[ -e "${gate}/up" ]] && break; sleep 0.1; done
	server="$(tr -dc '0-9' <"/tmp/.X${n}-lock" 2>/dev/null || true)"
	fCheck "the first run's server is up" test -n "${server}"
	rc=0; fRig "${n}" start >/dev/null 2>&1 || rc=$?
	fCheck "a second run's start is refused rather than shared" test "${rc}" -ne 0
	rc=0; fRig "${n}" stop >/dev/null 2>&1 || rc=$?
	fCheck "a second run's stop is refused" test "${rc}" -ne 0
	fCheck "and the first run's server is still up" test -d "/proc/${server:-0}"
	touch "${gate}/go"; wait "${firstRun}" || true
	fCheck "the first run's own stop ends it" fGone "${server:-0}"
	rm -f "${gate}/up" "${gate}/go"; rmdir "${gate}"
	rm -rf "${runDir}"
else
	echo "  skip the rig's display checks (no Xvfb or xdpyinfo)"
fi

if ((failures)); then echo "${failures} failed"; exit 1; fi
echo "all passed"

##	History:
##		- 20260908 JC: Created.
##		- 20260917 JC: The rig's display: taken, stale and shared.
##		- 20260925 JC: Which release is picked, API failures, and a re-run that
##		               puts back a missing launcher.
##		- 20260926 JC: A release list as one object, an option with no value, and
##		               install.ps1 without -Yes where it cannot ask.
