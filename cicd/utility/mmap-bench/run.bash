#!/usr/bin/env bash
# Throughput with the minimap on against off: flood one screen-sized terminal
# with 32 MiB and time how long the shell takes to get through it.
#
#	run.bash <silkterm-binary> on|off <tag>
#
# Needs the headless display up (gui-headless.bash start, CICD_HEADLESS_DISPLAY=:98)
# and an OPTIMIZED binary - a debug build measures the debug build.
# Everything it writes goes under target/, which is not tracked: the flood file
# alone is 32 MiB.

##	History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

set -uo pipefail

scriptDir="$(cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")" && pwd)"; readonly scriptDir
repoDir="$(cd "${scriptDir}/../../.." && pwd)"; readonly repoDir
readonly workDir="${repoDir}/target/mmap-bench"
readonly floodMib=32

fMain(){
	local -r binary="${1:?binary}" mode="${2:?on|off}" tag="${3:?tag}"
	mkdir -p "${workDir}"
	fMakeFlood
	fWriteScene
	rm -f "${workDir}/time.txt"
	cp "${scriptDir}/cfg-${mode}.shcl" "${workDir}/run-${mode}.shcl"
	XAUTHORITY="$(fXauthority)"; export XAUTHORITY
	DISPLAY=:98 LIBGL_ALWAYS_SOFTWARE=1 SILK_PERF=1 timeout 300 "${binary}" \
		--config "${workDir}/run-${mode}.shcl" \
		--shell "/bin/dash ${workDir}/scene.sh" \
		> "${workDir}/out-${tag}-${mode}.log" 2>&1
	fReport "${tag}" "${mode}"
}

# 32 MiB of plain rows, made once and kept.
fMakeFlood(){
	[[ -s "${workDir}/flood.txt" ]] && return 0
	python3 -c "
import sys
row = ('x' * 78 + '\n').encode()
want = ${floodMib} * 1024 * 1024
out = sys.stdout.buffer
written = 0
while written < want:
	out.write(row)
	written += len(row)
" > "${workDir}/flood.txt"
}

# The scene has to name its own work directory, so it is written next to it.
fWriteScene(){
	cat > "${workDir}/scene.sh" <<-SCENE
		#!/bin/dash
		sleep 1
		stty size > ${workDir}/size.txt
		s=\$(date +%s.%N); cat ${workDir}/flood.txt; e=\$(date +%s.%N)
		echo "\$s \$e" > ${workDir}/time.txt
		sleep 0.5
	SCENE
}

fXauthority(){
	local -r current="/tmp/cicd-gui-headless-${USER}/Xauthority-98"
	[[ -f "${current}" ]] && { printf '%s' "${current}"; return 0 ;}
	printf '/tmp/rpd-gui-headless/Xauthority-98'
}

fReport(){
	local -r tag="${1}" mode="${2}"
	if [[ ! -s "${workDir}/time.txt" ]]; then
		echo "${tag} ${mode}: no timing - see ${workDir}/out-${tag}-${mode}.log" >&2
		return 1
	fi
	local start end
	read -r start end < "${workDir}/time.txt"
	python3 -c "
import sys
start, end = float(sys.argv[1]), float(sys.argv[2])
took = end - start
print(f'{sys.argv[3]} {sys.argv[4]}: {took:.2f}s {${floodMib}/took:.1f} MiB/s')
" "${start}" "${end}" "${tag}" "${mode}"
	cat "${workDir}/size.txt"
}

fMain "${@}"

##	History:
##		- 20260920 JC: Created.
