##	Purpose:
##		- Windows-native CI/CD pipeline for SilkTerm. A PowerShell port of the
##		  Linux cicd.bash, doing as much of the same work as Windows allows -
##		  including the parts cicd.bash farms out to helper scripts (the git
##		  backup/publish). Does NOT touch cicd.bash (that stays the
##		  Linux/cross pipeline).
##		- Stages (fail-fast; any error aborts before the next stage):
##		   0. remote sync    (fetch; fast-forward if safely behind; abort if diverged)
##		   1. format         (cargo fmt)
##		   2. debug build    (cargo build)
##		   3. tests + lints  (cargo test; clippy + cargo-deny are ADVISORY here)
##		   4. release builds  x86_64 msvc AND gnu (always both), + ARM64 when its
##		                      toolchain is present (auto-detected, else warn-skip)
##		   5. packages       (NSIS installer .exe per built arch, if makensis found)
##		   6. linux half     (-Wsl: hand the Linux-only work to WSL2 - see below)
##		   7. dogfood        (copy the best x86_64 build to <dogfood>\silkterm.exe)
##		   8. publish        (stash -> pull -> add -> commit -> push, current branch)
##		- What Windows can't do (dropped vs cicd.bash): the profiler (pprof's
##		  SIGPROF sampler is Unix-only - the profiling feature can't even compile
##		  for a Windows target), the headless scroll harness / demo
##		  (need Xvfb), .deb/.rpm packages (Linux), and the rar version-archive step
##		  of publish (skipped by request). clippy is advisory, not gating: the
##		  Unix-gated ctl code emits dead_code warnings here, so -D warnings can't
##		  pass.
##		- -Wsl gets all of that back on a box that has WSL2, by running the Linux
##		  pipeline (cicd.bash --no-windows) there against THIS working tree. The
##		  two halves split cleanly: Windows builds what only Windows can, msvc
##		  above all, and WSL builds what only Linux can. Neither repeats the
##		  other's targets. Off by default - it roughly doubles a run.
##		- Dogfood pick: prefer the msvc build IF it's self-contained (statically
##		  linked, no VCRUNTIME140/MSVCP140 dependency); else the gnu build; else
##		  whichever single build exists. The fixed silkterm.exe goes to the SYNCED
##		  app dir; the runterm launcher keeps its own rotated pool locally
##		  (the two stay separate dirs on purpose).
##		- Syntax:
##		  pwsh cicd/cicd-win.ps1 [options]
##		  Options:
##		   -Yes            run unattended (no confirm / message prompt)
##		   -Quiet          quiet + unattended (implies -Yes); publish runs quiet too
##		   -Quick          skip the slow stages (ARM builds + packages)
##		   -Gate           merge gate only: fmt --check + clippy + tests, then exit
##		   -NoFmt          skip the formatter stage
##		   -NoArm          skip the ARM64 release builds + their packages
##		   -NoPackage      skip the packages stage (NSIS installers)
##		   -NoDogfood      skip the dogfood install
##		   -NoPublish      skip the git publish stage
##		   -Wsl            also run the Linux half in WSL2 (.deb/.rpm, profiler,
##		                   scroll harness - everything Windows can't do)
##		   -WslDistro NAME which distribution to use (default: the first WSL2 one)
##		   -NoSync         skip the remote sync check (stage 0)
##		   -Message MSG    publish hands-off with this commit message (no editor)
##		   -Help           show this help
##	History: At bottom of script.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	Licensed under The MIT License (MIT). Full text at:
##		https://mit-license.org/
##	SPDX-License-Identifier: MIT

