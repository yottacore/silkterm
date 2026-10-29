##	Copyright (C) 2026 Jim Collier
##	Licensed under The MIT License (MIT). Full text at:
##		https://mit-license.org/
##	SPDX-License-Identifier: MIT

<#
.SYNOPSIS
	Windows-native CI/CD pipeline for SilkTerm.
.DESCRIPTION
	A PowerShell port of the Linux cicd.bash, doing as much of the same work as
	Windows allows - including the parts cicd.bash farms out to helper scripts
	(the git backup/publish). Does NOT touch cicd.bash (that stays the
	Linux/cross pipeline).

	Stages (fail-fast; any error aborts before the next stage):
	 0. remote sync    (fetch; fast-forward if safely behind; abort if diverged)
	 1. format         (cargo fmt)
	 2. debug build    (cargo build)
	 3. tests + lints  (cargo test; clippy + cargo-deny are ADVISORY here, but
	                    the advisory scan gates)
	 4. release builds  x86_64 msvc AND gnu (always both), + ARM64 when its
	                    toolchain is present (auto-detected, else warn-skip)
	 5. packages       (NSIS installer .exe per built arch, if makensis found)
	 6. linux half     (-Wsl: hand the Linux-only work to WSL2 - see below)
	 7. dogfood        (copy the best x86_64 build to <dogfood>\silkterm.exe)
	 8. publish        (stash -> pull -> add -> commit -> push, current branch)

	What Windows can't do (dropped vs cicd.bash): the profiler (pprof's
	SIGPROF sampler is Unix-only - the profiling feature can't even compile
	for a Windows target), the headless scroll harness / demo
	(need Xvfb), .deb/.rpm packages (Linux), and the rar version-archive step
	of publish (skipped by request). clippy is advisory, not gating: the
	Unix-gated ctl code emits dead_code warnings here, so -D warnings can't
	pass.

	-Wsl gets all of that back on a box that has WSL2, by running the Linux
	pipeline (cicd.bash --no-windows) there against THIS working tree. The
	two halves split cleanly: Windows builds what only Windows can, msvc
	above all, and WSL builds what only Linux can. Neither repeats the
	other's targets. Off by default - it roughly doubles a run.

	Dogfood pick: prefer the msvc build IF it's self-contained (statically
	linked, no VCRUNTIME140/MSVCP140 dependency); else the gnu build; else
	whichever single build exists. The fixed silkterm.exe goes to the SYNCED
	app dir; the runterm launcher keeps its own rotated pool locally
	(the two stay separate dirs on purpose).
.PARAMETER Yes
	Run unattended (no confirm / message prompt).
.PARAMETER Quiet
	Quiet + unattended (implies -Yes); publish runs quiet too.
.PARAMETER Quick
	Skip the slow stages (ARM builds + packages).
.PARAMETER Long
	Everything, the steps that take hours too. The Windows half has none, so it
	matters under -Wsl, which hands --long to the Linux half. -Slow is another
	name for it. Not with -Quick or -BacklogRunQuick.
.PARAMETER BacklogRunQuick
	The fast checks only, then exit: every area, or the ones named after it.
	Areas: fmt (cargo fmt --check), clippy (clippy and cargo-deny, advisory
	here), tests (cargo test), scripts (the PowerShell lint), docs (cicd.bash
	only, so under -Wsl). No sync, dogfood, publish or prompt.
.PARAMETER Gate
	Another name for -BacklogRunQuick.
.PARAMETER BacklogRunFull
	Every check of the default run, with the format checked, not rewritten; no
	sync, dogfood or publish, and no prompt.
.PARAMETER MergeGate
	The same checks, before a merge to main.
.PARAMETER Ci
	Another name for -MergeGate.
.PARAMETER Areas
	The areas -BacklogRunQuick runs, named after it: -BacklogRunQuick fmt tests.
.PARAMETER NoFmt
	Skip the formatter stage.
.PARAMETER NoArm
	Skip the ARM64 release builds + their packages.
.PARAMETER NoPackage
	Skip the packages stage (NSIS installers).
.PARAMETER NoDogfood
	Skip the dogfood install.
.PARAMETER NoPublish
	Skip the git publish stage.
.PARAMETER NoSync
	Skip the remote sync check (stage 0).
.PARAMETER NoLeakcheck
	-Wsl only: the Linux half skips its leak checks (cicd.bash --no-leakcheck).
	The Windows half has none.
.PARAMETER Leakcheck
	-Wsl only: the Linux half runs its leak checks under -Quick too.
.PARAMETER NoRemote
	-Wsl only: the Linux half skips every other box, the Windows scenarios and
	the private runner (cicd.bash --no-remote).
.PARAMETER NoContainer
	-Wsl only: the Linux half runs on the distro itself, not in its image
	(cicd.bash --no-container). The Windows half uses none.
.PARAMETER KeepTemp
	Keep the run's test folder, and print where it is. Under -Wsl the Linux half
	keeps its own too (cicd.bash --keep-temp).
.PARAMETER List
	Print the steps and test IDs the other options would run, and run nothing.
	Under -Wsl the Linux half prints its own (cicd.bash --list).
