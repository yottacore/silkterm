#!/usr/bin/env bash

#  shellcheck disable=2034  ## 'variable appears unused.' The namerefs in fSweep_MakeDir write the caller's variables.
#  shellcheck disable=2154  ## 'referenced but not assigned.' The globals are declared at the top, which shellcheck misses for a sourced file.

##	Purpose:
##		- What a pipeline run leaves behind, and taking it away once it is stale.
##		  Source this. It defines functions and runs nothing.
##		- Run folders: a test run folder, or the container's /tmp. Each one is
##		  written down in a ledger with the token in its owner mark. A later run
##		  removes the ones past the newest few still there, and only one whose
##		  mark still has the token the ledger wrote. A run taht passes has
##		  already removed its own.
##		- Cargo build dirs: everything built before a toolchain change, since
##		  cargo rebuilds all of it anyway; a dir the config no longer builds into;
##		  and an incremental cache its profile has not used for a while.
##		- Only what the pipeline made and can show it made. A dir has to be a real
##		  dir of this user's, and a build dir has to be cargo's, checked right
##		  before it goes.
##		- Zig caches: one emptied whole once it is over the cap, or once another
##		  Zig version filled it, going by the stamp it carries. Never part of
##		  one, since its manifests point into the rest.
##		- fSweep_Summary prints what went, on one line.
##	History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	Licensed under The MIT License (MIT). Full text at:
##		https://mit-license.org/
##	SPDX-License-Identifier: MIT

#  shellcheck source=cicd/utility/include/saferm.bash
source "$(dirname "${BASH_SOURCE[0]}")/saferm.bash"

##	The same mark the test run folders get (cicd/tests/_testdir.bash).
__sweepMark=".test_silkterm_owner"
declare -i __sweepBytes=0
declare -a __sweepKinds=()
declare -A __sweepCounts=()

##	fSweep_NotOurs <dir> <token>: why <dir> is not the run folder the token
##	names, or nothing when it is.
fSweep_NotOurs(){
	local -r dir="${1}" token="${2:-}" mark="${1}/${__sweepMark}"
	local line=""
	if [[ -L "${dir}" ]]; then echo "it is a link"
	elif [[ ! -d "${dir}" ]]; then echo "not a folder"
	elif [[ ! -O "${dir}" ]]; then echo "not this user's"
	elif [[ -L "${mark}" || ! -f "${mark}" ]]; then echo "no owner mark"
	else
		read -r line <"${mark}" || true
		[[ -n "${token}" && "${line}" == "${token}" ]] || echo "another run's owner mark"
	fi
}

##	fSweep_Count <what> <n>: counts something removed, under <what>.
fSweep_Count(){
	((${2} > 0)) || return 0
	[[ -n "${__sweepCounts[${1}]:-}" ]] || __sweepKinds+=("${1}")
	__sweepCounts[${1}]=$(( ${__sweepCounts[${1}]:-0} + ${2} ))
}

