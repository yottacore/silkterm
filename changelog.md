<!-- markdownlint-disable MD007 -- Unordered list indentation -->
<!-- markdownlint-disable MD010 -- No hard tabs -->
<!-- markdownlint-disable MD033 -- No inline html -->
<!-- markdownlint-disable MD055 -- Table pipe style [Expected: leading_and_trailing; Actual: leading_only; Missing trailing pipe] -->
<!-- markdownlint-disable MD041 -- First line in a file should be a top-level heading -->

# Changelog

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

<!--
## vNEXT - DATE

### Notes

### Added

### Changed

### Removed

### Other work
-->

## v1.0.0-beta4 - 2026-11-26

### Notes

- The project moved to [yottacore/silkterm](https://github.com/yottacore/silkterm). Old links still redirect.

- The settings file is SHCL format 3 now. A file from an earlier beta is converted on the first launch, and the old one is kept beside it.

- This is the first release to be documented here.

### Added

- New minimap of the scrollback beside the text.

- Tabs name themselves from the shell, program, or folder, and can be manually set by the user.

- New tabs, splits, and windows start in the folder of the pane you're in.

- SilkTerm finds the shells on the system, WSL distributions included. Each new tab can also start in its own tmux session.

- bash and PowerShell can optionally have a git-aware prompt.

- On Windows, scripts and folders can be opened in SilkTerm from Explorer.

- Window size and zoom are remembered per monitor.

	- This will probably perpetually be an "experimental" feature off by default. Even though I have several hardware and VM testing rigs, I can't test on all possible combinations, and it's a tricky problem to solve with unpredictable wake-up times per-monitor. Never crashes or loses data, there just can't be a guarantee it will restore to the exact expected position every time for every multimonitor combination, when they wake up from power saving mode.

- Text colors optionally based on wallpaper color.

- A performance profile is picked on the first run, and a basic temporary profile is chosen when used over common remote desktops. (All can be manually overridden.)

- An idle window returns its memory and GPU resources, and recovers nearly instantly once needed.

	- This is important for running numerous terminals, all which run in their own process for stability. (A future version will allow running all in a single process but unless you have an old/slow rig, I would advise multi-process mode.)

- Double-click intelligently selects whole paths, URLs, and such.

- Settings has a search box, and most relevant settings activate immediately in every other open window.

- There's a fourth theme, and light or dark can be set to follow the system.

- A program in a pane can set the clipboard.

- A contrast floor and a text outline can be used along with the scrim.

- A wallpaper can set its own visibility and blur, and `--wallpaper` changes it from inside a pane.

- There are new docs for usage and installing, and a glossary.

### Changed

- Smooth output scrolling works in full-screen programs like `less` and `vim` too. (Game-changing.)

- There are lots of fixes and speedups, mostly around saving settings, Wayland, and Windows.

### Removed

- The wallpaper pack is a separate download now, and a single default "rainbow paper" wallpaper is built in to the executable.

### Other work

- There are many more tests, and a lot of build and release cleanup.

- SilkTerm builds on macOS (Intel and M-series) from the same code. Releases don't include a macOS build.