.PARAMETER Wsl
	Also run the Linux half in WSL2 (.deb/.rpm, profiler, scroll harness -
	everything Windows can't do).
.PARAMETER WslDistro
	Which distribution to use (default: the first WSL2 one).
.PARAMETER Message
	Publish hands-off with this commit message (no editor).
.PARAMETER Help
	Show this help.
.EXAMPLE
	pwsh cicd/cicd-win.ps1 [options]
.NOTES
	History: At bottom of script.
#>

## No positional binding: the words after -BacklogRunQuick are its areas.
[CmdletBinding(PositionalBinding = $false)]
param(
	[switch]$Yes,
	[switch]$Quiet,
	[switch]$Quick,
	[Alias('Slow')][switch]$Long,
	[switch]$BacklogRunQuick,
	[switch]$Gate,
	[switch]$BacklogRunFull,
	[switch]$MergeGate,
	[switch]$Ci,
	[switch]$NoFmt,
	[switch]$NoArm,
	[switch]$NoPackage,
	[switch]$NoDogfood,
	[switch]$NoPublish,
	[switch]$NoSync,
	[switch]$NoLeakcheck,
	[switch]$Leakcheck,
	[switch]$NoRemote,
	[switch]$NoContainer,
	[switch]$KeepTemp,
	[switch]$List,
	[switch]$Wsl,
	[string]$WslDistro = "",
	[string]$Message = "",
	[switch]$Help,
	[Parameter(ValueFromRemainingArguments)][string[]]$Areas = @()
)

## Requires PowerShell 7+ (pwsh): this script uses $IsWindows and PS7 semantics.
## Windows PowerShell 5.1 has no $IsWindows, so the StrictMode guard below would
## throw a cryptic error instead. Bail early with a clear pointer.
if ($PSVersionTable.PSVersion.Major -lt 6) {
	Write-Error "cicd-win.ps1 needs PowerShell 7+ (pwsh); you're on Windows PowerShell $($PSVersionTable.PSVersion). Run: pwsh -File cicd/cicd-win.ps1"
	exit 1
}

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
## We drive native tools (git, cargo) by hand and read $LASTEXITCODE - several
## probes (git diff --quiet, clippy) return non-zero ON PURPOSE. Keep a non-zero
## native exit from throwing so those reads work regardless of the caller's shell.
$PSNativeCommandUseErrorActionPreference = $false
## Low priority for the whole run, set here so every child inherits it.
try { (Get-Process -Id $PID).PriorityClass = 'BelowNormal' } catch { Write-Warning "could not lower the priority: $_" }

if ($Help) {
	Get-Help -Name $PSCommandPath -Detailed
	exit 0
}

## Windows-only: this pipeline shells out to makensis, reads PE imports, and
## writes into a Windows dogfood dir. Refuse to run anywhere else.
if (-not $IsWindows) {
	Write-Error "cicd-win.ps1: this pipeline only runs on Windows (use cicd/cicd.bash on Linux)."
	exit 1
}

## -Quiet implies unattended; both suppress the preflight prompt.
$Unattended = ($Yes -or $Quiet)


#==============================================================================
# Configuration

## Repo root = the parent of this script's cicd/ dir. All cargo commands run here.
$Root    = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$AppName = "SilkTerm"
$ExeName = "silkterm"

## One folder for every file the tests write, shared by both cargo test runs and
## the installer tests, made once the run is past its prompt. TEMP and TMP stay
## put, so builds keep the system temp dir.
. (Join-Path $Root "cicd\tests\_testdir.ps1")
## fSafeRm and fSafeRm_Under: every recursive removal goes through them.
. (Join-Path $Root "cicd\utility\include\saferm.ps1")

## Test folders that failed runs kept, past the newest this many, go at the start
## of the next run. Only ones a run here wrote down, with its token.
$TestFolderKeep = 5

## The single version source (first `version = "..."` line).
$VersionManifest = Join-Path $Root "source\Cargo.toml"

## Release build matrix. x86_64 msvc + gnu build every run (house rule: always
## build both). ARM64 rows are attempted only when their toolchain is detected
## (see fArmSkipReason); otherwise they warn-skip. os-arch feeds the artifact
## name (<exe>-<version>-<os-arch>.exe).
##
## No arm64 msvc row: gnullvm is the shipped ARM64 binary, and the only reason to
## keep an msvc build is local debugging, which an x86_64 box can't do to an ARM64
## exe anyway. Add it back on a machine that can actually run one.
$Targets = @(
	[pscustomobject]@{ Arch="x86_64"; Tk="msvc";    Triple="x86_64-pc-windows-msvc";     OsArch="windows-x86_64-msvc";    Builder="build";    Arm=$false }
	[pscustomobject]@{ Arch="x86_64"; Tk="gnu";     Triple="x86_64-pc-windows-gnu";      OsArch="windows-x86_64-gnu";     Builder="build";    Arm=$false }
	[pscustomobject]@{ Arch="arm64";  Tk="gnullvm"; Triple="aarch64-pc-windows-gnullvm"; OsArch="windows-arm64-gnullvm";  Builder="zigbuild"; Arm=$true  }
)

## Collected release binaries + checksums go here (its own dir so the Linux
## pipeline's cicd/artifacts/release wipe can't nuke Windows artifacts, or v.v.).
$ReleaseArtifactDir = Join-Path $Root "cicd\artifacts\release-win"

## NSIS installer template (shared with the Linux pipeline).
$NsisTemplate = Join-Path $Root "cicd\packaging\windows\installer.nsi.in"

## Full-run transcript (gitignored, alongside the Linux lint logs' sibling).
$LogDir = Join-Path $Root "cicd\artifacts\lint-win"

## Dogfood: the fixed-name copy, into the SYNCED app dir so it rides Dropbox and
## any box can grab it. Deliberately a SEPARATE dir from the launcher's local
## versions folder - runterm copies from here into that, and the two never share a
## folder. Same layout the Linux pipeline's DOGFOOD_DESTS uses.
## First one that exists wins, the same list the launcher reads. 'synced' can read
## as empty on Windows, so the real Dropbox spelling follows it.
$DogfoodDirs     = @(
	(Join-Path $env:USERPROFILE "synced\0-0\common\exec\app\mswin")
	(Join-Path $env:USERPROFILE "Dropbox\0-0\common\exec\app\mswin")
)
$DogfoodDir      = @($DogfoodDirs | Where-Object { Test-Path -LiteralPath $_ }) + $DogfoodDirs | Select-Object -First 1
$DogfoodFixedExe = "silkterm.exe"
## Dropped beside it: the icon a shortcut points at, and a sidecar naming the build,
## since a cross-build says nothing about the box that later reads it.
$DogfoodIcon     = "source\assets\logo.png"

## Pinned helper-tool versions, shared with cicd.bash: the lines of
## tool-pins.txt marked windows or both. Warn (non-gating) when an installed tool
## has drifted, so a box update can't silently change results.
$ToolPinsFile = Join-Path $PSScriptRoot "tool-pins.txt"

## Full-run transcripts kept in $LogDir. Nothing reads the old ones, so the
## newest few are enough.
$LogKeep = 30

## Half the CPUs, or a quarter when the box is busy or the step is hard on it.
## fSetJobs works it out again before each step.
$Cores       = [Environment]::ProcessorCount
$CicdMaxJobs = [Math]::Max(1, [Math]::Floor($Cores / 2))
$script:JobsLast = 0

## -BacklogRunQuick's areas, the same as cicd.bash's, in their default order.
$QuickAreas  = @('fmt', 'clippy', 'tests', 'scripts', 'docs')
$QuickTitles = @{ fmt = 'Format check'; clippy = 'Lints (advisory)'; tests = 'Tests'; scripts = 'Script lints (advisory)'; docs = 'Docs checks' }

## Where the toolchains live. cargo/rustup first, then the mingw linker for the
## gnu target (matches the memory'd build setup).
$CargoBin  = Join-Path $env:USERPROFILE ".cargo\bin"
$MingwBin  = "C:\ProgramData\mingw64\mingw64\bin"


#==============================================================================
# Output helpers (mirror cicd.bash: fEcho / fEcho_Clean / fSection / steps)

$script:WasLastEchoBlank = $false
$script:Letterbox = ([string][char]0x2022) * 79

function fEcho_Clean {
	param([string]$Msg = "")
	if ($Msg) { Write-Host $Msg; $script:WasLastEchoBlank = $false }
	elseif (-not $script:WasLastEchoBlank) { Write-Host ""; $script:WasLastEchoBlank = $true }
}
function fEcho     { param([string]$Msg = ""); if ($Msg) { fEcho_Clean "[ $Msg ]" } else { fEcho_Clean } }
function fSection  { param([string]$Msg);      fEcho_Clean; fEcho_Clean $script:Letterbox; fEcho $Msg }
function fNote     { param([string]$Msg); fEcho_Clean $Msg }

## Over ssh, or into a file, pwsh writes in the console's code page, 437 by
## default, where the rule's dot is the bell byte. Write UTF-8 there instead, as
## cicd.bash does. Write-Host keeps the encoding of its first write, so this has
## to run before any output. Where the dot still can't be written, the rule is
## ASCII. Returns the encoding to put back, or $null when nothing changed.
function fUtf8Output {
	if (-not [Console]::IsOutputRedirected) { return $null }
	$was = [Console]::OutputEncoding
	if ($was.CodePage -ne 65001) {
		try { [Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false) }
		catch { Write-Verbose "output stays in code page $($was.CodePage): $_" }
	}
	$now = [Console]::OutputEncoding
	if ($now.GetString($now.GetBytes($script:Letterbox)) -ne $script:Letterbox) { $script:Letterbox = '-' * 79 }
	if ($now.CodePage -eq $was.CodePage) { return $null }
	return $was
}

## The run's steps, in order: StepsOn is what it takes and counts, StepsOff
## "<name> (<flag or reason>)" what it leaves out, for the plan.
$script:StepsOn  = [System.Collections.Generic.List[string]]::new()
$script:StepsOff = [System.Collections.Generic.List[string]]::new()
$script:StepNum  = 0
$script:StepName = ''
function fStamp { (Get-Date).ToString('yyyyMMdd-HHmmss.ff', [System.Globalization.CultureInfo]::InvariantCulture) }
function fStepLine { param([string]$Msg); fEcho "$(fStamp): Step $($script:StepNum) of $($script:StepsOn.Count); $Msg" }
function fStepIf {
	param([Parameter(Mandatory)][string]$Name, [string]$Off = '')
	if ($Off) { $script:StepsOff.Add("$Name ($Off)") } else { $script:StepsOn.Add($Name) }
}
## fStepOpen: the step's Starting line, or false when it's off.
function fStepOpen {
	param([Parameter(Mandatory)][string]$Name)
	$at = $script:StepsOn.IndexOf($Name)
	if ($at -lt 0) { return $false }
	if ($script:StepName) { fStepClose }
	$script:StepNum = $at + 1; $script:StepName = $Name
	fEcho_Clean; fEcho_Clean $script:Letterbox; fStepLine "Starting $Name"
	return $true
}
function fStepClose {
	param([ValidateSet('PASS', 'FAIL')][string]$Status = 'PASS')
	if (-not $script:StepName) { return }
	fStepLine "$Status $($script:StepName)"; $script:StepName = ''
	fEcho_Clean
}
function fShowSteps {
	fEcho_Clean "Steps .......: $($script:StepsOn.Count)$(if ($script:StepsOff.Count) { ', and these off:' })"
	foreach ($off in $script:StepsOff) { fEcho_Clean "    - $off" }
	if ($List) { for ($i = 0; $i -lt $script:StepsOn.Count; $i++) { fEcho_Clean "  Step $($i + 1) of $($script:StepsOn.Count)  $($script:StepsOn[$i])" } }
}

## Windows keeps no load average, so busy is the CPUs over half used, by
## Win32_Processor's LoadPercentage. A hard step also drops to Idle.
function fSetJobs {
	param([switch]$Hard)
	$why = ''
	if ($Hard) { $why = 'hard step' }
	else {
		$load = $null
		try { $load = (Get-CimInstance -ClassName Win32_Processor -ErrorAction Stop | Measure-Object -Property LoadPercentage -Average).Average } catch { $load = $null }
		if ($null -ne $load -and $load -gt 50) { $why = "busy, CPU at $([int]$load)%" }
	}
	$jobs = [Math]::Floor($Cores / 2)
	if ($why) { $jobs = [Math]::Min($jobs, [Math]::Floor($Cores / 4)) }
	$jobs = [int][Math]::Max(1, $jobs)
	$script:CicdMaxJobs    = $jobs
	$env:CARGO_BUILD_JOBS  = "$jobs"
	$env:RUST_TEST_THREADS = "$jobs"
	$class = if ($Hard) { 'Idle' } else { 'BelowNormal' }
	try { (Get-Process -Id $PID).PriorityClass = $class } catch { fWarn "could not set the priority to ${class}: $_" }
	if ($script:JobsLast -ne $jobs) { fNote "jobs: $jobs of $Cores CPUs$(if ($why) { " ($why)" })"; $script:JobsLast = $jobs }
}
function fWarn     { param([string]$Msg); fEcho "WARNING: $Msg" }
function fDie      { param([string]$Msg); fEcho "FAILED: $Msg"; fStepClose FAIL; fTestDir_End 1; exit 1 }


#==============================================================================
# Functions

## The run options as one set: the other names, what each run implies, and the
## pairs that make no sense, as cicd.bash has them. Returns why a run is refused,
## or nothing.
function fRunFlags {
	if ($Gate) { [Console]::Error.WriteLine("-Gate is another name for -BacklogRunQuick"); $script:BacklogRunQuick = $true }
	if ($Ci)   { [Console]::Error.WriteLine("-Ci is another name for -MergeGate");          $script:MergeGate = $true }
	$script:Areas = @($Areas | ForEach-Object { $_.ToLowerInvariant() })
	foreach ($area in $script:Areas) {
		if (-not $BacklogRunQuick)          { return "unknown option: $area (try -Help)" }
		if ($QuickAreas -notcontains $area) { return "unknown area: $area (areas: $($QuickAreas -join ' '))" }
	}
	$script:FullRunName = if ($MergeGate) { '-MergeGate' } elseif ($BacklogRunFull) { '-BacklogRunFull' } else { '' }
	$script:FmtCheck = [bool]$script:FullRunName
	if ($BacklogRunQuick -and $script:FullRunName) { return "-BacklogRunQuick and $($script:FullRunName) are different runs; pick one" }
	if ($script:FullRunName -and $Quick) { return "$($script:FullRunName) runs every check of the default run, so not -Quick" }
	if ($Long -and $Quick) { return "-Long runs everything, so not -Quick" }
	if ($Long -and $BacklogRunQuick) { return "-BacklogRunQuick runs the fast checks only, so not -Long" }
	if ($Leakcheck -and $BacklogRunQuick) { return "-Leakcheck adds to a full run; the backlog quick run leaves it out" }
	if ($script:FullRunName) {
		$script:Unattended = $true
		$script:NoSync = $true; $script:NoDogfood = $true; $script:NoPublish = $true
	}
}

## Run a native command from the repo root; abort (fail-fast) on a non-zero exit.
function fExec {
	param(
		[Parameter(Mandatory)][string]$What,
		[Parameter(Mandatory)][string]$File,
		[string[]]$CmdArgs = @()
	)
	& $File @CmdArgs
	if ($LASTEXITCODE -ne 0) { fDie "$What failed (exit $LASTEXITCODE): $File $($CmdArgs -join ' ')" }
}

## One of stage 3's test scripts, in a pwsh of its own, or in this one with
## -InProcess. Under -List it prints the script's test ID and name instead, so
## the list is the run's own calls.
function fScriptTest {
	param(
		[Parameter(Mandatory)][string]$What,
		[Parameter(Mandatory)][string]$Script,
		[hashtable]$Params = @{},
		[switch]$InProcess
	)
	$path = Join-Path $Root $Script
	if ($List) {
		$id = Select-String -LiteralPath $path -Pattern '^##\s+Test ID:\s+(\S+)' | Select-Object -First 1
		fEcho_Clean ("  {0,-7}  {1}" -f $(if ($id) { $id.Matches[0].Groups[1].Value } else { '-------' }), $What)
		return
	}
	if ($InProcess) { & $path @Params } else { & pwsh -NoProfile -NonInteractive -File $path }
	if ($LASTEXITCODE -ne 0) { fDie "$What failed (exit $LASTEXITCODE)" }
	fEcho "OK: $What"
}

## The installer tests. They run in this process and point TEMP and TMP at the
## test run folder, so both go back afterward for the builds that follow.
function fInstallerTests {
	fTestDir_Keep {
		## install.ps1's signature check, with the OpenSSH that ships on Windows.
		fScriptTest -What "installer signing" -Script "cicd\tests\release\verify-sign.ps1" -InProcess
		## ...and its temp folder step, where the shared temp folder is the one it guards against.
		fScriptTest -What "installer temp folder" -Script "cicd\tests\install\tempdir.ps1" -InProcess
		## ...and a real install, upgrade and repair, under both PowerShells.
		foreach ($shell in @("pwsh", "powershell")) {
			fScriptTest -What "installer on Windows ($shell)" -Script "cicd\tests\install\windows.ps1" -Params @{ Shell = $shell } -InProcess
		}
	}
}

## Stage 3's test scripts. -List goes through it too.
function fScriptTests {
	fInstallerTests
	## In a process of its own, since fTestDir_Use moves TEMP and TMP for the whole process.
	fScriptTest -What "test run folder removal" -Script "cicd\tests\testdir\remove.ps1"
	## The dogfood launcher's Start menu entry, and what a launch with nothing to do asks for.
	fScriptTest -What "launcher Start menu entry" -Script "cicd\tests\launcher\startmenu.ps1"
	## The wrapper a shortcut runs, finding PowerShell 7 with it gone from PATH.
	fScriptTest -What "launcher wrapper" -Script "cicd\tests\launcher\wrapper.ps1"
	## The guard every recursive removal goes through.
	fScriptTest -What "safe removal" -Script "cicd\tests\saferm\win.ps1"
}

## -List: the Rust tests cargo test runs, as ID and name, from the script
## cicd.bash asks. It needs python.
function fListRustTests {
	$lines = @()
	$py = Get-Command -Name python3, python -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
	if ($py) {
		$found = & $py.Source (Join-Path $Root "cicd\utility\test-id.py") --list 2>$null
		if ($LASTEXITCODE -eq 0) { $lines = @($found | Where-Object { $_ }) }
	}
	fEcho_Clean "Rust tests (cargo test, $($lines.Count)):"
	foreach ($line in $lines) { fEcho_Clean "  $line" }
	if (-not $lines) { fEcho_Clean "  (not listed: no python here)" }
}

## First `version = "x"` from the manifest.
function fVersion {
	$line = Select-String -LiteralPath $VersionManifest -Pattern '^\s*version\s*=\s*"([^"]+)"' |
		Select-Object -First 1
	if (-not $line) { fDie "no version found in $VersionManifest" }
	return $line.Matches[0].Groups[1].Value
}

## True if the exe is self-contained: no dynamic dependency on the VC runtime
## (VCRUNTIME140 / MSVCP140). Scans the PE for those import names - present only
## when msvc links the CRT dynamically (i.e. without +crt-static). The gnu build
## is always static (see .cargo/config.toml), so it reads standalone too.
function fExeIsStandalone {
	param([Parameter(Mandatory)][string]$Path)
	$bytes = [System.IO.File]::ReadAllBytes($Path)
	$ascii = [System.Text.Encoding]::ASCII.GetString($bytes)
	foreach ($dep in @("VCRUNTIME140", "MSVCP140")) {
		if ($ascii -match [regex]::Escape($dep)) { return $false }
	}
	return $true
}

## Locate makensis (not on PATH by default). $null if NSIS isn't installed.
function fFindMakensis {
	$cmd = Get-Command makensis -ErrorAction SilentlyContinue
	if ($cmd) { return $cmd.Source }
	foreach ($p in @(
		"C:\Program Files (x86)\NSIS\makensis.exe",
		"C:\Program Files\NSIS\makensis.exe",
		"C:\ProgramData\chocolatey\bin\makensis.exe")) {
		if (Test-Path -LiteralPath $p) { return $p }
	}
	return $null
}

## Warn (non-gating) when a pinned helper tool is missing or has drifted from its
## pin. Mirrors cicd.bash's TOOL_PINS loop. makensis is special-cased because it
## isn't on PATH by default (resolved via fFindMakensis).
function fCheckToolPins {
	if (-not (Test-Path -LiteralPath $ToolPinsFile)) { fWarn "no $ToolPinsFile; tool versions not checked"; return }
	foreach ($pin in (Get-Content -LiteralPath $ToolPinsFile)) {
		if (-not $pin -or $pin.StartsWith("#")) { continue }
		$parts   = $pin -split '\|', 4
		if ($parts.Count -ne 4 -or $parts[2] -eq "linux") { continue }
		$name    = $parts[0]; $want = $parts[1]; $cmd = $parts[3]
		$found   = $false; $verLine = $null
		try {
			$exe  = ($cmd -split '\s+')[0]
			$rest = @(($cmd -split '\s+') | Select-Object -Skip 1)
			if ($exe -eq "makensis") {
				$mk = fFindMakensis
				if ($mk) { $found = $true; $out = & $mk @rest 2>$null; $verLine = $out | Select-Object -First 1 }
			} else {
				if (Get-Command $exe -ErrorAction SilentlyContinue) {
					## Collect the whole output FIRST, then take the first line. Piping a
					## native command straight into `Select-Object -First 1` races: the
					## early upstream-stop can kill the tool mid-print (exit 101) and drop
					## its version line, which read as a false "not found".
					$found = $true; $out = & $exe @rest 2>$null; $verLine = $out | Select-Object -First 1
				}
			}
		} catch { $verLine = $null }
		if (-not $found)   { fWarn "$name not found (pinned $want)"; continue }
		if (-not $verLine) { fWarn "$name present but version unreadable (pinned $want)"; continue }
		$m = [regex]::Match($verLine, '[0-9]+(\.[0-9]+)+')
		$have = if ($m.Success) { $m.Value } else { "$verLine".Trim() }
		if ($have -ne $want) { fWarn "$name is $have, pinned $want (update the pin or the tool)" }
	}
}

## True if a rustup target is installed.
function fTargetInstalled {
	param([Parameter(Mandatory)][string]$Triple)
	return ((& rustup target list --installed) -contains $Triple)
}

## Decide whether an ARM64 target can build here; returns a reason string when it
## can't (for a clear warn-skip), or $null when it's good to go.
function fArmSkipReason {
	param([Parameter(Mandatory)][pscustomobject]$Target)
	if (-not (fTargetInstalled $Target.Triple)) { return "rustup target $($Target.Triple) not installed" }
	if ($Target.Builder -eq "zigbuild") {
		if (-not (Get-Command cargo-zigbuild -ErrorAction SilentlyContinue)) { return "cargo-zigbuild not found" }
		if (-not (Get-Command zig -ErrorAction SilentlyContinue))            { return "zig not found" }
	}
	return $null
}

## Where cargo actually puts a build. config.bash's rule: nothing may assume
## 'target', or a stage builds and then looks for its binary where it was never
## put. An absolute CARGO_TARGET_DIR is taken as it stands.
function fTargetDir {
	$td = $env:CARGO_TARGET_DIR
	if (-not $td) { return (Join-Path $Root "target") }
	if ([System.IO.Path]::IsPathRooted($td)) { return $td }
	return (Join-Path $Root $td)
}

## Panic locations and generated bindings carry the build box's paths, which put
## the profile folder and account name into every binary and made builds differ
## between boxes. The same remap cicd.bash writes: a cfg(all()) entry is joined
## with the per-target flags in .cargo/config.toml, where RUSTFLAGS would replace
## them, and later entries win, so the target dir comes after the root it
## usually sits in. Basic TOML strings, since a profile folder can hold a quote.
function fRemapConfig {
	$targetDir = fTargetDir
	New-Item -ItemType Directory -Path $targetDir -Force | Out-Null
	$cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { [System.IO.Path]::Combine($env:USERPROFILE, ".cargo") }
	$flags = foreach ($pair in @("$cargoHome=/cargo", "$Root=/silkterm", "$targetDir=/target")) {
		'"--remap-path-prefix=' + ($pair -replace '\\', '\\' -replace '"', '\"') + '"'
	}
	$cfg = Join-Path $targetDir "remap-paths.toml"
	Set-Content -LiteralPath $cfg -Encoding utf8 -Value @("[target.'cfg(all())']", "rustflags = [$($flags -join ', ')]")
	return $cfg
}

## True when a built file still names this box's profile folder or checkout, with
## either slash.
function fHasLocalPaths {
	param([Parameter(Mandatory)][string]$Path)
	$text = [System.Text.Encoding]::UTF8.GetString([System.IO.File]::ReadAllBytes($Path))
	foreach ($dir in @($env:USERPROFILE, $Root)) {
		foreach ($form in @("$dir\", ("$dir/" -replace '\\', '/'))) {
			if ($text.IndexOf($form, [StringComparison]::OrdinalIgnoreCase) -ge 0) { return $true }
		}
	}
	return $false
}

## Keep the newest $Keep - 1 run logs in $Dir, leaving room for the one about to
## be written.
function fRotateLogs {
	param([Parameter(Mandatory)][string]$Dir, [Parameter(Mandatory)][int]$Keep)
	Get-ChildItem -LiteralPath $Dir -Filter "run_*.log" -File | Sort-Object Name -Descending |
		Select-Object -Skip ($Keep - 1) | Remove-Item -Force -ErrorAction SilentlyContinue
}

## Build one release target. Returns a result object on success, or $null when an
## ARM target is skipped (x86_64 failures abort - house rule: always build both).
function fBuildTarget {
	param([Parameter(Mandatory)][pscustomobject]$Target)

	## The plan decided whether it builds here, and says why not.
	if (-not (fStepOpen "Release build: $($Target.OsArch)")) { return $null }
	fSetJobs
	$exe = Join-Path (fTargetDir) "$($Target.Triple)\release\$ExeName.exe"

	$cargoArgs = if ($Target.Builder -eq "zigbuild") {
		@("zigbuild", "--release", "--target", $Target.Triple)
	} else {
		@("build", "--release", "--target", $Target.Triple)
	}
	$cargoArgs += @("--config", $script:RemapConfig)

	if ($Target.Arm) {
		## Non-gating: an ARM toolchain hiccup warns and skips, never aborts.
		& cargo @cargoArgs
		if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $exe)) {
			fWarn "$($Target.OsArch) build failed (non-gating)"; fStepClose FAIL; return $null
		}
	} else {
		fExec -What "release build ($($Target.OsArch))" -File "cargo" -CmdArgs $cargoArgs
		if (-not (Test-Path -LiteralPath $exe)) { fDie "missing artifact for $($Target.OsArch): $exe" }
	}

	$size = "{0:N1} MB" -f ((Get-Item -LiteralPath $exe).Length / 1MB)
	fEcho "OK: $($Target.OsArch): $exe ($size)"
	fStepClose
	return [pscustomobject]@{ Arch=$Target.Arch; Tk=$Target.Tk; OsArch=$Target.OsArch; Exe=$exe }
}

