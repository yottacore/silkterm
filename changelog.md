<!-- markdownlint-disable MD024 -- No duplicate headings [OK with no TOC] -->
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

### Added

- There's a minimap of the scrollback beside the text.

- Tabs name themselves from the shell, program, or folder, and what they say can be changed.

- New tabs, splits, and windows start in the folder of the pane you're in.

- SilkTerm finds the shells on the system, WSL distributions included. Each new tab can also start in its own tmux session.

- bash and PowerShell can have a git-aware prompt.

- On Windows, scripts and folders can be opened in SilkTerm from Explorer.

- Window size and zoom are remembered per monitor.

- Text colors can follow the wallpaper.

- A performance profile is picked on the first run, and a lighter one is used over remote desktop.

- An idle window gives back its memory and GPU resources.

- Double-click selects whole paths, URLs, and such.

- Settings has a search box, and a saved change shows up in every open window.

- There's a fourth theme, and light or dark can follow the system.

- A program in a pane can set the clipboard.

- A contrast floor and a text outline can be used along with the scrim.

- A wallpaper can set its own visibility and blur, and `--wallpaper` changes it from inside a pane.

- There are new docs for usage and installing, and a glossary.

### Changed

- Smooth output scrolling works in full-screen programs like `less` and `vim` too.

- There are lots of fixes and speedups, mostly around saving settings, Wayland, and Windows.

### Removed

- The wallpaper pack is a separate download now, and a single wallpaper is built in.

### Other work

- There are many more tests, and a lot of build and release cleanup.

- SilkTerm builds on macOS from the same code. Releases don't include a macOS build.

## v1.0.0-beta3 - 2026-08-23

### Notes

- It's the last release before this changelog started. The README at the [v1.0.0-beta3 tag](https://github.com/yottacore/silkterm/tree/v1.0.0-beta3) lists what it had.
