#!/bin/bash

#  shellcheck disable=2001  ## 'See if you can use ${variable//search/replace} instead.' Complains about good uses of sed.
#  shellcheck disable=2016  ## 'Expressions don't expand in single quotes, use double quotes for that.' I know, and I often want an explicit '$'.
#  shellcheck disable=2034  ## 'variable appears unused.' Complains about valid use of variable indirection (e.g. later use of local -n var=$1)
#  shellcheck disable=2046  ## 'Quote to prevent word-splitting.' (OK for integers.)
#  shellcheck disable=2086  ## 'Double quote to prevent globbing and word splitting.' (OK for integers.)
#  shellcheck disable=2119  ## 'Use foo "$@" if function's $1 should mean script's $1.' Confusing and inapplicable.
#  shellcheck disable=2120  ## 'Foo references arguments, but none are ever passed.' Valid function argument overloading.
#  shellcheck disable=2128  ## 'Expanding an array without an index only gives the element in the index 0.' False hits on associative arrays.
#  shellcheck disable=2155  ## 'Declare and assign separately to avoid masking return values.' Cumbersome and unnecessary. For integers it's sometimes required to even come into existence for counters.
#  shellcheck disable=2162  ## 'read without -r will mangle backslashes.'
#  shellcheck disable=2178  ## 'Variable was used as an array but is now assigned a string.' False hits on associative arrays with e.g. 'local -n assocArray=$1'.
#  shellcheck disable=2181  ## 'Check exit code directly, not indirectly with $?.'
#  shellcheck disable=2317  ## 'Can't reach.' (I.e. an 'exit' is used for debugging - and makes an unusable visual mess.)
## shellcheck disable=2002  ## 'Useless use of cat.'
## shellcheck disable=2004  ## '$/${} is unnecessary on arithmetic variables.' Inappropriate complaining?
## shellcheck disable=2053  ## 'Quote the right-hand sid of = in [[ ]] to prevent glob matching.' Disable for Yoda Notation.
## shellcheck disable=2143  ## 'Use grep -q instead of echo | grep'

##	Purpose:
##		- Project-specific CI/CD settings.
##		- To reuse this pipeline in another project,
##		  copy the whole cicd/ directory and edit THIS file (cicd.bash stays generic).
##		  All command arrays run from the repo root. The engine prepends ~/.cargo/bin to
##		  PATH so the rustup toolchain (cross targets, edition 2024) wins over system rust.
##	History: At bottom of script.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	Licensed under The MIT License (MIT). Full text at:
##		https://mit-license.org/
##	SPDX-License-Identifier: MIT


## Check if sourced
declare -i isSourced_t6wqf=0; [[ "${BASH_SOURCE[0]}" == "${0}" ]] || isSourced_t6wqf=1
((isSourced_t6wqf)) || { echo -e "\nError in $(basename "${BASH_SOURCE[0]}"): This script is meant to be 'sourced' from within another script.\n"; exit ${ERRNUM_MSG_ALREADY_SHOWN}; }


## Identity
APP_NAME="SilkTerm"
EXE_NAME="silkterm"

## Stage 1: format the source in place before anything is compiled or tested.
## Empty it (FMT_CMD=()) when reusing the pipeline in a non-Rust project.
FMT_CMD=(cargo fmt)
## Non-mutating variant for the --gate mode (fails on drift instead of rewriting).
FMT_CHECK_CMD=(cargo fmt --check)

