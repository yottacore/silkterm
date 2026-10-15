<!-- markdownlint-disable MD055 -- Table pipe style [Expected: leading_and_trailing; Actual: leading_only; Missing trailing pipe] -->
<!-- markdownlint-disable MD041 -- First line in a file should be a top-level heading -->

<!-- TOC ignore:true -->
# Using SilkTerm

Where the config lives, shell integration, and opening scripts and folders on Windows.

<!-- TOC ignore:true -->
## Table of contents

<!-- TOC -->

- [Configuration](#configuration)
- [Shell integration](#shell-integration)
- [Opening scripts and folders on Windows](#opening-scripts-and-folders-on-windows)

<!-- /TOC -->

## Configuration

On first run SilkTerm writes a commented config file with all defaults, in the place each platform keeps settings:

| OS      | Config                                            | Wallpaper folder
| :------ | :------------------------------------------------ | :-----------------------------------
| Linux   | `$XDG_CONFIG_HOME/silkterm/` (or `~/.config/...`) | beside the config
| Windows | `%APPDATA%\silkterm\`                             | `%LOCALAPPDATA%\silkterm\wallpaper\`
| macOS   | `~/Library/Application Support/silkterm/`         | beside the config

Setting `XDG_CONFIG_HOME` overrides the platform default everywhere, and `--config PATH` overrides everything - an alternate config keeps its own wallpaper and history beside itself rather than sharing the defaults.

Windows splits the two because settings are worth roaming between machines and a 60 MiB wallpaper pack is not. A pack already sitting beside the config still works; nothing has to be moved.

If making changes directly (rather than through Settings), you can apply them immediately with the "Reload config" menu item. Settings reads the file each time it opens, so it also shows what another SilkTerm window saved.

Settings names are meant to read plainly, but a few of them - scrim, contrast mask, automask mix - are particular to SilkTerm. [glossary.md](glossary.md) defines those.

To start over from the shipped defaults, run `silkterm --reset-config`. The old file is kept alongside as `config.shcl.bak` rather than deleted.

When an update converts the file to a newer format, the file as it was is kept alongside, named for the local time it was converted and the format it had, such as `config_backup_20261003-142233_format-v2.shcl`. Every older version is kept, and a file named with `--config` keeps its own name in front. A line the new format can't hold stays in the file as written, and SilkTerm puts up a notice saying how many there were and where the old file is. If the file can't be converted where it is, because it isn't UTF-8 text or the new format can't read it, SilkTerm writes a new one from the defaults with every setting it can still read, and the notice says how many it couldn't copy to the new file. A file already in the current format with lines that aren't UTF-8 text is written again without those lines, and kept alongside the same way. A notice names the lines and where the old file is.

Drop a few images into a `wallpaper` folder next to the config and SilkTerm picks one each launch, favoring whatever it hasn't shown lately. Naming a wallpaper in the config, or passing one on the command line, takes precedence. The [wallpaper pack](install.md#wallpaper-pack) is a ready-made folder to start from.

## Shell integration

A new tab, split or window starts in the directory the current pane is in. For most shells that needs nothing set up - SilkTerm reads the shell process's own directory.

PowerShell is the exception: `Set-Location` moves PowerShell's own idea of where it is and leaves the process where it was launched, so there is nothing to read and a new pane would start in the launch directory.

SilkTerm handles that one for you. A few seconds after launch it adds a small directory-reporting block to each PowerShell profile - and it will not touch a profile that already reports (a Windows Terminal setup, oh-my-posh, anything else), will not rewrite what is there (it appends, after saving a copy beside it), will not change your prompt, and will not put the block back if you delete it. A shell whose execution policy would refuse to load the profile is left alone and said so, rather than being handed a file it cannot read. Clear "Update PowerShell profiles" on the Shell tab of Settings, or set `shell.integration: false`, to switch it off before it ever runs.

Same story for a shell running behind `ssh` or in a container - that one is yours to set up, since the shell you are typing at is not the process SilkTerm started.

[shell-integration.md](shell-integration.md) covers all of it, including the snippets for bash, zsh and fish.

## Opening scripts and folders on Windows

The Shell tab of Settings has an "Open with SilkTerm" group, one row per kind of file:

- Batch files: `.bat` and `.cmd`.

- PowerShell scripts: `.ps1`, with PowerShell 7 if it is installed and Windows PowerShell if not.

- VBScript files: `.vbs`, through the console script host, so their messages print in the pane.

- Folder menu: an "Open in SilkTerm" entry for folders and drives. On Windows 11 it is under "Show more options".

Register makes a double-click run that kind of file in SilkTerm. The pane stays open after the script ends, so its output and exit status can be read. It is for your account only and needs no administrator rights. The arrow at the end of the row puts back whatever was there before. Register again after moving SilkTerm.

What it does not cover:

- A console program started any other way, such as `cmd` from the Run box or a double-clicked `.exe`, still opens where it did. SilkTerm is not a replacement for Windows' default terminal setting.

- A file type you picked an app for under "Open with" keeps that app, because Windows keeps that choice out of reach of other programs. Settings says so, and choosing SilkTerm there yourself is what changes it.

- A script run as administrator still opens in the Windows console.
