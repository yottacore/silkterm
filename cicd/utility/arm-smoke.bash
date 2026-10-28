#!/usr/bin/env bash

#  shellcheck disable=2029  ## 'Note that, unescaped, this expands on the client side.' The remote commands are built here on purpose.

##	- Purpose:
##		The Linux ARM64 build's smoke run. Copy it to the arm64 Linux box, run
##		--version there, and check the first line starts with what the caller
##		expects, such as the native build's name, version and build number. Only
##		a run on real arm64 shows the cross build starts at all.
##	- Syntax:
##		arm-smoke.bash <binary> <expected start of the first line>
##	- Notes:
##		The box comes from $ARMRIG_CONF, default ~/.config/silkterm/armrig.conf,
##		outside the repo like winrig.conf: a '<host> [<ssh target>]' line, an
##		'@lock <command>' line naming the host lock, and an '@vmstate <command>'
##		line naming the VM state helper. The helper holds the box through the
##		same lock, wakes a VM that is paused or off, and puts it back the way it
##		was found after. Without it, a VM that is asleep doesn't answer, and is a
##		skip like any box that isn't there.
##		A skip prints "SKIP arm64 smoke: <why>" and adds that line to the file
##		SILK_CICD_SKIPS names, when it's set. ARMRIG_LOCK_WAIT caps the wait for
##		the lock in seconds, default 100.
##		The binary goes in its own folder under the box's temp root and is
##		removed after, folder and all.
##	- Exit: 0 passed, 1 failed, 2 usage, 3 skipped.
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

set -Eeuo pipefail
origArgs=("${@}")
conf="${ARMRIG_CONF:-${XDG_CONFIG_HOME:-${HOME}/.config}/silkterm/armrig.conf}"
sshOpts=(-o BatchMode=yes -o ConnectTimeout=6 -o StrictHostKeyChecking=accept-new -o LogLevel=ERROR)

held=0
if [[ "${1:-}" == --held ]]; then held=1; shift; fi   ## internal, the run under the lock
(($# == 2)) && [[ -f "${1}" && -n "${2}" ]] || { echo "usage: arm-smoke.bash <binary> <expected start of the first line>" >&2; exit 2; }
bin="${1}"; expect="${2}"

fSkip(){
	echo "SKIP arm64 smoke: ${1}"
	[[ -z "${SILK_CICD_SKIPS:-}" ]] || echo "SKIP arm64 smoke: ${1}" >>"${SILK_CICD_SKIPS}"
	exit 3
}

host=""; target=""; lockCmd=""; vmStateCmd=""
[[ -r "${conf}" ]] || fSkip "no host config at ${conf}"
while read -r key value _; do
	[[ -z "${key}" || "${key:0:1}" == "#" ]] && continue
	case "${key}" in
		@lock)    lockCmd="${value}" ;;
		@vmstate) vmStateCmd="${value}" ;;
		*)        [[ -n "${host}" ]] || { host="${key}"; target="${value:-${key}}"; } ;;
	esac
done <"${conf}"
[[ -n "${host}" ]] || fSkip "no host in ${conf}"

fHave(){ [[ -n "${1}" ]] && command -v "${1}" >/dev/null; }
fUp(){ ssh "${sshOpts[@]}" "${target}" exit </dev/null >/dev/null 2>&1; }

##	Copy, run, remove. Only the run's own result decides pass or fail.
fSmoke(){
	local dir out rc=0
	dir="$(ssh "${sshOpts[@]}" "${target}" 'base="${HOST_TEMP:-${TMPDIR:-/tmp}}/silkterm"; s="$(date +%Y%m%d-%H%M%S)"; n=1
		mkdir -p "${base}" || exit 1
		while ! mkdir "${base}/${s}_$(printf %03d "${n}")" 2>/dev/null; do n=$((n + 1)); [ "${n}" -lt 1000 ] || exit 1; done
		echo "${base}/${s}_$(printf %03d "${n}")"' </dev/null)" || { echo "arm64 smoke: no work folder on ${host}" >&2; return 1; }
	if scp -q "${sshOpts[@]}" "${bin}" "${target}:${dir}/silkterm"; then
		out="$(ssh "${sshOpts[@]}" "${target}" "chmod +x '${dir}/silkterm' && nice -n 19 timeout 300 '${dir}/silkterm' --version" </dev/null 2>&1)" || rc=$?
	else
		out="could not copy the binary"; rc=1
	fi
	ssh "${sshOpts[@]}" "${target}" "rm -f -- '${dir}/silkterm'; rmdir -- '${dir}'" </dev/null >/dev/null 2>&1 || true
	printf '%s\n' "${out}" | sed 's/^/  /'
	if ((rc)); then echo "arm64 smoke: --version on ${host} exited ${rc}" >&2; return 1; fi
	if [[ "${out%%$'\n'*}" != "${expect}"* ]]; then echo "arm64 smoke: expected the first line to start with '${expect}'" >&2; return 1; fi
	echo "arm64 smoke: passed on ${host}"
}

if ((held)); then
	##	Started either way, so the caller passes on this exit as it is.
	if [[ -n "${ARMRIG_STARTED:-}" ]]; then : >"${ARMRIG_STARTED}"; fi
	fUp || fSkip "${host} unreachable"
	fSmoke; exit
fi

##	Held by the caller already, or no lock here: no waking, so the box has to answer.
if ! fHave "${lockCmd}"; then
	fUp || fSkip "${host} unreachable"
	fSmoke; exit
fi
rc=0; "${lockCmd}" check "${host}" >/dev/null 2>&1 || rc=$?
case "${rc}" in
	0) fUp || fSkip "${host} unreachable"; fSmoke; exit ;;
	1) ;;
	*) fUp || fSkip "${host} unreachable"; echo "arm64 smoke: host lock unusable here (exit ${rc}), going ahead without it" >&2; fSmoke; exit ;;
esac

wrapper="${lockCmd}"
if fHave "${vmStateCmd}"; then wrapper="${vmStateCmd}"; else fUp || fSkip "${host} unreachable"; fi
started="$(mktemp)"; rm -f "${started}"
rc=0
ARMRIG_STARTED="${started}" "${wrapper}" wrap "${host}" --why "silkterm arm64 smoke" --wait "${ARMRIG_LOCK_WAIT:-100}" \
	-- "${BASH_SOURCE[0]}" --held "${origArgs[@]}" || rc=$?
if [[ -e "${started}" ]]; then rm -f "${started}"; exit "${rc}"; fi
##	The run never started: the box was held past the wait, or a VM never woke.
((rc == 3)) && fSkip "${host} held by another session"
((rc == 0)) || fSkip "${host} didn't come up (exit ${rc})"
exit 1


##	History:
##		- 20261028: Created.