## Pinned helper tools, "name|version|command..." each. The list lives in
## tool-pins.txt beside this file, which cicd-win.ps1 reads too; the engine warns
## (non-gating) when an installed tool has drifted from its pin.
TOOL_PINS=()
while IFS='|' read -r pinName pinVer pinWhere pinCmd; do
	[[ -n "${pinName}" && "${pinName}" != \#* && "${pinWhere}" != "windows" ]] || continue
	TOOL_PINS+=("${pinName}|${pinVer}|${pinCmd}")
done < "$(dirname "${BASH_SOURCE[0]}")/tool-pins.txt"
unset pinName pinVer pinWhere pinCmd

## Where cargo writes build output. CARGO_TARGET_DIR moves it, and a run driven
## from another host does exactly that (cicd-win.ps1 -Wsl builds this tree with
## the target dir on its own filesystem, so the two platforms' objects don't
## evict each other). Nothing below may assume 'target' - a stage that does
## still builds, then looks for its binary somewhere it was never put.
TARGET_DIR="${CARGO_TARGET_DIR:-target}"

## Stage 2: debug build (fast compile sanity)
DEBUG_BUILD_CMD=(cargo build)

## Stage 3: regression tests
TEST_CMD=(cargo test)

## Stage 3 (after tests): lints. Gating; house allows live in the workspace
## Cargo.toml [workspace.lints.clippy]. PROBE decides tool availability -
## a failed probe skips the step with a warning instead of aborting.
## rust-toolchain.toml pins one toolchain for every rustup-routed cargo, but a
## shell where system cargo wins PATH can still populate target/ with the other
## rustc (E0514: artifacts from a different compiler), so lint pins the rustup
## PATH and keeps its own target dir as insurance. That dir hangs off
## CARGO_TARGET_DIR when one is set, so a run driven from elsewhere (a Windows
## box delegating the Linux half through WSL) keeps its build output where it
## put the rest of it, rather than in the source tree it was handed.
LINT_PROBE=(env "PATH=${HOME}/.cargo/bin:${PATH}" cargo clippy --version)
LINT_CMD=(env "PATH=${HOME}/.cargo/bin:${PATH}" "CARGO_TARGET_DIR=${TARGET_DIR}/lint" cargo clippy --workspace --all-targets -- -D warnings)

## Stage 3 (after lints): the same lints again for Windows. The cross-builds build
## the binary only, and the lints above run for this box, so Windows-only code is
## seen by neither - a cfg-gated item used by an ungated test broke the suite there
## and two lint findings sat in code this box never compiles. Both took a Windows
## box to find, and both are answered from here in under a minute. Empty () to
## disable.
XLINT_CMD=(env "PATH=${HOME}/.cargo/bin:${PATH}" "CARGO_TARGET_DIR=${TARGET_DIR}/lint" cargo clippy --workspace --all-targets --target x86_64-pc-windows-gnu -- -D warnings)

## Stage 3 (after tests): the fuzz soak. The targets are ordinary tests, so the
## run above already exercised them at a fraction of a second each; this gives
## every one a real budget. Seconds PER TARGET, and they run in parallel, so the
## wall time is roughly this plus the build. 0 disables; --quick sets it to 0.
FUZZ_SECS=20
FUZZ_CMD=(cargo test fuzz::)

## Stage 3 (after lints): dependency police (licenses/advisories/duplicates,
## policy in deny.toml). Non-gating for now; tighten once the report is tuned.
DENY_PROBE=(cargo deny --version)
DENY_CMD=(cargo deny check)

## Stage 3 (last): headless scroll regression harness. Slow (a private Xvfb + GL
## per scene), so it is skipped under --quick; non-fatal on an environment miss,
## but a measured scroll regression aborts. Empty () to disable.
SCROLL_HARNESS=(cicd/tests/scroll/run.bash)

## Stage 3: graphical scenarios against a real Windows desktop, over ssh. Slow, so
## it is skipped under --quick. A box that is off, or whose session is locked, is a
## skip and not a failure - both are somebody's machine rather than build hardware.
## Empty () to disable. Extra scenario names may be listed after run.bash.
WINGUI_HARNESS=(cicd/tests/wingui/run.bash smoke injchar settingsdlg perfladder pathannounce consolemsg savenotice openwith)

## Stage 3: what the dogfood launcher does to files it did not create. Needs pwsh;
## skipped with a warning where it is missing. Empty () to disable.
LAUNCHER_HARNESS=(cicd/tests/launcher/run.ps1)

## Also run the harness a second time under a headless Wayland compositor (cage), to
## prove the Wayland backend renders + scrolls the same as X11. Self-skips (non-fatal)
## where cage is not installed. 0/unset to disable.
SCROLL_HARNESS_WAYLAND=1

## Stages 4 + 5: how many times a fat-LTO build may be attempted before the pipeline
## calls it a failure. rustc crashes inside LLVM here every so often and compiles the
## same source clean on the next try; 1 disables retrying.
BUILD_ATTEMPTS=3

## Stage 5: native release build + its artifact (this is what gets dogfooded)
RELEASE_NATIVE_CMD=(cargo build --release)
RELEASE_NATIVE_BIN="${TARGET_DIR}/release/${EXE_NAME}"
RELEASE_NATIVE_OSARCH="linux-x86_64"

## Stage 5: cross-release targets. One per line: "label|os-arch|artifact|command...".
## os-arch feeds the versioned artifact name (<exe>-<version>-<os-arch>[.exe]).
## Set BUILD_CROSS=0 to skip them for a quick local run.
BUILD_CROSS=1
CROSS_TARGETS=(
	"Windows x86_64 (mingw)|windows-x86_64|${TARGET_DIR}/x86_64-pc-windows-gnu/release/${EXE_NAME}.exe|cargo build --release --target x86_64-pc-windows-gnu"
	"Linux ARM64 (zig)|linux-arm64|${TARGET_DIR}/aarch64-unknown-linux-gnu/release/${EXE_NAME}|cargo zigbuild --release --target aarch64-unknown-linux-gnu"
	"Windows ARM64 (zig)|windows-arm64|${TARGET_DIR}/aarch64-pc-windows-gnullvm/release/${EXE_NAME}.exe|cargo zigbuild --release --target aarch64-pc-windows-gnullvm"
)

## Stage 5 (after builds): collect the built binaries under versioned names plus
## a sha256 checksums file, ready to attach to a release as plain uploads.
## Naming scheme (stable; download links depend on it):
##   <exe>-<version>-<os-arch>[.exe]   e.g. silkterm-1.0.0-beta1-linux-x86_64
##   <exe>-<version>-sha256sums.txt
## Version comes from source/Cargo.toml alone. Empty to disable collection.
RELEASE_ARTIFACT_DIR="cicd/artifacts/release"   # relative to repo root; gitignored
VERSION_MANIFEST="source/Cargo.toml"            # the single version source

## Stage 6: distributable packages, built from the stage-5 release binaries (never
## rebuilt) when --quick is NOT passed. Linux -> .deb + .rpm (cargo-deb /
## cargo-generate-rpm, metadata in source/Cargo.toml). Windows -> a single self-
## contained NSIS installer .exe per arch (makensis), which upgrades an existing
## install in place. macOS is built on a Mac by the private runner below. BSD is
## deferred: this box has no FreeBSD sysroot to cross-build for. ARM64 packages
## follow the same --no-arm gate as the ARM release builds. Packages go in
## RELEASE_ARTIFACT_DIR and fold into the sha256sums. Set PACKAGE_ENABLE=0 (or
## --no-package) to skip.
PACKAGE_ENABLE=1
NSIS_TEMPLATE="cicd/packaging/windows/installer.nsi.in"

## Stage 6, after the packages: the macOS build and the Microsoft Store bundle.
## Both need signing material that stays out of this repo, so their scripts live
## in a private one. They run only when that repo is checked out at this path and
## has its runner, which takes --public <this repo>, does its own waiting on the
## Mac and the Windows box, and exits 0 when it built or skipped and 1 when a job
## failed. Its output stays in the private repo. Empty, or --no-private, to skip.
PRIVATE_RUNNER="${SILK_PRIVATE_RUNNER-$(cd "${root}/../.." && pwd)/silkterm-private/repo/cicd/run.bash}"

## Stage 4: profiler (non-gating artifact, not a pass/fail test). Builds an
## optimized+symbols binary (cargo --profile $PROFILE_PROFILE --features
## $PROFILE_FEATURE), runs the real app under an in-process sampler against a heavy
## workload for $PROFILE_SECS, and writes a flamegraph SVG. See cicd.bash for the
## skip-vs-abort failure policy ($PROFILE_STRICT to force abort on any failure).
PROFILE_ENABLE=1
PROFILE_SECS=8
PROFILE_FEATURE="profiling"
PROFILE_PROFILE="profiling"
PROFILE_BIN="${TARGET_DIR}/profiling/${EXE_NAME}"
PROFILE_WORKLOAD_SCRIPT="cicd/utility/n8output-random-unicode.py"
PROFILE_WORKLOAD_ARGS="600 0"          # <duration_s> <delay_s>; duration >> PROFILE_SECS, no delay = max output
PROFILE_OUT_DIR="cicd/artifacts/profiling"  # relative to repo root; created if missing (gitignored)
PROFILE_STRICT=0                        # 1 = any profiler failure aborts the pipeline

## Demo video re-record (cicd/utility/demo-video/demo-video.py). Off by default -
## only worth re-recording after major visual/feature changes; flip to 1 or pass
## --demo for one run. Also skipped under --quick. Video GFS-rotates into
## ../private/demo-video/; the README highlight gif goes in assets/demo.gif.
DEMO_ENABLE=0

## Full run output is tee'd here (gitignored) so warnings from any stage can be
## reviewed after the fact. Kept rotated like the flamegraphs.
LINT_LOG_DIR="cicd/artifacts/lint"      # relative to repo root; created if missing (gitignored)

## Old SVGs are pruned by gfs_rotate (cicd/utility/include/gfs-rotate.bash): keeps
## ~30 - first + newest-per-hour/day/week/month/year + last 10. Tune with the
## GFS_KEEP_* env vars (GFS_KEEP_FREQUENT, GFS_KEEP_DAILY, ...) if needed.

## Stage 7: dogfood. Every build this run made is installed under a fixed name in
## the synced app dir for the platform it TARGETS, so a box that cannot build for
## itself still gets a current binary over Dropbox. Nothing here keeps a pool of
## dated copies any more - the 'runterm' launcher on each box does that, in its own
## rotated versions folder.
## One line per flavor: "<os-arch>|<name at dest>|<dir>[|<dir>...]", where os-arch
## names a CROSS_TARGETS row or RELEASE_NATIVE_OSARCH. The first writable dir wins;
## none writable warn-skips, and a target not built this run is skipped quietly.
## The copy keeps its mtime, which is the build date the launcher reads.
DOGFOOD_DESTS=(
	"linux-x86_64|${EXE_NAME}|${HOME}/synced/0-0/common/exec/app/linux"
	"windows-x86_64|${EXE_NAME}.exe|${HOME}/synced/0-0/common/exec/app/mswin"
	## ARM64 builds are released but not dogfooded: an app dir is per-OS, so only
	## one binary can hold the name, and both boxes here are x86_64.
	## The Mac build comes back from the private runner as one universal binary.
	"macos-universal|${EXE_NAME}|${HOME}/synced/0-0/common/exec/app/macos"
)
## Dropped beside each installed binary. The icon is what a .desktop entry points
## at, by way of the launcher's symlink dir. Empty either to skip it.
DOGFOOD_ICON="source/assets/logo.png"
## Which build a copy holds: "<toolchain: gnu|msvc><built on: l|m|b|w><target: l|m|b|w><arch: i|a>"
## - so a pool of copies from several hosts stays readable. It goes in a "<name>.tag"
## sidecar, because a cross-build says nothing about the box that later reads it.
## Left unset it is derived per dest from this host plus the target; set it to pin
## every dest to one value, empty to drop the sidecar.
# DOGFOOD_TAG="gnulli"

## Signing the release. A checksum file fetched from the same place as the
## artifact it covers proves the download was not corrupted in transit, which TLS
## already gave; a signature is what says the release came from here.
##
## The private key never goes in the repo. To set one up once:
##   ssh-keygen -t ed25519 -C releases@silkterm -f ~/.ssh/silkterm-release
## then put the CONTENTS of ~/.ssh/silkterm-release.pub into releaseSignPubkey
## in install.bash and install.ps1, and point this at the private half. Empty
## means the release goes out unsigned, and release.bash says so.
RELEASE_SIGN_KEY="${SILKTERM_RELEASE_KEY:-}"
RELEASE_SIGN_IDENTITY="releases@silkterm"
RELEASE_SIGN_NAMESPACE="silkterm-release"

## Stage 7: backup + publish to git (runs from repo root).
GIT_PUBLISH=(cicd/utility/n8git_backup-and-publish)

## Extra backup excludes for this project, on top of the publisher's own. Those
## already cover cicd/artifacts scratch (logs, profiling, GUI captures, the
## size-comparison downloads), a wineprefix's dosdevices and drive_c, which map
## Z: to '/', private/source, and every 0_archive. cicd/artifacts/release stays
## in: the packages and installers there exist nowhere else. A pattern naming a
## dir skips it whole, without rar going in, so one line per dir is enough.
##
## forks/ holds working clones of the patched dependencies. Each has its own
## remote, so the archive would otherwise carry 50 MB of a copy of something
## already kept elsewhere, and more as each one gets built. What that costs is
## work committed in a clone and not pushed, which is not in the backup either -
## already true of this repo, whose .git is excluded the same way.
##
## target/mmap-bench is the minimap rig's scratch: a 32 MiB flood file it makes
## on demand, plus one log per run.
##
## Everything else under target/ that is not a final build is scratch too: the
## before and after builds kept for a comparison, their captures, and a docs
## backup from the 09-15 history rewrite. source/target and the clipboard-race
## test crate's target are stray lint and test builds, and target/darwin is a
## macOS type check. A new scratch dir under target/ needs a line here.
##
## The wallpaper originals (about 350 MB) and duplicates (80 MB) sit behind the
## private/wallpaper/source symlink, which rar follows. The originals are the
## full-size sources the shipped pack is made from, and both are kept in their
## own tree outside this project.
##
## One rar pattern per line, no '-x' prefix and no shell quoting: the publish
## script adds the flag and passes each line through as one argument.
export GIT_BACKUP_AND_PUBLISH_RAR_EXCLUDES='*/forks
*/target/mmap-bench
*/target/gpu-stress
*/target/gpuctl
*/target/darwin
*/target/lightnew
*/target/lightold
*/target/mapopt
*/target/wpmix
*/target/rewrite-20260915.docs-backup
*/target/repro.*
*/source/target
*/clipboard-race/target
*/wallpaper/source/010_origs
*/wallpaper/source/0_dupes'

## Set a non-empty commit message to publish hands-off (suppresses the script's
## prompt and supplies the message so `git commit` won't open an editor). Left
## empty, publish is interactive unless -m/--message or -y is given (see cicd.bash).
PUBLISH_AUTO_MESSAGE=""


##	History:
##		- 2026-06-05 JC: Created.
