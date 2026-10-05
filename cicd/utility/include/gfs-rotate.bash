#!/bin/bash

#  shellcheck disable=2001  ## 'See if you can use ${variable//search/replace} instead.' Complains about good uses of sed.
#  shellcheck disable=2016  ## 'Expressions don't expand in single quotes, use double quotes for that.' I know, and I often want an explicit '$'.
#  shellcheck disable=2034  ## 'variable appears unused.' Complains about valid use of variable indirection (e.g. later use of local -n var=$1)
#  shellcheck disable=2046  ## 'Quote to prevent word-splitting.' (OK for integers.)
#  shellcheck disable=2086  ## 'Double quote to prevent globbing and word splitting.' (OK for integers.)
#  shellcheck disable=2119  ## 'Use foo "$@" if function's $1 should mean script's $1.' Confusing and inapplicable.
#  shellcheck disable=2120  ## 'Foo references arguments, but none are ever passed.' Valid function argument overloading.
#  shellcheck disable=2128  ## 'Expanding an array without an index only gives the element in the index 0.' False hits on associative arrays.
#  shellcheck disable=2155  ## 'Declare and assign separately to avoid masking return values.' Cumbersome and unnecessary. For integers it's sometimes required to even come into existence for counters.
#  shellcheck disable=2162  ## 'read without -r will mangle backslashes.'
#  shellcheck disable=2178  ## 'Variable was used as an array but is now assigned a string.' False hits on associative arrays with e.g. 'local -n assocArray=$1'.
#  shellcheck disable=2181  ## 'Check exit code directly, not indirectly with $?.'
#  shellcheck disable=2317  ## 'Can't reach.' (I.e. an 'exit' is used for debugging - and makes an unusable visual mess.)
#  shellcheck enable=require-variable-braces  ## Every expansion braced: "${var}", not "$var".
## shellcheck disable=2002  ## 'Useless use of cat.'
## shellcheck disable=2004  ## '$/${} is unnecessary on arithmetic variables.' Inappropriate complaining?
## shellcheck disable=2053  ## 'Quote the right-hand sid of = in [[ ]] to prevent glob matching.' Disable for Yoda Notation.
## shellcheck disable=2143  ## 'Use grep -q instead of echo | grep'

## Purpose:
##	- Reusable GFS (grandfather-father-son) file rotation. Source this and call:
##		gfs_rotate <dir> <prefix> <ext>
##	- Keeps a bounded, time-spread set of files and prunes the rest: the newest of
##	  each recent hour/day/week/month/year, plus the last N most recent ("frequent"),
##	  plus the very first (kept forever) - about 30 files total.
##	- Period roles are RETROSPECTIVE: a file is tagged hour/day/week/month/year only
##	  once that period has ended and it is the last file in it; until then it stays
##	  "frequent".
##	- Kept files are renamed to a canonical, naturally-sorting name:
##		<prefix>_<YYYYmmDD-HHMMSS>_<role>.<ext>
##	- The constant <prefix> is first and the sortable timestamp second, so a plain
##	  directory listing is chronological. Pre-existing files that don't follow the
##	  convention are conformed: the timestamp is parsed from the name, else taken
##	  from the file mtime. Re-running is idempotent (already-canonical files are left
##	  alone until their role actually changes).
##	- Retention is tunable via env (defaults sum to ~30):
##		GFS_KEEP_FREQUENT, GFS_KEEP_HOURLY, GFS_KEEP_DAILY, GFS_KEEP_WEEKLY, GFS_KEEP_MONTHLY,
##		GFS_KEEP_YEARLY. GFS_NOW (epoch seconds) overrides "now" for testing.

##	History: At bottom of script.

##	Copyright (c) 2026 Bubbles
##	Licensed under The MIT License (MIT). Full text at:
##		https://mit-license.org/
##	SPDX-License-Identifier: MIT


## Globals for other modules to consume. For linter support, modules may
## redefine these as required interfaces, but only AFTER this module is loaded.
[[ -v ERRNUM_MSG_ALREADY_SHOWN    ]] || declare -gri ERRNUM_MSG_ALREADY_SHOWN=3

## Dates are done with printf's %(...)T and arithmetic rather than date, since
## a fork per file per field was most of the run time.

## Set $1 to strftime format $2 of epoch $3. printf reads -1 and -2 as "now" and
## "shell start", so those two seconds of 1969 go to date.
_gfs_fmt(){
	if [[ "$3" == -1 || "$3" == -2 ]]; then printf -v "$1" '%s' "$(date -d "@$3" "+$2")"
	else printf -v "$1" "%($2)T" "$3"; fi
}

