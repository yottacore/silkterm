#!/usr/bin/env bash

##	Purpose:
##		- git and gh calls that reach a git host go through 'gitsby raw' where
##		  gitsby is installed, so they act as the account this folder belongs to
##		  rather than whichever key or login git and gh would pick on their own.
##		  Plain git and gh otherwise, so a clone elsewhere runs the same.
##		- Source this, then call fRemoteGit / fRemoteGh as git / gh.
##	History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	Licensed under The MIT License (MIT). Full text at:
##		https://mit-license.org/
##	SPDX-License-Identifier: MIT

fRemoteGit(){ if command -v gitsby >/dev/null 2>&1; then gitsby raw git "${@}"; else git "${@}"; fi; }
fRemoteGh(){ if command -v gitsby >/dev/null 2>&1; then gitsby raw gh "${@}"; else gh "${@}"; fi; }

##	History:
##		- 20260925 JC: Created.