##	fSweep_Remove <dir> <what> [parent]: removes it through the guard and counts
##	it under <what>. The caller has checked it is ours. A run folder goes by the
##	temp guard; anything else names the <parent> it has to be under. The folders
##	above it are resolved first, since the target dir is often a link.
fSweep_Remove(){
	local -r what="${2}" parent="${3:-}"
	local dir="${1%/}" up="" kb="" err=""
	[[ "${dir}" == */* ]] || dir="./${dir}"
	up="$(realpath -e -- "${dir%/*}" 2>/dev/null)" && dir="${up}/${dir##*/}"
	kb="$(du -sk -- "${dir}" 2>/dev/null)" || true
	kb="${kb%%[[:space:]]*}"
	if [[ -n "${parent}" ]]; then err="$(fSafeRm_Under "$(realpath -e -- "${parent}" 2>/dev/null)" "${dir}" 2>&1)"
	else err="$(fSafeRm "${dir}" 2>&1)"; fi || { echo "cleanup: could not remove ${dir}${err:+: ${err}}" >&2; return 1; }
	[[ "${kb}" =~ ^[0-9]+$ ]] && __sweepBytes+=$((kb * 1024))
	fSweep_Count "${what}" 1
}

##	fSweep_Record <ledger> <dir> <token>
fSweep_Record(){
	mkdir -p -- "$(dirname -- "${1}")" || return 1
	printf '%s\t%s\n' "${2}" "${3}" >>"${1}"
}

##	fSweep_Ledger <ledger> <keep> <what>: removes the folders <ledger> names,
##	past the newest <keep> still there. One that is gone, or turns out not to be
##	ours, comes off the ledger. Lines are in the order the folders were made.
fSweep_Ledger(){
	local -r ledger="${1}" what="${3}"
	local -ri keep="${2}"
	[[ -f "${ledger}" && ! -L "${ledger}" ]] || return 0
	local -a lines=() kept=()
	local -i i held=0
	local dir token why
	mapfile -t lines <"${ledger}"
	for ((i = ${#lines[@]} - 1; i >= 0; i--)); do
		IFS=$'\t' read -r dir token <<<"${lines[i]}"
		[[ -n "${dir}" ]] || continue
		[[ -e "${dir}" || -L "${dir}" ]] || continue
		if ((held < keep)); then kept=("${lines[i]}" "${kept[@]}"); held+=1; continue; fi
		why="$(fSweep_NotOurs "${dir}" "${token}")"
		if [[ -n "${why}" ]]; then echo "cleanup: left ${dir} in place: ${why}" >&2; continue; fi
		fSweep_Remove "${dir}" "${what}" || kept=("${lines[i]}" "${kept[@]}")
	done
	if ((${#kept[@]})); then printf '%s\n' "${kept[@]}" >"${ledger}.part"; else : >"${ledger}.part"; fi
	mv -f -- "${ledger}.part" "${ledger}"
}

##	fSweep_MakeDir <parent> <prefix> <dir var> <token var>: a new 0700 folder,
##	<parent>/<prefix>_YYYYmmDD-HHMMSSNN, with an owner mark, made the way the
##	test run folders are. Never mkdir -p for the folder itself, which would
##	adopt one planted at the name.
fSweep_MakeDir(){
	local -n dirOut_q7w="${3}" tokenOut_q7w="${4}"
	local -r parent_q7w="${1}" prefix_q7w="${2}"
	local made_q7w=""
	local -i tries_q7w
	mkdir -p -- "${parent_q7w}" || return 1
	for ((tries_q7w = 1; tries_q7w <= 100; tries_q7w++)); do
		made_q7w="${parent_q7w%/}/${prefix_q7w}_$(date +%Y%m%d-%H%M%S%2N)"
		if mkdir -m 700 -- "${made_q7w}" 2>/dev/null; then
			tokenOut_q7w="${BASHPID}-$(date +%s%N)"
			echo "${tokenOut_q7w}" >"${made_q7w}/${__sweepMark}" || return 1
			dirOut_q7w="${made_q7w}"
			return 0
		fi
		sleep 0.01
	done
	echo "cleanup: could not make a folder under ${parent_q7w}" >&2
	return 1
}

##	fSweep_IsBuildDir <dir>: a real dir of this user's that cargo made, either a
##	profile dir (it has .fingerprint) or a target dir of its own (CACHEDIR.TAG
##	with cargo's signature, like the lint one).
fSweep_IsBuildDir(){
	local -r dir="${1}"
	local sig=""
	[[ -d "${dir}" && ! -L "${dir}" && -O "${dir}" ]] || return 1
	[[ -d "${dir}/.fingerprint" && ! -L "${dir}/.fingerprint" ]] && return 0
	[[ -f "${dir}/CACHEDIR.TAG" && ! -L "${dir}/CACHEDIR.TAG" ]] && read -r sig <"${dir}/CACHEDIR.TAG"
	[[ "${sig}" == "Signature: 8a477f597d28d172789f06886806bc55" ]]
}

##	fSweep_Toolchain <target dir> <key> <build dir...>: once the toolchain key
##	changes, the build dirs go, since every unit in them names the old rustc
##	and cargo rebuilds the lot anyway. No key on record yet removes nothing.
fSweep_Toolchain(){
	local -r target="${1}" key="${2}" keyFile="${1}/.cicd-toolchain"
	shift 2
	local old="" dir
	local -i failed=0
	[[ -n "${key}" && -d "${target}" ]] || return 0
	[[ -f "${keyFile}" && ! -L "${keyFile}" ]] && read -r old <"${keyFile}"
	[[ "${old}" != "${key}" ]] || return 0
	if [[ -n "${old}" ]]; then
		for dir in "${@}"; do
			fSweep_IsBuildDir "${dir}" || continue
			fSweep_Remove "${dir}" "builds from an older toolchain" "${target}" || failed=1
		done
	fi
	((failed)) || printf '%s\n' "${key}" >"${keyFile}"
}

##	fSweep_OldBuildDirs <target dir> <build dir...>: a build dir the last run
##	wrote down that this one does not name, such as a cross target since
##	dropped, goes, and so does its parent once that is empty. Then the list is
##	written down for the next run. Only dirs under <target dir>.
fSweep_OldBuildDirs(){
	local -r target="${1%/}" listFile="${1%/}/.cicd-build-dirs"
	shift
	local -A now=()
	local -a rels=() before=()
	local dir rel parent
	for dir in "${@}"; do
		[[ "${dir}" == "${target}/"* ]] || continue
		rel="${dir#"${target}/"}"
		now[${rel}]=1; rels+=("${rel}")
	done
	[[ -d "${target}" ]] || return 0
	if [[ -f "${listFile}" && ! -L "${listFile}" ]]; then mapfile -t before <"${listFile}"; fi
	for rel in "${before[@]}"; do
		[[ -n "${rel}" && "${rel}" != /* && "/${rel}/" != *"/../"* && -z "${now[${rel}]:-}" ]] || continue
		dir="${target}/${rel}"
		fSweep_IsBuildDir "${dir}" || continue
		fSweep_Remove "${dir}" "build dirs no longer built" "${target}" || { rels+=("${rel}"); continue; }
		parent="${dir%/*}"
		[[ "${parent}" == "${target}" ]] || rmdir -- "${parent}" 2>/dev/null || true
	done
	if ((${#rels[@]})); then printf '%s\n' "${rels[@]}" >"${listFile}"; else : >"${listFile}"; fi
}

##	fSweep_Incremental <days> <build dir...>: rustc keeps one incremental cache
##	per crate and build setup, and never drops a setup it stopped using. One
##	not touched for <days> before the newest in the same profile goes. Measured
##	from the newest rather than now, so a pipeline left alone for a month does
##	not throw away the caches it is about to use.
fSweep_Incremental(){
	local -ri days="${1}"
	shift
	local dir inc mtime name uid
	local -i newest cutoff
	local -a found=()
	uid="$(id -u)"
	for dir in "${@}"; do
		fSweep_IsBuildDir "${dir}" || continue
		while IFS= read -r inc; do
			[[ -d "${inc%/incremental}/.fingerprint" && ! -L "${inc}" ]] || continue
			mapfile -t found < <(find "${inc}" -mindepth 1 -maxdepth 1 -type d -user "${uid}" -printf '%T@ %f\n' 2>/dev/null | sort -rn)
			((${#found[@]})) || continue
			newest="${found[0]%%.*}"
			cutoff=$((newest - days * 86400))
			for name in "${found[@]:1}"; do
				mtime="${name%%.*}"; name="${name#* }"
				[[ "${name}" =~ ^[A-Za-z0-9_]+-[0-9a-z]+$ ]] || continue
				((mtime < cutoff)) || continue
				fSweep_Remove "${inc}/${name}" "unused incremental caches" "${dir}" || true
			done
		done < <(find "${dir}" -maxdepth 3 -type d -name incremental -prune 2>/dev/null)
	done
}

##	fSweep_ZigCache <dir> <zig version> <cap MB>: a Zig cache goes whole when the
##	stamp in it names another Zig, or it is over <cap MB>. One with no stamp yet
##	gets this version's. The cap is on apparent size, as for the cache volumes,
##	since ZFS and btrfs report compressed blocks and the answer has to be the
##	same on any filesystem.
fSweep_ZigCache(){
	local -r dir="${1%/}" ver="${2}" stampFile="${1%/}/.cicd-zig-version"
	local -ri capBytes=$(( ${3} * 1048576 ))
	local old="" bytes=""
	[[ -n "${ver}" && -d "${dir}" && ! -L "${dir}" && -O "${dir}" ]] || return 0
	[[ -f "${stampFile}" && ! -L "${stampFile}" ]] && read -r old <"${stampFile}"
	if [[ -n "${old}" && "${old}" != "${ver}" ]]; then fSweep_Remove "${dir}" "zig caches" "${dir%/*}"; return; fi
	bytes="$(du -sb -- "${dir}" 2>/dev/null)" || true
	bytes="${bytes%%[[:space:]]*}"
	if [[ "${bytes}" =~ ^[0-9]+$ ]] && ((bytes > capBytes)); then fSweep_Remove "${dir}" "zig caches" "${dir%/*}"; return; fi
	[[ -n "${old}" ]] || printf '%s\n' "${ver}" >"${stampFile}"
}

##	fSweep_Summary: one line, what went and about what it freed.
fSweep_Summary(){
	local what list="" size=""
	if ((! ${#__sweepKinds[@]})); then echo "cleanup: nothing stale"; return 0; fi
	for what in "${__sweepKinds[@]}"; do list+="${list:+, }${__sweepCounts[${what}]} ${what}"; done
	if ((__sweepBytes >= 1073741824)); then size="$(awk -v b="${__sweepBytes}" 'BEGIN { printf "%.1f GB", b / 1073741824 }')"
	elif ((__sweepBytes >= 1048576)); then size="$(( (__sweepBytes + 524288) / 1048576 )) MB"
	else size=""; fi
	echo "cleanup: removed ${list}; ${size:+about }${size:-under 1 MB} freed"
}

##	History:
##		- 20261019 JC: Created.
##		- 20261025 JC: Zig caches.
##		- 20261026 JC: The Zig cap goes by apparent size.
##		- 20261028 JC: Removals go through the guard in saferm.bash.