## Copy the built binaries into the artifact dir under versioned names and write a
## sha256sums file over them (parallels cicd.bash's fWriteSums).
function fCollectArtifacts {
	param([Parameter(Mandatory)][array]$Built, [Parameter(Mandatory)][string]$Ver)
	if ((Test-Path -LiteralPath $ReleaseArtifactDir) -and -not (fSafeRm_Under (Join-Path $Root "cicd\artifacts") $ReleaseArtifactDir)) {
		fDie "could not clear $ReleaseArtifactDir (above)"
	}
	New-Item -ItemType Directory -Path $ReleaseArtifactDir -Force | Out-Null
	foreach ($b in $Built) {
		Copy-Item -LiteralPath $b.Exe -Destination (Join-Path $ReleaseArtifactDir "$ExeName-$Ver-$($b.OsArch).exe") -Force
	}
	fWriteSums $Ver
	fEcho "OK: $($Built.Count) release artifact(s) -> $ReleaseArtifactDir"
}

## (Re)write the checksums file over every artifact in the dir except itself.
function fWriteSums {
	param([Parameter(Mandatory)][string]$Ver)
	$sumsName = "$ExeName-$Ver-sha256sums.txt"
	$sumsPath = Join-Path $ReleaseArtifactDir $sumsName
	$lines = Get-ChildItem -LiteralPath $ReleaseArtifactDir -File |
		Where-Object { $_.Name -ne $sumsName } |
		ForEach-Object { "{0}  {1}" -f (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLower(), $_.Name }
	if ($lines) { Set-Content -LiteralPath $sumsPath -Value $lines -Encoding ascii }
}

## Build a self-contained NSIS installer per built arch (upgrades in place). A
## missing makensis warns and skips, never aborts.
function fBuildPackages {
	param([Parameter(Mandatory)][array]$Built, [Parameter(Mandatory)][string]$Ver)
	$makensis = fFindMakensis
	if (-not $makensis) { fWarn "makensis not found; installers skipped"; return }
	if (-not (Test-Path -LiteralPath $NsisTemplate)) { fWarn "NSIS template missing; installers skipped"; return }

	$made = 0
	foreach ($b in $Built) {
		$out = Join-Path $ReleaseArtifactDir "$ExeName-$Ver-$($b.OsArch)-setup.exe"
		$nsi = [System.IO.Path]::GetTempFileName() + ".nsi"
		## Four numbers for the version block: the release, less any pre-release tag.
		(Get-Content -Raw -LiteralPath $NsisTemplate).
			Replace("@VERSION@", $Ver).
			Replace("@ARCH@",    $b.OsArch).
			Replace("@SRCEXE@",  $b.Exe).
			Replace("@OUTFILE@", $out).
			Replace("@ICON@",    (Join-Path $Root "source\assets\icon.ico")).
			Replace("@VERNUM@",  (($Ver -replace '[-+].*$', '') + ".0")) | Set-Content -LiteralPath $nsi -Encoding utf8
		& $makensis -INPUTCHARSET UTF8 -V2 $nsi | Out-Null
		$rc = $LASTEXITCODE
		Remove-Item -LiteralPath $nsi -Force -ErrorAction SilentlyContinue
		if ($rc -eq 0 -and (Test-Path -LiteralPath $out)) { fEcho "OK: installer ($($b.OsArch))"; $made++ }
		else { fWarn "NSIS installer failed ($($b.OsArch))" }
	}
	if ($made) { fWriteSums $Ver }
	fEcho "OK: $made installer(s) -> $ReleaseArtifactDir"
}

## Install the fixed-name dogfood copy. Prefer the standalone msvc build, else
## gnu, else whichever single x86_64 build exists (this box runs x64).
function fDogfood {
	param([Parameter(Mandatory)][array]$Built)
	$x64  = @($Built | Where-Object { $_.Arch -eq "x86_64" })
	$msvc = @($x64 | Where-Object { $_.Tk -eq "msvc" }) | Select-Object -First 1
	$gnu  = @($x64 | Where-Object { $_.Tk -eq "gnu"  }) | Select-Object -First 1

	$pick = $null; $why = ""
	if ($msvc -and $gnu) {
		if (fExeIsStandalone $msvc.Exe) { $pick = $msvc; $why = "msvc is standalone" }
		else                            { $pick = $gnu;  $why = "msvc has a VC-runtime dependency" }
	} elseif ($msvc) { $pick = $msvc; $why = "only msvc built" }
	elseif   ($gnu)  { $pick = $gnu;  $why = "only gnu built" }

	if (-not $pick) { fWarn "no x86_64 build to dogfood; skipping"; return }

	if (-not (Test-Path -LiteralPath $DogfoodDir)) {
		New-Item -ItemType Directory -Path $DogfoodDir -Force | Out-Null
	}
	$dst = Join-Path $DogfoodDir $DogfoodFixedExe
	## A running copy is locked on Windows. Leave it be rather than end the run here.
	try { Copy-Item -LiteralPath $pick.Exe -Destination $dst -Force -ErrorAction Stop }
	catch { fWarn "dogfood copy is in use; skipped ($dst)"; return }
	Set-Content -LiteralPath "$dst.tag" -Value "$($pick.Tk)wwi" -Encoding ascii

	$icon = Join-Path $Root $DogfoodIcon
	if (Test-Path -LiteralPath $icon) {
		Copy-Item -LiteralPath $icon -Destination (Join-Path $DogfoodDir "silkterm.png") -Force
	}

	fEcho "OK: dogfood ($($pick.Tk); $why) -> $dst"
}

## git calls that reach the git host go through gitsby where its executable is on
## PATH, so they act as the account this folder belongs to, the same as cicd.bash.
## Only the executable: the script and cmd forms do not pass arguments through
## intact. Plain git otherwise.
function fRemoteGit {
	if (Get-Command gitsby -CommandType Application -ErrorAction SilentlyContinue) { & gitsby raw git @args }
	else { & git @args }
}

## Stash local changes, tracked and untracked, when there are any. True when a
## stash was made, so the caller knows to pop it. git's own lines go to the
## host, or they would ride out in the answer.
function fStashIfDirty {
	[OutputType([bool])]
	param()
	& git diff --quiet;          $dirtyTracked = ($LASTEXITCODE -ne 0)
	& git diff --cached --quiet; $dirtyStaged  = ($LASTEXITCODE -ne 0)
	$untracked = (& git ls-files --others --exclude-standard)
	if (-not ($dirtyTracked -or $dirtyStaged -or $untracked)) { return $false }
	$before = @(& git stash list).Count
	fEcho_Clean "git stash push --include-untracked ..."
	fExec -What "git stash" -File "git" -CmdArgs @("stash", "push", "--include-untracked", "-m", "auto-stash") | Out-Host
	$after = @(& git stash list).Count
	return ($after -gt $before)
}

## Stage 0: make sure the local branch can be safely refreshed from its upstream
## BEFORE spending the build - what stage 7 pushes should be what got built and
## tested here, not an untested post-build merge. Behind-only is safe (fast-
## forward, stash-wrapped for a dirty tree); diverged aborts now rather than at
## publish. Offline just warns - a local build shouldn't need the net.
function fRemoteSync {
	& git rev-parse --abbrev-ref '@{u}' 2>$null | Out-Null
	if ($LASTEXITCODE -ne 0) {
		$branch = (& git rev-parse --abbrev-ref HEAD).Trim()
		fNote "no upstream for ${branch}; nothing to sync"
		return
	}
	fRemoteGit fetch --quiet 2>$null
	if ($LASTEXITCODE -ne 0) { fWarn "git fetch failed (offline?); continuing with the local tree"; return }
	$ahead  = [int](& git rev-list --count '@{u}..HEAD')
	$behind = [int](& git rev-list --count 'HEAD..@{u}')
	if ($behind -eq 0) {
		if ($ahead) { fEcho "OK: up to date with upstream ($ahead ahead)" }
		else        { fEcho "OK: up to date with upstream" }
		return
	}
	if ($ahead -gt 0) { fDie "diverged from upstream ($ahead ahead, $behind behind) - reconcile first, or rerun with -NoSync" }
	## Behind only: a fast-forward can't lose anything. Same stash dance as
	## fPublish so a dirty tree can't block the pull.
	$didStash = fStashIfDirty
	fEcho_Clean "git pull --ff-only ..."
	fExec -What "git pull" -File "fRemoteGit" -CmdArgs @("pull", "--ff-only")
	if ($didStash) {
		fEcho_Clean "git stash pop ..."
		fExec -What "git stash pop" -File "git" -CmdArgs @("stash", "pop")
	}
	fEcho "OK: fast-forwarded $behind commit(s) from upstream"
}

## Publish: a native port of n8git_backup-and-publish MINUS the rar archive step.
## stash (if dirty) -> pull --no-ff (if upstream) -> pop -> add -> commit -> push.
## $Msg empty means "let git open its editor" (git uses core.editor / EDITOR).
function fPublish {
	param([Parameter(Mandatory)][AllowEmptyString()][string]$Msg)
	$branch = (& git rev-parse --abbrev-ref HEAD).Trim()
	fNote "branch: $branch"

	## Stash local changes (tracked + untracked) before syncing with upstream.
	$didStash = fStashIfDirty

	## Sync with this branch's upstream if it has one (a brand-new local branch has
	## nothing to pull; the push below sets its upstream on first publish). Stage 0
	## already made sure the branch is only behind, so this never needs a merge.
	& git rev-parse --abbrev-ref '@{u}' 2>$null | Out-Null
	$hasUpstream = ($LASTEXITCODE -eq 0)
	if ($hasUpstream) {
		fEcho_Clean "git pull --ff-only ..."
		fExec -What "git pull" -File "fRemoteGit" -CmdArgs @("pull", "--ff-only")
	}
	if ($didStash) {
		fEcho_Clean "git stash pop ..."
		fExec -What "git stash pop" -File "git" -CmdArgs @("stash", "pop")
	}

	fEcho_Clean "git add --all ..."
	fExec -What "git add" -File "git" -CmdArgs @("add", "--all")

	& git diff --cached --quiet; $hasStaged = ($LASTEXITCODE -ne 0)
	if ($hasStaged) {
		if ($Msg) {
			fExec -What "git commit" -File "git" -CmdArgs @("commit", "-m", $Msg)
			fEcho "OK: committed (`"$Msg`")"
		} else {
			## No message -> let git open the configured editor (core.editor / EDITOR).
			& git commit
			if ($LASTEXITCODE -ne 0) { fDie "git commit failed or was aborted (empty message?)" }
			fEcho "OK: committed (via editor)"
		}
	} else {
		fNote "nothing to commit"
	}

	## Push: set upstream on first publish, else push only when ahead.
	if (-not $hasUpstream) {
		fEcho_Clean "git push -u origin HEAD ..."
		fExec -What "git push" -File "fRemoteGit" -CmdArgs @("push", "-u", "origin", "HEAD")
		fEcho "OK: pushed $branch (upstream set)"
	} else {
		$ahead = (& git log '@{u}..' --oneline)
		if ($ahead) {
			fEcho_Clean "git push origin ..."
			fExec -What "git push" -File "fRemoteGit" -CmdArgs @("push", "origin")
			fEcho "OK: pushed $branch"
		} else {
			fNote "up to date with upstream; nothing to push"
		}
	}
}

## Advisory lint pass: clippy can't gate on Windows (Unix-gated ctl code emits
## dead_code, so -D warnings never passes), so run it plain and just report.
function fLintAdvisory {
	if (Get-Command cargo-clippy -ErrorAction SilentlyContinue) {
		$saved = $env:CARGO_TARGET_DIR
		$env:CARGO_TARGET_DIR = "target/lint"   ## don't invalidate the build cache
		try {
			& cargo clippy --workspace --all-targets
			if ($LASTEXITCODE -ne 0) { fWarn "clippy reported findings (advisory on Windows)" }
			else { fEcho "OK: clippy clean" }
		} finally { $env:CARGO_TARGET_DIR = $saved }
	} else { fNote "clippy skipped (component not installed)" }

	if (Get-Command cargo-deny -ErrorAction SilentlyContinue) {
		& cargo deny check licenses bans sources
		## The OK line also keeps the next section's spacing right: raw deny output
		## bypasses the blank counter, so end the stage with our own line.
		if ($LASTEXITCODE -ne 0) { fWarn "cargo-deny reported findings (advisory)" }
		else { fEcho "OK: deps clean (cargo-deny)" }
	} else { fNote "cargo-deny skipped (not installed)" }
}

## Known advisories against the locked dependencies. Unlike the lints above this
## one gates, as in cicd.bash: any report fails the run, a warning too.
function fAdvisoryScan {
	if (Get-Command cargo-deny -ErrorAction SilentlyContinue) {
		## cargo-deny reads yanks from cargo's cached index and never refreshes it,
		## see INDEX_REFRESH_CMD in config.bash.
		& cargo update --dry-run --quiet --config net.offline=false
		if ($LASTEXITCODE -ne 0) { fDie "can't refresh the crates.io index, so the advisory scan can't see a yanked crate (above)" }
		& cargo deny check advisories -D warnings
		if ($LASTEXITCODE -ne 0) { fDie "known advisories against the dependencies (above)" }
		fEcho "OK: no known advisories"
	} else { fWarn "advisory scan skipped (cargo-deny not installed, see cicd\tool-pins.txt)" }
}

## PowerShell scripts, through the same script cicd.bash runs. Advisory like the
## rest of the lints here.
function fLintPowerShell {
	& (Join-Path $PSScriptRoot "utility\ps-lint.ps1")
	switch ($LASTEXITCODE) {
		0       { fEcho "OK: PowerShell scripts clean" }
		2       { fNote "PowerShell lint skipped (PSScriptAnalyzer not installed)" }
		default { fWarn "PSScriptAnalyzer reported findings (advisory)" }
	}
}


#==============================================================================
# The Linux half, handed to WSL2

## A box with both can cover the whole matrix. Windows builds what only Windows
## can (msvc above all); WSL builds what only Linux can - .deb/.rpm, the profiler,
## the headless scroll harness. --no-windows on that side is what stops the two
## halves rebuilding each other's targets.
##
## It builds THIS working tree over /mnt rather than a second checkout, so there
## is nothing to keep in sync. Reading the source over 9p was measured and costs
## nothing. CARGO_TARGET_DIR is the part that matters: left alone, the Linux build
## would go in the same target\ the Windows build just used, and the two would
## evict each other's artifacts on every run. It has to point somewhere native.

## WSL2 distribution names, the default one first. WSL_UTF8 stops `wsl --list`
## answering in UTF-16, which arrives here full of NULs and parses as nothing.
function fWslDistros {
	if (-not (Get-Command wsl.exe -ErrorAction SilentlyContinue)) { return @() }
	$saved = $env:WSL_UTF8
	$env:WSL_UTF8 = "1"
	try { $lines = & wsl.exe --list --verbose 2>$null } finally { $env:WSL_UTF8 = $saved }
	if ($LASTEXITCODE -ne 0 -or -not $lines) { return @() }
	$found = [System.Collections.Generic.List[string]]::new()
	foreach ($line in $lines) {
		## "  NAME  STATE  VERSION", '*' marking the default. The header line has
		## no digit in its last column, so it falls out here without a special case.
		if ($line -match '^\s*(\*?)\s*(\S+)\s+(\S+)\s+(\d+)\s*$') {
			if ($Matches[4] -ne "2") { continue }        ## WSL1 has no kernel to build on
			if ($Matches[1]) { $found.Insert(0, $Matches[2]) } else { $found.Add($Matches[2]) }
		}
	}
	return $found
}

## The distro's HOME if it can run the Linux pipeline, else $null. cicd.bash puts
## ~/.cargo/bin and ~/.local/bin on PATH itself, so cargo is the only thing worth
## proving before handing over.
function fWslHome {
	param([string]$Distro)
	$homeDir = & wsl.exe -d $Distro -e printenv HOME 2>$null
	if ($LASTEXITCODE -ne 0 -or -not $homeDir) { return $null }
	$homeDir = "$homeDir".Trim()
	& wsl.exe -d $Distro -e test -x "$homeDir/.cargo/bin/cargo" 2>$null | Out-Null
	if ($LASTEXITCODE -ne 0) { return $null }
	return $homeDir
}

## What the Linux half's cicd.bash is given: the matching run, and the options
## that are its alone.
function fWslArgs {
	$cicdArgs = [System.Collections.Generic.List[string]]::new()
	if ($BacklogRunQuick) {
		$cicdArgs.Add("--backlog-run-quick")
		foreach ($area in $Areas) { $cicdArgs.Add($area) }
	} else {
		## --no-fmt: formatting is the Windows half's job and it has already run. The
		## rest are stages this side owns or has already done.
		$cicdArgs.AddRange([string[]]@("-y", "--no-sync", "--no-fmt", "--no-windows", "--no-dogfood", "--no-publish"))
		if ($MergeGate)          { $cicdArgs.Add("--merge-gate") }
		elseif ($BacklogRunFull) { $cicdArgs.Add("--backlog-run-full") }
		if ($Quick)       { $cicdArgs.Add("--quick") }
		if ($Long)        { $cicdArgs.Add("--long") }
		if ($NoArm)       { $cicdArgs.Add("--no-arm") }
		if ($NoPackage)   { $cicdArgs.Add("--no-package") }
		if ($NoLeakcheck) { $cicdArgs.Add("--no-leakcheck") }
		elseif ($Leakcheck) { $cicdArgs.Add("--leakcheck") }
	}
	if ($NoRemote)    { $cicdArgs.Add("--no-remote") }
	if ($NoContainer) { $cicdArgs.Add("--no-container") }
	if ($KeepTemp)    { $cicdArgs.Add("--keep-temp") }
	if ($List)        { $cicdArgs.Add("--list") }
	return , $cicdArgs.ToArray()
}

## Anything environmental warn-skips (no WSL, no toolchain, tree not visible); an
## actual failure of the Linux pipeline aborts the run like any other stage.
function fWslLinuxHalf {
	$distro = $WslDistro
	if (-not $distro) { $distro = fWslDistros | Select-Object -First 1 }
	if (-not $distro) { fWarn "Linux half skipped: no WSL2 distribution found"; return }

	$wslHome = fWslHome -Distro $distro
	if (-not $wslHome) { fWarn "Linux half skipped: $distro has no rustup cargo (see prerequisites.md)"; return }

	$wslRepo = & wsl.exe -d $distro -e wslpath -a -u $Root 2>$null
	if ($LASTEXITCODE -ne 0 -or -not $wslRepo) { fWarn "Linux half skipped: $distro cannot see $Root"; return }
	$wslRepo = "$wslRepo".Trim()

	## A script this side checked out CRLF dies under Linux as "$'\r': command not
	## found" - exit 127, no useful message, and from a file nobody was looking at.
	## .gitattributes asks for LF on all of them, but git never re-normalizes files
	## that were already in the tree when the rule arrived, so a long-lived clone
	## can carry them for years without noticing. Say so rather than let it happen.
	$crlf = @(& git ls-files --eol | Where-Object { $_ -match "w/crlf" -and $_ -match "eol=lf" } |
		ForEach-Object { ($_ -split "`t")[-1] })
	if ($crlf) {
		fWarn "Linux half skipped: $($crlf.Count) script(s) are CRLF here but must be LF to run under Linux"
		foreach ($f in ($crlf | Select-Object -First 5)) { fNote "  $f" }
		if ($crlf.Count -gt 5) { fNote "  ... and $($crlf.Count - 5) more" }
		fNote "fix: delete those paths and 'git checkout --' them, so git applies the attributes it never did"
		return
	}

	## Keyed by app so two projects delegating from one box don't share a dir.
	$wslTarget = "$wslHome/.cache/$AppName-wsl-target"

	$cicdArgs = fWslArgs

	fNote "distro ......: $distro"
	fNote "tree ........: $wslRepo (this one, not a second checkout)"
	fNote "target dir ..: $wslTarget"
	fNote "cicd.bash ...: $($cicdArgs -join ' ')"
	fEcho_Clean

	& wsl.exe -d $distro --cd $wslRepo -e env "CARGO_TARGET_DIR=$wslTarget" "SILK_BUILD_MINUTES=$env:SILK_BUILD_MINUTES" "SILK_RELEASE=$env:SILK_RELEASE" CICD_LINUX_HALF=1 bash cicd/cicd.bash @cicdArgs
	if ($LASTEXITCODE -ne 0) { fDie "Linux half failed (exit $LASTEXITCODE)" }
	fEcho "OK: Linux half"
}


## The plan's steps, in the order fMain runs them.
function fPlanSteps {
	$script:StepsOn.Clear(); $script:StepsOff.Clear()
	$by = { param([string]$Flag) if ($script:FullRunName) { $script:FullRunName } else { $Flag } }
	fStepIf -Name 'Remote sync' -Off $(if ($NoSync) { & $by '-NoSync' })
	fStepIf -Name 'Format' -Off $(if ($NoFmt) { '-NoFmt' })
	fStepIf -Name 'Debug build'
	fStepIf -Name 'Tests'
	foreach ($t in $Targets) {
		$off = if (-not $t.Arm) { '' } elseif ($NoArm) { '-NoArm' } elseif ($Quick) { '-Quick' } else { fArmSkipReason $t }
		fStepIf -Name "Release build: $($t.OsArch)" -Off $off
	}
	fStepIf -Name 'Release artifacts'
	fStepIf -Name 'Packages' -Off $(if ($Quick) { '-Quick' } elseif ($NoPackage) { '-NoPackage' })
	fStepIf -Name 'Linux half (WSL2)' -Off $(if (-not $Wsl) { '-Wsl adds it' })
	fStepIf -Name 'Dogfood' -Off $(if ($NoDogfood) { & $by '-NoDogfood' })
	fStepIf -Name 'Publish' -Off $(if ($NoPublish) { & $by '-NoPublish' })
}


## -BacklogRunQuick: the fast checks to run before a commit, every area or the
## ones named, then exit. Nothing is rewritten, built for release or published.
function fQuickRun {
	$run = @(if ($Areas.Count) { $Areas } else { $QuickAreas })
	$script:StepsOn.Clear(); $script:StepsOff.Clear()
	foreach ($area in $QuickAreas) { fStepIf -Name $QuickTitles[$area] -Off $(if ($run -notcontains $area) { 'not named' }) }
	fStepIf -Name 'Linux half (WSL2)' -Off $(if (-not $Wsl) { '-Wsl adds it' })
	if ($List) {
		fEcho_Clean "$AppName backlog quick run ($($run -join ' ')):"
		fShowSteps
		if ($run -contains 'tests') { fListRustTests }
	} else {
		fEcho_Clean
		fEcho_Clean "$AppName backlog quick run"
		fShowSteps
		foreach ($area in $run) {
			$null = fStepOpen $QuickTitles[$area]
			fSetJobs
			switch ($area) {
				'fmt'     { fExec -What "format check" -File "cargo" -CmdArgs @("fmt", "--check"); fEcho "OK: formatting clean" }
				'clippy'  { fLintAdvisory }
				'tests'   { fExec -What "tests" -File "cargo" -CmdArgs @("test"); fEcho "OK: tests passed" }
				'scripts' { fLintPowerShell }
				'docs'    { fNote "the docs checks are cicd.bash's$(if ($Wsl) { '; the Linux half runs them' } else { '; -Wsl runs them here' })" }
			}
			fStepClose
		}
	}
	if ($Wsl) {
		## Its own run, with its own plan and steps.
		if (-not $List) { $null = fStepOpen 'Linux half (WSL2)' }
		fSetJobs
		fWslLinuxHalf
		fStepClose
	}
	if (-not $List) { fSection "$AppName backlog quick run: PASSED." }
	fEcho_Clean
}


#==============================================================================
# Entry point

function fMain {
	$refused = fRunFlags
	if ($refused) { [Console]::Error.WriteLine($refused); exit 2 }

	## Toolchain PATH: rustup (cross targets, edition 2024) then the mingw linker.
	$env:PATH = "$CargoBin;$MingwBin;$env:PATH"
	fSetJobs

	Set-Location -LiteralPath $Root
	$stamp = Get-Date -Format "yyyyMMdd-HHmmss"

	if ($BacklogRunQuick) { fQuickRun; return }

	## Warn (non-gating) on any drifted/missing pinned helper tool.
	if (-not $List) { fCheckToolPins }

	## Resolve the publish commit message: -Message wins, then an auto stamp when
	## unattended; interactive runs capture it at the preflight prompt below. An
	## empty message at commit time means "let git open its editor".
	$publishMsg = ""
	if     ($Message)     { $publishMsg = $Message }
	elseif ($Unattended)  { $publishMsg = "$AppName CI/CD $stamp" }

	## Preflight summary.
	fEcho_Clean
	fEcho_Clean "$AppName Windows CI/CD"
	fEcho_Clean
	if ($FullRunName) { fEcho_Clean "Run .........: $FullRunName, every check of the default run$(if ($Long) { ' and -Long' }); no sync, dogfood, publish or prompt" }
	fEcho_Clean "Repo root ...: $Root"
	fEcho_Clean "Jobs ........: $CicdMaxJobs of $Cores cores"
	fEcho_Clean "Remote sync .: $(if ($NoSync) { '(skipped)' } else { 'fetch + fast-forward check' })"
	fEcho_Clean "Format ......: $(if ($NoFmt) { '(skipped)' } elseif ($FmtCheck) { 'cargo fmt --check' } else { 'cargo fmt' })"
	fEcho_Clean "Release .....: x86_64 msvc + gnu$(if ($NoArm -or $Quick) { '' } else { ' + ARM64 (if toolchain present)' })"
	fEcho_Clean "Packages ....: $(if ($NoPackage -or $Quick) { '(skipped)' } else { 'NSIS installers (if makensis present)' })"
	if ($Wsl) { fEcho_Clean "Linux half ..: WSL2 (.deb/.rpm, profiler, scroll harness)" }
	else {
		## Say it is available rather than leaving it to be found in the help.
		$wslSeen = fWslDistros | Select-Object -First 1
		if ($wslSeen) { fEcho_Clean "Linux half ..: (not requested - $wslSeen is here, -Wsl adds it)" }
		else          { fEcho_Clean "Linux half ..: (no WSL2 on this box)" }
	}
	if ($Long) { fEcho_Clean "Long steps ..: none on Windows$(if ($Wsl) { '; the Linux half gets --long' } else { '; -Wsl hands --long to the Linux half' })" }
	fEcho_Clean "Dogfood .....: $(if ($NoDogfood) { '(skipped)' } else { "$DogfoodDir\$DogfoodFixedExe" })"
	if ($NoPublish)          { fEcho_Clean "Publish .....: (skipped)" }
	elseif ($publishMsg)     { fEcho_Clean "Publish .....: commit + push current branch (hands-off: `"$publishMsg`")" }
	else                     { fEcho_Clean "Publish .....: commit + push current branch (will prompt; blank = editor)" }
	fPlanSteps
	fShowSteps
	fEcho_Clean
	fEcho_Clean "Fail-fast: any error aborts before the next step."
	fEcho_Clean

	## -List: the tests stage 3 runs, then the Linux half's own list, and nothing else.
	if ($List) {
		fEcho_Clean "Stage 3 tests:"
		fListRustTests
		fEcho_Clean "Test scripts:"
		fScriptTests
		if ($Wsl) {
			fSection "Linux half (WSL2)"
			fWslLinuxHalf
		}
		fEcho_Clean
		return
	}

	## Capture the commit message up front so the run finishes unattended. This is
	## the natural place to bail on the common (publish) path - Ctrl+C aborts.
	if (-not $Unattended -and -not $NoPublish -and -not $publishMsg) {
		$m = Read-Host "Publish commit message (blank = editor; Ctrl+C aborts)"
		## Read-Host bypasses the blank counter; reset it so the next section's
		## leading blank isn't swallowed (the prompt line is now the last output).
		$script:WasLastEchoBlank = $false
		if ($m) { $publishMsg = $m }
	}

	## Start the transcript once past the preflight, dropping all but the newest
	## few from earlier runs. The names sort by time.
	New-Item -ItemType Directory -Path $LogDir -Force | Out-Null
	fRotateLogs $LogDir $LogKeep
	try { Start-Transcript -LiteralPath (Join-Path $LogDir "run_$stamp.log") | Out-Null } catch {}

	## The test run folder, then the ones earlier runs kept, past the newest
	## few. This run's own goes on the list after the sweep.
	fTestDir_Make
	$testLedger = Join-Path (fTargetDir) ".cicd-test-folders"
	$gone = fTestDir_Sweep -Ledger $testLedger -Keep $TestFolderKeep
	fTestDir_Record -Ledger $testLedger
	if ($gone) { fNote "cleanup: removed $gone old test run folder(s)" }

	if (fStepOpen 'Remote sync') { fSetJobs; fRemoteSync; fStepClose }

	## Pin the build number for the run, as cicd.bash does, and hand it to the WSL half.
	## It is the commit's time, dirty tree or not, so a rebuild of a commit gets the
	## same number. The clock only stands in where there is no commit.
	if (-not $env:SILK_BUILD_MINUTES) {
		$buildSecs = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
		$commitSecs = "$(& git log -1 --format=%ct 2>$null)".Trim()
		if ($commitSecs -match '^\d+$') { $buildSecs = [long]$commitSecs }
		$env:SILK_BUILD_MINUTES = "$([math]::Floor(($buildSecs - 946684800) / 60))"
	}

	## Only a clean main can be tagged, so only it builds without the date.
	if (-not $env:SILK_RELEASE) {
		$branchNow = "$(& git rev-parse --abbrev-ref HEAD 2>$null)".Trim()
		$dirtyNow = @(& git status --porcelain --untracked-files=no 2>$null)
		$env:SILK_RELEASE = if ($branchNow -eq 'main' -and $dirtyNow.Count -eq 0) { '1' } else { '0' }
	}

	if (fStepOpen 'Format') {
		fSetJobs
		if ($FmtCheck) { fExec -What "format check" -File "cargo" -CmdArgs @("fmt", "--check"); fEcho "OK: formatting clean" }
		else { fExec -What "format" -File "cargo" -CmdArgs @("fmt"); fEcho "OK: formatted" }
		fStepClose
	}

	$null = fStepOpen 'Debug build'
	fSetJobs
	fExec -What "debug build" -File "cargo" -CmdArgs @("build")
	fEcho "OK: debug build"
	fStepClose

	## Tests and the advisory lints.
	$null = fStepOpen 'Tests'
	fSetJobs
	fExec -What "tests" -File "cargo" -CmdArgs @("test")
	fEcho "OK: tests passed"
	fScriptTests
	fLintAdvisory
	fLintPowerShell
	fAdvisoryScan
	fStepClose

	## Release builds (x86_64 msvc + gnu always; ARM64 when ready).
	## (No profiler stage here: pprof's SIGPROF sampler is Unix-only - the
	## profiling feature can't even compile for a Windows target.)
	$script:RemapConfig = fRemapConfig
	$built = @(foreach ($t in $Targets) { fBuildTarget -Target $t | Where-Object { $_ } })
	$null = fStepOpen 'Release artifacts'
	if (-not $built) { fDie "no release binaries were produced" }
	foreach ($b in $built) {
		if (fHasLocalPaths $b.Exe) { fDie "$($b.Exe) still holds a local path ($env:USERPROFILE or $Root)" }
	}
	fEcho "OK: no local paths in $($built.Count) binary(s)"
	$ver = fVersion
	fCollectArtifacts -Built $built -Ver $ver
	fStepClose

	if (fStepOpen 'Packages') { fSetJobs; fBuildPackages -Built $built -Ver $ver; fStepClose }
	## The Linux half is a run of its own, with its own plan and steps.
	if (fStepOpen 'Linux half (WSL2)') { fSetJobs; fWslLinuxHalf; fStepClose }
	if (fStepOpen 'Dogfood') { fSetJobs; fDogfood -Built $built; fStepClose }
	if (fStepOpen 'Publish') { fSetJobs; fPublish -Msg $publishMsg; fStepClose }

	## -KeepTemp: the run's test folder stays. A failed run keeps it anyway.
	if ($KeepTemp -and $script:TestDirOwned) {
		fNote "kept the run's temp folder (-KeepTemp): $($script:TestDirOwned)"
		$script:TestDirOwned = $null
	}

	fSection "$AppName Windows CI/CD: done."
	fEcho_Clean
}

$script:OutputWas = $null
try {
	$script:OutputWas = fUtf8Output
	fMain
	fTestDir_End 0
} finally {
	## A step a thrown error ended.
	fStepClose FAIL
	try { Stop-Transcript | Out-Null } catch {}
	## The console's code page is shared with the shell that started this.
	if ($script:OutputWas) { try { [Console]::OutputEncoding = $script:OutputWas } catch { Write-Verbose "code page not put back: $_" } }
}


##	History:
##		- 2026-10-28: Output sent over ssh or into a file is UTF-8, so the rule
##		  is dots rather than bell bytes, or ASCII where UTF-8 can't be set.
##		- 2026-10-28: Steps as cicd.bash prints them: a 79-wide rule, then a
##		  stamped Starting line numbered of the steps this run takes, and a PASS
##		  or FAIL line. The plan lists the steps turned off and why.
##		- 2026-10-28: cicd.bash's runs: -BacklogRunQuick with its areas,
##		  -BacklogRunFull, -MergeGate, -Long, -NoContainer, -KeepTemp and -List.
##		  -Gate and -Ci are other names, -Slow too. The Linux half gets each.
##		  The release artifacts folder is cleared through the safe removal guard.
##		- 2026-10-26: Jobs per step, a quarter when busy, at BelowNormal, or Idle
##		  for a hard step. The build number is the commit's time on a dirty tree too.
##		- 2026-10-25: -NoRemote, passed on to the Linux half.
##		- 2026-10-22: -NoLeakcheck and -Leakcheck, passed on to the Linux half.
##		- 2026-10-19 JC: The test run folder is made after the prompt, and the
##		  ones failed runs kept go once past the newest few.
##		- 2026-10-06 JC: Stage 3 runs the launcher's Start menu test.
##		- 2026-10-06 JC: Help block, read by -Help; named arguments to fExec; no
##		  array growth in loops.
##		- 2026-09-25 JC: Release builds map the box's paths away and fail if one
##		  is left; tool pins come from tool-pins.txt; old run logs are pruned;
##		  PowerShell scripts are linted when PSScriptAnalyzer is installed;
##		  fetch, pull and push go through gitsby where it is installed; the
##		  installer carries the program's icon and a version block.
##		- 2026-08-24 JC: -Wsl runs the Linux half (cicd.bash --no-windows) in WSL2
##		  against this same tree, so one box covers both platforms; stages
##		  renumbered to 8.
##		- 2026-07-15 JC: Created (Windows-native port of cicd.bash: build/test/
##		  package/dogfood/publish; msvc + gnu always, ARM64 when ready).
##		- 2026-07-15 JC: Parity pass - profiler stage, full stash/pull/commit/push
##		  publish (rar skipped), preflight message prompt + -Quiet, -Quick skips
##		  the profiler, tool-pin drift warnings.
##		- 2026-07-22 JC: Stage 0 remote sync - fetch, fast-forward if safely
##		  behind, abort if diverged.
##		- 2026-07-22 JC: Dropped the profiler stage (pprof is Unix-only: SIGPROF
##		  sampling, can't compile or run on Windows); stages renumbered. Fixed
##		  the missing blank line after the commit-message prompt.
