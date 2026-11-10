<!-- markdownlint-disable MD007 -- Unordered list indentation -->
<!-- markdownlint-disable MD010 -- No hard tabs -->
<!-- markdownlint-disable MD055 -- Table pipe style [Expected: leading_and_trailing; Actual: leading_only; Missing trailing pipe] -->
<!-- markdownlint-disable MD041 -- First line in a file should be a top-level heading -->

# Building SilkTerm

SilkTerm is a Rust project using `wgpu` (GPU), `winit` (windowing), `glyphon` (text), and `alacritty_terminal` (VT parser and PTY). From an x86_64 Linux host it builds Linux x86_64/ARM64 and Windows x86_64/ARM64, and all but Linux x86_64 are cross-compiled. macOS is built natively on a Mac.

## Toolchain

It needs a Rust toolchain for edition 2024, at rustc 1.96, which is the version pinned in `rust-toolchain.toml`. The code doesn't build on 1.94. `rustup` is the simplest way to manage targets:

~~~sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
~~~

## Linux (native)

~~~sh
cargo build --release
./target/release/silkterm
~~~

Running it needs a GPU with Vulkan or GL, under X11 or Wayland. On Debian with X11, the dependencies are the usual Mesa and Vulkan packages that are already on a desktop.

## Windows (cross-compile from Linux)

This uses the GNU ABI, so it links with mingw-w64 and doesn't need MSVC or a Windows host.

One-time setup:

~~~sh
rustup target add x86_64-pc-windows-gnu
sudo apt-get install -y gcc-mingw-w64-x86-64    # Debian/Ubuntu
~~~

Build:

~~~sh
cargo build --release --target x86_64-pc-windows-gnu
	# -> target/x86_64-pc-windows-gnu/release/silkterm.exe
~~~

The linker and a static-CRT flag are set in `.cargo/config.toml`, so the resulting `.exe` is self-contained. It depends only on stock Windows system DLLs, and it's a GUI binary, so a release build doesn't open a console window.

### Running the Windows build on Linux, under wine

`utility/run-windows-build-via-wine.bash` runs that `.exe` here, so the Windows build can be looked at without a Windows machine:

~~~sh
./utility/run-windows-build-via-wine.bash              # newest build, on the current display
./utility/run-windows-build-via-wine.bash --restage    # rebuild the wineprefix first
./utility/run-windows-build-via-wine.bash --attach     # foreground, output on this terminal
~~~

It stages a private wineprefix, the exe, and its own `config.shcl` under `cicd/artifacts/win-run/` (gitignored), so `~/.wine` and the real `~/.config/silkterm` are untouched. It needs `wine`. It also uses mingw to build the small shim that's described below, and mingw is already a prerequisite for the cross-build. Wine's menu builder is turned off for these runs, so nothing gets registered on the desktop.

A wineprefix maps `Z:` to `/`, and further drives to raw `/dev` nodes, as symlinks under `prefix/dosdevices/`. So anything that walks the tree and follows symlinks ends up out of the repo and in the whole filesystem. `cicd/config.bash` excludes `cicd/artifacts` from the backup for that reason. Keep any new tree walker off it too, or point it at `--exclude`.

What this gets you, and what it doesn't:

- Rendering, fonts, menus, tabs, dialogs, wallpaper, and the scrim all work, on the real GPU, since wgpu gets to the card through winevulkan. That's most of what a Windows check is for.

- The shell doesn't work. Wine's ConPTY is only half implemented. `CreatePseudoConsole` succeeds and the initial grid size is honored, but a child started with `PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE` never attaches to it, so no output gets to the grid. Expect a correctly drawn, empty terminal.

- `ResizePseudoConsole` is a stub that returns `E_NOTIMPL`, and the terminal backend asserts that it returns `S_OK`, so the app would die on its first resize. The script builds a tiny `conpty.dll` that passes create and close through to kernel32 and returns `S_OK` for a resize. The backend prefers any `conpty.dll` it can load over kernel32. Microsoft's own `conpty.dll` and `OpenConsole.exe` fail earlier under wine, so there isn't anything better to drop in.

- Only the x86_64 build runs. Wine on x86_64 can't execute the ARM64 exe.

A newer wine doesn't help, so there's no reason to chase one. On wine 11.14 staging the shell is still dead. The pty pipe doesn't get any bytes, and the child's output escapes to the parent, exactly as on 10.0. The one thing that changes is `ResizePseudoConsole`, which fakes success from wine 11.2 onward, so the shim above isn't needed there. It's still a stub and resizes nothing. Wine 11.0 stable isn't enough, since it still returns `E_NOTIMPL`.

To try a different wine anyway, put it first on `PATH`. The script calls `wine` and `wineboot` unqualified, so nothing else is needed. Give it its own prefix, since wine upgrades a prefix in place and an older wine won't take it back:

~~~sh
PATH=/path/to/other-wine/bin:$PATH ./utility/run-windows-build-via-wine.bash --restage
~~~

WineHQ publishes Debian packages rather than tarballs, but they install self-contained under `/opt/wine-*`, so `dpkg -x wine-staging-amd64_*.deb <dir>` gives a runnable tree without touching the system wine.

## ARM64 (Linux & Windows, cross-compile via cargo-zigbuild)

`cargo-zigbuild` uses `zig` as a universal cross-linker, so ARM64 Linux and Windows build from an x86_64 Linux host without a gcc or SDK for each target.

One-time setup, with zig 0.16.0 from its binary tarball. `pip install ziglang` also works.

