#!/usr/bin/env bash

#  shellcheck disable=2016  ## 'Expressions don't expand in single quotes.' They expand in the bash -c they go to.
#  shellcheck disable=2031  ## 'Modified in a subshell.' Each case runs in its own on purpose.
#  shellcheck disable=2034  ## 'appears unused.' The lifted fStepMem reads them.

##	- Purpose:
##		The memory cap every step runs under (utility/include/memcap.bash). It's a
##		third of RAM from 4g to 32g, or a step's own default when it has one, and
##		CICD_MEM_CAP overrides both. "none" means no cap, and any other bad value
##		stops the run before it starts anything. A container gets the cap with no
##		swap and goes first to the OOM killer, and the pipeline runs itself again
##		once, in a systemd user scope with the same cap.
##	- Test ID: df2ss
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

set -euo pipefail
meDir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use
root="$(cd "${meDir}/../../.." && pwd)"
memLib="${root}/cicd/utility/include/memcap.bash"

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }
fNot(){ ! "${@}"; }

work="$(mktemp -d "${TMPDIR:-/tmp}/silk-memcap.XXXXXX")"
fEnd(){ local -r rc=$?; fTestDir_Rm "${work}" || true; fTestDir_End "${rc}"; }
trap fEnd EXIT

## fArgs <cap or "-" for unset> [default]: docker run's flags for it, or the
## failure and its status.
fArgs(){
	(
		# shellcheck source=cicd/utility/include/memcap.bash
		source "${memLib}" 2>/dev/null || exit 9
		if [[ "${1}" == - ]]; then unset CICD_MEM_CAP; else export CICD_MEM_CAP="${1}"; fi
		fMemCap_DockerArgs ${2:+"${2}"} 2>&1 || { echo "rc=${?}"; exit 0; }
		echo "${memCapArgs[*]:-}"
	)
}
kb="$(awk '/^MemTotal:/ { print $2 }' /proc/meminfo)"; want=$(( kb / 3 / 1048576 ))
((want <= 32)) || want=32; ((want >= 4)) || want=4
fCheck "the default is a third of RAM, from 4g to 32g, with as much swap, first to the OOM killer" \
	test "$(fArgs -)" = "--memory ${want}g --memory-swap ${want}g --oom-score-adj 1000"
fCheck "empty is the default too" test "$(fArgs '')" = "--memory ${want}g --memory-swap ${want}g --oom-score-adj 1000"
fCheck "a step's own default wins over the general one" test "$(fArgs - 48g)" = "--memory 48g --memory-swap 48g --oom-score-adj 1000"
fCheck "CICD_MEM_CAP wins over both" bash -c '[[ "$1" == "--memory 7g --memory-swap 7g --oom-score-adj 1000" && "$2" == "$1" ]]' _ "$(fArgs 7g)" "$(fArgs 7g 48g)"
fCheck "none is no cap, a step's default or not" bash -c '[[ -z "$1" && -z "$2" ]]' _ "$(fArgs none)" "$(fArgs none 48g)"
mark="${work}/ran"
for bad in '24 g' 24gb 0 0g -1 'x[$(touch '"${mark}"')]' 'none ' '$(touch '"${mark}"')'; do
	fCheck "'${bad}' fails, saying what it wants" bash -c '[[ "$1" == *"CICD_MEM_CAP"*"isn'"'"'t a size like 24g, or none"*"rc=1" ]]' _ "$(fArgs "${bad}")"
done
fCheck "and none of them is run as arithmetic" test ! -e "${mark}"

## The scope, with a stand-in systemd-run that logs how it was called and stops.
fake="${work}/bin"; mkdir -p "${fake}"
cat >"${fake}/systemd-run" <<'EOF'
#!/usr/bin/env bash
[[ "${*}" != "--user --scope --quiet true" ]] || exit "${FAKE_PROBE_RC:-0}"
printf '%s|%s\n' "${SILK_CICD_MEM_SCOPE:-}" "${*}" >>"${FAKE_LOG}"
exit 0
EOF
printf '#!/usr/bin/env bash\nexec "${@:4}"\n' >"${fake}/choom"
chmod +x "${fake}/systemd-run" "${fake}/choom"
export SILK_SYSTEMD_RUN="${fake}/systemd-run" FAKE_LOG="${work}/scope.log"
## fScope <cap or "-"> [env...]: the library's re-run of a stand-in script; sets
## rc and out, and the log is the stand-in's.
fScope(){
	local -r cap="${1}"; shift
	local kv
	rm -f "${FAKE_LOG}"
	rc=0; out="$(
		export PATH="${fake}:${PATH}"
		unset SILK_CICD_MEM_SCOPE SILK_CICD_IN_CONTAINER
		if [[ "${cap}" == - ]]; then unset CICD_MEM_CAP; else export CICD_MEM_CAP="${cap}"; fi
		for kv in "${@}"; do export "${kv?}"; done
		# shellcheck source=cicd/utility/include/memcap.bash
		source "${memLib}" 2>/dev/null || exit 9
		fMemCap_Scope /x/script.bash -a 'b c' 2>&1 || exit "${?}"
		echo "went on"
	)" || rc=$?
}
fScope 7g
fCheck "with a user manager it runs again in a scope, with the cap and no swap, first to the OOM killer" \
	bash -c '[[ "$(cat "$1")" == "7g|--user --scope --quiet -p MemoryMax=7G -p MemorySwapMax=0 choom -n 1000 -- "*"bash /x/script.bash -a b c" ]]' _ "${FAKE_LOG}"
