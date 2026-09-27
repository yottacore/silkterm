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
cicd="$(cd "${meDir}/../.." && pwd)"
winRemote="${cicd}/utility/win-remote.bash"

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }

work="$(mktemp -d "${TMPDIR:-/tmp}/silk-winremote.XXXXXX")"
trap 'rm -rf "${work}"' EXIT

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
		STUB_DOWN="${down}" "${winRemote}" "$@" 2>&1)" || rc=$?
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

if ((failures)); then echo "${failures} failed"; exit 1; fi
echo "all passed"

##	History:
##		- 20260926 JC: Created.
