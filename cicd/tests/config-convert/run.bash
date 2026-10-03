#!/usr/bin/env bash

##	- Purpose:
##		Converts config files in the old formats with the real program and checks
##		the converted file, the copy of the old one, and what the launch says
##		about settings it could not keep. Each launch runs with no display, so it
##		reads and converts the file and then stops where the window would open.
##			- A pre-nesting flat file, converted to the nested layout.
##			- A shcl 2.x file with no Format line, and one stamped Format 2, both
##			  converted in place with their comments.
##			- One shcl cannot migrate (a raw block that never closes) and one it
##			  cannot read (not UTF-8), each written new from the template with the
##			  settings that still read.
##		A second launch on each must change nothing. Where Xvfb is installed, one
##		launch on the private display checks that lost settings bring up the
##		notice window too.
##	- Syntax: run.bash [--bin PATH]   (default: the debug build, then release)
##	- Exit: 0 passed, 1 a check failed, 3 nothing ran (no binary).
##	- Test ID: ErgDpjX
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

set -euo pipefail
meDir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "${meDir}/../../.." && pwd)"
headless="${root}/cicd/utility/gui-headless.bash"

bin=""
while (($#)); do case "${1}" in
	--bin) bin="${2-}"; shift 2 ;;
	-h|--help) sed -n '/^##	- Purpose:/,/^##	- History:/p' "${BASH_SOURCE[0]}" | sed 's/^##	\{0,1\}//'; exit 0 ;;
	*) echo "unknown option: ${1} (try --help)" >&2; exit 2 ;;