## Set $1 to the seconds from the epoch to date $2 (YYYYmmDD) at time $3
## (HHMMSS), all read as UTC. Any digits do; the caller checks the result.
_gfs_civil(){
	local -i y=$((10#${2:0:4})) m=$((10#${2:4:2})) d=$((10#${2:6:2})) era yoe doy doe
	y=$((y - (m <= 2))); era=$(( (y >= 0 ? y : y - 399) / 400 )); yoe=$((y - era * 400))
	doy=$(( (153 * (m > 2 ? m - 3 : m + 9) + 2) / 5 + d - 1 )); doe=$((yoe * 365 + yoe / 4 - yoe / 100 + doy))
	printf -v "$1" '%s' "$(( (era * 146097 + doe - 719468) * 86400 + 10#${3:0:2} * 3600 + 10#${3:2:2} * 60 + 10#${3:4:2} ))"
}

## Set $1 to the epoch of local time $2 $3 (YYYYmmDD HHMMSS), or to "" when there
## is no such time: a bad date, or one skipped by a clock change. It steps the way
## glibc's mktime does from a fresh start, so a time a clock change repeats gets
## the same one of its two instants that date -d gives.
_gfs_epoch(){
	local -n epoch_x5q="$1"; local -i wall e i; local s
	_gfs_civil wall "$2" "$3"; e=wall; epoch_x5q=""
	for i in 1 2 3; do
		_gfs_fmt s '%Y%m%d%H%M%S' "${e}"; _gfs_civil s "${s:0:8}" "${s:8:6}"
		e=$((e + wall - s))
		_gfs_fmt s '%Y%m%d%H%M%S' "${e}"
		if [[ "${s}" == "$2$3" ]]; then epoch_x5q="${e}"; return 0; fi
	done
	return 0
}

## Set $2 to the epoch and $3 to <YYYYmmDD-HHMMSS> for file $1, from its name if
## it has a date, else from its mtime.
_gfs_ts(){
	local -n tsEpoch_w2j="$2" tsCanon_w2j="$3"
	local base="${1##*/}" d="" t="000000" epoch=""
	if [[ "${base}" =~ (19|20)([0-9]{2})([0-9]{2})([0-9]{2})[-_]?([0-9]{2})([0-9]{2})([0-9]{2}) ]]; then
		d="${BASH_REMATCH[1]}${BASH_REMATCH[2]}${BASH_REMATCH[3]}${BASH_REMATCH[4]}"
		t="${BASH_REMATCH[5]}${BASH_REMATCH[6]}${BASH_REMATCH[7]}"
	elif [[ "${base}" =~ (19|20)([0-9]{2})([0-9]{2})([0-9]{2}) ]]; then
		d="${BASH_REMATCH[1]}${BASH_REMATCH[2]}${BASH_REMATCH[3]}${BASH_REMATCH[4]}"
	fi
	## An impossible date that still matches the pattern falls through to the mtime.
	## Only a name with no date at all costs a fork here, and a kept file is renamed
	## to one with a date.
	[[ -z "${d}" ]] || _gfs_epoch epoch "${d}" "${t}"
	[[ -n "${epoch}" ]] || epoch="$(stat -c %Y "$1" 2>/dev/null || true)"
	[[ -n "${epoch}" ]] || printf -v epoch '%(%s)T' -1
	tsEpoch_w2j="${epoch}"; _gfs_fmt tsCanon_w2j '%Y%m%d-%H%M%S' "${epoch}"
}

## Set variable $1 to the count in environment variable $2, or to default $3.
## The count reaches arithmetic, where bash runs a command substitution hidden
## in an array subscript, so anything but plain digits gets the default.
_gfs_count(){
	local val="${!2:-}"
	if [[ -z "${val}" ]]; then
		val="$3"
	elif [[ ! "${val}" =~ ^(0|[1-9][0-9]{0,8})$ ]]; then
		printf '  rotate: %s is not a count; using %s\n' "$2" "$3" >&2
		val="$3"
	fi
	printf -v "$1" '%s' "${val}"
}

gfs_rotate(){
	local dir="$1" prefix="$2" ext="$3"
	local now="${GFS_NOW:-}"; [[ -n "${now}" ]] || printf -v now '%(%s)T' -1
	local kFreq kHour kDay kWeek kMonth kYear
	_gfs_count kFreq  GFS_KEEP_FREQUENT 10; _gfs_count kHour GFS_KEEP_HOURLY 4
	_gfs_count kDay   GFS_KEEP_DAILY    5;  _gfs_count kWeek GFS_KEEP_WEEKLY 4
	_gfs_count kMonth GFS_KEEP_MONTHLY  4;  _gfs_count kYear GFS_KEEP_YEARLY 2

	## Glob with nullglob so no match yields an empty list; restore the caller's setting.
	local hadNullglob=0; shopt -q nullglob && hadNullglob=1
	shopt -s nullglob; local cands=("${dir}/${prefix}"_*."${ext}"); ((hadNullglob)) || shopt -u nullglob
	((${#cands[@]})) || return 0

	## "epoch<TAB>canon<TAB>path", oldest first.
	## Not 'epoch': _gfs_ts has a local of that name, which its nameref would find first.
	local -a items=(); local file fileEpoch canon
	for file in "${cands[@]}"; do
		_gfs_ts "${file}" fileEpoch canon
		[[ -n "${fileEpoch}" ]] && items+=("${fileEpoch}"$'\t'"${canon}"$'\t'"${file}")
	done
	((${#items[@]})) || return 0
	mapfile -t items < <(printf '%s\n' "${items[@]}" | sort -n)

	## Latest file in each *completed* period (the still-open current one is skipped
	##   so it can't be tagged yet - that is what makes the roles retrospective).
	## One format per file, "YYYYmmDDHH GGGGVV", cut into the five period keys.
	local cur curH curD curW curM curY
	_gfs_fmt cur '%Y%m%d%H %G%V' "${now}"
	curH="${cur:0:10}"; curD="${cur:0:8}"; curW="${cur:11}"; curM="${cur:0:6}"; curY="${cur:0:4}"
	local -A perHour perDay perWeek perMonth perYear
	local it periodKeys keyHour keyDay keyWeek keyMonth keyYear
	# shellcheck disable=SC2034  # perHour..perYear are populated here, read later through the namerefs
	for it in "${items[@]}"; do
		fileEpoch="${it%%$'\t'*}"
		_gfs_fmt periodKeys '%Y%m%d%H %G%V' "${fileEpoch}"
		keyHour="${periodKeys:0:10}"; keyDay="${periodKeys:0:8}"; keyWeek="${periodKeys:11}"
		keyMonth="${periodKeys:0:6}"; keyYear="${periodKeys:0:4}"
		[[ "${keyHour}"  != "${curH}" ]] && perHour["${keyHour}"]="${it}"
		[[ "${keyDay}"   != "${curD}" ]] && perDay["${keyDay}"]="${it}"
		[[ "${keyWeek}"  != "${curW}" ]] && perWeek["${keyWeek}"]="${it}"
		[[ "${keyMonth}" != "${curM}" ]] && perMonth["${keyMonth}"]="${it}"
		[[ "${keyYear}"  != "${curY}" ]] && perYear["${keyYear}"]="${it}"
	done

	## Assign the coarsest role to each kept file:
	##   first > year > month > week > day > hour > frequent (newest tagged "latest")
	## First-set wins, so process coarsest first.
	local -A role; role["${items[0]}"]="first"
	local spec rn cnt nk i
	for spec in "year perYear ${kYear}" "month perMonth ${kMonth}" "week perWeek ${kWeek}" "day perDay ${kDay}" "hour perHour ${kHour}"; do
		# shellcheck disable=SC2086
		set -- ${spec}; rn="$1"; cnt="$3"; local -n arr="$2"
		local -a keys=("${!arr[@]}")
		if ((${#keys[@]})); then
			mapfile -t keys < <(printf '%s\n' "${keys[@]}" | sort)
			nk=${#keys[@]}
			for ((i = nk>cnt ? nk-cnt : 0; i<nk; i++)); do
				it="${arr[${keys[i]}]}"; [[ -z "${role[${it}]:-}" ]] && role["${it}"]="${rn}"
			done
		fi
		unset -n arr
	done

	## Frequent: most recent kFreq not already claimed by a coarser role. The
	## single newest file is labeled "latest" instead - a stable, naturally-
	## sorting pointer to the most recent file (no separate "<prefix>-latest" copy).
	local nItems=${#items[@]}
	for ((i = nItems>kFreq ? nItems-kFreq : 0; i<nItems; i++)); do
		[[ -z "${role[${items[i]}]:-}" ]] && role["${items[i]}"]="frequent"
	done
	[[ "${role[${items[nItems-1]}]:-}" == "first" ]] || role["${items[nItems-1]}"]="latest"

	## Prune the unrole'd; rename the kept to canonical (no-op if already canonical).
	local rest r want
	for it in "${items[@]}"; do
		rest="${it#*$'\t'}"; canon="${rest%%$'\t'*}"; file="${rest#*$'\t'}"
		r="${role[${it}]:-}"
		if [[ -z "${r}" ]]; then
			rm -f "${file}"; printf '  rotate: pruned %s\n' "${file##*/}"
		else
			want="${dir}/${prefix}_${canon}_${r}.${ext}"
			if [[ "${file}" != "${want}" ]]; then
				[[ -e "${want}" ]] && continue   # never clobber a same-name collision
				mv -f "${file}" "${want}"; printf '  rotate: %s -> %s\n' "${file##*/}" "${want##*/}"
			fi
		fi
	done
}

## Check if sourced
declare -i isSourced_t6wq5=0; [[ "${BASH_SOURCE[0]}" == "${0}" ]] || isSourced_t6wq5=1
((isSourced_t6wq5)) || { echo -e "\nError in $(basename "${BASH_SOURCE[0]}"): This script is meant to be 'sourced' from within another script.\n"; exit "${ERRNUM_MSG_ALREADY_SHOWN}"; }


##	History:
##		- 2026-06-05: Created.
##		- 2026-07-25: Harmonized every copy of this file to one identical file.
##		- 2026-09-15: A GFS_KEEP_* value that is not a plain count gets its
##		  default. It reached arithmetic, which could run a command.
##		- 2026-10-04: Dates are read and written by printf and arithmetic, not a
##		  date fork per file and field. Same names and output as before.
##		- 2026-10-04: Every expansion braced, and shellcheck enforces it. The loop
##		  variables in gfs_rotate have real names. No change in behavior.
