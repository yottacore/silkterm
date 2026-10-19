<!-- markdownlint-disable MD055 -- Table pipe style [Expected: leading_and_trailing; Actual: leading_only; Missing trailing pipe] -->
<!-- markdownlint-disable MD041 -- First line in a file should be a top-level heading -->

<!-- TOC ignore:true -->
# Installation

How to get SilkTerm onto a machine, build it from source, and set up to work on it. The optional wallpaper pack is at the end.

<!-- TOC ignore:true -->
## Table of contents

<!-- TOC -->

- [Packages and installers](#packages-and-installers)
- [Direct stable and dev install scripts](#direct-stable-and-dev-install-scripts)
- [Build it yourself](#build-it-yourself)
- [Set up development environment](#set-up-development-environment)
- [Wallpaper pack](#wallpaper-pack)

<!-- /TOC -->

## Packages and installers

The primary install is a native package from the [releases page](https://github.com/yottacore/silkterm/releases): `.deb` / `.rpm` on Linux, or the NSIS setup `.exe` on Windows. Optional either way: fetch the wallpaper pack, as the [Wallpaper pack](#wallpaper-pack) section shows.

On macOS, SilkTerm is a Mac app sold through an app store. A Microsoft Store version is on the way as well. Links go here once the listings are up. The packages on the releases page stay free.

## Direct stable and dev install scripts

Prefer a plain binary? These one-liners work out your operating system and CPU on their own, download the release built for it, check its sha256, and install it. Once a release is signed, the checksums file has to have a good signature from the release key or nothing is installed. Each prints what it is about to do and asks before touching anything, and does nothing at all when you are already up to date. The defaults suit most people - add `--help` for the handful of things you can change.

Bash, written for 3.2 or newer and tested on 5 (Linux, macOS, WSL):

~~~bash
bash <(curl -fsSL https://raw.githubusercontent.com/yottacore/silkterm/main/install.bash)
~~~

PowerShell 5.1 or 7+ (Windows, Linux, macOS):

~~~powershell
irm https://raw.githubusercontent.com/yottacore/silkterm/main/install.ps1 | iex
~~~

PowerShell needs the script-block form to pass anything, `-Help` included:

~~~powershell
& ([scriptblock]::Create((irm 'https://raw.githubusercontent.com/yottacore/silkterm/main/install.ps1'))) -Help
~~~

Install locations:

| OS      | User install (default)              | <- Launcher                                                   | (or) System install          | <- Launcher
| :------ | :---------------------------------- | :------------------------------------------------------------ | :--------------------------- | :---------------------------------------------------
| Linux   | `~/.local/bin/silkterm`             | `~/.local/share/applications/silkterm.desktop`                | `/usr/local/bin/silkterm`    | `/usr/local/share/applications/silkterm.desktop`
| Windows | `%LOCALAPPDATA%\Programs\SilkTerm\` | Start Menu shortcut, and the install dir is added to `%PATH%` | `C:\Program Files\SilkTerm\` | Common Start Menu shortcut (needs an elevated shell)

The releases page has Linux and Windows binaries only, and the Mac app comes from its store listing. On anything else the installer says so and lists what the release does include, so build it yourself - below.

## Build it yourself

Install the per-platform prerequisites first ([prerequisites.md](prerequisites.md)), then on Linux:

~~~bash
cargo run --release
~~~

That's the whole of it for a native build. [build.md](build.md) covers the cross-builds (Windows, and ARM64 for both) which all run from an x86_64 Linux box.

Release builds are reproducible on the same system and toolchain. A commit built in two different folders gives byte-identical binaries for all four published targets, since neither the folder nor the time of the build goes into them. [`cicd/utility/repro-check.bash`](cicd/utility/repro-check.bash) checks that the way the pipeline builds a release. A build on another distribution or with another linker can still differ.

## Set up development environment

[prerequisites.md](prerequisites.md) lists what each platform needs, down to the package names and the one-time toolchain setup. [build.md](build.md) covers the build and cross-build commands, and [contributing.md](contributing.md) covers the branch and review flow.

The toolchain version is pinned in `rust-toolchain.toml`, so rustup picks the right one on its own.

To run everything a change has to pass before it can be pushed - format, lint, regression tests, profiling, the release and cross builds, packaging, then backup and publish:

~~~bash
cicd/cicd.bash [--quick]
~~~

`--quick` runs only the quick lints and tests, builds for x86_64 only, and skips the slow stages and anything that needs another machine. `--container` runs the build and test stages in a pinned Debian image, so only docker is needed for them (see [build.md](build.md)). `--leakcheck` adds the slow leak checks, which a full run leaves out. A fast subset of it - format check, lint, tests - also runs as a pre-push hook on any push to main (`cicd/cicd.bash --gate`). Turn the hooks on once per clone:

~~~bash
git config core.hooksPath utility/git-hooks
~~~

## Wallpaper pack

The wallpapers themselves are shown in the [README](README.md#wallpaper-pack), with a link to the full gallery.

They come to 58 MiB against an 11 MiB terminal, so no package or installer includes them - fetch the folder on its own. Bash (Linux, macOS, WSL):

~~~bash
dir="${XDG_CONFIG_HOME:-$HOME/.config}/silkterm" && mkdir -p "$dir" && curl -fsSL https://github.com/yottacore/silkterm/archive/refs/heads/main.tar.gz | tar -xz -C "$dir" --strip-components=5 silkterm-main/filesystem/home/.config/silkterm/wallpaper
~~~

PowerShell (Windows):

~~~powershell
$dest = "$env:LOCALAPPDATA\silkterm"; $tgz = "$env:TEMP\silkterm-main.tar.gz"; New-Item -ItemType Directory -Force $dest | Out-Null; curl.exe -fsSL https://github.com/yottacore/silkterm/archive/refs/heads/main.tar.gz -o $tgz; tar -xzf $tgz -C $dest --strip-components=5 silkterm-main/filesystem/home/.config/silkterm/wallpaper; Remove-Item $tgz
~~~

Either one is a single line, so it survives a paste however your terminal handles one, and puts the images where rotation looks for them - `wallpaper/` beside the config on Linux and macOS, and under `%LOCALAPPDATA%` on Windows (see the table in [Configuration](usage.md#configuration)). Both pull the whole repository archive, since GitHub serves no smaller unit - about 67 MiB over the wire.