esac; done
if [[ -z "${bin}" ]]; then
	targetDir="${CARGO_TARGET_DIR:-target}"
	[[ "${targetDir}" == /* ]] || targetDir="${root}/${targetDir}"
	for candidate in "${targetDir}/debug/silkterm" "${targetDir}/release/silkterm"; do
		if [[ -x "${candidate}" ]]; then bin="${candidate}"; break; fi
	done
fi
if [[ -z "${bin}" || ! -x "${bin}" ]]; then echo "  skip: no SilkTerm binary (build it first, or pass --bin)"; exit 3; fi
echo "  binary: ${bin}"

# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }

work="$(mktemp -d "${TMPDIR:-/tmp}/silk-convert.XXXXXX")"
noticePid=""; startedDisplay=0
fCleanup(){
	local -r rc="${1}"
	if [[ -n "${noticePid}" ]]; then fStopOurs "${noticePid}"; fi
	if ((startedDisplay)); then "${headless}" stop >/dev/null 2>&1 || true; fi
	rm -rf "${work}"
	fTestDir_End "${rc}"
}
trap 'fCleanup $?' EXIT

## Only the process launched here, and only while it is still this binary: the
## repo path says silkterm, and a dogfood copy may be running.
fStopOurs(){
	local -r pid="${1}"
	local want exe
	want="$(realpath -e "${bin}" 2>/dev/null || true)"
	exe="$(realpath -e "/proc/${pid}/exe" 2>/dev/null || true)"
	if [[ -z "${want}" || "${exe}" != "${want}" ]]; then return 0; fi
	kill "${pid}" 2>/dev/null || true
	local _
	for _ in {1..20}; do kill -0 "${pid}" 2>/dev/null || return 0; sleep 0.1; done
	kill -9 "${pid}" 2>/dev/null || true
}

## A home of its own, so the launch never reads or moves anything of the user's.
fLaunch(){  ## fLaunch <case dir> <stderr file>
	local -r dir="${1}" said="${2}"
	mkdir -p "${dir}/home"
	env -u DISPLAY -u WAYLAND_DISPLAY -u WAYLAND_SOCKET \
		HOME="${dir}/home" XDG_CONFIG_HOME="${dir}/home/.config" XDG_DATA_HOME="${dir}/home/.local/share" XDG_RUNTIME_DIR="${dir}/home" \
		timeout 60 "${bin}" --config "${dir}/config.shcl" >/dev/null 2>"${said}" || true
}

## What an active line sets at a dotted path, as written. Tab indents only, as
## the program writes them.
fGet(){  ## fGet <path> <file>
	WANT="${1}" awk '
		{ sub(/\r$/, "") }
		/^[ \t]*(#|$)/ { next }
		{
			depth = match($0, /[^\t]/) - 1
			line = substr($0, depth + 1)
			at = index(line, ":")
			if (at == 0) next
			key = substr(line, 1, at - 1)
			value = substr(line, at + 1)
			sub(/^[ \t]+/, "", value); sub(/[ \t]+$/, "", value)
			stack[depth] = key
			path = stack[0]
			for (i = 1; i <= depth; i++) path = path "." stack[i]
			if (path == ENVIRON["WANT"] && value != "") { print value; exit }
		}' "${2}"
}
fHas(){ [[ "$(fGet "${1}" "${3}")" == "${2}" ]]; }  ## fHas <path> <value> <file>
fCopies(){ find "${1}" -maxdepth 1 -name 'config_backup_*' -printf '%f\n' | sort; }
fIsCopyName(){ [[ "${1}" =~ ^config_backup_[0-9]{8}-[0-9]{6}_format-v2\.shcl$ ]]; }
fSaid(){ grep -qF -- "${1}" "${2}"; }
fNotSaid(){ ! grep -qE -- "${1}" "${2}"; }
fLacks(){ ! grep -qF -- "${1}" "${2}"; }
fUtf8(){ python3 -c 'import sys; open(sys.argv[1], encoding="utf-8").read()' "${1}" 2>/dev/null; }
fNotUtf8(){ ! fUtf8 "${1}"; }

## The checks every case shares. <how> is inplace (comments kept) or new (the
## template's layout); <lost> is what the launch should report.
fConverts(){  ## fConverts <case> <how> <lost>
	local -r name="${1}" how="${2}" lost="${3}"
	local -r dir="${work}/${name}"
	local -r file="${dir}/config.shcl"
	local copies=() copy=""
	cp -p "${file}" "${dir}/original"
	fLaunch "${dir}" "${dir}/said1.txt"
	fCheck "${name}: the launch did not panic" fNotSaid 'panicked' "${dir}/said1.txt"
	mapfile -t copies < <(fCopies "${dir}")
	fCheck "${name}: one copy of the old file" test "${#copies[@]}" = 1
	copy="${copies[0]:-none}"
	fCheck "${name}: named config_backup_<time>_format-v2.shcl" fIsCopyName "${copy}"
	fCheck "${name}: holding the old file byte for byte" cmp -s "${dir}/original" "${dir}/${copy}"
	fCheck "${name}: the file is in the current format" grep -qxF '##    Format   3' "${file}"
	fCheck "${name}: and is UTF-8" fUtf8 "${file}"
	fCheck "${name}: the launch says where the copy is" fSaid "the old file is kept at ${dir}/${copy}" "${dir}/said1.txt"
	case "${how}" in
		inplace)
			fCheck "${name}: converted in place, its comment kept" grep -qxF '## kept as written' "${file}"
			if [[ "${lost}" == 0 ]]; then
				fCheck "${name}: nothing reported lost" fNotSaid 'set a list in brackets|could not be carried' "${dir}/said1.txt"
			else
				fCheck "${name}: ${lost} lost setting(s) reported, naming the copy" fSaid ": ${lost} line(s) set a list in brackets, which the new format cannot hold; they are kept as written but set nothing. The old file is at ${dir}/${copy}." "${dir}/said1.txt"
			fi
			;;
		new)
			fCheck "${name}: written new from the template" test "$(head -n 1 "${file}")" = '# SilkTerm configuration file.'
			fCheck "${name}: its own comment only in the copy" fLacks 'kept as written' "${file}"
			if [[ "${lost}" == 0 ]]; then
				fCheck "${name}: nothing reported lost" fNotSaid 'set a list in brackets|could not be carried' "${dir}/said1.txt"
			else
				fCheck "${name}: ${lost} lost setting(s) reported, naming the copy" fSaid ": could not be converted in place, so a new file was written; ${lost} setting(s) could not be carried over. The old file is at ${dir}/${copy}." "${dir}/said1.txt"
			fi
			;;
	esac
	cp -p "${file}" "${dir}/converted"
	fLaunch "${dir}" "${dir}/said2.txt"
	fCheck "${name}: a second launch changes nothing" cmp -s "${dir}/converted" "${file}"
	fCheck "${name}: and keeps no second copy" test "$(fCopies "${dir}" | wc -l)" = 1
	fCheck "${name}: and reports nothing converted or lost" fNotSaid 'converted to SHCL|set a list in brackets|could not be carried' "${dir}/said2.txt"
}

## Fixtures are written line by line, since a heredoc indented with tabs loses
## them and the nesting with it.
fFixture(){  ## fFixture <case> <line>...
	local -r dir="${work}/${1}"; shift
	mkdir -p "${dir}"
	printf '%s\n' "${@}" >"${dir}/config.shcl"
}
tab=$'\t'

echo "pre-nesting flat file"
fFixture flat '## kept as written' 'font_size: 15' 'columns: 101' 'wallpaper_opacity: 0.3'
fConverts flat new 0
fCheck "flat: font_size is font.size" fHas font.size 15 "${work}/flat/config.shcl"
fCheck "flat: columns is window.columns" fHas window.columns 101 "${work}/flat/config.shcl"
fCheck "flat: wallpaper_opacity is wallpaper.opacity" fHas wallpaper.opacity 0.3 "${work}/flat/config.shcl"
fCheck "flat: the layout change keeps its own .bak" test -f "${work}/flat/config.shcl.bak"

echo "shcl 2.x, no Format line"
fFixture shcl2 '## kept as written' 'font:' "${tab}size: 15" "${tab}family:[One, Two]" 'window:' "${tab}columns: 101" 'wallpaper:' "${tab}image: C:\\\\pics\\\\sea.jpg"
fConverts shcl2 inplace 1
fCheck "shcl2: values kept" fHas window.columns 101 "${work}/shcl2/config.shcl"
fCheck "shcl2: a 2.x backslash path respelled" fHas wallpaper.image '"C:\\pics\\sea.jpg"' "${work}/shcl2/config.shcl"

echo "Format 2"
fFixture format2 '## kept as written' 'font:' "${tab}size: 15" 'window:' "${tab}columns: 101" '' '##    Format   2'
fConverts format2 inplace 0
fCheck "format2: values kept" fHas font.size 15 "${work}/format2/config.shcl"

echo "a file shcl cannot migrate"
fFixture nomigrate '## kept as written' 'font:' "${tab}size: 15" "${tab}family:[One, Two]" 'window:' "${tab}columns: 101" 'notes: ```' 'written before the block was closed'
fConverts nomigrate new 2
fCheck "nomigrate: font.size carried" fHas font.size 15 "${work}/nomigrate/config.shcl"
fCheck "nomigrate: window.columns carried" fHas window.columns 101 "${work}/nomigrate/config.shcl"
fCheck "nomigrate: no open raw block in the new file" fLacks '```' "${work}/nomigrate/config.shcl"

echo "a file shcl cannot read"
mkdir -p "${work}/unreadable"
printf '## caf\xe9, kept as written\nfont:\n\tsize: 15\nwindow:\n\tcolumns: 101\nwallpaper:\n\timage: /pics/caf\xe9.jpg\n' >"${work}/unreadable/config.shcl"
fCheck "unreadable: the fixture is not UTF-8" fNotUtf8 "${work}/unreadable/config.shcl"
fConverts unreadable new 1
fCheck "unreadable: font.size carried" fHas font.size 15 "${work}/unreadable/config.shcl"
fCheck "unreadable: window.columns carried" fHas window.columns 101 "${work}/unreadable/config.shcl"

## The notice window, on the private display only.
fNotice(){
	if ! command -v Xvfb >/dev/null 2>&1 || ! command -v xdotool >/dev/null 2>&1 || [[ ! -x "${headless}" ]]; then
		echo "  skip the notice window (no Xvfb, xdotool or gui-headless.bash)"
		return 0
	fi
	export CICD_HEADLESS_DISPLAY="${CICD_HEADLESS_DISPLAY:-:98}"
	local -r display="${CICD_HEADLESS_DISPLAY}"
	local status=""
	status="$("${headless}" status 2>/dev/null || true)"
	if [[ "${status}" == *"no Xvfb"* ]]; then
		if ! "${headless}" start >/dev/null 2>&1; then echo "  skip the notice window (the display on ${display} did not start)"; return 0; fi
		startedDisplay=1
	fi
	local -r auth="/tmp/cicd-gui-headless-${USER:-$(id -un)}/Xauthority-${display#:}"
	local -r dir="${work}/notice"
	fFixture notice 'font:' "${tab}size: 15" "${tab}family:[One, Two]" 'notes: ```' 'never closed'
	mkdir -p "${dir}/home"
	DISPLAY="${display}" XAUTHORITY="${auth}" LIBGL_ALWAYS_SOFTWARE=1 \
		HOME="${dir}/home" XDG_CONFIG_HOME="${dir}/home/.config" XDG_DATA_HOME="${dir}/home/.local/share" XDG_RUNTIME_DIR="${dir}/home" \
		"${bin}" --config "${dir}/config.shcl" --shell "/bin/dash -c 'sleep 60'" >/dev/null 2>"${dir}/said.txt" &
	noticePid=$!
	local window="" _
	for _ in {1..240}; do
		window="$(DISPLAY="${display}" XAUTHORITY="${auth}" xdotool search --name '^Settings not converted$' 2>/dev/null || true)"
		if [[ -n "${window}" ]]; then break; fi
		kill -0 "${noticePid}" 2>/dev/null || break
		sleep 0.25
	done
	fCheck "notice: lost settings bring up the notice window" test -n "${window}"
	fStopOurs "${noticePid}"; noticePid=""
	if ((startedDisplay)); then "${headless}" stop >/dev/null 2>&1 || true; startedDisplay=0; fi
}
echo "the notice window"
fNotice

if ((failures)); then echo "${failures} failed"; exit 1; fi
echo "all passed"

##	History:
##		- 20261003 JC: Created.
