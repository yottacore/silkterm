<!-- markdownlint-disable MD007 -- Unordered list indentation -->
<!-- markdownlint-disable MD010 -- No hard tabs -->
<!-- markdownlint-disable MD041 -- First line in a file should be a top-level heading -->

# Terminal showdown rigs

This is everything behind the "Terminal showdown" table in [comparison.md](../../comparison.md). There's one entry point, `utility/update-showdown.py`, over 2 rigs that measure different things:

~~~sh
utility/update-showdown.py                               # measure this terminal, any OS
utility/update-showdown.py --list
utility/update-showdown.py --term alacritty              # both rigs, writes the table
utility/update-showdown.py --term kitty --size-only
utility/update-showdown.py --all --no-readme             # measure everything, write nothing
~~~

|       | Rig                  | Grid   | Display                       | Columns it owns
| :---- | :------------------- | :----- | :---------------------------- | :------------------------------
| speed | `termbench-run.bash` | 160x42 | headless sway on the real GPU | the width classes and the score
| size  | `sizebench-run.bash` | 100x30 | private Xvfb                  | File+deps and Mem

The speed rig makes one exception. xterm draws only on X11, so it runs on a private Xvfb (`--display`, default `:98`) rather than through the compositor's Xwayland. The notes on xterm below say why.

Without a terminal named, it takes the third path, which doesn't need a rig at all. It measures whatever terminal you're in, from inside it. That's the only way to measure a terminal that exists only on Windows or macOS, and the only mode that works off Linux. Both halves are available that way, but only one at a time, since the 2 need different window sizes.

**The 2 grids differ on purpose and must not be unified.** Speed needs a realistic working grid. Memory scales with the window surface. The same SilkTerm binary reads 38 MiB heavier at its default geometry than at 100x30, so the size rows are taken small and identical.

Neither rig is part of cicd. They take minutes, need a GPU, and republishing numbers on every build would churn the table. Run them by hand when a row needs adding or refreshing.

The 2 writers own separate columns and key on the terminal name, so they never write the same cell. `termbench.py` refreshes the speed cells, `showdown-readme.py` refreshes the size cells, and everything else in the table stays exactly as written by hand.

## Speed: what the number means

The number is ingest and keep-up throughput, as in "why does it bog down when something dumps text", rather than rasterization. At 160x42 only ~3400 cells are ever visible, so most of a 100 MB stream is parsed, stored, and scrolled past without being drawn. The clock stops when the terminal replies to a Primary DA query (`ESC[c`), which it can only reply to once it has worked through everything queued.

A terminal that never replies can't be timed this way. `termbench.py` records that as `synced: false` and refuses to publish the row, so a figure without the barrier never gets into the table, since it would be timing a timeout. Hyper is the standing example.

**On Windows, ConPTY replies to the barrier rather than the terminal, and that changes what a Windows figure means.** As measured on 20260818, the query never appears in the stream the terminal receives. The child gets back a DEC-level `ESC[?61;...c`, which is conhost's own reply, where the terminal would have said `ESC[?6c`. So a Windows number times the console host consuming the payload, with the terminal in the loop only through the pipe between them. It bounds the whole chain well enough, but it doesn't isolate the terminal, and the ceiling it sets is low. A consumer that reads the bytes and discards them measures 12.45 MB/s of ASCII on the VM, against 12.44 for a real terminal. Any 2 Windows terminals within a percent of each other are both at that ceiling.

### The rig is not neutral - this is the whole reason it exists

These were measured on 20260730, with the same terminal and grid on 3 rigs:

| Rig                               | SilkTerm ascii | xterm ascii
| :-------------------------------- | -------------: | ----------:
| Software GL (Xvfb + llvmpipe)     |        45 MB/s |    ~29 MB/s
| VirtualGL on Xvfb                 |        76 MB/s |    ~29 MB/s
| Headless sway on the discrete GPU |        88 MB/s |   28.3 MB/s

GPU-accelerated terminals vary by a factor of 2, but CPU-rendered ones don't move at all. So a table assembled from mixed rigs can rank the wrong terminal first. A VirtualGL run would have published a GTK terminal as faster than SilkTerm. **Every published row must come from one rig.** Don't assume a rig is neutral because one terminal reproduces on it.

Headless sway is used rather than the live desktop so the measurement doesn't disturb, and isn't disturbed by, whatever else the machine is doing.

### Every terminal runs on a throwaway account

