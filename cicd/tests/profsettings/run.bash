#!/usr/bin/env bash
#  shellcheck disable=2016  ## 'Expressions don't expand in single quotes.' They expand in the bash -c they go to.
#  shellcheck disable=2034  ## 'appears unused.' The lifted fRunProfiler reads them.
#  shellcheck disable=2154  ## 'referenced but not assigned.' The lifted fRunProfiler assigns them, inside an eval.

##	- Purpose:
##		The profiler's app runs on a settings tree of its own in the test run
##		folder, never the live one, with Choose automatically off at Max and tmux
##		off, so one flamegraph compares with the last whoever runs it. The live
##		file's tmux setting once put the workload in tmux and doubled the text
##		shaping share. This runs cicd.bash's own fRunProfiler with a stand-in app
##		that writes down what it was handed.
##	- Test ID: df4v4
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

set -euo pipefail
meDir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use
root="$(cd "${meDir}/../../.." && pwd)"
engine="${root}/cicd/cicd.bash"

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }
fLift(){ sed -n "/${1}/,/${2}/p" "${engine}"; }

work="$(mktemp -d "${TMPDIR:-/tmp}/silk-profsettings.XXXXXX")"
trap 'rc=$?; fTestDir_Rm "${work}" || true; fTestDir_End "${rc}"' EXIT

## The stand-in app: what it was handed, and the settings file it would read.
seen="${work}/seen"
cat >"${work}/app.bash" <<-'APP'
	#!/usr/bin/env bash
	seen="${PROFSETTINGS_SEEN:?}"
	{
		printf 'config=%s\n' "${XDG_CONFIG_HOME:-}"
		printf 'data=%s\n'   "${XDG_DATA_HOME:-}"
		printf 'state=%s\n'  "${XDG_STATE_HOME:-}"
		printf 'cache=%s\n'  "${XDG_CACHE_HOME:-}"
		printf 'tmux=%s\n'   "${TMUX-unset}"
	} >"${seen}.env"
	cp -- "${XDG_CONFIG_HOME:-/nonexistent}/silkterm/config.shcl" "${seen}.shcl" 2>/dev/null || true
	printf '<svg total_samples="1"></svg>\n' >"${SILK_PROFILE_OUT:?}"
	APP
chmod +x "${work}/app.bash"
touch "${work}/workload.py"

eval "$(fLift '^fRunProfiler(){' '^}')"
fEcho(){ echo "    ${*}"; }
fEcho_Clean(){ echo "    ${*}"; }
fDie(){ echo "    DIE ${*}"; return 1; }
fRetryBuild(){ :; }
gfs_rotate(){ :; }
fCardDisplay_Start(){ cardDisplay=":fake"; cardSway="fake"; }
fCardDisplay_Stop(){ cardDisplay=""; cardSway=""; }

mkdir -p "${work}/tests"
out="$(
	export SILKTERM_TEST_DIR="${work}/tests" PROFSETTINGS_SEEN="${seen}" TMUX="/tmp/tmux-fake,1,0" TMUX_PANE="%1"
	unset SILK_CICD_IN_CONTAINER XDG_CONFIG_HOME XDG_DATA_HOME XDG_STATE_HOME XDG_CACHE_HOME
	PROFILE_ENABLE=1 PROFILE_STRICT=1 PROFILE_SECS=1 PROFILE_BIN="${work}/app.bash" PROFILE_PROFILE=x PROFILE_FEATURE=x PROFILE_WORKLOAD_ARGS=""
	absScript="${work}/workload.py" here="${work}" profileDir="${work}/profiling" stamp=20260101-000000
	fRunProfiler 2>&1
)" || true
fCheck "the stand-in app ran to the end" bash -c '[[ -s "$1.env" && "$2" == *"OK: flamegraph"* ]]' _ "${seen}" "${out}"

fSeen(){ sed -n "s/^${1}=//p" "${seen}.env" 2>/dev/null; }
fUnder(){ local -r dir="$(fSeen "${1}")"; [[ "${dir}" == "${work}/tests/"?* && -d "${dir}" ]]; }
fCheck "its settings folder is in the test run folder" fUnder config
fCheck "and so are its data, state and cache folders" bash -c 'for d in "$@"; do [[ "${d}" == "$0/"?* ]] || exit 1; done' "${work}/tests" "$(fSeen data)" "$(fSeen state)" "$(fSeen cache)"
fCheck "with no rating in its state folder" bash -c '[[ -d "$1" && ! -e "$1/silkterm/state.shcl" ]]' _ "$(fSeen state)"
fCheck "it runs outside tmux" test "$(fSeen tmux)" = "unset"

## The settings file it reads, by the nesting shcl gives it.
fValue(){ awk -v want="${1}" '
	/^[ \t]*#/ || !NF { next }
	{ depth = match($0, /[^\t]/) - 1; line = $0; sub(/^\t*/, "", line)
	  key = line; sub(/:.*/, "", key); val = line; sub(/^[^:]*:[ \t]*/, "", val)
	  path[depth] = key; full = path[0]; for (i = 1; i <= depth; i++) full = full "." path[i]
	  if (full == want) print val }' "${seen}.shcl" 2>/dev/null; }
fCheck "Choose automatically is off" test "$(fValue performance.automatic)" = "false"
fCheck "at Max" test "$(fValue performance.profile)" = "max"
fCheck "tmux is off for a plain launch" test "$(fValue shell.tmux.always_new_session)" = "false"
fCheck "and the file is stamped, so the launch doesn't convert it" grep -qE '^##[[:space:]]+Format[[:space:]]+[0-9]+$' "${seen}.shcl" 2>/dev/null

if ((failures)); then echo "${out}"; echo "${failures} failed"; exit 1; fi
echo "all passed"

##	History:
##		- 20261110: Created.
