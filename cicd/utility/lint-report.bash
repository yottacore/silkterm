#!/usr/bin/env bash

#  shellcheck disable=2155  ## 'Declare and assign separately.' Cumbersome and unnecessary here.
#  shellcheck disable=2086  ## 'Double quote to prevent word splitting.' OK for integer flags.

##	Purpose:
##		Surface warnings from the newest cicd run log. cicd tees each run to
##		cicd/artifacts/lint/run_<ts>.log (gitignored); this greps out the warning /
##		advisory lines so they can be addressed. Two modes: plain (print warnings
##		from the newest log) and --check (print only when that log is newer than the
##		one last recorded in a local marker, then record it - meant for a per-session
##		startup look that is a no-op until a new cicd run has happened).
##	Syntax:
##		lint-report.bash [--check] [--force] [--no-mark] [--dir DIR] [--file LOG]
##		  --check     gate on the .lint-seen marker; print SEEN and stop if not newer
##		  --force     with --check, report even if already seen
##		  --no-mark   with --check, do not update the marker
##		  --dir DIR   log directory (default: cicd/artifacts/lint next to this script)
##		  --file LOG  report on this log instead of the newest in DIR
##	Exit: 0 normal, 2 skip (no dir / no logs).
##	History: At bottom of script.

##	Copyright (c) 2026 Bubbles
##	Licensed under The MIT License (MIT). Full text at:
##		https://mit-license.org/
##	SPDX-License-Identifier: MIT


set -Eeuo pipefail

meDir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
dir="${meDir}/../artifacts/lint"        ## this script lives in cicd/utility -> cicd/artifacts/lint
file=""; check=0; force=0; noMark=0

while (($#)); do case "$1" in
	--check)    check=1; shift ;;
	--force)    force=1; shift ;;
	--no-mark)  noMark=1; shift ;;
	--dir)      dir="${2:-}"; shift 2 ;;
	--file)     file="${2:-}"; shift 2 ;;
	-h|--help)  grep -E '^##' "$0" | sed 's/^##\t\?//'; exit 0 ;;
	*) echo "lint-report: unknown option: $1" >&2; exit 2 ;;
esac; done

fSkip() { echo "lint-report: $1" >&2; exit 2; }   ## 2 = non-fatal skip (matches the cicd profiler stage)

##	Newest run_<ts>[_role].log by timestamp - the role suffix, if gfs rotation added
##	one, is ignored (the timestamp is stable).
fNewest() {
	local d="$1" best="" newest="" f b t
	for f in "$d"/run_*.log; do
		[[ -e "$f" ]] || continue
		b="$(basename "$f")"; t="${b#run_}"; t="${t%%_*}"; t="${t%.log}"
		[[ "$t" > "$best" ]] && { best="$t"; newest="$f"; }
	done
	[[ -n "$newest" ]] || return 1
	printf '%s\t%s\n' "$best" "$newest"
}

if [[ -n "$file" ]]; then
	[[ -f "$file" ]] || fSkip "no such file: $file"
	log="$file"; b="$(basename "$file")"; ts="${b#run_}"; ts="${ts%%_*}"; ts="${ts%.log}"
else
	[[ -d "$dir" ]] || fSkip "no log dir: $dir"
	nb="$(fNewest "$dir")" || fSkip "no run logs in $dir"
	ts="${nb%%$'\t'*}"; log="${nb#*$'\t'}"
fi

marker="${dir}/.lint-seen"
if ((check)) && ((! force)); then
	seen=""; [[ -f "$marker" ]] && seen="$(tr -d '[:space:]' < "$marker" 2>/dev/null)"
	if [[ -n "$ts" && -n "$seen" && ! "$ts" > "$seen" ]]; then
		echo "SEEN $(basename "$log")  (nothing newer than $seen)"; exit 0
	fi
fi

##	Record the marker now (before printing the body) so a caller that pipes stdout
##	to head/less and closes it early still records the look.
if ((check)) && ((! noMark)) && [[ -n "$ts" ]]; then
	printf '%s\n' "$ts" > "$marker" 2>/dev/null || echo "lint-report: could not write marker: $marker" >&2
fi

##	Distil warnings/advisories. rustc/clippy/cargo-deny all say "warning"; deny adds
##	RUSTSEC ids + vulnerable/unmaintained/yanked. Hard errors only appear in a failed
##	run's log (a passing run aborts on the first error). Drop the "0 warnings" noise,
##	and test result lines, whose test names can hold any of these words: cargo test's
##	"<status> <ID> <name>" (test-id.py --annotate) and a script's "[ OK: <what> (<ID>) ]".
warns="$(grep -inE 'warning|rustsec-|vulnerab|unmaintained|yanked|error\[' "$log" 2>/dev/null \
	| grep -viE 'generated 0 warnings|: 0 warnings|no warnings|0 warnings emitted' \
	| grep -vE '^[0-9]+:((ok|FAILED|ignored) +([0-9A-Za-z]{7}|-------)  [^ ]|\[ OK: .* \([0-9A-Za-z]{7}\) \]$)' || true)"
if [[ -n "$warns" ]]; then n=$(printf '%s\n' "$warns" | grep -c .); else n=0; fi

tag="FLAG"; ((check)) && tag="NEW"
if ((n)); then
	echo "${tag} $(basename "$log")  (${n} warning line(s))"
	echo
	printf '%s\n' "$warns"
else
	echo "CLEAN $(basename "$log")  (0 warnings)"
fi


##	Script history:
##		- 20260709: Created.
##		- 20261003: Test result lines no longer count as warnings.
