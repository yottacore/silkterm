#!/usr/bin/env bash

##	Purpose:
##		The house Bash conventions that shellcheck does not hold on its own. Exits 1
##		and names each line that drifts.
##		- Every expansion braced, "${var}" (shellcheck's optional require-variable-braces).
##		- [[ ]] rather than [ ] in Bash (require-double-brackets; sh and dash scripts
##		  are exempt, since they have no [[ ]]).
##		- fCamelCase function names. A function named after a real command is a
##		  stand-in for it, as in a test, and keeps that name.
##	Syntax:
##		bash-style.bash [FILE...]   ## default: every first-party script in the repo
##	Notes:
##		Copies of scripts shared with other projects are skipped, since their
##		canonical versions live elsewhere: gfs-rotate.bash, x9ps1-git.bash,
##		n8git_backup-and-publish and runterm.
##	History: At bottom of script.

##	Copyright (c) 2026 Bubbles
##	Licensed under The MIT License (MIT). Full text at:
##		https://mit-license.org/
##	SPDX-License-Identifier: MIT


set -Eeuo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${root}"

if ((${#})); then
	files=("${@}")
else
	mapfile -t files < <(git ls-files '*.bash' '*.sh' utility/git-hooks/pre-commit utility/git-hooks/pre-push \
		| grep -vE '(^|/)(gfs-rotate|x9ps1-git)\.bash$' || true)
fi
((${#files[@]})) || { echo "bash-style: no scripts to check" >&2; exit 1; }

bad=0
shellcheck -o require-variable-braces,require-double-brackets -i SC2250,SC2292 -f gcc "${files[@]}" || bad=1

## One pass over every file; awk prints file:line:name for each definition not in fCamelCase.
named="$(awk '
	FNR == 1 { here = 0 }
	/^[[:space:]]*#/ { next }
	/<<-?[[:space:]]*["'"'"']?[A-Za-z_]+/ { here = 1; tag = $0; sub(/.*<<-?[[:space:]]*["'"'"']?/, "", tag); sub(/[^A-Za-z_].*/, "", tag); next }
	here { t = $0; sub(/^[[:space:]]+/, "", t); if (t == tag) here = 0; next }
	match($0, /^[[:space:]]*(function[[:space:]]+)?[A-Za-z_][A-Za-z0-9_]*[[:space:]]*\(\)[[:space:]]*(\{|$)/) {
		name = substr($0, RSTART, RLENGTH); sub(/^[[:space:]]*(function[[:space:]]+)?/, "", name); sub(/[[:space:]]*\(.*/, "", name)
		if (name !~ /^f[A-Z]/) print FILENAME ":" FNR ": " name
	}' "${files[@]}")"
while IFS= read -r line; do
	[[ -n "${line}" ]] || continue
	type -P "${line##* }" >/dev/null 2>&1 && continue
	echo "${line}: function name is not fCamelCase"
	bad=1
done <<<"${named}"

if ((bad)); then echo "bash-style: scripts drift from the house conventions (style-guide.md, Bash)" >&2; exit 1; fi
echo "bash-style: ${#files[@]} scripts clean"


##	History:
##		- 20261006: Created.
