#!/usr/bin/env bash

##	- Purpose:
##		A release may only publish artifacts built from the source being tagged.
##		The checksums only say the artifacts match each other, which let a stale
##		build go out under a new tag with everything reporting green.
##	- Test ID: EpHWVGS
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

set -euo pipefail
meDir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use
root="$(cd "${meDir}/../.." && pwd)"
# shellcheck source=cicd/utility/built-from.bash
source "${root}/utility/built-from.bash"

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }

work="$(mktemp -d)"
fEnd(){ local -r rc=$?; rm -rf "${work}"; fTestDir_End "${rc}"; }
trap fEnd EXIT
cd "${work}"
git init -q .
## The artifact directory is gitignored in the real tree, as it has to be now that
## an untracked file counts against the build.
printf 'art/\nignored/\n' > .gitignore
git -c user.name=t -c user.email=t@t add .gitignore
git -c user.name=t -c user.email=t@t commit -q -m first
mkdir art

fCommit(){ git -c user.name=t -c user.email=t@t commit -q "${@}"; }
fNot(){ ! fCheckBuiltFrom art >/dev/null; }
fYes(){ fCheckBuiltFrom art >/dev/null; }
fWhy(){ fCheckBuiltFrom art 2>&1 || true; }
fSays(){ [[ "$(fWhy)" == *"${1}"* ]]; }

fCheck "no note at all is refused" fNot

fWriteBuiltFrom art
fCheck "a note written here and now is accepted" fYes

## More commits, then the release is cut: the artifacts are now stale.
echo change > file.txt
git add file.txt
fCommit -m second
fCheck "a stale artifact directory is refused" fNot

## A merge keeps the tree, so a release cut from main after 'dev -> main --no-ff'
## still matches what the pipeline built on dev.
fWriteBuiltFrom art
git checkout -q -b rel
fCommit --allow-empty -m "merge (same tree)"
fCheck "the same tree on another branch is accepted" fYes

## A dirty tree at build time says nothing about what was built.
echo more >> file.txt
fWriteBuiltFrom art
fCheck "a build from a dirty tree is refused" fNot
git checkout -q -- file.txt

## A commit made in this tree while the builds run. The note used to be written
## from the tree as it stood by then, so it named the new commit with a clean flag
## while the binaries held the source from before it - and a run long enough to
## cross-build can hold one of each.
state="$(fSourceState)"
echo "second session" > other.txt
git add other.txt
fCommit -m "another session, mid-build"
fWriteBuiltFrom art "${state}"
fCheck "a commit made while the builds ran is refused" fNot
fCheck "and it says the source changed" fSays "the source changed"

## An untracked source file builds here and is not in the tag, so the tagged source
## may not even compile: a module whose 'mod' line is committed and whose file is
## not is the shape that got through.
fWriteBuiltFrom art
fCheck "a clean tree is still accepted" fYes
echo "fn extra() {}" > extra.rs
fWriteBuiltFrom art
fCheck "an untracked source file is refused" fNot
fCheck "and the file is named" fSays "extra.rs"
rm -f extra.rs

## An ignored one is not source, so it says nothing about the build.
mkdir -p ignored && echo scratch > ignored/thing
fWriteBuiltFrom art
fCheck "an ignored file leaves the tree clean" fYes

## The set has to be whole. --quick and --no-cross leave the native binary alone in
## there, and a package step that cannot find its tool only warns, so a release went
## out with nothing for Windows to install and the checksums verified either way.
fWriteBuiltFrom art "" app-linux app-windows.exe
fCheck "an artifact set missing a target is refused" fNot
fCheck "and the missing file is named" fSays "app-windows.exe"
: > art/app-linux
fCheck "a set still missing one is refused" fNot
: > art/app-windows.exe
fCheck "a whole set is accepted" fYes

## The release notes name the build, read out of the binary's --version by
## release.bash's own pattern, lifted here so the two cannot drift apart.
pattern="$(sed -n "s/^[[:space:]]*build_id=.*sed -n '\\(.*\\)' || true)\"$/\\1/p" "${root}/utility/release.bash")"
built="$(ls -t "${root}/../target/debug/silkterm" "${root}/../target/release/silkterm" 2>/dev/null | head -1 || true)"
if [[ -z "${pattern}" ]]; then
	echo "  FAIL release.bash's build number pattern was not found"; failures=$((failures + 1))
elif [[ -n "${built}" ]]; then
	got="$("${built}" --version | sed -n "${pattern}")"
	fCheck "release.bash reads the build number from --version" test -n "${got}"
