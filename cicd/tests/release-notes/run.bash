#!/usr/bin/env bash

##	- Purpose:
##		The release notes' download table: OS in rows, architecture in columns,
##		each cell linking only the files that are really uploaded, under the
##		names GitHub gives them.
##	- Test ID: Erl3R5g
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: MIT

set -euo pipefail
meDir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "${meDir}/../.." && pwd)"
# shellcheck source=cicd/utility/release-notes.bash
source "${root}/utility/release-notes.bash"

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }

slug="someone/silkterm"
tag="v1.2.3-beta4"
base="https://github.com/${slug}/releases/download/${tag}"
pre="silkterm-1.2.3-beta4-"
all=(
	"${pre}linux-arm64" "${pre}linux-arm64.deb" "${pre}linux-arm64.rpm"
	"${pre}linux-x86_64" "${pre}linux-x86_64.deb" "${pre}linux-x86_64.rpm"
	"${pre}sha256sums.txt"
	"${pre}windows-arm64-setup.exe" "${pre}windows-arm64.exe"
	"${pre}windows-x86_64-setup.exe" "${pre}windows-x86_64.exe"
)

notes=""
fNotes(){ notes="$(fReleaseNotes "${slug}" "${tag}" silkterm 1.2.3-beta4 "${@}")"; }
fHas(){ [[ "${notes}" == *"${1}"* ]]; }
fLacks(){ [[ "${notes}" != *"${1}"* ]]; }
## One table cell: row by its first column, column 1 for x86_64, 2 for ARM64.
fCell(){
	local line
	line="$(grep -F -- "| ${1} " <<< "${notes}" || true)"
	local -a cells
	IFS='|' read -r -a cells <<< "${line}"
	local value="${cells[$(( ${2} + 1 ))]:-}"
	value="${value#"${value%%[! ]*}"}"; printf '%s' "${value%"${value##*[! ]}"}"
}
fCellIs(){ [[ "$(fCell "${1}" "${2}")" == "${3}" ]]; }
fTableRows(){ grep -c '^| ' <<< "${notes}" || true; }
fRowsAre(){ [[ "$(fTableRows)" == "${1}" ]]; }
fNoTrailingPipe(){ ! grep -q '|[[:space:]]*$' <<< "${notes}"; }

## Paths, as release.bash passes them; only the name may reach a link.
fNotes "Bxyz" "${all[@]/#//some/dir/}"
echo "full set:"
fCheck "header has OS, then x86_64, then ARM64" fHas $'\n| OS      | x86_64 '
fCheck "delimiter row is :---" fHas $'\n| :---    | :---'
fCheck "four table lines, no macOS row" fRowsAre 4
fCheck "no macOS anywhere" fLacks "macOS"
fCheck "no trailing pipes" fNoTrailingPipe
fCheck "Linux x86_64 cell: binary, .deb, .rpm in that order" fCellIs Linux 1 \
	"[Binary](${base}/${pre}linux-x86_64), [.deb](${base}/${pre}linux-x86_64.deb), [.rpm](${base}/${pre}linux-x86_64.rpm)"
fCheck "Linux ARM64 cell" fCellIs Linux 2 \
	"[Binary](${base}/${pre}linux-arm64), [.deb](${base}/${pre}linux-arm64.deb), [.rpm](${base}/${pre}linux-arm64.rpm)"
fCheck "Windows x86_64 cell: installer, then portable" fCellIs Windows 1 \
	"[Installer](${base}/${pre}windows-x86_64-setup.exe), [Portable](${base}/${pre}windows-x86_64.exe)"
fCheck "Windows ARM64 cell" fCellIs Windows 2 \
	"[Installer](${base}/${pre}windows-arm64-setup.exe), [Portable](${base}/${pre}windows-arm64.exe)"
fCheck "no directory leaks into a link" fLacks "/some/dir"
fCheck "checksums on a line of their own" fHas $'\n\nSHA-256 checksums: ['"${pre}sha256sums.txt](${base}/${pre}sha256sums.txt)"$'\n'
fCheck "checksums not in the table" fLacks "sha256sums.txt) |"
fCheck "build line kept" fHas $'\n\nBuild Bxyz. Every download here is that build; `silkterm --version` says'
fCheck "README line kept" fHas "See the README for details."
fCheck "nothing else listed" fLacks "Other downloads"

## Missing assets: a cell keeps what is there, and an empty one is marked.
some=()
for name in "${all[@]}"; do
	case "${name}" in *linux-arm64.rpm|*windows-arm64*) ;; *) some+=("${name}") ;; esac
