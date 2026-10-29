#!/usr/bin/env bash

##	- Purpose:
##		arm-smoke.bash runs the Linux ARM64 build's --version on the arm64 VM. The
##		VM is often asleep, so with a VM state helper named it has to be woken
##		through it and put back after, and a box that isn't there is a skip with a
##		SKIP line, not a failure. Run against a stand-in ssh, scp, host lock and
##		VM state helper on PATH, so nothing reaches a box.
##	- Test ID: dehx3
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

set -euo pipefail
meDir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use
smoke="$(cd "${meDir}/../.." && pwd)/utility/arm-smoke.bash"

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }
trap 'fTestDir_End $?' EXIT

work="$(mktemp -d "${SILKTERM_TEST_DIR:?}/armsmoke.XXXXXX")"
stubs="${work}/stubs"; mkdir -p "${stubs}"
printf 'not really arm64\n' >"${work}/silkterm"

##	Boxes listed in ${work}/up answer. --version prints STUB_OUT and exits
##	STUB_RC. Every other command is noted, so the clean-up can be seen.
cat >"${stubs}/ssh" <<'STUB'
#!/usr/bin/env bash
while [[ "${1:-}" == -o ]]; do shift 2; done
h="${1}"; shift
grep -qx "${h}" "${STUB_DIR}/up" 2>/dev/null || exit 255
[[ "$*" == exit ]] && exit 0
if [[ "$*" == *--version* ]]; then echo "ran ${h}" >>"${STUB_DIR}/calls"; printf '%s\n' "${STUB_OUT:-}"; exit "${STUB_RC:-0}"; fi
if [[ "$*" == *rmdir* ]]; then echo "cleaned ${h}" >>"${STUB_DIR}/calls"; exit 0; fi
if [[ "$*" == *mkdir* ]]; then echo "/tmp/silkterm/20261028-000000_001"; exit 0; fi
exit 0
STUB
cat >"${stubs}/scp" <<'STUB'
#!/usr/bin/env bash
dest="${*: -1}"; h="${dest%%:*}"
grep -qx "${h}" "${STUB_DIR}/up" 2>/dev/null || exit 1
echo "copied ${dest}" >>"${STUB_DIR}/calls"
STUB
cat >"${stubs}/lock" <<'STUB'
#!/usr/bin/env bash
case "${1}" in
	check) exit "${STUB_HELD:-1}" ;;
	wrap) echo "lock wrap ${2}" >>"${STUB_DIR}/calls"; while [[ "${1}" != -- ]]; do shift; done; shift; "$@" ;;
esac
STUB
##	A box in STUB_ASLEEP is a guest that is off: wrap wakes it, runs the
##	command and puts it back. STUB_NOWAKE: it never answers. STUB_BUSY: the
##	lock's wait runs out.
cat >"${stubs}/vmstate" <<'STUB'
#!/usr/bin/env bash
[[ "${1}" == wrap ]] || exit 2
h="${2}"; echo "vmstate wrap ${h}" >>"${STUB_DIR}/calls"
[[ -z "${STUB_BUSY:-}" ]] || exit 3
while [[ "${1}" != -- ]]; do shift; done; shift
if [[ " ${STUB_ASLEEP:-} " == *" ${h} "* ]]; then
	[[ -z "${STUB_NOWAKE:-}" ]] || { echo "${h}: no ssh after 600s" >&2; exit 1; }
	echo "${h}" >>"${STUB_DIR}/up"
fi
rc=0; "$@" || rc=$?
if [[ " ${STUB_ASLEEP:-} " == *" ${h} "* ]]; then
	grep -vx "${h}" "${STUB_DIR}/up" >"${STUB_DIR}/up.new" || true; mv "${STUB_DIR}/up.new" "${STUB_DIR}/up"
	echo "vmstate restore ${h}" >>"${STUB_DIR}/calls"
fi
exit "${rc}"
STUB
chmod +x "${stubs}/ssh" "${stubs}/scp" "${stubs}/lock" "${stubs}/vmstate"

