#!/usr/bin/env bash

##	Purpose:
##		- Non-interactive GIT_EDITOR for the cicd publish stage.
##	 	  git invokes it as `git-auto-msg.bash <msgfile>`. If the message is empty
##		  (a plain `git commit` with no -m), fill it from $GIT_AUTO_MESSAGE; if git already pre-filled one
##		  (e.g. a `pull --no-ff` merge message), leave it. Either way, never block.

##	Copyright (c) 2026 Bubbles
##	Licensed under The MIT License (MIT). Full text at:
##		https://mit-license.org/
##	SPDX-License-Identifier: MIT

set -euo pipefail

file="$1"

## Ask git for its comment string rather than assume `#`: core.commentChar can
## be anything, and git strips by whatever it is.
cc="$(printf 'x\n' | git stripspace --comment-lines)"; cc="${cc% x}"
scissors="${cc} ------------------------ >8 ------------------------"

## What git keeps: everything above the scissors (commit.verbose puts a diff
## below it), minus comment lines and blank runs.
fKept(){ SC="$scissors" awk '$0 == ENVIRON["SC"] {exit} {print}' | git stripspace --strip-comments; }

tmpl="$(git config --path commit.template 2>/dev/null || true)"
existing="$(fKept <"$file")"
## An untouched template is not a message; git refuses it as unedited.
if [[ -n "$existing" && -n "$tmpl" && -r "$tmpl" && "$existing" == "$(fKept <"$tmpl")" ]]; then existing=""; fi
[[ -n "$existing" ]] && exit 0

msg="${GIT_AUTO_MESSAGE:-}"
[[ -n "$(printf '%s' "$msg" | tr -d '[:space:]')" ]] || msg="CI/CD automated commit"
kept="$(printf '%s\n' "$msg" | git stripspace --strip-comments)"
if [[ "$kept" != "$(printf '%s\n' "$msg" | git stripspace)" ]]; then
	printf 'git-auto-msg: message has a line starting with "%s", which git drops as a comment\n' "$cc" >&2
	exit 1
fi

## Keep git's own comment lines and anything from the scissors down; drop
## template text above them.
rest="$(SC="$scissors" CC="$cc" awk 'f || $0 == ENVIRON["SC"] {f=1; print; next} index($0, ENVIRON["CC"]) == 1 {print}' "$file")"
printf '%s\n\n%s\n' "$msg" "$rest" >"$file"

##	History:
##		- 20260628: First commit.