done
fNotes "" "${some[@]}"
echo "missing assets:"
fCheck "no link to the missing .rpm" fLacks "linux-arm64.rpm"
fCheck "Linux ARM64 keeps the rest" fCellIs Linux 2 \
	"[Binary](${base}/${pre}linux-arm64), [.deb](${base}/${pre}linux-arm64.deb)"
fCheck "an empty cell is marked" fCellIs Windows 2 "Not available"
fCheck "no link to the missing Windows ARM64 files" fLacks "windows-arm64"
fCheck "the other cells are untouched" fCellIs Windows 1 \
	"[Installer](${base}/${pre}windows-x86_64-setup.exe), [Portable](${base}/${pre}windows-x86_64.exe)"
fCheck "no build line without a build number" fLacks "Every download here"

## Nothing uploaded but the checksums.
fNotes "" "${pre}sha256sums.txt"
echo "nothing built:"
fCheck "every cell is marked" fCellIs Linux 1 "Not available"
fCheck "and still four table lines" fRowsAre 4
fCheck "no download links at all" fLacks "${pre}linux"

## The signature goes with the checksums, and a file the table has no place for
## is still linked rather than dropped.
fNotes "" "${all[@]}" "${pre}sha256sums.txt.sig" "${pre}macos-universal.tar.gz"
echo "extra files:"
fCheck "signature beside the checksums" fHas "${pre}sha256sums.txt), signed: [${pre}sha256sums.txt.sig](${base}/${pre}sha256sums.txt.sig)"
fCheck "an unplaced file is listed under other downloads" fHas "Other downloads: [${pre}macos-universal.tar.gz](${base}/${pre}macos-universal.tar.gz)"
fCheck "and is not a table row" fRowsAre 4

## GitHub renames a '~' (Debian-style pre-release) to '.', and the link must use
## the name it ends up with.
notes="$(fReleaseNotes "${slug}" v1.0.0-alpha.1 silkterm '1.0.0~alpha.1' "" 'silkterm-1.0.0~alpha.1-linux-x86_64')"
echo "renamed asset:"
fCheck "link uses GitHub's name" fCellIs Linux 1 "[Binary](https://github.com/${slug}/releases/download/v1.0.0-alpha.1/silkterm-1.0.0.alpha.1-linux-x86_64)"
fCheck "no '~' in any link" fLacks "~"

echo "owner/repo from the remote:"
fSlugIs(){ [[ "$(fReleaseNotes_RepoSlug "${1}" 2>/dev/null || true)" == "${2}" ]]; }
fCheck "ssh alias" fSlugIs "github_jim-collier:yottacore/silkterm.git" "yottacore/silkterm"
fCheck "scp form" fSlugIs "git@github.com:yottacore/silkterm.git" "yottacore/silkterm"
fCheck "https" fSlugIs "https://github.com/yottacore/silkterm.git" "yottacore/silkterm"
fCheck "https, no .git, trailing slash" fSlugIs "https://github.com/yottacore/silkterm/" "yottacore/silkterm"
fCheck "ssh url" fSlugIs "ssh://git@github.com/yottacore/silkterm.git" "yottacore/silkterm"
fCheck "a bare name is refused" fSlugIs "silkterm" ""

if ((failures)); then echo "${failures} failed"; exit 1; fi
echo "all passed"

##	History:
##		- 20261004 JC: Created.