good="SilkTerm v9.9.9 build abcde (2026-10-28)"
fRun(){  ## fRun <boxes that answer> <vmstate 0|1>: sets rc and out
	local -r up="${1}" vm="${2}"
	tr ' ' '\n' <<<"${up}" >"${work}/up"
	: >"${work}/calls"; : >"${work}/skips"
	{ echo "armbox"; echo "@lock ${stubs}/lock"; ((vm)) && echo "@vmstate ${stubs}/vmstate"; } >"${work}/armrig.conf"
	rc=0; out="$(PATH="${stubs}:${PATH}" ARMRIG_CONF="${work}/armrig.conf" STUB_DIR="${work}" SILK_CICD_SKIPS="${work}/skips" \
		STUB_OUT="${STUB_OUT-${good}}" "${smoke}" "${work}/silkterm" "SilkTerm v9.9.9 build abcde" </dev/null 2>&1)" || rc=$?
}
fSaid(){ grep -qF -- "${1}" <<<"${out}"; }
fCalled(){ grep -qxF -- "${1}" "${work}/calls"; }
fSkipLine(){ grep -qxF -- "${1}" "${work}/skips"; }

export STUB_ASLEEP="armbox"

fRun "" 1
fCheck "an asleep VM is woken through the helper" fCalled "vmstate wrap armbox"
fCheck "the binary goes to it" fCalled "copied armbox:/tmp/silkterm/20261028-000000_001/silkterm"
fCheck "and --version runs there" fCalled "ran armbox"
fCheck "and its folder is removed" fCalled "cleaned armbox"
fCheck "and the VM is put back" fCalled "vmstate restore armbox"
fCheck "and the run passes" test "${rc}" -eq 0
fCheck "saying so" fSaid "arm64 smoke: passed on armbox"

STUB_OUT="SilkTerm v9.9.8 build zzzzz (2026-10-28)" fRun "" 1
fCheck "another version's line fails" test "${rc}" -eq 1
fCheck "and the VM is still put back" fCalled "vmstate restore armbox"

STUB_RC=139 STUB_OUT="" fRun "" 1
fCheck "a binary that crashes fails" test "${rc}" -eq 1
fCheck "and its folder is still removed" fCalled "cleaned armbox"

fRun "" 0
fCheck "without the helper an asleep VM is a skip" test "${rc}" -eq 3
fCheck "with a SKIP line in the run's list" fSkipLine "SKIP arm64 smoke: armbox unreachable"
fCheck "and nothing ran" test ! -s "${work}/calls"

STUB_NOWAKE=1 fRun "" 1
fCheck "a VM that never wakes is a skip" test "${rc}" -eq 3
fCheck "saying why" fSkipLine "SKIP arm64 smoke: armbox didn't come up (exit 1)"

STUB_BUSY=1 fRun "" 1
fCheck "a box held past the wait is a skip" fSkipLine "SKIP arm64 smoke: armbox held by another session"

STUB_ASLEEP="" fRun "" 1
fCheck "a box that's down and not a VM is a skip" test "${rc}" -eq 3
fCheck "with one SKIP line" test "$(grep -c '^SKIP ' "${work}/skips")" -eq 1

STUB_ASLEEP="" fRun "armbox" 0
fCheck "a box that answers goes through the lock" fCalled "lock wrap armbox"
fCheck "and passes" test "${rc}" -eq 0

STUB_HELD=0 STUB_ASLEEP="" fRun "armbox" 1
fCheck "a box the caller holds is run on as it is" test "${rc}" -eq 0 -a "$(grep -c wrap "${work}/calls")" -eq 0

: >"${work}/skips"
rc=0; out="$(ARMRIG_CONF="${work}/none.conf" SILK_CICD_SKIPS="${work}/skips" "${smoke}" "${work}/silkterm" x 2>&1)" || rc=$?
fCheck "no host config is a skip" test "${rc}" -eq 3
fCheck "with a SKIP line" fSkipLine "SKIP arm64 smoke: no host config at ${work}/none.conf"

if ((failures)); then echo "${failures} failed"; exit 1; fi
echo "all passed"

##	History:
##		- 20261028: Created.
