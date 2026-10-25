#!/usr/bin/env bash

##	- Purpose:
##		win-remote.bash reaches the Windows boxes over ssh. A box can answer on
##		one of several addresses, and one that is off is the normal case, so the
##		runner has to try each address, step over a box that is down, and fail
##		only when nothing answers and the caller did not say that was fine. Run
##		against a stand-in ssh and scp on PATH and a host list of its own.
##	- Test ID: Er2UgYA
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

set -euo pipefail
meDir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use
cicd="$(cd "${meDir}/../.." && pwd)"
winRemote="${cicd}/utility/win-remote.bash"

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }

work="$(mktemp -d "${TMPDIR:-/tmp}/silk-winremote.XXXXXX")"
trap 'rc=$?; rm -rf "${work}"; fTestDir_End "${rc}"' EXIT

## An ssh that answers for every address not listed in STUB_DOWN, and notes each
## command it was given; an scp that does the same for its target. Neither ever
## reaches the network.
stubs="${work}/stubs"
mkdir -p "${stubs}"
cat >"${stubs}/ssh" <<'STUB'
#!/usr/bin/env bash
while [[ "${1:-}" == -o ]]; do shift 2; done
addr="${1#*@}"; shift
[[ " ${STUB_DOWN:-} " == *" ${addr} "* ]] && exit 255
[[ "$*" == "exit 0" ]] && exit 0
echo "ssh ${addr} $*" >>"${STUB_LOG}"
[[ "$*" == pwsh* ]] && echo "ran on ${addr}"
exit 0
STUB
cat >"${stubs}/scp" <<'STUB'
#!/usr/bin/env bash
dest="${*: -1}"; addr="${dest#*@}"; addr="${addr%%:*}"
[[ " ${STUB_DOWN:-} " == *" ${addr} "* ]] && exit 1
echo "scp ${addr}" >>"${STUB_LOG}"
exit 0
STUB
chmod +x "${stubs}/ssh" "${stubs}/scp"

## Two boxes, the first on two addresses, the way the laptop is on wired or wifi.
conf="${work}/winrig.conf"
printf '# a comment\nboxa 192.0.2.10,192.0.2.11\nboxb 192.0.2.20\n' >"${conf}"
printf 'Write-Output hi\n' >"${work}/job.ps1"

fRemote(){  ## fRemote <down addresses> <args...>: sets rc, out and calls
	local -r down="${1}"; shift
	: >"${work}/calls"
	rc=0; out="$(PATH="${stubs}:${PATH}" WINRIG_CONF="${conf}" WINRIG_USER=tester STUB_LOG="${work}/calls" \
		STUB_DOWN="${down}" "${winRemote}" "${@}" 2>&1)" || rc=$?
}
fSaid(){ grep -qF -- "${1}" <<<"${out}"; }
fCalled(){ grep -qF -- "${1}" "${work}/calls"; }
fNotCalled(){ ! grep -qF -- "${1}" "${work}/calls"; }

fRemote "" run "${work}/job.ps1"
fCheck "with every box up, the job runs on both" test "${rc}" -eq 0 -a "$(grep -c '^ran on' <<<"${out}")" -eq 2
fCheck "on the first address that answers" fSaid "== boxa (192.0.2.10)"

fRemote "192.0.2.10" run "${work}/job.ps1"
fCheck "a first address that does not answer is passed over" fSaid "== boxa (192.0.2.11)"
fCheck "and the job runs on the second" fCalled "ssh 192.0.2.11 pwsh"
fCheck "and never on the dead one" fNotCalled "192.0.2.10"
fCheck "and the run passes" test "${rc}" -eq 0

fRemote "192.0.2.20" run "${work}/job.ps1"
fCheck "one box down is a warning" fSaid "boxb: unreachable, skipped"
fCheck "and the other still runs the job" fSaid "ran on 192.0.2.10"
fCheck "and the run passes" test "${rc}" -eq 0

