#!/usr/bin/env bash

##	Purpose:
##		- The release notes, with the downloads in a table: target OS in rows,
##		  CPU architecture in columns. Built from the files being uploaded, so a
##		  build that is missing leaves its cell marked rather than a dead link.
##		- Sourced by release.bash. No network and no gh, so a test can run it on
##		  made-up file names.
##	History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	Licensed under The MIT License (MIT). Full text at:
##		https://mit-license.org/
##	SPDX-License-Identifier: MIT

## GitHub turns any character outside [A-Za-z0-9._-] in an uploaded asset name
## into '.', so a link has to use the name as it ends up there.
fReleaseNotes_AssetName(){ local name="${1}"; printf '%s' "${name//[^A-Za-z0-9._-]/.}"; }

## fReleaseNotes_RepoSlug <remote url>
## owner/repo from a remote such as https://github.com/o/r.git, git@github.com:o/r
## or an ssh alias like host_alias:o/r.git.
fReleaseNotes_RepoSlug(){
	local url="${1%/}"; url="${url%.git}"
	local -r repo="${url##*[/:]}"
	url="${url%[/:]*}"
	local -r owner="${url##*[/:]}"
	if [[ -z "${owner}" || -z "${repo}" || "${owner}" == "${url}" ]]; then
		echo "cannot read owner/repo from remote '${1}'" >&2; return 1
	fi
	printf '%s/%s' "${owner}" "${repo}"
}

## fReleaseNotes <owner/repo> <tag> <exe name> <version> <build id or ""> <file>...
## Prints the notes. Files are paths or bare names; only the name is used.
fReleaseNotes(){
	local -r slug="${1}" tag="${2}" exeName="${3}" version="${4}" buildId="${5}"
	shift 5
	local -r base="https://github.com/${slug}/releases/download/${tag}"
	local prefix="${exeName}-${version}-"
	prefix="${prefix//[^A-Za-z0-9._-]/.}"

	local -A uploaded=() used=()
	local -a order=()
	local file name
	for file in "${@}"; do
		name="${file##*/}"; name="${name//[^A-Za-z0-9._-]/.}"
		[[ -n "${uploaded[${name}]:-}" ]] && continue
		uploaded[${name}]=1; order+=("${name}")
	done

	## "os|arch|suffix|label" in the order a cell lists them. Arch is as the
	## file names have it; the column header is separate.
	local -ra slots=(
		"linux|x86_64||Binary"     "linux|x86_64|.deb|.deb"           "linux|x86_64|.rpm|.rpm"
		"linux|arm64||Binary"      "linux|arm64|.deb|.deb"            "linux|arm64|.rpm|.rpm"
		"windows|x86_64|-setup.exe|Installer" "windows|x86_64|.exe|Portable"
		"windows|arm64|-setup.exe|Installer"  "windows|arm64|.exe|Portable"
	)
	local -A cell=()
	local slot os arch suffix label key
	for slot in "${slots[@]}"; do
		IFS='|' read -r os arch suffix label <<< "${slot}"
		name="${prefix}${os}-${arch}${suffix}"
		[[ -n "${uploaded[${name}]:-}" ]] || continue
		used[${name}]=1
		key="${os}|${arch}"
		cell[${key}]+="${cell[${key}]:+, }[${label}](${base}/${name})"
	done

	local -ra rowNames=("OS" ":---" "Linux" "Windows")
	local -ra rowKeys=("" "" "linux" "windows")
	local -ra colHeads=("x86_64" "ARM64")
	local -ra colKeys=("x86_64" "arm64")
	local -a grid=()
	local row col value
	for row in "${!rowNames[@]}"; do
		grid+=("${rowNames[row]}")
		for col in "${!colKeys[@]}"; do
			case "${row}" in
				0) value="${colHeads[col]}" ;;
				1) value=":---" ;;
				*) value="${cell[${rowKeys[row]}|${colKeys[col]}]:-Not available}" ;;
			esac
			grid+=("${value}")
		done
	done

	## Pad every column but the last, which has no trailing pipe to line up.
	local -r cols=$(( ${#colKeys[@]} + 1 ))
	local -a width=()
	local i
	for i in "${!grid[@]}"; do
		col=$(( i % cols ))
		if (( ${#grid[i]} > ${width[col]:-0} )); then width[col]=${#grid[i]}; fi
	done

	printf '%s\n\n' "See the README for details."
	local line=""
	for i in "${!grid[@]}"; do
		col=$(( i % cols ))
		if (( col == cols - 1 )); then
			printf '%s| %s\n' "${line}" "${grid[i]}"; line=""
		else
			printf -v line '%s| %-*s ' "${line}" "${width[col]}" "${grid[i]}"
		fi
	done

	local -r sums="${prefix}sha256sums.txt"
	local others=""
	for name in "${order[@]}"; do
		[[ -n "${used[${name}]:-}" || "${name}" == "${sums}" || "${name}" == "${sums}.sig" ]] && continue
		others+="${others:+, }[${name}](${base}/${name})"
	done
	[[ -z "${others}" ]] || printf '\nOther downloads: %s\n' "${others}"

	if [[ -n "${uploaded[${sums}]:-}" ]]; then
		printf '\nSHA-256 checksums: [%s](%s/%s)' "${sums}" "${base}" "${sums}"
		[[ -z "${uploaded[${sums}.sig]:-}" ]] || printf ', signed: [%s.sig](%s/%s.sig)' "${sums}" "${base}" "${sums}"
		printf '\n'
	fi

	if [[ -n "${buildId}" ]]; then
		printf '\nBuild %s. Every download here is that build; %s says which one you are running.\n' "${buildId}" "\`${exeName} --version\`"
	fi
}

##	History:
##		- 20261004 JC: Created.
