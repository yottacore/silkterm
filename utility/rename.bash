#!/usr/bin/env bash
# Rename the project everywhere (development-time helper).
#
#   utility/rename.bash [--untracked] <NewDisplayName>
#
# <NewDisplayName> is the display name (e.g. "SilkTerm"). The lowercase
# identifier used for the cargo package, the binary, and the config directory
# is derived from it (e.g. "silkterm").
#
# Replaces the display name (SilkTerm) and the id (silkterm) in every tracked
# text file that mentions either, and renames any file or directory with the
# id - the resource template a Windows build reads by name, the packaging files,
# the wallpaper directory. Binaries are left alone, and so is Cargo.lock, which
# `cargo build` regenerates. Review `git diff` and `git status` afterwards.
#
# Untracked files are left alone, since they can be someone's work in progress,
# unless --untracked is given. Ignored files are never touched. An untracked
# file inside a directory that gets renamed still moves along with it.

##	Copyright (c) 2026 Bubbles
##	Licensed under The MIT License (MIT). Full text at:
##		https://mit-license.org/
##	SPDX-License-Identifier: MIT
set -euo pipefail

untracked=0
if [[ "${1:-}" = "--untracked" ]]; then untracked=1; shift; fi
newDisplay="${1:-}"
if [[ -z "${newDisplay}" ]] || [[ "$#" -gt 1 ]] || [[ "${newDisplay}" = -* ]]; then
	echo "usage: utility/rename.bash [--untracked] <NewDisplayName>" >&2
	exit 1
fi
case "${newDisplay}" in
	*/* | *'&'* | *'\'*)
		echo "error: name must not contain / & or backslash" >&2
		exit 1
		;;
esac

newId="$(printf '%s' "${newDisplay}" | tr '[:upper:]' '[:lower:]' | tr -cd 'a-z0-9_-')"
if [[ -z "${newId}" ]]; then
	echo "error: '${newDisplay}' yields no usable lowercase identifier" >&2
	exit 1
fi

oldDisplay="SilkTerm"
oldId="silkterm"

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${root}"

mapfile -t candidates < <(
	{ git ls-files; [[ "${untracked}" = "0" ]] || git ls-files --others --exclude-standard; } | sort -u
)

# Every text file that mentions either name. Narrowing this to Rust and Markdown
# is what used to leave the pipeline, the packaging and the installers behind.
# -I skips binaries, so the wallpaper pack and the icons never get rewritten.
texts=()
for f in "${candidates[@]}"; do
	[[ -f "${f}" ]] || continue
	case "${f}" in utility/rename.bash | Cargo.lock | */Cargo.lock) continue ;; esac
	texts+=("${f}")
done
files=()
if ((${#texts[@]})); then
	mapfile -t -d '' files < <(grep -lI --null -e "${oldDisplay}" -e "${oldId}" -- "${texts[@]}" 2>/dev/null || true)
fi
if ((${#files[@]})); then
	sed -i "s/${oldDisplay}/${newDisplay}/g; s/${oldId}/${newId}/g" "${files[@]}"
fi

# Paths with the id, deepest component first so a rename never invalidates
# one still to come. source/assets/silkterm.rc.in is the one a build reads by
# name: miss it and every Windows build of the renamed tree fails in build.rs.
mapfile -t paths < <(
	printf '%s\n' "${candidates[@]}" \
		| awk -v id="${oldId}" '
			{
				n = split($0, part, "/"); path = ""
				for (i = 1; i <= n; i++) {
					path = (i == 1) ? part[i] : path "/" part[i]
					if (index(part[i], id)) print i "\t" path
				}
			}' \
		| sort -u | sort -rns -k1,1 | cut -f2- | awk '!seen[$0]++'
)

renamed=0
for p in "${paths[@]}"; do
	[[ -e "${p}" ]] || continue
	base="${p##*/}"
	new="${base//${oldId}/${newId}}"
	[[ "${p}" != */* ]] || new="${p%/*}/${new}"
	[[ "${p}" != "${new}" ]] || continue
	git mv "${p}" "${new}" 2>/dev/null || mv "${p}" "${new}"
	renamed=$((renamed + 1))
done

echo "Renamed:"
echo "  display : ${oldDisplay} -> ${newDisplay}"
echo "  id      : ${oldId} -> ${newId}"
echo "  in      : ${#files[@]} files"
echo "  paths   : ${renamed} renamed"
echo
echo "Next steps:"
echo "  - review 'git diff' and 'git status'"
echo "  - 'cargo build' (regenerates Cargo.lock with the new package name)"
echo "  - if you rename the GitHub repo too, update the 'git remote' URL"

##	History:
##		- 20260628: First commit.
##		- 20261118: Untracked files only with --untracked.
