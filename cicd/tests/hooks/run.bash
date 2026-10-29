#!/usr/bin/env bash
#  shellcheck disable=2034  ## 'appears unused.' skipList is read by fNeedsGate, lifted from pre-push inside an eval.

##	- Purpose:
##		The git hooks act on a commit or a push, where a mistake is awkward to undo.
##		Drives them in a scratch repository: the pre-commit formatter must touch only
##		what is staged, and the pre-push gate must verify the commit being pushed
##		rather than whatever the working tree happens to hold.
##	- Test ID: dcrj6
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

set -euo pipefail
meDir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use
root="$(cd "${meDir}/../../.." && pwd)"
hooks="${root}/utility/git-hooks"

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }

command -v rustfmt >/dev/null 2>&1 || PATH="${HOME}/.cargo/bin:${PATH}"
if ! command -v rustfmt >/dev/null 2>&1; then
	echo "  skip hooks: rustfmt not found"
	exit 0
fi

work="$(mktemp -d "${TMPDIR:-/tmp}/silk-hooks.XXXXXX")"
fEnd(){ local -r rc=$?; rm -rf "${work}"; fTestDir_End "${rc}"; }
trap fEnd EXIT

## A repository of our own, so nothing here can reach the real one. The identity
## is local because this tree has no global one (user.useConfigOnly).
repo="${work}/repo"
mkdir -p "${repo}"
git -C "${repo}" init -q -b main
git -C "${repo}" config user.name "Hook Test"
git -C "${repo}" config user.email "hooks@example.invalid"
git -C "${repo}" config core.hooksPath "${hooks}"
cp "${root}/rustfmt.toml" "${repo}/rustfmt.toml" 2>/dev/null || true

## Two functions, so one change can be staged and the other left behind. Both are
## written badly enough that rustfmt has something to do.
cat > "${repo}/a.rs" <<'EOF'
fn one() -> i32 {
	1
}

fn two() -> i32 {
	2
}
EOF
git -C "${repo}" add -A
git -C "${repo}" commit -q --no-verify -m "start"

## Change one: staged, and written so rustfmt must reformat it.
## Change two: left in the working tree, and must not reach the commit.
python3 - "${repo}/a.rs" <<'EOF'
import sys
p = sys.argv[1]
s = open(p).read()
s = s.replace("fn one() -> i32 {\n\t1\n}", "fn one( ) -> i32 {\n      1 + 1\n}")
open(p, "w").write(s)
EOF
git -C "${repo}" add a.rs
python3 - "${repo}/a.rs" <<'EOF'
import sys
p = sys.argv[1]
s = open(p).read()
s = s.replace("fn two() -> i32 {\n\t2\n}", "fn two() -> i32 {\n\t222\n}")
open(p, "w").write(s)
EOF

git -C "${repo}" commit -q -m "one only" 2>/dev/null

committed="$(git -C "${repo}" show HEAD:a.rs)"
fCheck "the staged change is committed" \
	bash -c 'printf "%s" "$1" | grep -q "1 + 1"' _ "${committed}"
fCheck "the unstaged change is not" \
	bash -c '! printf "%s" "$1" | grep -q "222"' _ "${committed}"
fCheck "and it is still in the working tree" \
	grep -q "222" "${repo}/a.rs"
fCheck "the committed hunk was formatted" \
	bash -c 'printf "%s\n" "$1" | grep -Fxq "fn one() -> i32 {"' _ "${committed}"

## A file whose changes are all staged still gets its working copy formatted, so
## the tree does not read as dirty the moment the commit lands.
cat > "${repo}/b.rs" <<'EOF'
fn three( ) -> i32 {
      3
}
EOF
git -C "${repo}" add b.rs
git -C "${repo}" commit -q -m "three"
fCheck "a fully staged file is formatted in the working tree too" \
	bash -c 'git -C "$1" diff --quiet -- b.rs' _ "${repo}"
fCheck "and formatted in the commit" \
	bash -c 'git -C "$1" show HEAD:b.rs | grep -Fxq "fn three() -> i32 {"' _ "${repo}"

## A crate root names its modules by file. The staged copy is formatted away
## from them, so a hook that let rustfmt follow those lines refused the commit.
printf 'mod a;\nmod b;\nfn main( ) {\n      a::one();\n}\n' > "${repo}/main.rs"
git -C "${repo}" add main.rs
fCheck "a file with mod lines commits" \
	git -C "${repo}" commit -q -m "root"
fCheck "and is formatted" \
	bash -c 'git -C "$1" show HEAD:main.rs | grep -Fxq "fn main() {"' _ "${repo}"