else
	echo "  skip reading the build number (no built binary)"
fi

## Remote git and gh go through gitsby where it is on PATH, and plain git and gh
## where it is not. Stand-ins that print what they were asked.
stubs="${work}/stubs"; plain="${work}/plain"
mkdir -p "${stubs}" "${plain}"
for tool in gitsby git gh; do
	printf '#!/bin/sh\necho "%s $*"\n' "${tool}" > "${stubs}/${tool}"
	chmod +x "${stubs}/${tool}"
done
cp "${stubs}/git" "${stubs}/gh" "${plain}/"
helper="${root}/utility/include/remote-git.bash"
got="$(PATH="${stubs}:/usr/bin:/bin" bash -c 'source "$1"; fRemoteGit push origin main; fRemoteGh release view' _ "${helper}")"
fCheck "remote git and gh go through gitsby when it is there" \
	test "${got}" = $'gitsby raw git push origin main\ngitsby raw gh release view'
got="$(PATH="${plain}:/usr/bin:/bin" bash -c 'source "$1"; fRemoteGit push origin main; fRemoteGh release view' _ "${helper}")"
fCheck "and plain git and gh when it is not" \
	test "${got}" = $'git push origin main\ngh release view'

## Signing. The checksums file says the download was not corrupted; the signature
## is what says it came from here. Driven with a throwaway key: signed the way
## release.bash signs, and checked by each installer's OWN verify function, so a
## retyped command cannot pass where the installer fails.
if command -v ssh-keygen >/dev/null 2>&1; then
	keyDir="${work}/key"
	mkdir -p "${keyDir}"
	ssh-keygen -q -t ed25519 -N "" -C releases@silkterm -f "${keyDir}/id" </dev/null
	namespace="silkterm-release"
	tag="v0.0.0-test"
	mkdir -p "${work}/dl/${tag}" "${work}/inst"
	printf 'checksum line\n' > "${work}/dl/${tag}/sums.txt"
	ssh-keygen -Y sign -f "${keyDir}/id" -n "${namespace}" "${work}/dl/${tag}/sums.txt" >/dev/null 2>&1

	## install.bash, sourced: its entry block is guarded, the release page is a
	## folder, and fGet copies from it. fFail exits, so each run is a subshell.
	fBashInstaller(){
		cp "${1}" "${work}/inst/sums.txt"
		## the settings and fGet are read by the sourced function, not by us
		# shellcheck disable=SC2034,SC2329
		(
			# shellcheck source=/dev/null
			source "${root}/../install.bash"
			releaseSignPubkey="$(cat "${keyDir}/id.pub")"
			dlBase="${work}/dl"
			fGet(){ cp "${1}" "${2}"; }
			fVerifySignature "${work}/inst" sums.txt "${tag}"
		) >/dev/null 2>&1
	}
	fCheck "install.bash accepts a signed checksums file" fBashInstaller "${work}/dl/${tag}/sums.txt"

	printf 'checksum line tampered\n' > "${work}/tampered.txt"
	if fBashInstaller "${work}/tampered.txt"; then
		echo "  FAIL install.bash accepted a changed checksums file"; failures=$((failures + 1))
	else
		echo "  ok   install.bash refuses a changed one"
	fi

	## and a signature by a different key is not the release key's
	ssh-keygen -q -t ed25519 -N "" -C other -f "${keyDir}/other" </dev/null
	rm -f "${work}/dl/${tag}/sums.txt.sig"   ## or ssh-keygen stops to ask about overwriting
	ssh-keygen -Y sign -f "${keyDir}/other" -n "${namespace}" "${work}/dl/${tag}/sums.txt" >/dev/null 2>&1
	if fBashInstaller "${work}/dl/${tag}/sums.txt"; then
		echo "  FAIL install.bash accepted another key's signature"; failures=$((failures + 1))
	else
		echo "  ok   install.bash refuses another key's signature"
	fi

	## install.ps1 the same way, wherever pwsh is. The driver makes its own key.
	if command -v pwsh >/dev/null 2>&1; then
		pwsh -NoProfile -File "${meDir}/verify-sign.ps1" -Installer "${root}/../install.ps1" || failures=$((failures + 1))
	else
		echo "  skip install.ps1 signing (no pwsh)"
	fi
else
	echo "  skip signing (no ssh-keygen)"
fi

if ((failures)); then echo "${failures} failed"; exit 1; fi
echo "all passed"

##	History:
##		- 20260908 JC: Created.
##		- 20260917 JC: The installers' own verify functions, and install.ps1 through verify-sign.ps1.
