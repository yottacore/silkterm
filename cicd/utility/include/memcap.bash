#!/usr/bin/env bash

#  shellcheck disable=2034  ## 'appears unused.' memCap and memCapArgs are for the script that sources this.

##	Purpose:
##		- The memory cap a pipeline step runs under, so one that runs away is
##		  killed alone and the desktop is not. Source this.
##		- CICD_MEM_CAP sets it: a docker size such as 24g, or none for no cap.
##		  Anything else stops the run before it starts anything. The default is
##		  a third of RAM, from 4g to 32g, or a step's own when it has one.
##		- fMemCap [default]: sets memCap to the cap, or to "" for none.
##		- fMemCap_DockerArgs [default]: sets memCapArgs to docker run's flags for
##		  it. No swap, and the container goes first when the box runs short.
##		- fMemCap_Scope <script> [args]...: runs the script again in a systemd
##		  user scope with the cap, no swap, and first in line for the OOM killer,
##		  when there is a systemd user manager. Elsewhere, and in the container,
##		  it does nothing. The scope covers this side's steps; a container is a
##		  child of dockerd, not of the scope, so it takes the flags as well.
##		- A step over the cap is killed, and exits 137.
##		- Why: on 2026-11-08 another project's CI container took 67 GB plus 18 GB
##		  of swap on b23, and earlyoom killed the whole desktop session first.
##	History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	Licensed under The MIT License (MIT). Full text at:
##		https://mit-license.org/
##	SPDX-License-Identifier: MIT

memCap=""; memCapArgs=()

fMemCap(){
	local cap="${CICD_MEM_CAP:-}" kb
	memCap=""
	[[ "${cap}" != none ]] || return 0
	if [[ -z "${cap}" && -n "${1:-}" ]]; then cap="${1}"
	elif [[ -z "${cap}" ]]; then
		kb="$(awk '/^MemTotal:/ { print $2 }' /proc/meminfo 2>/dev/null || true)"
		[[ "${kb}" =~ ^[0-9]+$ ]] || kb=0
		cap=$(( kb / 3 / 1048576 ))
		((cap <= 32)) || cap=32
		((cap >= 4)) || cap=4
		cap="${cap}g"
	fi
	## Never 0, which docker reads as no cap.
	if [[ ! "${cap}" =~ ^[1-9][0-9]*[kmgKMG]?$ ]]; then
		echo "[ FAILED: CICD_MEM_CAP '${cap}' isn't a size like 24g, or none ]" >&2
		return 1
	fi
	memCap="${cap}"
}

fMemCap_DockerArgs(){
	memCapArgs=()
	fMemCap "${@}" || return 1
	[[ -z "${memCap}" ]] || memCapArgs=(--memory "${memCap}" --memory-swap "${memCap}" --oom-score-adj 1000)
}

## SILK_CICD_MEM_SCOPE stays set for everything the run starts, since a scope
## started inside this one would be its sibling, outside the cap.
fMemCap_Scope(){
	local -r run="${SILK_SYSTEMD_RUN:-systemd-run}"
	fMemCap || return 1
	[[ -z "${SILK_CICD_MEM_SCOPE:-}" && -z "${SILK_CICD_IN_CONTAINER:-}" && -n "${memCap}" ]] || return 0
	command -v "${run}" >/dev/null 2>&1 && command -v choom >/dev/null 2>&1 || return 0
	"${run}" --user --scope --quiet true >/dev/null 2>&1 || return 0
	export SILK_CICD_MEM_SCOPE="${memCap}"
	## systemd takes the suffix in capitals.
	exec "${run}" --user --scope --quiet -p "MemoryMax=${memCap^^}" -p MemorySwapMax=0 choom -n 1000 -- "${BASH:-bash}" "${@}"
}

##	History:
##		- 20261109 JC: Created, so every step of the pipeline runs under a memory cap.