## The pre-push gate, with a stub in place of the pipeline: a real gate is a full
## build, and what is being tested is WHICH source it reads, not what it does.
## The stub passes when the tree it runs in says "good" and fails when it says
## "bad", so the answer names the source the hook handed it.
push="${work}/push"
origin="${push}/origin.git"
clone="${push}/clone"
mkdir -p "${push}"
git init -q --bare -b main "${origin}"
git init -q -b main "${clone}"
git -C "${clone}" config user.name "Hook Test"
git -C "${clone}" config user.email "hooks@example.invalid"
git -C "${clone}" config core.hooksPath "${hooks}"
git -C "${clone}" remote add origin "${origin}"
mkdir -p "${clone}/cicd" "${clone}/source"
cat > "${clone}/cicd/cicd.bash" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
[ "$(cat "${here}/verdict")" = "good" ]
EOF
chmod +x "${clone}/cicd/cicd.bash"
## The release guard runs first and has its own rules, so give it what it wants.
printf 'version = "9.0.0"\n' > "${clone}/source/Cargo.toml"
printf '[![Release](Release-9.0.0-blue)]\n' > "${clone}/README.md"

printf 'bad\n' > "${clone}/verdict"
git -C "${clone}" add -A
git -C "${clone}" commit -q --no-verify -m "a commit that does not pass"
printf 'good\n' > "${clone}/verdict"   # the fix, left uncommitted
fCheck "a push is refused for a commit that fails, whatever the tree holds" \
	bash -c '! git -C "$1" push -q origin main 2>/dev/null' _ "${clone}"

## A fresh version, so this case does not depend on whether the one above pushed.
printf 'version = "9.0.1"\n' > "${clone}/source/Cargo.toml"
printf '[![Release](Release-9.0.1-blue)]\n' > "${clone}/README.md"
git -C "${clone}" add -A
git -C "${clone}" commit -q --no-verify -m "the fix, committed"
printf 'bad\n' > "${clone}/verdict"    # and now the tree is the broken one
fCheck "and allowed once the commit itself passes" \
	git -C "${clone}" push -q origin main
fCheck "the uncommitted change is still there afterwards" \
	bash -c 'grep -Fxq bad "$1/verdict"' _ "${clone}"
fCheck "the gate worktree is cleaned up" \
	bash -c 'test "$(git -C "$1" worktree list | wc -l)" -eq 1' _ "${clone}"

## The release guard itself: main only takes a higher version, with the README
## badge naming it. The stub gate passes throughout, so a refusal here is the
## guard's. $1 is the version, $2 the badge.
fRelease(){
	printf 'version = "%s"\n' "${1}" > "${clone}/source/Cargo.toml"
	printf '[![Release](Release-%s-blue)]\n' "${2}" > "${clone}/README.md"
	printf 'good\n' > "${clone}/verdict"
	printf '%s\n' "${1}" >> "${clone}/releases"
	git -C "${clone}" add -A
	git -C "${clone}" commit -q --no-verify -m "release ${1}"
}
fPushSays(){  ## fPushSays <text>: the push is refused, and says <text>
	local out
	if out="$(git -C "${clone}" push -q origin main 2>&1)"; then return 1; fi
	grep -qF -- "${1}" <<<"${out}"
}
fRelease 9.0.2 9.0.1
fCheck "a release whose badge still names the last version is refused" fPushSays "README Release badge does not match version 9.0.2"
git -C "${clone}" reset -q --hard origin/main
fRelease 9.0.0 9.0.0
fCheck "a version below the one on main is refused" fPushSays "version 9.0.0 must be greater than 9.0.1"
git -C "${clone}" reset -q --hard origin/main
fRelease 9.0.1 9.0.1
fCheck "and so is the same version again" fPushSays "version 9.0.1 must be greater than 9.0.1"
git -C "${clone}" reset -q --hard origin/main
fRelease 10.0.0-beta2 10.0.0--beta2
fCheck "a pre-release above it is taken, its badge escaped" git -C "${clone}" push -q origin main
fRelease 10.0.0 10.0.0
fCheck "and the release it leads to is taken after it" git -C "${clone}" push -q origin main
fRelease 10.0.0-rc1 10.0.0--rc1
fCheck "but not a pre-release of the version already out" fPushSays "version 10.0.0-rc1 must be greater than 10.0.0"

