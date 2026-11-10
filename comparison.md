<!-- markdownlint-disable MD033 -- No inline html -->
<!-- markdownlint-disable MD055 -- Table pipe style [Expected: leading_and_trailing; Actual: leading_only; Missing trailing pipe] -->
<!-- markdownlint-disable MD041 -- First line in a file should be a top-level heading -->

# Compared to other terminals

Smooth scrolling isn't useful if the terminal falls behind the moment something dumps a lot of text, so throughput is measured and reported here.

The table is sorted by speed. Terminals that haven't been measured for speed yet come after, ordered by what it takes to install them.

<!-- termbench:begin -->

| OS<sup>9</sup> | Terminal                                             | Ver         | 1-byte<sup>1</sup> | 4-byte<sup>1</sup> | Speed score<sup>2</sup> | File size<sup>3</sup> (MiB) | File+ deps<sup>4</sup> (MiB) | Mem<sup>4</sup> (MiB)
| :------------- | :--------------------------------------------------- | :---------- | -----------------: | -----------------: | ----------------------: | --------------------------: | ---------------------------: | --------------------:
| \[multi\]      | $\textcolor{limegreen}{SilkTerm}$ plain<sup>6</sup>  | 1.0.0-beta2 |               86.9 |              129.3 |                **71.1** |                        10.5 |                         14.1 |                 100.1
| \[multi\]      | Alacritty<sup>8</sup>                                | 0.15.1      |               79.8 |              129.1 |                **68.4** |                         8.5 |                         12.7 |                  50.4
| \[multi\]      | $\textcolor{limegreen}{SilkTerm}$ +candy<sup>5</sup> | 1.0.0-beta2 |               77.4 |              135.1 |                **67.6** |                        10.5 |                         14.1 |                 167.7
| Linux          | GNOME Terminal                                       | 3.58.1      |              100.2 |               62.6 |                **55.3** |                         0.4 |                         84.0 |                  53.6
| Linux          | XFCE4 Terminal                                       | 1.2.0       |               94.2 |               65.0 |                **54.0** |                         0.3 |                         84.1 |                  48.6
| Linux          | Terminator                                           | 3.13.5      |               87.8 |               67.3 |                **51.8** |                      script |                         92.6 |                  82.2
| Linux          | XTerm                                                | 407         |               28.3 |               48.5 |                **23.9** |                         0.9 |                          6.0 |                   9.4
| \[multi\]      | kitty                                                | 0.48.1      |               24.2 |               59.6 |                **22.6** |                         0.2 |                        115.0 |                 140.8
| \[multi\]      | WezTerm                                              | 20240203    |               15.6 |               22.2 |                **10.4** |                        70.5 |                        129.9 |                  84.8
| \[multi\]      | Tabby                                                | 1.0.235     |                8.5 |                9.0 |                 **5.7** |                       192.1 |                        454.2 |                 473.4
| Win            | conhost.exe                                          | -           |                  - |                  - |                       - |                         1.0 |                          1.0 |                  21.1
| Win            | PuTTY                                                | -           |                  - |                  - |                       - |             1.6<sup>7</sup> |                            - |                     -
| Linux          | Guake                                                | -           |                  - |                  - |                       - |             1.7<sup>7</sup> |                            - |                     -
| Linux          | Konsole                                              | -           |                  - |                  - |                       - |             7.3<sup>7</sup> |                            - |                     -
| \[multi\]      | Windows Terminal                                     | -           |                  - |                  - |                       - |            11.1<sup>7</sup> |                         14.2 |                  93.0
| \[multi\]      | Ghostty                                              | -           |                  - |                  - |                       - |            32.0<sup>7</sup> |                            - |                     -
| macOS          | iTerm2                                               | -           |                  - |                  - |                       - |            43.0<sup>7</sup> |                            - |                     -
| Win            | MobaXterm                                            | -           |                  - |                  - |                       - |            43.4<sup>7</sup> |                            - |                     -
| \[multi\]      | Hyper                                                | -           |                  - |                  - |                       - |                       147.8 |                        300.9 |                 309.4
| macOS          | Terminal.app                                         | -           |                  - |                  - |                       - |                           - |                            - |                     -
| macOS          | Warp                                                 | -           |                  - |                  - |                       - |                           - |                            - |                     -

<!-- termbench:end -->

In testing, each terminal is fed byte-identical, deterministic streams of one UTF-8 width class at a time. That's plain ASCII, then 2-byte, 3-byte, and 4-byte characters, and then a mix. Each one is timed to a device-attributes reply, so the clock stops when the terminal has consumed the stream rather than when the pipe accepted it. Speed is measured at a 160x42 grid.

