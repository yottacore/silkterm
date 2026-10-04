##	Proves the harness can still report a failure. Run it by hand after changing
##	anything in _run.ps1 or run.bash - it must come back 'fail' with exit 1:
##		cicd/tests/wingui/run.bash --host <box> _selftest
##	Delete the check below and it must come back 'fail' for a second reason, that
##	the scenario asserted nothing. Never listed in WINGUI_HARNESS.

##	History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

fNote "this scenario exists to fail"
[void](fCheck "an assertion that is false" $false)

##	History:
##		- 20260908 JC: Created.
