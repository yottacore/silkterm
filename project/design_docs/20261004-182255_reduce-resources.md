<!-- markdownlint-disable MD007 -- Unordered list indentation -->
<!-- markdownlint-disable MD010 -- No hard tabs -->
<!-- markdownlint-disable MD055 -- Table pipe style [Expected: leading_and_trailing; Actual: leading_only; Missing trailing pipe] -->
<!-- markdownlint-disable MD041 -- First line in a file should be a top-level heading -->

<!-- TOC ignore:true -->
# Reducing resources

<!-- TOC ignore:true -->
## Table of contents

<!-- TOC -->

- [Summary](#summary)
- [Specification](#specification)
- [Goals](#goals)
	- [Non-goals](#non-goals)
- [Design](#design)
	- [Measure first](#measure-first)
	- [The memory hint](#the-memory-hint)
	- [A smaller scrim](#a-smaller-scrim)
	- [The wallpaper at window size](#the-wallpaper-at-window-size)
	- [Block compression for the wallpaper](#block-compression-for-the-wallpaper)
	- [The dialogs' kept GPU context](#the-dialogs-kept-gpu-context)
	- [A shorter wait for a minimized window](#a-shorter-wait-for-a-minimized-window)
	- [Software rendering](#software-rendering)
	- [The Resource use group](#the-resource-use-group)
- [Alternative ideas](#alternative-ideas)
	- [Rejected](#rejected)
- [Research findings](#research-findings)
- [Roadmap](#roadmap)
- [Related backlog issues](#related-backlog-issues)

<!-- /TOC -->

## Summary

One SilkTerm window holds about 330 MB of graphics memory, and many windows add up fast. Most of it is a few full-window textures that are bigger than they need to be. Measured, the two biggest parts are the scrim's textures and the dialogs' kept GPU context. A memory hint has since cut the context from about 200 MiB to 21. This doc covers making them smaller, a way to run without the graphics card at all, and a clearer place in Settings for what a window gives back.

What a window already gives back while unused is in the [Releasing resources](20260930-151334_releasing-resources.md) design doc. This doc is about what a window costs while it is in use.

## Specification

- The scrim, the wallpaper and the dialogs' kept context each cost a fraction of what they do now, with no visible change at the same settings.

- The wallpaper is held at the size it is drawn at, and is prepared again in the background after the window is resized.

- A minimized window lets go of the graphics card after 1 minute by default, on its own setting.

- "Always use software rendering" makes SilkTerm draw on the CPU. Off by default.

- When the graphics card cannot make a device, SilkTerm falls back to software rendering rather than failing.

- The idle release and software rendering rows sit together in a "Resource use" group. A row that cannot work on the running platform is grayed with a tip that says why.

## Goals

- Many windows open at once on a card with little memory.

- No visible change. Legibility comes first, since the scrim is what makes text readable over the wallpaper.

- A window always opens, whatever the card can give it.

### Non-goals

- Lossless compression of anything on the GPU. The drivers already compress render targets on their own, and it saves bandwidth, not memory.

- Moving textures to regular memory. Anything drawn every frame has to be on the card.

## Design

### Measure first

- Measured on 2026-10-04 on b23: RTX 3060 Ti with 8 GB, NVIDIA driver 595.58, one window with one pane, the default font, a 2560x1440 wallpaper from the shipped pack, and an empty scrollback unless a row says otherwise.
	- Graphics memory is what the driver bills the process for. Sizes of single textures are from wgpu's allocator report, which only the Vulkan and DX12 paths have.
	- Regular memory is the unique resident footprint, with the graphics driver's libraries left out.
	- `SILK_MEMDBG=1` prints the allocator report and the heap counts below to stderr whenever they change. `cicd/utility/mem-per-window/run.bash` measures one window again on b23.

- The 330 MB reading was a window plus the dialogs' kept context. The context is about 200 MiB, not the 52 MiB the estimate had.

- Graphics memory on X11, which always draws through GL, in MiB:

	| Part                                          | 1280x720 | 1920x1080 | 2560x1440
	| :-------------------------------------------- | -------: | --------: | --------:
	| Window buffers, glyph atlases and the context |       28 |        56 |        86
	| Scrim, five full-window textures              |       40 |        90 |       150
	| Wallpaper, 2560x1440                          |       32 |        32 |        32
	| The window                                    |      100 |       178 |       268
	| Dialogs' kept context, once per process       |      201 |       201 |       201
	| The process                                   |      301 |       379 |       469

	- These are with wgpu's default memory hint. With the hint every device uses now, the context is 21 MiB. See [The memory hint](#the-memory-hint).
	- The window buffers grow by about 22 bytes a pixel. 8 of those are the full-window Rgba16Float texture the GL path draws into before the flip.
	- A 6000x4000 image, cut to 4096x2731, costs 88 MiB rather than 32.
	- Opening Settings adds 12 MiB, given back when it closes.

- The same window on the Vulkan path, as on Wayland, at 2560x1440, from the allocator:

	| Allocation                                    | MiB
	| :-------------------------------------------- | ---:
	| Each scrim texture, 8 bytes a pixel           | 30.0
	| The same at 1920x1080                         | 16.9
	| The same at 1280x720                          |  7.5
	| Wallpaper, 2560x1440                          | 15.0
	| Wallpaper, 4096x2731                          | 44.0
	| Glyph atlases, four                           |  2.5
	| Minimap texture                               |  0.6
	| In use                                        |  167
	| Reserved in blocks of 128+256+64+64           |  512
	| Driver's figure, without the dialogs' context |  506
	| Driver's figure, with it                      |  702

- What the numbers show:
	- The scrim is the largest part of a window: 150 of 268 MiB at 2560x1440. Its textures exist whenever the halo or the outline is on. Turning only the halo off saves 30 MiB on X11 and nothing on Vulkan, where all five are made either way.
	- The dialogs' kept context reserves a 128 MiB and a 64 MiB block from wgpu's allocator for less than 1 MiB of use. It also costs about 16 MiB of regular memory.
	- On Vulkan, most of the bill is the allocator's unused space: 167 MiB in use against 512 reserved. The allocator takes big blocks because wgpu's default memory hint is `Performance`.
	- With `MemoryHints::MemoryUsage` on both devices, the dialogs' context costs 18 MiB with no dialog open and 31 MiB with Settings open. The Vulkan window above came to 252 MiB instead of 702, context included. Settings still opened from it. Every device uses that hint now; see [The memory hint](#the-memory-hint).
	- The wallpaper costs about twice its texture on X11: 32 MiB for a 15 MiB texture, 88 for 44. In regular memory a 2560x1440 one costs 44 MiB: a 14 MiB copy the size of the decoded image stays mapped in the process, plus 27 MiB that the driver maps. The 4096x2731 one costs 106 MiB. Why the copy stays is not known yet.
	- On Vulkan, the wallpaper's 20 KB readback buffer for the lost-texture check makes the allocator keep a 64 MiB block of regular memory.

- Regular memory on X11, in MiB:

	| Part                                             | 1280x720      | 2560x1440
	| :----------------------------------------------- | ------------: | ------------:
	| Window with no wallpaper and an empty scrollback |            44 |            53
	| Wallpaper, 2560x1440                             |            45 |            44
	| Scrollback full, 10,000 lines                    | 25 (110 cols) | 54 (231 cols)
	| Minimap store at 10,000 lines                    |             6 |             6

	- A scrollback cell is 24 bytes, for every column of every line, blank or not, per pane. The alt screen adds one more screen of cells.
	- The minimap store is a row of preview pixels per line, so it grows with the lines and not with the window.
	- Glyphs go straight into the GPU atlases, so there is no glyph cache in regular memory. The heap is 14 to 26 MiB, mostly the font list (1067 faces here) and shaped text.

### The memory hint

- wgpu's default hint, `Performance`, has the Vulkan and DX12 allocator take blocks of 128 to 256 MiB of graphics memory and 64 to 128 MiB of host memory. `MemoryUsage` starts at 8 and 4 MiB and grows to 64 and 32. GL and Metal ignore the hint.

- Every device asks for `MemoryUsage` since 2026-10-04: the window's on every backend, and the dialogs' kept context.

- Measured on b23 at 2560x1440 against a control with the default hint, in the same session, in MiB:

	| Part                                | Default | MemoryUsage
	| :---------------------------------- | ------: | ----------:
	| X11 process, Settings closed        |     453 |         273
	| X11 process, Settings open          |     465 |         285
	| Dialogs' kept context               |     201 |          21
	| Its allocator, reserved             |     192 |          12
	| Vulkan window plus the context      |     702 |         256
	| Vulkan window's allocator, reserved |     512 |         186

	- The X11 process with no kept context at all is 252 MiB with either hint, since the GL window ignores it. The context's figure is the difference. After a first Settings open the difference is 16 MiB.
	- The Vulkan window had 159 MiB in use in both runs.
	- The control read 453 where the table in [Measure first](#measure-first) has 469 for the same window. Each comparison here is within one session.

- Regular memory does not change with the hint. The kept context costs about 8 MiB of anonymous memory either way, 58.7 against 51.0 with no context. The 16 MiB in Measure first was the unique footprint, which moves with the page cache.

- Settings open time, from the key to the dialog's first frame (`SILK_DLGDBG=1`), eight opens a run, three runs each:
	- Kept context, default hint: 108 ms for each of the first three opens, then a median of 62.
	- Kept context, `MemoryUsage`: 105 ms, then 66.
	- No kept context: about 230 ms every time.

- Frame times on the Vulkan path, under a 20 second scroll flood, two runs each: about 22 ms of render time and 1.9 ms of text preparation a frame with either hint, at the same frame rate. A control run with the card clocked up by another program read 18 ms, which is the spread between runs.

- Windows uses the same allocator through DX12 and was not measured.

### A smaller scrim

- The scrim holds five textures the size of the window, each four 16-bit floats, so 8 bytes a pixel and 40 in all.

- Most of them use one channel:
	- The text and cursor coverage layers use only alpha. One byte a pixel should do.
	- The two blur layers hold either blurred coverage or a distance. One 16-bit float each should do. Eight bits may band in a dark, soft halo, so test both.
	- The background color map uses all four channels. Its precision is worth testing too.

- From 40 bytes a pixel to about 10 to 16, before any change in size.

- The blur passes could run at half size, which is a quarter of the pixels. The blur already steps more than one pixel per tap at larger radii, so the loss may not show.
	- If it shows, Max silk keeps full size and the lower profiles use half size.
	- If it never shows, every profile uses half size.

- The text coverage is drawn by the text renderer, which writes color. A one-channel target needs the coverage pass to write white, and the shaders to read the red channel.

- Proof is a capture diff at the same settings, in all four themes, dark and light, at a few radii. Legibility is the bar, not a pixel count.

### The wallpaper at window size

- Today the image is cut to 4096 on its long side, blurred on the CPU at that size, and uploaded whole. The GPU scales it to the window every frame.

- Prepare it at the size it is drawn at instead, by its fit. Scale, never crop, so the anchor and fit still work during a resize.

- After a resize, keep drawing the current texture, scaled. Once resizing has been still for a short wait, the worker prepares the image at the new size and swaps it in with no fade.

- The blur is in image pixels today. The new size changes that, so the blur is scaled with the image to keep today's look at the same window size.

- A blurred image needs less than the window's size. Detail finer than the blur is gone anyway, so the image can be held smaller by a factor tied to the blur, and the GPU scales it up.
	- At the shipped blur, that may cut it by four or more with no encoder at all.
	- A cap keeps a small blur from shrinking it too far.

### Block compression for the wallpaper

- GPUs read block-compressed formats directly. BC1 is half a byte a pixel and BC7 one byte, against four for plain RGBA.

- They work only for textures that do not change. A GPU cannot draw into one, so this is for the wallpaper only.

- The image to compress is the one already prepared at window size and blur.

- Pick by blur and size:
	- A heavy blur: hold it smaller and skip compression. A quarter-size image in plain RGBA is already smaller than BC1 at full size.
	- Little or no blur: BC1. The wallpaper sits dimmed behind text, so its artifacts should not show.
	- BC7 where BC1 shows banding, if a test finds any.

- Encoding runs on the wallpaper worker once per image and per resize. A pure Rust encoder is preferred.

- A plain RGBA fallback is kept for an adapter without BC support, though every one checked has it.

- Executable size is a top priority, so the encoder's cost in bytes decides as much as its quality.

### The dialogs' kept GPU context

- Settings and About draw through a GPU context built once and kept for the life of the process. It holds about 52 MiB to open Settings in 86 ms rather than 310 ms. See the [Settings dialog](20260930-145721_settings-dialog.md) design doc.
	- Measured on b23, it holds about 200 MiB of graphics memory and 16 MiB of regular memory, almost all of it two blocks wgpu's allocator reserves up front. With the `MemoryUsage` hint it is 18 MiB. See [Measure first](#measure-first).
	- With the hint every device uses since 2026-10-04, it is about 21 MiB of graphics memory and 8 MiB of anonymous memory. See [The memory hint](#the-memory-hint).

- With many windows, that is 52 MiB each, all the time, for a dialog that is rarely open.

- Options:
	- Drop it when the dialog closes, and keep it only while a dialog is open.
	- Share the main window's device instead of a second one, where the backend allows. That also saves a second driver context. On X11 the dialog's GL context cannot outlive the window's, so this needs care there.
	- Keep it, and say so.

- Decided 2026-10-04, open to reversal: keep it.
	- With the hint it costs about 21 MiB of graphics memory and 8 MiB of regular memory per process.
	- The idle release already lets its device go along with the window's.
	- Dropped on close, every open would take about 230 ms, against 66 to 105 ms kept.
	- Sharing the main window's device cannot work on X11, where the window draws through GL. Elsewhere it would save about 20 MiB.
	- `WARM_DIALOG_GPU` in app.rs is still the one-line way back to building the context on each open.

- Moved here from the releasing resources doc's roadmap.

### A shorter wait for a minimized window

- Today a minimized window and a covered one share "Minutes when hidden", 30 by default.

- A minimized window gets its own wait, "Minutes when minimized", 1 by default. A covered window keeps the 30.

- The cost is the way back. Taking the card back is about 25 ms on Linux, but 0.8 to 1.2 s on Windows with an RTX 2060. What a restored window shows on Windows in that second needs a look.

- The release code knows only "hidden" today, which joins minimized, covered and a window with no size. Minimized has to be told apart.

- Built 2026-10-04. A window with no size counts as minimized, and so does one reported as both minimized and covered. Wayland reports neither, so a window there still waits "Minutes otherwise".

### Software rendering

- SilkTerm uses a software adapter only when no graphics card is found at launch. A card that is found but cannot make a device, as when its memory is full, ends the launch, and an idle rebuild retries the same card.

- "Always use software rendering", off by default. It asks wgpu for the fallback adapter: lavapipe or llvmpipe on Linux, WARP on Windows.
	- The performance profile follows it. Automatic already starts a software adapter at Low.
	- macOS has no software adapter in wgpu. The row is grayed there, with a tip.
	- The X11 transparency path uses its own GL context, and libGL picks its driver once per process. Whether a launch can steer it to Mesa's software driver needs checking. If not, the row is grayed under Transparency on X11.
	- A change takes effect at the next device build, which a rebuild can trigger, rather than only at relaunch.

- The fallback: when a device cannot be made, at launch or at a rebuild, try the software adapter once before giving up.
	- A window that fell back stays on it until the next rebuild, which tries the card first again.
	- The title or Help > About says software rendering is in use, the same way it does now.
	- This is also a way out for the blank window under GPU load, where `vkCreateDevice` was seen failing.

### The Resource use group

- "Free resources when idle" and its waits move into a group named "Resource use", with "Always use software rendering" beside them.

- "Free resources when idle" gets a warning mark. Its tip says it matters most on a card with little memory, or next to GPU-heavy programs, with many windows open that are not all in view. Turn it off only if the graphics driver has trouble with it.

- A setting that stops a window from giving memory back gets a warning mark where it does so. Today that is Transparency on Windows, since a window in view with Transparency on is never let go. The mark shows only in the Windows build.

- A row that cannot work on the running platform is grayed with a tip saying why, using the existing grayed-row tips. A row is hidden only in a build that can never use it, as with the Windows-only file association rows.

- Tips stay short, per the UI style guide.

## Alternative ideas

### Rejected

- Lossless compression on the GPU. No graphics API lets a program hold a texture in less space losslessly.

- An encoder with a C toolchain, such as `intel_tex_2`. A pure Rust one builds for every target with no extra setup.

- Block compression for the scrim or swapchain. A GPU cannot render into a compressed format.

## Research findings

- Block compression support, checked on b23 with Mesa 26.1.2:
	- lavapipe reports `textureCompressionBC`.
	- llvmpipe's GLES has `GL_EXT_texture_compression_s3tc_srgb`, `_rgtc` and `_bptc`, the three wgpu wants before it turns BC on for GLES.
	- WARP: D3D12 requires the BC formats on every device, WARP included.
	- Metal: wgpu turns BC on where Metal reports it. Not checked on b26.

- Pure Rust encoders: `texpresso` does BC1 to BC5, and `rusty_dds` does BC1 to BC7. Neither is measured here for speed, quality or binary size.

- The scrim's blur takes 25 taps a pass and spaces them more than a pixel apart once the radius passes about 4 pixels.

- The wallpaper's default blur is a sigma of 10, in pixels of the image after the 4096 cut.

## Roadmap

1. Measure.

2. Shrink the scrim and hold the wallpaper at window size. Settle the dialogs' context. Give a minimized window its own short wait.

3. "Always use software rendering" and the fallback on device failure.

4. The Resource use group and its warning marks.

5. Block compression for the wallpaper, once the size cut has been measured.

## Related backlog issues

- "Measure graphics and regular memory per window" (ID 2026100418225501)

- "The scrim's textures are bigger than they need to be" (ID 2026100418225502)

- "Hold the wallpaper at the size it is drawn at" (ID 2026100418225503)

- "Always use software rendering, and fall back to it when the card cannot make a device" (ID 2026100418225504)

- "The dialogs' kept GPU context costs every process about 52 MiB" (ID 2026100418225505)

- "A minimized window lets go of the graphics card after its own short wait" (ID 2026100418354006)

- "Settings: a Resource use group, with warning marks" (ID 2026100418225506)

- "Block compression for the wallpaper" (ID 2026100418225507)

- "The window doesn't paint while the GPU is busy or short on memory, and stays blank after the load ends" (ID 2026100312470535)

- "Release the GPU device after a long idle." (Opened 20260905-181131). Its open point, "figure out a way to reduce CPU and memory usage", moved here.