The speed rig starts each terminal with a home folder, settings, data, and cache folders, and a session bus that it makes for the run and removes after. A published figure then can't depend on the measuring account's own settings, fonts, or wallpaper, and a terminal that writes its settings at launch writes them into the throwaway folder. SilkTerm does that. It fills in its settings file and adds a block to the PowerShell profile. The session bus is part of it because GNOME Terminal and xfce4-terminal keep their settings behind it. Setting `HOME` alone isn't enough on a desktop that exports `XDG_CONFIG_HOME`, which most do.

- The measuring tool inside the terminal gets the real home back, so its run history still goes to `~/.local/share/silkterm-bench`.

- Terminals fall back to the system's default monospace font. A cell of a different size can make the grid fitter miss 160x42 on either side, so it takes the middle of 2 near misses.

- The bus offers only the services a terminal keeps its settings in (GNOME Terminal's own, dconf, and xfconf). With the desktop's whole set on offer, GNOME Terminal's server asks for the desktop portal, which asks for a keyring it can't get to from there, and the server is still waiting when its launcher gives up.

- Re-measured through the new launch on 2026-09-18, xfce4-terminal read 94.0 MB/s ASCII against 94.2 published, so the throwaway account doesn't move a figure by itself. GNOME Terminal read 78 against 100 over 2 runs, which may be the account's own profile showing in the published row. Published rows stay as they are until the whole table is measured again. The size rig already gave each terminal its own settings folder, and still uses the account's session bus.

Both SilkTerm rows pin the automatic performance profile off (`termbench-candy.shcl` and `termbench-plain.shcl`). Left on, SilkTerm rates the renderer at first launch and turns effects down on a slow one. On the size rig's software X server it chose Low, which switches off the text scrim and the cursor animation, and the "+candy" row was then measured without them. Each rig prints the profile that was in force, and refuses a "+candy" run where it moved.

### Shortening a run

Use `--reps`. Fewer repetitions of the same payloads leaves the measured rate directly comparable nad only widens the confidence interval. Tabby is in the published table at 4 reps.

Never use `--scale`. Shrinking the payload changes what's measured. Hyper reads 32 MB/s at `--scale 0.05` but roughly 3 MB/s at full size, a tenfold difference. `--quick` isn't suitable for publishing either, because `mode` is part of the comparability key, so quick runs are aggregated separately and never get into the table.

Watch the `CV%` column. On a machine that is also being used for other work, a run that got stepped on shows up there. Published rows have run 1-4%, so re-run anything much worse.

## Size and memory: what the numbers mean

**File+deps** is the executable plus every shared library the running process maps, minus the base OS runtime (libc, libm, libgcc, and the loader) and minus the graphics stack. Self-contained bundles count their extracted payload plus the system libraries they still use. That's a different basis from a packaged terminal's, on purpose, because it's the real answer to "what does installing this cost" for each packaging style.

**Mem** is the unique resident footprint of the whole process tree: private pages summed per process, plus each shared mapping counted once. It doesn't depend on neighbors, and it doesn't double count within the tree, which the obvious alternatives do. Summed RSS charges a multi-process terminal several times for the same pages, and PSS divides shared pages by however many unrelated programs happen to be running.

### Three ways to get this wrong

**`ldd` understates any program that dlopens.** It lists 3 libraries for SilkTerm, but the running process maps 64. Always read a live process, never the binary on disk.

**Name-matching the driver misses what the driver pulls in.** libstdc++ and libxml2 appear in that map only because mesa wants them. Billing them to the terminal inflated SilkTerm's install size by a third. Classify by ldd *closure* from the graphics roots instead, and let the app closure win. A library the app needs in its own right belongs to the app, however many other things also want it.

The name match still has to find the graphics *roots*, and the vendor back ends are called `libEGL_nvidia`, `libGLX_mesa`, and `libdrm_intel`. A pattern anchored with a word boundary misses every one of them. They then look like libraries that nothing in the graphics stack needs, and seeding the app closure with one pulls libLLVM in behind it. That single character put 200 MiB of mesa on the terminal's bill.

**Window size moves the memory number a long way**, for the same reason the 2 rigs use different grids. Every row must be measured at the same grid or the column is meaningless.

### Reproduce before publishing

The rows in the table were measured across several sessions, so a new row is only comparable if the rig reproduces an old one. Controls re-run when the size rig was written:

| Check                     | Published | This rig
| :------------------------ | --------: | -------:
| xterm File+deps           |       6.0 |      6.0
| xterm driver excluded     |       0.0 |      0.0
| SilkTerm driver excluded  |       108 |    108.3
| SilkTerm Mem              |     121.4 |    119.4
| SilkTerm File+deps        |      17.4 |     16.8
| SilkTerm ascii throughput |     93.23 |    92.64

xterm is the useful control because it draws on the CPU and should classify *nothing* as driver. SilkTerm is the other end, with a third of a gigabyte of mapped graphics libraries that all have to fall on the right side of the line.

The published figures are left as they were rather than refreshed to a later pass. They're inside the drift the table already warns about, and rewriting one row's numbers while the others keep their originals would make the column less consistent, not more. Expect a few MiB either way, since libraries load on demand. GTK terminals are the worst for it. gnome-terminal moved from 28.5 to 53.6 between 2 runs of one pass, because the first measured the thin client wrapper rather than `gnome-terminal-server`.

The rig's entries for GNOME Terminal, WezTerm, Tabby, and Hyper were checked against their published rows on 20260928, over 3 runs. Nothing was published from them.

| Row            | File+deps published | This rig | Mem published | This rig
| :------------- | ------------------: | -------: | ------------: | :------------------
| GNOME Terminal |                84.0 |     84.0 |          53.6 | 52.2, 52.3, 52.2
| WezTerm        |               129.9 |    128.9 |          84.8 | 83.1, 82.6, 82.9
| Tabby          |               454.2 |    454.7 |         473.4 | 365.6, 476.3, 473.6
| Hyper          |               300.9 |    299.5 |         309.4 | 267.3, 267.7, 265.6

- A bundle's File+deps agrees only since the classifier stopped counting the bundle's own libraries twice, once in its unpacked size and again as deps. That had put WezTerm 3 MiB over its row and Tabby 7.

- An Electron terminal's Mem moves a long way between runs. Tabby read 366 once and 474 to 476 twice on the same day. Hyper stayed at 266 to 268, against 309 published. Take several runs before publishing either.

## Per-terminal notes

Most terminals don't need anything but their key. The awkward ones, and why:

- **gnome-terminal** never resizes with the compositor output, so it's the one terminal told its geometry directly (`--geometry`). Its first measured run silently came out at 180x45 and had to be redone. Always confirm the fitted grid in the output.
	- The size rig gives it a session bus of its own, as the speed rig does. On the account's bus the window would go to a server already running for the desktop. Its server is started by that bus and is nobody's child, so the rig finds it by the throwaway account's home in its environment and measures it from there down. The `gnome-terminal` command that only asks for the window is left out, as is the bus.

- **xterm** is X11-only. Xwayland fails on this rig (`/tmp/.X11-unix` ownership), so its speed figure was taken on X11, and its cross-rig agreement (28.8 vs 29.2, 1.3%) is what justifies publishing it beside Wayland rows. The size rig runs on Xvfb anyway, so it doesn't need anything special there.
	- Xwayland worked on this rig on 20260928, but xterm read 18.4 MB/s of ASCII through it against the 28.3 published, in one single-rep run on a busy machine.
	- Measured again on 20260929, 6 pairs of runs at 6 reps, the 2 ways taken in turn: 16.1 to 18.6 MB/s of ASCII through Xwayland against 25.1 to 28.7 on a private Xvfb, and a score of 14.5 to 16.4 against 21.5 to 23.9. The low ends of both came with the load average at 10 to 16 on 32 cores. The last 4 pairs, at 3 to 9, read 18.2 to 18.6 and 27.2 to 28.7. So the gap is real, and it's about a third.
	- The X server isn't doing more. Xwayland used 1.1 to 1.6 s of CPU a run and Xvfb 0.2 to 0.4 s. xterm's own CPU went from 62 s to 90 s for the same payloads, nearly all of it user time. The grid and the window size matched both ways, and giving xterm focus on Xvfb didn't change anything.
	- As a control, xfce4-terminal's ASCII doesn't move through Xwayland (92 to 94 MB/s, against 92 to 100 on Xvfb and 84 to 94 on the compositor itself), so the rig's Xwayland isn't slow in general. Its 4-byte scene does drop there, from 67 to 15, and that time is the X server's.
	- So the speed rig runs xterm on a private Xvfb, since its row came from X11 too. At a quiet moment it read 28.32, 49.22 and a score of 23.9, against 28.3, 48.5 and 23.9 published. The row keeps its figure.

- **WezTerm** 20240203 silently falls back to X11 under sway 1.10 despite `enable_wayland`. It's parser-bound and agreed within 1.7% across rigs, so its figure is valid anywhere.
	- Sway tells only its own children where its Xwayland is, so the speed rig asks sway for that display and passes it on.
	- The size rig starts `wezterm-gui` directly, with `initial_cols` and `initial_rows`, and bills the whole extracted AppImage as the Electron bundles are.

- **Hyper** rewrites `~/.hyper.js` on every launch, so it has to be written fresh per run. It never replies to the barrier, so it can't be timed at all.
	- With `XDG_CONFIG_HOME` set, as it is on the throwaway account, the file is `$XDG_CONFIG_HOME/hyper/.hyper.js`. Its `shell` and `shellArgs` start the rig's own shell.
	- It has a size entry only.

- **Tabby** ignores `SHELL` and doesn't offer a profile hook that works. Dismiss its Welcome tab once by clicking "Close and never show again", then hook the run through the login shell's `.bashrc`.
	- Both rigs do that on the throwaway account: `enableWelcomeTab: false` in its `config.yaml` stands in for the click, and `.bashrc` and `.bash_profile` start the rig's shell. That assumes the account's login shell is bash.
	- Tabby 1.0.235 is built on Electron 38, which picks Wayland by itself under the speed rig. The rig still passes `--ozone-platform=wayland`.

- **Tabby and Hyper** don't take a grid on the command line. The size rig starts them on a shell that reports its grid, and resizes the window until that reads 100x30.

- **Electron terminals** driven by hand must not be run under a fake `HOME`. The results store is under `~/.local/share/silkterm-bench`, and redirecting `HOME` sends the results there too. The rig's own launches hand the real home back to the measuring tool, so this is only about the ones it can't start. Give `AppRun` an `APPDIR` or run the inner binary directly. They exit with SIGTRAP or SIGILL after measurement under Xvfb, which is harmless, because the pids have already been sampled.

Only processes these scripts launched are ever signaled, and only by pid. A pattern kill would match the harness's own command line, and has taken out a live session before now.

Binaries are resolved from `PATH` first, then `cicd/artifacts/sizebench/terms/`. That directory is gitignored and excluded from the backup archive, and keeps the downloaded comparison artifacts on purpose, so a re-measure doesn't need a re-download. Alacritty is kept there as an extracted `.deb`:

~~~sh
apt-get download alacritty                                   # no sudo needed
dpkg -x alacritty_*.deb cicd/artifacts/sizebench/terms/..    # gives terms/usr/bin/alacritty
~~~

The WezTerm, Tabby, and Hyper AppImages are kept there too, but they are run extracted and from `PATH`. Extracted under the repo, the 2 Electron bundles include their own `.md` files, which the markdown checks then read. For example:

~~~sh
cicd/artifacts/sizebench/tabby-1.0.235-linux-x64.AppImage --appimage-extract    # makes ./squashfs-root
mv squashfs-root ~/.local/opt/tabby
ln -s ~/.local/opt/tabby/tabby ~/.local/bin/tabby
~~~

WezTerm links as `usr/bin/wezterm` and Hyper as `hyper`. The size rig finds the extracted folder again through the link, since it bills the whole of it.

## Why the table has no Windows speed rows

Footnote 9 in [comparison.md](../../comparison.md) is the short form of this.

Every speed figure comes from one machine, because the measuring rig isn't neutral. Rendering through software OpenGL roughly halves SilkTerm's throughput, and going through VirtualGL still costs it about 14%, while terminals that draw on the CPU don't move at all. A table built from mixed rigs can therefore rank the wrong terminal first. These figures were taken on a headless Wayland compositor driving a discrete GPU (Linux, Ryzen host, GeForce RTX 3060 Ti), so nothing on the desktop competes for the card.

**There are no Windows rows, and there will not be.** On Windows a terminal never receives its output directly. The child writes into a console host, which relays the bytes over a pipe. On the measuring machine a consumer that reads that pipe and throws the bytes away is done with 32 MiB in 1.45 s, and nothing changes that. Pipe buffers from the default to 16 MB, reads from 64 KB to 1 MB, and Microsoft's own redistributable console host are all within the noise. Any Windows terminal near that figure is showing the transport rather than itself.

The high Windows throughput numbers reported anywhere, this benchmark included, are worse than ambiguous. They measure how fast bytes are accepted into buffers rather than consumed. Bundling the same console host beside SilkTerm takes its writer-visible rate from 12.4 to 85.8 MB/s, while the terminal still finishes at the same moment. Sampling past the last write gives the only true end-to-end figures available. Windows Terminal settles in about 1.3 s against SilkTerm's 2.5 s, roughly 2x. Of SilkTerm's time, 1.45 s is the transport both of them pay, about 0.5 s is parsing, and the rest is pipe handling in the terminal engine we depend on. The renderer isn't part of it at all, since capping the frame rate at 5, 20, 30, or 60 doesn't change the time.

A cross-platform correction factor doesn't work either. SilkTerm measures 6.98x its own Linux row where WezTerm measures 2.93x its own, so no single multiplier fits the table.

The size and memory columns are unaffected by any of this, which is why Windows rows appear there. They come from a different rig: a private X server drawing in software, at a 100x30 grid, with the conhost.exe and Windows Terminal rows taken on a Windows machine. When it draws in software, a GPU terminal puts its window buffers in its own memory, so they count toward Mem.

## Measuring on Windows

There isn't a compositor rig on Windows, so a terminal can't be named on the command line, and nothing resizes the window for you. Both halves are measured from inside the terminal under test, and **each has its own grid**, so it takes 2 passes with a resize between them. Set the window size by hand, then:

~~~sh
rem  speed: size the window to 160 x 42 first
python utility\update-showdown.py --speed-only --reps 6 --label "Windows Terminal"

rem  size and memory: now size it to 100 x 30
python utility\update-showdown.py --size-only --label "Windows Terminal"
~~~

The grid is checked rather than trusted. At the wrong size each half says so and does nothing, because memory scales with the surface and throughput scales with how much of the stream is drawn. `--any-size` overrides that and doesn't mark anything, so only use it to explore. Without a terminal named, it runs whichever half the current window is already sized for.

`--label` is the table's row name, and both halves need it. One writes the speed cells, and the other writes File+deps and Mem. Without it the numbers are printed and nothing is written, since putting a figure in the wrong row is worse than leaving the row empty.

**Never trust what a terminal was asked for.** Windows Terminal's `--size` isn't the grid it gives. Asked for 100x30 it produces 100x33, and asked for 160x42 it produces 160x47. The offset isn't constant either, so there isn't a correction to apply. `--size 100,27` happens to give 100x30 on this machine, but may not on another. The grid check is what makes this safe rather than silent, and nothing else does.

**Before any of this worked on Windows at all, 4 things had to be fixed** (20260818). Each of them produced a plausible wrong answer or a flat refusal rather than an error:

- **The grid query never got to a screen buffer.** It asked stdout, then stderr, then stdin, but the wrapper reads this script through a pipe with stderr folded into it, and stdin is an input handle, which a grid query doesn't work on in Windows. All 3 failed however the window was sized, so the size half refused every run. It asks `CONOUT$` directly now, which is the screen buffer itself and works whatever the streams are pointed at.

- **The row writer couldn't read the README.** `read_text` defaults to the locale codec, which is cp1252 here, and the table's own characters aren't in it. Had the read succeeded the write would have been worse, silently re-encoding the whole file.

- **The wrapper's own interpreter was billed to the terminal.** Only the classifier's own subtree was dropped, and the wrapper is its parent, so about 19 MiB of Python went into every `--here` figure. Interpreters above this process are dropped too now. The walk stops at the shell, which the terminal is entitled to.

- **Injected libraries were billed to the terminal.** This machine has MacType hooked into every process, and 1.6 MiB of it showed up in conhost's 2.6 MiB File+deps. This isn't a local quirk. Security shims do the same thing on most Windows machines, so a row measured anywhere would include somebody's. A library that none of the tree's own binaries import, which is also mapped into this tool's process, is now counted for nobody and reported on its own line, since a silent exclusion reads as a wrong number.

The 3 Windows-only rows have to be measured this way or not at all. Each needs care:

- **conhost** isn't an ancestor of the shell. It's attached to the shell as a child, so the console window is asked which process owns it. On Windows 11 that query returns the console *program* rather than the host, so the host is looked for beside it: a child where the system attached one, a parent where it was launched explicitly. Only adjacent processes are checked, because walking further leaves the session and ends up on whatever started it. Measured that way it comes out as the host plus the one program running in it, which is the same terminal-plus-shell pair every Linux row is.

- **Windows Terminal can only be measured in a process of its own, and that means a process nothing else has ever run in.** Every window shares one. `wt.exe` joins an existing one rather than starting another, and `-w new` only means a new window inside it. So check that none is running before launching, rather than checking that the window looks empty. An attempt that took in the user's own session (20260818) resolved to 9 processes and reported 106 MiB of File+deps and 994 MiB of Mem, nearly all of it PowerShell's .NET runtime and unrelated tools that happened to run in the same terminal. The subtler half is that emptying it isn't enough. A window opened in a process that had hosted an earlier tab read 96.7 MiB against 93.0 for a fresh one, with the earlier tab's processes long gone. The first figure published for the row was that one, and it was corrected the same day.
	- Measured clean (20260818): File+deps 14.2 MiB, Mem 93.0, 5 runs from 92.9 to 93.1. There are 3 processes, which are the window, the console host it starts, and the one shell in it.
	- **Give it `cmd.exe`, the way the Linux rows are given `dash`.** The shell is inside the figure, so a PowerShell session would publish .NET's runtime as the terminal's memory.
	- Its File size cell stays the vendor's install figure. The executable alone is 0.6 MiB and the rest of it is in its own DLLs beside it, which is what File+deps counts. So a measured 0.6 in that column would read as a 0.6 MiB install, and the tail of the table is ordered by what installing costs.
	- One dependency is the machine's rather than the terminal's design: it maps Visual Studio's setup-discovery DLL (0.95 MiB) to enumerate developer-prompt profiles, so a box without Visual Studio would come out that much lighter.

- **MobaXterm** runs its local shell under Cygwin, so the ancestor walk finds it normally, but neither half can be run inside it right now. No Windows program gets a tty through that shell. `isatty` is false on both streams and the grid call fails, even though `stty` reports the size correctly. Its `python3` is the Windows one on `PATH`, so there isn't a Cygwin interpreter to fall back to, and both halves need a real terminal on stdin and stdout. Installing a Cygwin python into the plugin environment would be the way in.

A first pass ran on a laptop and didn't publish anything. The figures and the reasons are in `ancillary-notes.fods`. It found 3 things to know before the next attempt:

- **A terminal measured on non-comparable hardware can't be rescued by calibrating it.** The 3 terminals that run on both platforms all came within 1.5% of each other there, while they spanned 77.4 to 86.9 MB/s on the reference rig. That means they were all pinned at the console pipe rather than by the machine, so the ratio measures the platform rather than the hardware. There isn't any correction to derive from that.

- **No single factor fits the whole table anyway.** On one machine the console host read 13.6 MB/s of ASCII and Windows Terminal 93.1. They are limited by different things, so a multiplier fitted to one is wrong for the other.

- **Windows Terminal's 2-byte scene doesn't settle.** It read 12.67 to 31.08 MB/s inside a single run on an idle machine, and wasn't any better over 4 runs, against the 1 to 4% the published rows stay within.

A second pass then ran on the machine that was supposed to fix that, which is Windows in a VM on the b23 host with a discrete GPU passed through, and it still didn't publish anything. **The hardware was never the blocker.** Its figures and reasoning are in `ancillary-notes.fods` under the 3 `VM` sheets. These 4 things from it show whether a third attempt is worth making:

- **A single correction factor doesn't work, and this time that was measured directly rather than inferred.** On the laptop everything clustered, so the ratio said more about the console pipe than the machine. On the VM the cross-platform terminals don't cluster, and their ratios disagree. SilkTerm plain is 6.98x its own Linux row, and WezTerm is 2.93x its own. When 2 terminals differ by more than a factor of 2, no one multiplier fits the table.

- **Windows Terminal beats every published Linux row** (112.4 MB/s ascii against the fastest Linux row's 100.2), because it hosts the console itself rather than reading a relayed one. Sorting it into the main table would put it first overall on figures from another platform and another transport.

- **The fast terminals aren't being limited by themselves.** SilkTerm plain and +candy come out 0.9% apart on Windows against 12% apart on Linux. The eye candy is rendering cost, so switching all of it off should move the figure, and it doesn't. A number produced under that condition isn't the terminal's speed and must not be published as one.

- **Alacritty can't be benchmarked on Windows at all.** Stock 0.15.1 deadlocks partway through (see "What the Windows accounting does differently" below), so the one terminal that would anchor the whole comparison doesn't produce a row.

A third pass (20260818) replaced the barrier, and what it changed is the meaning of the Windows figures rather than the figures themselves.

- **Windows Terminal's bundled console host is byte-identical to the one already tested.** Its `OpenConsole.exe` and the NuGet package's are the same 1,066,296 bytes and the same sha256 `b7fd936c2668b87b9ecf7b3366dc6568afc1c6f981874cba3e955a1c35cf8160`, so the host binary was never a variable and no version of it explains anything.

- **A barrier-free instrument removes ConPTY from the timing question entirely.** The child writes 32 MiB of grid-width ASCII to `CONOUT$` in 1 MiB blocks and the clock stops when the last `WriteFile` returns, so nothing depends on who replies to a query. At 160x42 on the Windows box: `conhost` 3.37 MB/s, SilkTerm on the system pseudoconsole 12.11 to 12.44 over 3 runs, SilkTerm with `conpty.dll` and `OpenConsole.exe` beside the executable 84.5 to 85.8, Windows Terminal 215 to 231.

- **That spread is bytes accepted, not bytes consumed, and sampling past the last write is what separates them.** Bundling the console host returns SilkTerm's writes at 0.40 s but it keeps working to about 2.7 s, which is where the system path finishes anyway. The buffer moves the back-pressure off the writer without making the terminal faster. Windows Terminal settles by about 1.3 s having spent about 0.95 s of CPU.

- **So the end-to-end gap is around 2x, rather than the 10x the writer-side figures suggest.** Both the 112 MB/s from the VM pass and the 231 here are acceptance rates. This doesn't make a Windows row publishable, since it's one more way the same number can mean 2 things. But a report of an order-of-magnitude deficit is measuring the buffer.

A fourth pass (20260818) took the 2x apart, by giving the same 32 MiB of output to 4 different consumers on the one box (160x42 ASCII, system pseudoconsole).

- **The console host's delivery ceiling is about 22 MB/s and nothing gets past it.** Bytes read from the pipe and discarded take 1.45 s. Pipe buffers from the default to 16 MB, reads from 64 KB to 1 MB, and Microsoft's redistributable host beside the executable are all within the noise. So a consumer measured at 12 MB/s isn't being throttled by its own parser, and Windows Terminal's ~1.3 s is the ceiling rather than a better terminal.

- **Parsing was never the bottleneck.** One thread reading and parsing the same stream into a full grid takes 1.94 s, of which the parser itself is 0.4-0.55 s (66 MB/s on the captured bytes without a pipe involved).

- **The engine's Windows pipe handling costs about half a second more than doing both jobs on one thread.** Its tty plus event loop, without a window or a renderer, takes 2.45 s at 1.9 s of CPU. The consumer isn't what limits it. A variant that waits for the pipe rather than going back to the poller cut read cycles from 47,000 to 519, and didn't change the wall clock at all. The producer side reads 188 bytes per `ReadFile`, 180,000 times, because a dedicated reader thread is always first in line at an empty pipe.

- **The part that was ours was the notification traffic, and it's fixed.** Each read cycle became a window event, about 20,000 of them, and delivering them cost 2.5 s of window-thread CPU in the operating system's message queue, more than parsing and drawing together. Now there's one outstanding notice per pane until the window takes it. That took process CPU from 6.96 s to 4.76 s, and the window thread from 3.3 s to 1.5 s, with throughput unchanged over 4 alternating pairs.

- **So the remaining gap is about a second per 32 MiB and it's in the engine's pipe design rather than in the terminal.** Closing it means forking that, which is a decision rather than a tweak.

- **comparison.md says this now.** Note 9 used to promise Windows speed rows once they were calibrated, but now it states the finding instead. There are no Windows rows, the numbers reported anywhere can't be read as a terminal's speed, and the end-to-end time is 1.3 s against 2.5 s, with its parts named. Keep the 2 in step, so a figure changed here is changed there too.

What was still worth doing on Windows is the **size and memory half**, which doesn't need calibration and is untouched by any of this. That's done, and both measurable rows are published.

**Copy `termbench-plain.shcl` somewhere temporary before pointing SilkTerm at it.** `--config` names a file that the loader maintains as well as reads. It backfills every missing key and rewrites the layout, so one plain-row run turned the 13-line override list into 461 lines, with the header comment moved inside a block. It still measures correctly, but the file no longer means what it says.

There are 2 traps in driving a terminal from outside, and both produce a plausible wrong answer rather than an error. Windows Terminal keeps every window in one long-lived process, possibly one someone is using, and `wt.exe` joins an existing window unless given `-w new`. So a run can end up measuring inside somebody's session, and a name-based kill would take it out. The size collector also asks stdout, then stderr, then stdin for the grid, and only a screen-buffer handle works for that on Windows. Capture both streams, and it doesn't see a terminal at all and refuses.

### What the Windows accounting does differently

The 2 collectors take the same measurement by different routes: the module list stands in for the mapped library set, mapped regions plus their resident pages for `smaps`, and PE imports for `DT_NEEDED`. The self-check (`sizebench-classify.py --selftest`) measures the running process both ways and compares. It took the place of the reproduce gate while there wasn't a published Windows row. Both Windows rows have been published since 20260818, so a new Windows row should reproduce one of them first, the same as on Linux.

One difference is real and deliberate. "Beyond a base OS" means only the C runtime on Linux, because a desktop library there is something you installed. On Windows everything under System32 comes with the machine, so it's excluded by path. A Win32 terminal therefore shows a much smaller File+deps than a GTK one, and that's true rather than an artifact. But it means the 2 platforms' File+deps figures measure slightly different things, on top of everything below.

A managed assembly doesn't contribute any closure edges, because the runtime resolves its references rather than the loader. That can only ever leave a driver-side library billed to the terminal, never the reverse, so it's the safe direction to be wrong in.

Windows **size and memory** figures are comparable, and both rows that can be measured at all are published (20260818): `conhost.exe` at File+deps 1.0 MiB and Mem 21.1, 5 runs from 21.1 to 21.3, and Windows Terminal at 14.2 and 93.0, 5 runs from 92.9 to 93.1. conhost's File+deps is the executable alone because every library it needs is under System32, which is the base-OS exclusion working as it should. Windows Terminal's is almost all its own DLLs, which are beside it rather than in the system.

Windows **speed** figures aren't comparable with the Linux rows, and calibrating them is no longer an option, since it was tried on the VM and failed. Don't re-derive a multiplier. The terminals that run on both platforms disagree about it by more than a factor of 2, and the one that would anchor the comparison can't be run at all. The size and memory half is unaffected and remains publishable on its own terms.

**Alacritty deadlocks on Windows under sustained output, and that's a released defect rather than a rig problem.** The pty backend's reader thread arms its waker only when the pipe came back empty, so once the staging buffer is full the notice that would drain it can never be sent. The reader waits for room, the event loop waits for a notice, and both stay at zero CPU. Measured on the VM, stock 0.15.1 ran about 17 s and then stayed at exactly 0.00 s of CPU delta across 24 s. SilkTerm has a 2-line fix (alacritty/alacritty#9026, still open upstream) and completes every scene on the same box, which is the clearest demonstration of the fix, with one build patched and one not, on the same payloads and the same machine.

## Files

| File                    | What it is
| :---------------------- | :---------------------------------------------------------------------------------
| `../update-showdown.py` | the entry point: measures here or drives the rigs, writes the table
| `termbench.py`          | the throughput tool itself, which runs standalone on any terminal and any OS
| `bench-common.bash`     | output helpers and pid-safe teardown, sourced by both rigs
| `termbench-run.bash`    | speed rig: compositor bring-up, terminal launch, grid fit, teardown
| `termbench-scene.sh`    | runs inside the terminal; reports its grid, then runs the benchmark
| `termbench-plain.shcl`  | SilkTerm with every optional effect off, for the "plain" rows
| `termbench-candy.shcl`  | SilkTerm as released, with the automatic profile pinned off, for the "+candy" rows
| `sizebench-run.bash`    | size rig: display bring-up, launch, grid sizing, process-tree collection
| `sizebench-classify.py` | the closure classifier and the accounting, plus a collector for each platform
| `showdown-readme.py`    | writes the File+deps and Mem cells for one row
| `ancillary-notes.fods`  | measurements taken but not published, and why they couldn't be

The entry point is Python and the rigs are shell, which is the right split. The rigs drive a Linux display and are Linux-only by nature, while the entry point has to run wherever a terminal does. It's one file on purpose, rather than a shell copy plus a PowerShell copy. Copies of one program drift, and a fix then goes into whichever copy was handy rather than the one being run. That has happened here before, to `n8git_backup-and-publish`.
