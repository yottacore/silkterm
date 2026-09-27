#!/usr/bin/env bash

##	- Purpose:
##		cicd/tool-pins.txt is the one list of pinned helper tools. Both pipelines
##		have to read it, each taking the tools marked for its platform or both,
##		and the build docs have to quote the versions it pins. zig was pinned at
##		0.16 while build.md still said 0.13.
##	- Test ID: Er2UgYB
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

set -euo pipefail
meDir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "${meDir}/../../.." && pwd)"
pins="${root}/cicd/tool-pins.txt"

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }

work="$(mktemp -d "${TMPDIR:-/tmp}/silk-pins.XXXXXX")"
trap 'rm -rf "${work}"' EXIT

## "name|version" for each pin whose platform is one of $2..., straight from the file.
fListed(){ local -r file="${1}"; shift; awk -F'|' -v want=" $* " 'NF == 4 && $1 !~ /^#/ && index(want, " " $3 " ") { print $1 "|" $2 }' "${file}" | sort; }
## What config.bash makes of the list beside it.
fBashPins(){ bash -c 'source "$1" && for p in "${TOOL_PINS[@]}"; do n="${p%%|*}"; r="${p#*|}"; echo "${n}|${r%%|*}"; done' _ "${1}/config.bash" | sort; }
## What cicd-win.ps1 makes of it.
fWinPins(){ pwsh -NoProfile -NonInteractive -File "${meDir}/_pins.ps1" -Pins "${1}" -Pipeline "${root}/cicd/cicd-win.ps1" | tr -d '\r' | sort; }

## The docs: a version quoted right after a pinned tool's name is its pin, or
## the start of it ("zig 0.16" for 0.16.0), and zig's is quoted at all. Read as
## "zig 0.16", "zig --version 0.16.0" or "zig-x86_64-linux-0.16.0".
fDocsQuotePins(){
	local name ver quoted doc bad=0 zigQuotes=0
	while IFS='|' read -r name ver _; do
		[[ -z "${name}" || "${name}" == \#* ]] && continue
		for doc in build.md prerequisites.md; do
			while read -r quoted; do
				[[ -n "${quoted}" ]] || continue
				[[ "${name}" == zig ]] && zigQuotes=$((zigQuotes + 1))
				if [[ "${quoted}" != "${ver}" && "${ver}" != "${quoted}".* ]]; then echo "    ${doc} quotes ${name} ${quoted}, pinned ${ver}"; bad=1; fi
			done <<<"$(grep -oE "(^|[^[:alnum:]-])${name}( --version | v?|-[[:alnum:]_]+-[[:alnum:]_]+-)[0-9]+\.[0-9]+(\.[0-9]+)*" "${root}/${doc}" \
				| grep -oE '[0-9]+\.[0-9]+(\.[0-9]+)*$' || true)"
		done
	done <"${pins}"
	((zigQuotes)) || { echo "    the docs quote no zig version"; bad=1; }
	return "${bad}"
}
fCheck "the build docs quote the pinned versions" fDocsQuotePins

if ! command -v pwsh >/dev/null 2>&1; then
	echo "  skip cicd-win.ps1's reading of the pins (no pwsh)"
else
	## The real list.
	fBashPins "${root}/cicd" >"${work}/bash.txt"
	fWinPins "${pins}" >"${work}/win.txt"
	fListed "${pins}" linux both >"${work}/want-bash.txt"
	fListed "${pins}" windows both >"${work}/want-win.txt"
	fListed "${pins}" both >"${work}/want-both.txt"
	fCheck "cicd.bash takes the linux and both pins" cmp -s "${work}/bash.txt" "${work}/want-bash.txt"
	fCheck "cicd-win.ps1 takes the windows and both pins" cmp -s "${work}/win.txt" "${work}/want-win.txt"
	fCheck "and the ones they share are the both pins, at one version" \
		bash -c 'comm -12 "$1" "$2" | cmp -s - "$3"' _ "${work}/bash.txt" "${work}/win.txt" "${work}/want-both.txt"
	fCheck "there is at least one" test -s "${work}/want-both.txt"

	## Pins added to the file reach the pipelines they are marked for and no
	## other, so neither can have gone back to a list of its own. The shipped
	## list has no windows-only pin, which leaves that filter untested above.
	mkdir -p "${work}/cicd"
	cp "${root}/cicd/config.bash" "${work}/cicd/"
	extra="${work}/cicd/tool-pins.txt"
	{
		cat "${pins}"
		echo "silk-both|4.5.6|both|silk-both --version"
		echo "silk-linux|1.2.3|linux|silk-linux --version"
		echo "silk-windows|7.8.9|windows|silk-windows --version"
	} >"${extra}"
	fBashPins "${work}/cicd" >"${work}/bash-extra.txt"
	fWinPins "${extra}" >"${work}/win-extra.txt"
	fCheck "new pins reach cicd.bash as marked" cmp -s "${work}/bash-extra.txt" <(fListed "${extra}" linux both)
	fCheck "and cicd-win.ps1 as marked" cmp -s "${work}/win-extra.txt" <(fListed "${extra}" windows both)
fi

if ((failures)); then echo "${failures} failed"; exit 1; fi
echo "all passed"

##	History:
##		- 20260926 JC: Created.