fRemote "192.0.2.10 192.0.2.11 192.0.2.20" run "${work}/job.ps1"
fCheck "every box down fails the run" test "${rc}" -eq 1
fCheck "and says so" fSaid "no host reachable"
fRemote "192.0.2.10 192.0.2.11 192.0.2.20" --optional run "${work}/job.ps1"
fCheck "unless --optional, where it is a skip" test "${rc}" -eq 0
fCheck "that says so" fSaid "no host reachable, skipped"

fRemote "192.0.2.10 192.0.2.11" --host boxa run "${work}/job.ps1"
fCheck "a named box that is down fails the run" test "${rc}" -eq 1
fCheck "without touching the other box" fNotCalled "192.0.2.20"

fRemote "192.0.2.10 192.0.2.20" hosts
fCheck "hosts lists each box as up or down" test "${rc}" -eq 0 -a -n "$(grep -E '^boxa +up +192\.0\.2\.11$' <<<"${out}")" -a -n "$(grep -E '^boxb +down +192\.0\.2\.20$' <<<"${out}")"

## A host lock that another session holds boxa through. wrap waits on a busy box
## and gives up with the lock's own exit 3; a free one runs the command with it held.
cat >"${stubs}/lock" <<'STUB'
#!/usr/bin/env bash
cmd="${1}"; shift
case "${cmd}" in
	hosts) echo "boxa boxb" ;;
	check) for h in "$@"; do [[ " ${STUB_HELD:-} " == *" ${h} "* ]] || exit 1; done ;;
	wrap)
		names=(); while [[ "${1}" != -- ]]; do case "${1}" in --why|--wait) shift 2 ;; *) names+=("${1}"); shift ;; esac; done; shift
		for h in "${names[@]}"; do
			if [[ " ${STUB_BUSY:-} " == *" ${h} "* ]]; then echo "still queued: ${h} is held" >&2; exit 3; fi
		done
		echo "wrap ${names[*]}" >>"${STUB_LOG}"
		STUB_HELD="${names[*]}" exec "$@" ;;
esac
STUB
chmod +x "${stubs}/lock"
lockConf="${work}/winrig-lock.conf"
{ printf '@lock %s\n' "${stubs}/lock"; cat "${conf}"; } >"${lockConf}"
fLocked(){  ## fLocked <busy boxes> <exit of the held command> <args...>
	local -r busy="${1}" code="${2}"; shift 2
	: >"${work}/calls"
	rc=0; out="$(PATH="${stubs}:${PATH}" WINRIG_CONF="${lockConf}" WINRIG_USER=tester STUB_LOG="${work}/calls" \
		STUB_BUSY="${busy}" HELD_EXIT="${code}" WIN_REMOTE="${winRemote}" "${winRemote}" "${@}" 2>&1)" || rc=$?
}
cat >"${work}/held" <<'STUB'
#!/usr/bin/env bash
echo "held with ${STUB_HELD:-nothing}, sees $("${WIN_REMOTE}" hosts | tr -s ' \n' ' ')"
exit "${HELD_EXIT}"
STUB
chmod +x "${work}/held"

fLocked "boxa" 0 --optional hold "${work}/held"
fCheck "a box another session holds is stepped over" fSaid "boxa: held by another session, skipped"
fCheck "and the free one still runs, held on its own" fSaid "held with boxb, sees boxb up 192.0.2.20"
fCheck "and the run passes" test "${rc}" -eq 0
fLocked "" 0 --optional hold "${work}/held"
fCheck "with both free, each box gets a turn" test "$(grep -c '^held with' <<<"${out}")" -eq 2
fLocked "" 1 --optional hold "${work}/held"
fCheck "a command that ran and failed still fails the run" test "${rc}" -eq 1
fLocked "" 3 --optional hold "${work}/held"
fCheck "even when its own exit looks like the lock's" test "${rc}" -eq 3
fLocked "boxa" 0 hold "${work}/held"
fCheck "without --optional a busy box still stops the run" test "${rc}" -eq 3

if ((failures)); then echo "${failures} failed"; exit 1; fi
echo "all passed"

##	History:
##		- 20260926 JC: Created.
##		- 20260928 JC: a box another session holds.