fCheck "and marks it, so nothing it starts makes another" grep -q '^7g|' "${FAKE_LOG}"
fScope -
fCheck "the default the same way" grep -qE "^${want}g\|.* -p MemoryMax=${want}G " "${FAKE_LOG}"
fScope 7g SILK_CICD_MEM_SCOPE=7g
fCheck "inside a scope already it goes on as it is" bash -c '[[ "$1" == 0 && "$2" == "went on" && ! -e "$3" ]]' _ "${rc}" "${out}" "${FAKE_LOG}"
fScope 7g SILK_CICD_IN_CONTAINER=1
fCheck "and in the container" bash -c '[[ "$1" == 0 && "$2" == "went on" && ! -e "$3" ]]' _ "${rc}" "${out}" "${FAKE_LOG}"
fScope none
fCheck "and with none" bash -c '[[ "$1" == 0 && "$2" == "went on" && ! -e "$3" ]]' _ "${rc}" "${out}" "${FAKE_LOG}"
fScope 7g FAKE_PROBE_RC=1
fCheck "and with no user manager, as on a bare runner" bash -c '[[ "$1" == 0 && "$2" == "went on" && ! -e "$3" ]]' _ "${rc}" "${out}" "${FAKE_LOG}"
fScope 7g SILK_SYSTEMD_RUN="${work}/no-such-systemd-run"
fCheck "and with no systemd-run, as on macOS" bash -c '[[ "$1" == 0 && "$2" == "went on" ]]' _ "${rc}" "${out}"
fScope 7x
fCheck "a bad cap fails there too, and runs nothing" bash -c '[[ "$1" == 1 && "$2" == *"isn'"'"'t a size like 24g"* && "$2" != *"went on"* && ! -e "$3" ]]' _ "${rc}" "${out}" "${FAKE_LOG}"

## The pipeline: the scope before anything else, and a bad cap stops it there.
fakeDocker="${work}/docker.log"
printf '#!/usr/bin/env bash\necho "$*" >>"%s"\n' "${fakeDocker}" >"${fake}/docker"; chmod +x "${fake}/docker"
fPipe(){
	rm -f "${FAKE_LOG}" "${fakeDocker}"
	rc=0; out="$(cd "${root}" && env -u SILK_CICD_MEM_SCOPE -u SILK_CICD_IN_CONTAINER PATH="${fake}:${PATH}" SILK_DOCKER="${fake}/docker" "${@}" bash cicd/cicd.bash --list 2>&1)" || rc=$?
}
fPipe CICD_MEM_CAP=7g
fCheck "cicd.bash runs again in the scope first" \
	bash -c '[[ "$(cat "$1")" == "7g|--user --scope --quiet -p MemoryMax=7G -p MemorySwapMax=0 choom -n 1000 -- "*"bash cicd/cicd.bash --list" ]]' _ "${FAKE_LOG}"
fCheck "before docker is asked anything" test ! -e "${fakeDocker}"
fPipe CICD_MEM_CAP='x[$(touch '"${mark}"')]'
fCheck "a bad cap stops cicd.bash at once" test "${rc}" -eq 2
fCheck "saying why" grep -qF "isn't a size like 24g, or none" <<<"${out}"
fCheck "with nothing started: no scope, no docker, no arithmetic" bash -c '[[ ! -e "$1" && ! -e "$2" && ! -e "$3" ]]' _ "${FAKE_LOG}" "${fakeDocker}" "${mark}"

## Each step's container takes the flags, from one helper.
fCheck "the step container takes them" grep -qF -- '--cpu-shares 256 "${memCapArgs[@]}" --name "${EXE_NAME}-${runName}-${step}"' "${root}/cicd/cicd.bash"
fCheck "and so does the tidy one" grep -qF -- 'run "${labels[@]}" "${memCapArgs[@]}" --name "${tidy}"' "${root}/cicd/cicd.bash"
fCheck "and no other docker run is left without them" test "$(grep -cE '"\$\{docker\}" run ' "${root}/cicd/cicd.bash")" -eq 2
## CodeQL's own default, from fStepMem.
eval "$(sed -n '/^fStepMem(){/,/^}/p' "${root}/cicd/cicd.bash")"
fStepAt(){ (CODEQL_SUITE="${1}" leakcheck="${2}" codeql="${3}"; fStepMem "${4}"); }
fCheck "CodeQL's step gets 48g" test "$(fStepAt '' 0 0 codeql)" = 48g
fCheck "and so does an engine run that does CodeQL inside" test "$(fStepAt suite 1 1 engine)" = 48g
fCheck "but not one without it, nor the other steps" \
	bash -c '[[ -z "$1$2$3$4" ]]' _ "$(fStepAt suite 1 0 engine)" "$(fStepAt suite 0 1 engine)" "$(fStepAt '' 1 1 engine)" "$(fStepAt suite 1 1 gate)$(fStepAt suite 1 1 asan)"

if ((failures)); then echo "${failures} check(s) failed"; exit 1; fi
echo "PASS df2ss step memory cap"

##	History:
##		- 20261109 JC: Created.
