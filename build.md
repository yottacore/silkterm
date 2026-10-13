# Building SilkTerm

SilkTerm is a Rust project using `wgpu` (GPU), `winit` (windowing), `glyphon` (text), and `alacritty_terminal` (VT parser + PTY). From an x86_64 Linux host it builds Linux x86_64/ARM64 and Windows x86_64/ARM64 (the latter three are cross-compiled); macOS is built natively on a Mac.

## Toolchain

Requires a Rust toolchain, edition 2024. It needs rustc 1.96, the version `rust-toolchain.toml` pins. The code does not build on 1.94. `rustup` is the simplest way to manage targets:

~~~sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
~~~

## Linux (native)

~~~sh
cargo build --release
./target/release/silkterm
~~~

Runtime needs a GPU with Vulkan or GL (X11 or Wayland). On Debian/X11 the deps are the usual Mesa/Vulkan packages already present on a desktop.

## Windows (cross-compile from Linux)

Uses the GNU ABI so it links with mingw-w64 - no MSVC or Windows host needed.

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

The linker and a static-CRT flag are set in `.cargo/config.toml`, so the resulting `.exe` is self-contained (depends only on stock Windows system DLLs) and is a GUI binary (no console window in release builds).

### Running the Windows build on Linux, under wine

`utility/run-windows-build-via-wine.bash` runs that `.exe` here, so the Windows build can be looked at without a Windows machine:

~~~sh
./utility/run-windows-build-via-wine.bash              # newest build, on the current display
./utility/run-windows-build-via-wine.bash --restage    # rebuild the wineprefix first
./utility/run-windows-build-via-wine.bash --attach     # foreground, output on this terminal
~~~

It stages a private wineprefix, the exe, and its own `config.shcl` under `cicd/artifacts/win-run/` (gitignored), so `~/.wine` and the real `~/.config/silkterm` are untouched. Needs `wine`; mingw is used for a small shim (see below) and is already a prerequisite for the cross-build. Wine's menu builder is turned off for these runs, so nothing gets registered on the desktop.

One consequence: a wineprefix maps `Z:` to `/` (and further drives to raw `/dev` nodes) as symlinks under `prefix/dosdevices/`. Anything that walks the tree following symlinks therefore climbs out of the repo and into the whole filesystem. `cicd/config.bash` excludes `cicd/artifacts` from the backup for that reason - keep any new tree-walker off it too, or point it at `--exclude`.

What this does and does not buy you:

- Rendering, fonts, menus, tabs, dialogs, wallpaper and the scrim all work, on the real GPU (wgpu reaches the card through winevulkan). That covers most of what a Windows check is for.

- The shell does not work. Wine's ConPTY is only half implemented: `CreatePseudoConsole` succeeds and the initial grid size is honored, but a child started with `PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE` never attaches to it, so no output reaches the grid. Expect a correctly drawn, empty terminal.

- `ResizePseudoConsole` is a stub returning `E_NOTIMPL`, and the terminal backend asserts that it returns `S_OK` - so the app would die on its first resize. The script builds a tiny `conpty.dll` (the backend prefers any loadable `conpty.dll` over kernel32) that passes create/close through to kernel32 and answers resize with `S_OK`. Microsoft's own `conpty.dll`/`OpenConsole.exe` fail earlier under wine, so there is nothing better to drop in.

- Only the x86_64 build runs; wine on x86_64 cannot execute the ARM64 exe.

A newer wine does not help, so there is no reason to chase one. Measured against wine 11.14 (staging): the shell is still dead - the pty pipe gets zero bytes and the child's output escapes to the parent, exactly as on 10.0. The one thing that does change is `ResizePseudoConsole`, which fakes success from wine 11.2 onward (still a stub, it resizes nothing) and so merely makes the shim above redundant. Wine 11.0 stable is not enough - it still returns `E_NOTIMPL`.

To try a different wine anyway, put it first on `PATH`: the script calls `wine`/`wineboot` unqualified, so nothing else is needed. Give it its own prefix, since wine upgrades a prefix in place and an older wine will not take it back:

~~~sh
PATH=/path/to/other-wine/bin:$PATH ./utility/run-windows-build-via-wine.bash --restage
~~~

WineHQ provides Debian packages rather than tarballs, but they install self-contained under `/opt/wine-*`, so `dpkg -x wine-staging-amd64_*.deb <dir>` gives a runnable tree without touching the system wine.

## ARM64 (Linux & Windows, cross-compile via cargo-zigbuild)

`cargo-zigbuild` uses `zig` as a universal cross-linker, so ARM64 Linux and Windows build from an x86_64 Linux host with no per-target gcc/SDK.

One-time setup:

~~~sh
# zig 0.16.0 (binary tarball; `pip install ziglang` also works)
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

No link-time ARM64 system libraries are needed: X11/EGL/Wayland are loaded at runtime (dlopen), so the link succeeds with only zig's bundled libc/CRT. Verified build-clean on both ARM64 targets (Linux ELF aarch64; Windows PE32+ ARM64).

## Windows (native, MSVC)

If building on Windows with the MSVC toolchain instead:

~~~sh
rustup target add x86_64-pc-windows-msvc
cargo build --release            # default target on a Windows host
~~~

## macOS (native)

macOS is built natively on a Mac (cross-compiling Linux->macOS needs the Apple SDK and is not set up here). On a Mac:

~~~sh
cargo build --release
./target/release/silkterm
~~~

wgpu uses the Metal backend automatically. No extra system packages are needed beyond the Xcode command-line tools (`xcode-select --install`).

## The pipeline in a container

`cicd/cicd.bash --container` runs the build, lint, test, profiler, release and package stages in a Debian trixie image built from `cicd/container/Dockerfile`, with every tool at the version `rust-toolchain.toml` and `cicd/tool-pins.txt` pin. It needs docker and nothing else from this page. The first run builds the image, which takes a while. After that the image is reused until the recipe or a pin changes.

- The repo is mounted at its own path, and the run inside builds into `target/container`, so the builds here are left alone. Count on about as much disk again as `target/` takes.

- The crate cache and the run's home are a named volume, `silkterm-cicd-cache`. `docker volume rm silkterm-cicd-cache` starts it fresh.

- Everything that needs this machine stays outside and runs once the container is done: the remote sync, the Windows boxes, the private runner, dogfood, the demo and publish. The window tests inside run on software rendering, so the few that need a render node skip there.

- `cicd/cicd.bash --gate --container` runs the merge gate inside.

## Formatting

`rustfmt.toml` pins the style (`hard_tabs`). The hand-formatted data tables (the `Palette`/`Dlg` color matrices in `theme.rs`/`settings_ui.rs`, the About table in `dialog.rs`) have `#[rustfmt::skip]` so `cargo fmt` leaves them compact; everything else is rustfmt-canonical.

A pre-commit hook (`utility/git-hooks/pre-commit`) reformats the staged `.rs` files on every commit so they never drift. It formats what is staged, not the file on disk, so a file with only some of its changes staged still commits only those; the working copy is reformatted as well where it has nothing unstaged to lose. Activate it once per clone:

~~~sh
git config core.hooksPath utility/git-hooks
~~~