[CmdletBinding()]
param(
	[switch]$Yes,
	[switch]$Quiet,
	[switch]$Quick,
	[switch]$Gate,
	[switch]$NoFmt,
	[switch]$NoArm,
	[switch]$NoPackage,
	[switch]$NoDogfood,
	[switch]$NoPublish,
	[switch]$NoSync,
	[switch]$Wsl,
	[string]$WslDistro = "",
	[string]$Message = "",
	[switch]$Help
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

if ($Help) {
	## Print only the leading Purpose..History header block (mirrors cicd.bash's
	## `sed -n '/Purpose:/,/History:/p'`), not every top-level ## comment.
	$inBlock = $false
	foreach ($line in (Get-Content -LiteralPath $PSCommandPath)) {
		if ($line -match '^##\tPurpose:') { $inBlock = $true }
		if ($inBlock) {
			if ($line -match '^##\tHistory:') { break }
			$line -replace '^##\t?', ''
		}
	}
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


#••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
# Configuration

## Repo root = the parent of this script's cicd/ dir. All cargo commands run here.
$Root    = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$AppName = "SilkTerm"
$ExeName = "silkterm"

## One folder for every file the tests write, shared by both cargo test runs and
## the installer tests. TEMP and TMP stay put, so builds keep the system temp dir.
. (Join-Path $Root "cicd\tests\_testdir.ps1"); fTestDir_Make

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

## Cap compile/test parallelism to half the cores so a run stays usable.
$Cores       = [Environment]::ProcessorCount
$CicdMaxJobs = [Math]::Max(1, [Math]::Floor($Cores / 2))

## Where the toolchains live. cargo/rustup first, then the mingw linker for the
## gnu target (matches the memory'd build setup).
$CargoBin  = Join-Path $env:USERPROFILE ".cargo\bin"
$MingwBin  = "C:\ProgramData\mingw64\mingw64\bin"


#••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
# Output helpers (mirror cicd.bash: fEcho / fEcho_Clean / fSection)

$script:WasLastEchoBlank = $false
$script:Letterbox = "•" * 73

function fEcho_Clean {
	param([string]$Msg = "")
	if ($Msg) { Write-Host $Msg; $script:WasLastEchoBlank = $false }
	elseif (-not $script:WasLastEchoBlank) { Write-Host ""; $script:WasLastEchoBlank = $true }
}
function fEcho     { param([string]$Msg = ""); if ($Msg) { fEcho_Clean "[ $Msg ]" } else { fEcho_Clean } }
function fSection  { param([string]$Msg);      fEcho_Clean; fEcho_Clean $script:Letterbox; fEcho $Msg }
function fNote     { param([string]$Msg); fEcho_Clean $Msg }
function fWarn     { param([string]$Msg); fEcho "WARNING: $Msg" }
function fDie      { param([string]$Msg); fEcho "FAILED: $Msg"; fTestDir_End 1; exit 1 }


#••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
# Functions

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

## The installer tests. They run in this process and point TEMP and TMP at the
## test run folder, so both go back afterward for the builds that follow.
function fInstallerTests {
	fTestDir_Keep {
		## install.ps1's signature check, with the OpenSSH that ships on Windows.
		fExec "installer signing" (Join-Path $Root "cicd\tests\release\verify-sign.ps1")
		fEcho "OK: installer signing"
		## ...and its temp folder step, where the shared temp folder is the one it guards against.
		fExec "installer temp folder" (Join-Path $Root "cicd\tests\install\tempdir.ps1")
		fEcho "OK: installer temp folder"
		## ...and a real install, upgrade and repair, under both PowerShells. Called
		## directly, since fExec's array would reach -Shell as a plain value.
		foreach ($shell in @("pwsh", "powershell")) {
			& (Join-Path $Root "cicd\tests\install\windows.ps1") -Shell $shell
			if ($LASTEXITCODE -ne 0) { fDie "installer on Windows ($shell) failed" }
		}
		fEcho "OK: installer on Windows"
	}
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
	param([Parameter(Mandatory)]$Target)
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
	param([Parameter(Mandatory)]$Target)

	if ($Target.Arm) {
		if ($NoArm)  { fNote "skip $($Target.OsArch): -NoArm";  return $null }
		if ($Quick)  { fNote "skip $($Target.OsArch): -Quick";  return $null }
		$reason = fArmSkipReason $Target
		if ($reason) { fWarn "$($Target.OsArch) skipped: $reason"; return $null }
	}

	fSection "4  Release build: $($Target.OsArch)"
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
			fWarn "$($Target.OsArch) build failed (non-gating)"; return $null
		}
	} else {
		fExec "release build ($($Target.OsArch))" "cargo" $cargoArgs
		if (-not (Test-Path -LiteralPath $exe)) { fDie "missing artifact for $($Target.OsArch): $exe" }
	}

	$size = "{0:N1} MB" -f ((Get-Item -LiteralPath $exe).Length / 1MB)
	fEcho "OK: $($Target.OsArch): $exe ($size)"
	return [pscustomobject]@{ Arch=$Target.Arch; Tk=$Target.Tk; OsArch=$Target.OsArch; Exe=$exe }
}

