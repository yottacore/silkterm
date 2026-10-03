#!/usr/bin/env bash
#  shellcheck disable=2001  ## 'See if you can use ${variable//search/replace} instead.' Complains about good uses of sed.

##	- Purpose:
##		utility/rename.bash renames the project. It used to rewrite only
##		Cargo.toml, the Rust sources and the Markdown docs, and to rename no file
##		at all - so the resource template build.rs reads by name kept the old
##		name, and every Windows build of the renamed tree failed in its build
##		script. The pipeline, the packaging and the installers kept it too.
##		This clones the repository, renames the clone, and checks that nothing a
##		build reads by name went missing and that the old name is gone.
##	- Test ID: EqAwQqB
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

set -euo pipefail
meDir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use
root="$(cd "${meDir}/../../.." && pwd)"

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }

##	--shared reads the repository's objects in place rather than copying them,
##	so the clone is quick from any filesystem. --local hardlinks them, which
##	fails when the temp dir is on another filesystem. rename.bash only moves
##	and rewrites tracked files and writes no objects, so the source is untouched.
clone="$(mktemp -d)/clone"
fEnd(){ local -r rc=$?; rm -rf "${clone%/clone}"; fTestDir_End "${rc}"; }
trap fEnd EXIT
git -C "${root}" clone --quiet --shared . "${clone}"

( cd "${clone}" && utility/rename.bash Weavterm >/dev/null )

##	Every input build.rs reads by name: the files it watches, the ones it opens
##	under the crate directory, and the build-number input list. Anything spelled
##	with a {placeholder}, and anything under OUT_DIR, is an output.
missing=0
while IFS= read -r p; do
	[[ -n "${p}" ]] || continue
	[[ -e "${clone}/source/${p}" ]] || { echo "    build.rs names ${p}, which is not there"; missing=$((missing + 1)); }
done < <(
	{
		sed -n 's/.*rerun-if-changed=\([^"{]*\)".*/\1/p' "${clone}/source/build.rs"
		tr '\n' ' ' < "${clone}/source/build.rs" \
			| grep -oE 'Path::new\(&manifest\)[^;]*\.join\("[^"]*"' \
			| sed 's/.*\.join("//; s/"$//'
		sed -n '/^const BUILD_INPUTS/,/\];/p' "${clone}/source/src/buildnum.rs" \
			| grep -oE '"[^"]+"' | tr -d '"'
	} | sort -u
)
fCheck "build.rs still names files that are there" test "${missing}" -eq 0

##	And every include! in the sources, which resolve against their own file.
missing=0
while IFS= read -r hit; do
	[[ -n "${hit}" ]] || continue
	file="${hit%%:*}"
	p="${hit#*:}"
	[[ -e "$(dirname "${file}")/${p}" ]] || { echo "    ${file} includes ${p}, which is not there"; missing=$((missing + 1)); }
done < <(
	cd "${clone}" \
		&& grep -rn --include='*.rs' -oE 'include_(str|bytes)!\("[^"]*"' source/src \
		| sed 's/:[0-9]*:include_\(str\|bytes\)!("/:/; s/"$//'
)
fCheck "every include! still names a file that is there" test "${missing}" -eq 0

##	A file the sweep missed still carries the old name. Cargo.lock is left for
##	cargo to regenerate, and the script cannot rewrite itself.
left="$(cd "${clone}" && git grep -lI -e SilkTerm -e silkterm -- . \
	':(exclude)Cargo.lock' ':(exclude)utility/rename.bash' || true)"
fCheck "no tracked text file still carries the old name" test -z "${left}"
[[ -z "${left}" ]] || sed 's/^/    /' <<< "${left}"

##	The paths themselves. A file or directory named for the project is renamed.
left="$(cd "${clone}" && git ls-files | grep -i silkterm || true)"
fCheck "no tracked path still carries the old name" test -z "${left}"
[[ -z "${left}" ]] || sed 's/^/    /' <<< "${left}" | head -5

if ((failures)); then echo "${failures} failed"; exit 1; fi
echo "all passed"

##	History:
##		- 20260917 JC: Created.
##		- 20260930 JC: The clone goes in the test run folder, with --shared.
