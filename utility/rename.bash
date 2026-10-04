#!/usr/bin/env bash
# Rename the project everywhere (development-time helper).
#
#   utility/rename.bash <NewDisplayName>
#
# <NewDisplayName> is the display name (e.g. "SilkTerm"). The lowercase
# identifier used for the cargo package, the binary, and the config directory
# is derived from it (e.g. "silkterm").
#
# Replaces the display name (SilkTerm) and the id (silkterm) in every tracked
# text file that mentions either, and renames any file or directory carrying the
# id - the resource template a Windows build reads by name, the packaging files,
# the wallpaper directory. Binaries are left alone, and so is Cargo.lock, which
# `cargo build` regenerates. Review `git diff` and `git status` afterwards.

##	Copyright (c) 2026 Bubbles
##	Licensed under The MIT License (MIT). Full text at:
##		https://mit-license.org/
##	SPDX-License-Identifier: MIT
set -euo pipefail

new_display="${1:-}"
if [[ -z "$new_display" ]]; then
	echo "usage: utility/rename.bash <NewDisplayName>" >&2
	exit 1
fi
case "$new_display" in
	*/* | *'&'* | *'\'*)
		echo "error: name must not contain / & or backslash" >&2
		exit 1
		;;
esac

new_id="$(printf '%s' "$new_display" | tr '[:upper:]' '[:lower:]' | tr -cd 'a-z0-9_-')"
if [[ -z "$new_id" ]]; then
	echo "error: '$new_display' yields no usable lowercase identifier" >&2
	exit 1
fi

old_display="SilkTerm"
old_id="silkterm"

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

mapfile -t candidates < <(
	{ git ls-files; git ls-files --others --exclude-standard; } | sort -u
)

# Every text file that mentions either name. Narrowing this to Rust and Markdown
# is what used to leave the pipeline, the packaging and the installers behind.
# -I skips binaries, so the wallpaper pack and the icons never get rewritten.
files=()
for f in "${candidates[@]}"; do
	[[ -f "$f" ]] || continue
	case "$f" in utility/rename.bash | Cargo.lock | */Cargo.lock) continue ;; esac
	if grep -qI -e "$old_display" -e "$old_id" -- "$f" 2>/dev/null; then files+=("$f"); fi
done

for f in "${files[@]}"; do
	sed -i "s/${old_display}/${new_display}/g; s/${old_id}/${new_id}/g" "$f"
done

# Paths carrying the id, deepest component first so a rename never invalidates
# one still to come. source/assets/silkterm.rc.in is the one a build reads by
# name: miss it and every Windows build of the renamed tree fails in build.rs.
mapfile -t paths < <(
	printf '%s\n' "${candidates[@]}" \
		| awk -v id="$old_id" '
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
	[[ -e "$p" ]] || continue
	base="$(basename "$p")"
	parent="$(dirname "$p")"
	new="${base//${old_id}/${new_id}}"
	[[ "$parent" == "." ]] || new="${parent}/${new}"
	[[ "$p" != "$new" ]] || continue
	git mv "$p" "$new" 2>/dev/null || mv "$p" "$new"
	renamed=$((renamed + 1))
done

echo "Renamed:"
echo "  display : ${old_display} -> ${new_display}"
echo "  id      : ${old_id} -> ${new_id}"
echo "  in      : ${#files[@]} files"
echo "  paths   : ${renamed} renamed"
echo
echo "Next steps:"
echo "  - review 'git diff' and 'git status'"
echo "  - 'cargo build' (regenerates Cargo.lock with the new package name)"
echo "  - if you rename the GitHub repo too, update the 'git remote' URL"

##	History:
##		- 20260628: First commit.