## Copy the built binaries into the artifact dir under versioned names and write a
## sha256sums file over them (parallels cicd.bash's write_sums).
function fCollectArtifacts {
	param([Parameter(Mandatory)][array]$Built, [Parameter(Mandatory)][string]$Ver)
	if (Test-Path -LiteralPath $ReleaseArtifactDir) { Remove-Item -LiteralPath $ReleaseArtifactDir -Recurse -Force }
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
	& git diff --quiet;          $dirtyTracked = ($LASTEXITCODE -ne 0)
	& git diff --cached --quiet; $dirtyStaged  = ($LASTEXITCODE -ne 0)
	$untracked = (& git ls-files --others --exclude-standard)
	$didStash = $false
	if ($dirtyTracked -or $dirtyStaged -or $untracked) {
		$before = @(& git stash list).Count
		fEcho_Clean "git stash push --include-untracked ..."
		fExec "git stash" "git" @("stash", "push", "--include-untracked", "-m", "auto-stash")
		$after = @(& git stash list).Count
		$didStash = ($after -gt $before)
	}
	fEcho_Clean "git pull --ff-only ..."
	fExec "git pull" "fRemoteGit" @("pull", "--ff-only")
	if ($didStash) {
		fEcho_Clean "git stash pop ..."
		fExec "git stash pop" "git" @("stash", "pop")
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
	& git diff --quiet;        $dirtyTracked = ($LASTEXITCODE -ne 0)
	& git diff --cached --quiet; $dirtyStaged = ($LASTEXITCODE -ne 0)
	$untracked = (& git ls-files --others --exclude-standard)
	$didStash = $false
	if ($dirtyTracked -or $dirtyStaged -or $untracked) {
		$before = @(& git stash list).Count
		fEcho_Clean "git stash push --include-untracked ..."
		fExec "git stash" "git" @("stash", "push", "--include-untracked", "-m", "auto-stash")
		$after = @(& git stash list).Count
		$didStash = ($after -gt $before)
	}

	## Sync with this branch's upstream if it has one (a brand-new local branch has
	## nothing to pull; the push below sets its upstream on first publish). Stage 0
	## already made sure the branch is only behind, so this never needs a merge.
	& git rev-parse --abbrev-ref '@{u}' 2>$null | Out-Null
	$hasUpstream = ($LASTEXITCODE -eq 0)
	if ($hasUpstream) {
		fEcho_Clean "git pull --ff-only ..."
		fExec "git pull" "fRemoteGit" @("pull", "--ff-only")
	}
	if ($didStash) {
		fEcho_Clean "git stash pop ..."
		fExec "git stash pop" "git" @("stash", "pop")
	}

	fEcho_Clean "git add --all ..."
	fExec "git add" "git" @("add", "--all")

	& git diff --cached --quiet; $hasStaged = ($LASTEXITCODE -ne 0)
	if ($hasStaged) {
		if ($Msg) {
			fExec "git commit" "git" @("commit", "-m", $Msg)
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
		fExec "git push" "fRemoteGit" @("push", "-u", "origin", "HEAD")
		fEcho "OK: pushed $branch (upstream set)"
	} else {
		$ahead = (& git log '@{u}..' --oneline)
		if ($ahead) {
			fEcho_Clean "git push origin ..."
			fExec "git push" "fRemoteGit" @("push", "origin")
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
		& cargo deny check
		## The OK line also keeps the next section's spacing right: raw deny output
		## bypasses the blank counter, so end the stage with our own line.
		if ($LASTEXITCODE -ne 0) { fWarn "cargo-deny reported findings (advisory)" }
		else { fEcho "OK: deps clean (cargo-deny)" }
	} else { fNote "cargo-deny skipped (not installed)" }

	fLintPowerShell
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


#••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
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
	$found = @()
	foreach ($line in $lines) {
		## "  NAME  STATE  VERSION", '*' marking the default. The header line has
		## no digit in its last column, so it falls out here without a special case.
		if ($line -match '^\s*(\*?)\s*(\S+)\s+(\S+)\s+(\d+)\s*$') {
			if ($Matches[4] -ne "2") { continue }        ## WSL1 has no kernel to build on
			if ($Matches[1]) { $found = @($Matches[2]) + $found } else { $found += $Matches[2] }
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

	## --no-fmt: formatting is the Windows half's job and it has already run. The
	## rest are stages this side owns or has already done.
	$cicdArgs = @("-y", "--no-sync", "--no-fmt", "--no-windows", "--no-dogfood", "--no-publish")
	if ($Quick)     { $cicdArgs += "--quick" }
	if ($NoArm)     { $cicdArgs += "--no-arm" }
	if ($NoPackage) { $cicdArgs += "--no-package" }

	fNote "distro ......: $distro"
	fNote "tree ........: $wslRepo (this one, not a second checkout)"
	fNote "target dir ..: $wslTarget"
	fNote "cicd.bash ...: $($cicdArgs -join ' ')"
	fEcho_Clean

	& wsl.exe -d $distro --cd $wslRepo -e env "CARGO_TARGET_DIR=$wslTarget" "SILK_BUILD_MINUTES=$env:SILK_BUILD_MINUTES" CICD_LINUX_HALF=1 bash cicd/cicd.bash @cicdArgs
	if ($LASTEXITCODE -ne 0) { fDie "Linux half failed (exit $LASTEXITCODE)" }
	fEcho "OK: Linux half"
}


#••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
# Entry point

function fMain {
	## Toolchain PATH: rustup (cross targets, edition 2024) then the mingw linker.
	$env:PATH = "$CargoBin;$MingwBin;$env:PATH"
	## Cap parallelism.
	$env:CARGO_BUILD_JOBS  = "$CicdMaxJobs"
	$env:RUST_TEST_THREADS = "$CicdMaxJobs"

	Set-Location -LiteralPath $Root
	$stamp = Get-Date -Format "yyyyMMdd-HHmmss"

	## Gate mode: fmt --check + clippy (advisory) + tests, then exit. Fast local
	## stand-in for a hosted CI check; nothing is mutated or published.
	if ($Gate) {
		fSection "Gate 1/3  Format check"
		fExec "format check" "cargo" @("fmt", "--check")
		fEcho "OK: formatting clean"
		fSection "Gate 2/3  Lints (advisory)"
		fLintAdvisory
		fSection "Gate 3/3  Tests"
		fExec "tests" "cargo" @("test")
		fEcho "OK: tests passed"
		fSection "$AppName gate: PASSED."
		fEcho_Clean
		return
	}

	## Warn (non-gating) on any drifted/missing pinned helper tool.
	fCheckToolPins

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
	fEcho_Clean "Repo root ...: $Root"
	fEcho_Clean "Jobs ........: $CicdMaxJobs of $Cores cores"
	fEcho_Clean "Remote sync .: $(if ($NoSync) { '(skipped)' } else { 'fetch + fast-forward check' })"
	fEcho_Clean "Format ......: $(if ($NoFmt) { '(skipped)' } else { 'cargo fmt' })"
	fEcho_Clean "Release .....: x86_64 msvc + gnu$(if ($NoArm -or $Quick) { '' } else { ' + ARM64 (if toolchain present)' })"
	fEcho_Clean "Packages ....: $(if ($NoPackage -or $Quick) { '(skipped)' } else { 'NSIS installers (if makensis present)' })"
	if ($Wsl) { fEcho_Clean "Linux half ..: WSL2 (.deb/.rpm, profiler, scroll harness)" }
	else {
		## Say it is available rather than leaving it to be found in the help.
		$wslSeen = fWslDistros | Select-Object -First 1
		if ($wslSeen) { fEcho_Clean "Linux half ..: (not requested - $wslSeen is here, -Wsl adds it)" }
		else          { fEcho_Clean "Linux half ..: (no WSL2 on this box)" }
	}
	fEcho_Clean "Dogfood .....: $(if ($NoDogfood) { '(skipped)' } else { "$DogfoodDir\$DogfoodFixedExe" })"
	if ($NoPublish)          { fEcho_Clean "Publish .....: (skipped)" }
	elseif ($publishMsg)     { fEcho_Clean "Publish .....: commit + push current branch (hands-off: `"$publishMsg`")" }
	else                     { fEcho_Clean "Publish .....: commit + push current branch (will prompt; blank = editor)" }
	fEcho_Clean
	fEcho_Clean "Fail-fast: any error aborts before the next stage."
	fEcho_Clean

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

	## Stage 0: remote sync.
	fSection "0  Remote sync"
	if ($NoSync) { fNote "remote sync skipped" }
	else { fRemoteSync }

	## Pin the build number for the run, as cicd.bash does, and hand it to the WSL half.
	## A clean tree takes its commit's time, so a rebuild of a release commit gets the
	## same number. A dirty tree is a different binary, so it keeps the clock.
	if (-not $env:SILK_BUILD_MINUTES) {
		$buildSecs = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
		$dirtyLines = @(& git status --porcelain --untracked-files=no 2>$null)
		if ($LASTEXITCODE -eq 0 -and $dirtyLines.Count -eq 0) {
			$commitSecs = "$(& git log -1 --format=%ct 2>$null)".Trim()
			if ($commitSecs -match '^\d+$') { $buildSecs = [long]$commitSecs }
		}
		$env:SILK_BUILD_MINUTES = "$([math]::Floor(($buildSecs - 946684800) / 60))"
	}

	## Stage 1: format.
	fSection "1  Format"
	if ($NoFmt) { fNote "format skipped" }
	else { fExec "format" "cargo" @("fmt"); fEcho "OK: formatted" }

	## Stage 2: debug build.
	fSection "2  Debug build"
	fExec "debug build" "cargo" @("build")
	fEcho "OK: debug build"

	## Stage 3: tests + advisory lints.
	fSection "3  Tests"
	fExec "tests" "cargo" @("test")
	fEcho "OK: tests passed"
	fInstallerTests
	## In a process of its own, since fTestDir_Use moves TEMP and TMP for the whole process.
	fExec "test run folder removal" "pwsh" @("-NoProfile", "-NonInteractive", "-File", (Join-Path $Root "cicd\tests\testdir\remove.ps1"))
	fEcho "OK: test run folder removal"
	fLintAdvisory

	## Stage 4: release builds (x86_64 msvc + gnu always; ARM64 when ready).
	## (No profiler stage here: pprof's SIGPROF sampler is Unix-only - the
	## profiling feature can't even compile for a Windows target.)
	$script:RemapConfig = fRemapConfig
	$built = @()
	foreach ($t in $Targets) {
		$r = fBuildTarget $t
		if ($r) { $built += $r }
	}
	if (-not $built) { fDie "no release binaries were produced" }
	foreach ($b in $built) {
		if (fHasLocalPaths $b.Exe) { fDie "$($b.Exe) still holds a local path ($env:USERPROFILE or $Root)" }
	}
	fEcho "OK: no local paths in $($built.Count) binary(s)"
	$ver = fVersion
	fCollectArtifacts -Built $built -Ver $ver

	## Stage 5: packages.
	fSection "5  Packages"
	if ($NoPackage -or $Quick) { fNote "packages skipped" }
	else { fBuildPackages -Built $built -Ver $ver }

	## Stage 6: the Linux half.
	fSection "6  Linux half (WSL2)"
	if (-not $Wsl) { fNote "not requested (-Wsl builds the Linux artifacts here too)" }
	else { fWslLinuxHalf }

	## Stage 7: dogfood.
	fSection "7  Dogfood"
	if ($NoDogfood) { fNote "dogfood skipped" }
	else { fDogfood -Built $built }

	## Stage 8: publish.
	fSection "8  Publish"
	if ($NoPublish) { fNote "publish skipped" }
	else { fPublish -Msg $publishMsg }

	fSection "$AppName Windows CI/CD: done."
	fEcho_Clean
}

try {
	fMain
	fTestDir_End 0
} finally {
	try { Stop-Transcript | Out-Null } catch {}
}


##	History:
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