## The merge gate on main, and docs going there without it. The stub notes the
## options it was run with, so a push that skips it leaves no note.
git -C "${clone}" reset -q --hard origin/main
mark="${work}/gate-ran"
cat > "${clone}/cicd/cicd.bash" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
[ -z "${GATE_MARK:-}" ] || echo "${*}" > "${GATE_MARK}"
[ "$(cat "${here}/verdict")" = "good" ]
EOF
mkdir -p "${clone}/utility/git-hooks" "${clone}/project"
cp "${hooks}/no-gate-paths" "${clone}/utility/git-hooks/"
fRelease 10.0.1 10.0.1
fPushGate(){ rm -f "${mark}"; GATE_MARK="${mark}" git -C "${clone}" push -q origin main 2>&1; }
out="$(fPushGate)" || true
fCheck "a push to main runs --merge-gate" bash -c '[[ "$(cat "$1" 2>/dev/null)" == --merge-gate ]]' _ "${mark}"
fDocs(){  ## fDocs <path> <text>: commit that change alone, with no version bump
	mkdir -p "$(dirname "${clone}/${1}")"; printf '%s\n' "${2}" >> "${clone}/${1}"
	git -C "${clone}" add -A; git -C "${clone}" commit -q --no-verify -m "docs"
}
fDocs notes.md "a"; fDocs project/plan.md "b"; fDocs .github/ISSUE_TEMPLATE/bug.md "c"
out="$(fPushGate)" && rc=0 || rc=$?
fCheck "a push of docs alone goes to main with no version bump" test "${rc}" -eq 0
fCheck "and without the gate, saying so" bash -c '[[ ! -e "$1" && "$2" == *"so no merge gate"* ]]' _ "${mark}" "${out}"
fGatedPush(){  ## fGatedPush <path>: a change to it with docs is held to the version guard
	fDocs notes.md "d"; fDocs "${1}" "e"
	out="$(fPushGate)" && rc=0 || rc=$?
	git -C "${clone}" reset -q --hard origin/main
	[[ "${rc}" -ne 0 && "${out}" == *"must be greater than 10.0.1"* && ! -e "${mark}" ]]
}
fCheck "a doc built into the binary is not docs alone" fGatedPush usage.md
fCheck "nor is one a test reads" fGatedPush project/design_docs/20260930-145304_scrim.md
fCheck "nor a source file pushed with docs" fGatedPush source/main.rs
fCheck "nor a change to the list itself" fGatedPush utility/git-hooks/no-gate-paths
## The real list, against every file a build reads.
skipList="$(cat "${hooks}/no-gate-paths")"
eval "$(sed -n '/^fNeedsGate() {/,/^}/p' "${hooks}/pre-push")"
fGated(){ local f IFS=$'\n'; set -f; for f in "${@}"; do fNeedsGate "${f}" || { set +f; echo "    not gated: ${f}"; return 1; }; done; set +f; }
fSkipped(){ local f IFS=$'\n'; set -f; for f in "${@}"; do ! fNeedsGate "${f}" || { set +f; echo "    gated: ${f}"; return 1; }; done; set +f; }
mapfile -t built < <(cd "${root}" && git grep -o -E 'include_(str|bytes)!\("[^"]+"\)' -- source knobs \
	| sed -E 's/^([^:]+):include_[a-z]+!\("([^"]+)"\)$/\1 \2/' \
	| while read -r file inc; do realpath -m --relative-to=. "$(dirname "${file}")/${inc}"; done | sort -u)
mapfile -t -O "${#built[@]}" built < <(cd "${root}/source" && grep -o -E '"\.\./[^"]+"' Cargo.toml | tr -d '"' | while read -r f; do realpath -m --relative-to=.. "${f}"; done)
fCheck "the list holds patterns, and a build reads ${#built[@]} files" test "${#built[@]}" -gt 10
fCheck "and none of them is skipped" fGated "${built[@]}"
fCheck "nor any doc a test reads" fGated README.md build.md prerequisites.md comparison.md filesystem/home/.config/silkterm/wallpaper-attribution.md \
	"$(cd "${root}" && git ls-files 'project/design_docs/*_scrim.md')"
fCheck "while design notes, the backlog and a nested readme are" fSkipped project/design.md project/backlog.md glossary.md knobs/readme.md assets/demo.gif

if ((failures)); then
	echo "  ${failures} failure(s)"
	exit 1
fi
echo "  hooks: ok"

##	History:
##		20260917  Created for code review 20260914 items 48 and 51 (F80, F83): the
##		          pre-commit formatter staged a partly staged file whole, and the
##		          pre-push gate read the working tree rather than the commit.
##		20260918  A file with mod lines, which the formatter could not commit.
##		20260926  The release guard: badge, lower or equal version, pre-releases.
##		20261028  The merge gate, and docs going to main without it or the version
##		          guard. Every file a build or test reads stays gated.