A terminal is also the program that is always open, usually several times over, so what it costs while doing nothing matters. Size and memory are measured separately, with each terminal at a 100x30 grid and its own defaults.

<sub><sup>1</sup> Throughput in MB/s, higher is better, on a stream made entirely of characters of that UTF-8 width, where 1-byte is plain ASCII and 4-byte is emoji. The score also counts 2 more width classes and a mixed stream, and the tool prints all 5. Only rows measured at the same grid size are comparable.</sub>

<sub><sup>2</sup> Millions of cells per second. It's the weighted geometric mean of all 5 classes. Plain ASCII gets the most weight, since that's most of what a terminal ever sees, and the mean is geometric so no single class can dominate. It's counted in cells rather than bytes, because a wide-character stream moves far more bytes for the same amount of screen. It measures how fast a terminal takes in output and keeps up, rather than how fast it rasterizes glyphs. Only a screenful is ever visible, so most of a large stream is parsed, stored, and scrolled past. The clock stops when the terminal replies to a query that it can only reply to once it has worked through everything queued, so a terminal that never replies can't be timed this way and its speed cells stay blank. A slow terminal gets fewer repetitions of the same payloads, which makes its figures noisier but no less comparable.</sub>

<sub><sup>3</sup> This number is near-meaningless alone. A small executable usually means the code is in shared libraries instead. But they are loaded only once however many programs map them - so anything built on a stack the desktop already loads costs less than its File+deps implies. SilkTerm links nothing beyond the C runtime and what the graphics stack loads at runtime (for maximum portability and long-term stability without "bitrot"), so almost all of it is in the one file.</sub>

<sub><sup>4</sup> File+deps is the executable plus everything else it needs beyond a base OS. Memory is the unique resident footprint of the whole process tree, which is its private pages plus each shared mapping counted once. Self-contained bundles count their extracted payload plus the system libraries they still borrow. Both columns leave out the graphics stack and what it pulls in, because accelerated terminals share it with the compositor and every other accelerated program: 141 SilkTerm, 105 WezTerm, 73 kitty and Alacritty, 48 Tabby, and 1 Hyper. "A base OS" isn't the same size on every platform. On Linux it means the C runtime and nothing else, since a desktop library is something you installed, while on Windows the whole of System32 comes with the machine. So a Windows row counts less toward File+deps than a Linux one, on top of everything in note 9. Expect a few MiB of drift between runs, since libraries load on demand.</sub>

<sub><sup>5</sup> SilkTerm with its default settings, which have the eye candy on: wallpaper, text scrim and outline, animated cursor, smooth application scrolling, and color emoji. Every one of them is a setting, and the row below is the same binary with the lot switched off.</sub>

<sub><sup>6</sup> Wallpaper, scrim, outline, cursor animation, smooth app scrolling, transparency, and color emoji all off.</sub>

<sub><sup>7</sup> The vendor's released artifact. It wasn't measured here, so it can't be compared with the measured columns. A cell is blank where conhost.exe and Terminal.app come with the OS, Warp doesn't publish a size, and the macOS rows don't have anything here to run on.</sub>

<sub><sup>8</sup> SilkTerm uses Alacritty's terminal-emulation core, so the 2 share the parsing and grid work that this benchmark mostly measures. That's why they're within a few percent of each other, and why both are so far ahead of terminals that parse their own way. Alacritty is the lighter of the 2 to run, and the difference is what the eye candy costs. SilkTerm with everything switched off uses 50 MiB more than Alacritty, and 117 MiB more with its default settings.</sub>

<sub><sup>9</sup> Every speed figure comes from one machine, because the measuring rig isn't neutral. It's a headless Wayland compositor driving a discrete GPU, on a Linux Ryzen host with a GeForce RTX 3060 Ti. XTerm draws only on X11, so its row comes from a private X server. Through the compositor's Xwayland it reads about a third slower, 18 MB/s of plain ASCII against 28. <b>There are no Windows rows, and there will not be.</b> On Windows a terminal never receives its output directly. The console host relays it over a pipe, and that pipe sets the pace, so a Windows figure measures the transport rather than the terminal. The size and memory columns are unaffected, which is why Windows rows appear there. They come from a private X server drawing in software, at a 100x30 grid, with the conhost.exe and Windows Terminal rows taken on a Windows machine. When it draws in software, a GPU terminal puts its window buffers in its own memory, so they count toward Mem. The measurements behind all of this are in [showdown-readme.md](utility/include/showdown-readme.md).</sub>

Run it yourself with [`utility/update-showdown.py`](utility/update-showdown.py) (`--quick` for a 30-second version). It needs only Python 3 and a terminal, works on any emulator on any OS, and refreshes the speed columns in the table above as more terminals are measured.