~~~sh
curl -fsSL https://ziglang.org/download/0.16.0/zig-x86_64-linux-0.16.0.tar.xz | tar -xJ -C ~/.local
ln -sf ~/.local/zig-x86_64-linux-0.16.0/zig ~/.local/bin/zig    # ~/.local/bin on PATH
cargo install cargo-zigbuild
rustup target add aarch64-unknown-linux-gnu aarch64-pc-windows-gnullvm
~~~

Build:

~~~sh
cargo zigbuild --release --target aarch64-unknown-linux-gnu     # Linux ARM64
cargo zigbuild --release --target aarch64-pc-windows-gnullvm    # Windows ARM64
~~~

There aren't any ARM64 system libraries needed at link time. X11, EGL, and Wayland are loaded at runtime with dlopen, so the link succeeds with only zig's bundled libc and CRT. Both ARM64 targets build cleanly, as a Linux ELF aarch64 binary and a Windows PE32+ ARM64 one.

## Windows (native, MSVC)

If building on Windows with the MSVC toolchain instead:

~~~sh
rustup target add x86_64-pc-windows-msvc
cargo build --release            # default target on a Windows host
~~~

## macOS (native)

macOS is built natively on a Mac. Cross-compiling from Linux to macOS needs the Apple SDK, and that isn't set up here. On a Mac:

~~~sh
cargo build --release
./target/release/silkterm
~~~

wgpu uses the Metal backend automatically. No extra system packages are needed beyond the Xcode command-line tools (`xcode-select --install`).

`cicd/ci-mac.bash` builds, lints, and tests in one go, and needs bash 4.4 or newer ahead of Apple's 3.2 on `PATH` for the prompt tests. GitHub runs it on an arm64 Mac for each push to `dev` and `main`.

## The pipeline in a container

`cicd/cicd.bash` runs the build, lint, test, release, and package stages in a Debian trixie image built from `cicd/container/Dockerfile`, with every tool at the version `rust-toolchain.toml` and `cicd/tool-pins.txt` pin. It needs docker and nothing else from this page, and docker is used whenever it's installed. `--no-container` runs those stages on this machine instead, with the tools from this page. Without docker, that's what happens anyway, with a note saying so. The first run builds the image, which takes a while. After that the image is reused until the recipe or a pin changes.

- The repo is mounted at its own path, and the run inside builds into `target/container`, so the builds here are left alone. Count on about as much disk again as `target/` takes.

- The crate cache and the run's home are a named volume, `silkterm-cicd-cache`. `docker volume rm silkterm-cicd-cache` starts it fresh. Zig's cache, which the ARM64 builds fill, is a volume of its own, `silkterm-zig-cache`. A run empties either one when it's over 2 GB, and the Zig one when another Zig version filled it. The Zig cache in your home folder is never touched.

- Each run works in a folder of its own, `${HOST_TEMP}/silkterm/<YYYYmmDD-HHMMSS>_<NNN>`, or under `TMPDIR` or `/tmp` where `HOST_TEMP` isn't set, with `TMPDIR`, `TEMP`, and `TMP` pointed at it. The container's `/tmp` is a folder in it. It's removed when the run ends, even a failed or stopped one, unless `--keep-temp` is given. A folder that a killed run left is removed after 2 days, once no process from its `run.pid` is left.

- What the pipeline makes in docker is labeled `dev.project=silkterm` and `dev.run=<run>`, and only that is ever cleaned up. When a run ends, even a failed or stopped one, its containers are removed. Then our untagged images are removed, and once a run has passed, so is every image tag older than the one it used and the one before that. Build cache over a week old is removed as well, for every project, so the next build stays warm. The run ends with one cleanup line, which names what was removed, including its own temp folder and stale build output, and about how much that freed. For docker that's `docker system df` before the cleanup against after it. `--keep-temp` keeps the run's containers, and its temp folders too, and says where they are.

- Everything that needs this machine stays outside and runs once the container is done: the remote sync, the Windows boxes, the private runner, dogfood, the demo, and publish. So do the steps that need the graphics card, since the window tests inside run on software rendering: the render node test and the profiler.

- The profiler draws on the card through a headless sway with its own Xwayland, on the card's render node, so nothing shows on the desktop. Without sway, Xwayland, or a render node it falls back to a private Xvfb, which renders in software, and says so.

- `cicd/cicd.bash --backlog-run-quick` runs inside too.

- A full run with `--no-container` still uses the image for 3 steps. The test suite under AddressSanitizer, under ThreadSanitizer with `--long`, and under Miri with `--leakcheck` or `--long`, need the nightly that `rust-toolchain.toml` dates, and only the image has it. `--no-asan`, `--no-tsan`, and `--no-miri` skip those steps, and then the run doesn't need docker.

## Formatting

`rustfmt.toml` pins the style (`hard_tabs`). The hand-formatted data tables have `#[rustfmt::skip]`, so `cargo fmt` leaves them compact. Those are the `Palette` and `Dlg` color matrices in `theme.rs` and `settings_ui.rs`, and the About table in `dialog.rs`. Everything else is in rustfmt's canonical style.

A pre-commit hook (`utility/git-hooks/pre-commit`) reformats the staged `.rs` files on every commit so they never drift. It formats what's staged rather than the file on disk, so a file with only some of its changes staged still commits only those. The working copy is reformatted as well, where it doesn't have anything unstaged to lose. Activate it once per clone:

~~~sh
git config core.hooksPath utility/git-hooks
~~~
