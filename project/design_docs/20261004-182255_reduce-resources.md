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
	- [Kept copies on disk](#kept-copies-on-disk)
	- [Block compression for the wallpaper](#block-compression-for-the-wallpaper)
		- [BC7 by profile](#bc7-by-profile)
	- [The dialogs' kept GPU context](#the-dialogs-kept-gpu-context)
	- [A shorter wait for a minimized window](#a-shorter-wait-for-a-minimized-window)
	- [Software rendering](#software-rendering)
	- [The Resource use group](#the-resource-use-group)
- [Alternative ideas](#alternative-ideas)
	- [Rejected](#rejected)
	- [Superseded](#superseded)
- [Research findings](#research-findings)
- [Roadmap](#roadmap)
- [Related backlog issues](#related-backlog-issues)
- [Copyright and license](#copyright-and-license)

<!-- /TOC -->

## Summary

One SilkTerm window uses about 330 MB of graphics memory, and many windows add up fast. Most of it is a few full-window textures that are bigger than they need to be. Measurements showed that the 2 biggest parts are the scrim's textures and the dialogs' kept GPU context. A memory hint has since cut the context from about 200 MiB to 21. This doc is about making them smaller, a way to run without the graphics card, and a clearer place in Settings for what a window gives back.

What a window already gives back while unused is described in the [Releasing resources](20260930-151334_releasing-resources.md) design doc. This doc is about what a window costs while it's in use.

## Specification

- The scrim, the wallpaper, and the dialogs' kept context each cost a fraction of what they do now, without any visible change at the same settings.

- The wallpaper is held at the size it is drawn at, and is prepared again in the background after the window is resized.

- A minimized or covered window lets go of the graphics card after 1 minute by default.

- "Always use software rendering" makes SilkTerm draw on the CPU. It's off by default.

- When the graphics card can't make a device, SilkTerm falls back to software rendering rather than failing.

- The idle release and software rendering rows are together in a "Resource use" group. A row that can't work on the running platform is grayed with a tip that says why.

## Goals

- Many windows open at once on a card with little memory.

- No visible change. Legibility comes first, because the scrim is what makes text readable over the wallpaper.

- A window always opens, whatever the card can give it.

### Non-goals

- Lossless compression of anything on the GPU. The drivers already compress render targets on their own, and that saves bandwidth rather than memory.

- Moving textures to regular memory. Anything drawn every frame has to be on the card.

## Design

### Measure first

- The measurements are from 2026-10-04, on an RTX 3060 Ti with 8 GB and NVIDIA driver 595.58. They used one window with one pane, the default font, a 2560x1440 wallpaper from the bundled pack, and an empty scrollback unless a row says otherwise.
	- Graphics memory is what the driver attributes to the process. Sizes of single textures are from wgpu's allocator report, which only the Vulkan and DX12 paths have.
	- Regular memory is the unique resident footprint, with the graphics driver's libraries left out.
	- `SILK_MEMDBG=1` prints the allocator report and the heap counts below to stderr whenever they change. `cicd/utility/mem-per-window/run.bash` measures one window again.
		- Since 2026-10-05 it also prints the size the wallpaper is held at, which the GL path doesn't have an allocator report for.

- The 330 MB reading was a window plus the dialogs' kept context. The context is about 200 MiB, rather than the 52 MiB in the estimate.

- Graphics memory on X11, which always draws through GL, in MiB:

	| Part                                           | 1280x720 | 1920x1080 | 2560x1440
	| :--------------------------------------------- | -------: | --------: | --------:
	| Window buffers, glyph atlases, and the context |       28 |        56 |        86
	| Scrim, 5 full-window textures                  |       40 |        90 |       150
	| Wallpaper, 2560x1440                           |       32 |        32 |        32
	| The window                                     |      100 |       178 |       268
	| Dialogs' kept context, once per process        |      201 |       201 |       201
	| The process                                    |      301 |       379 |       469

	- These are with wgpu's default memory hint. With the hint every device uses now, the context is 21 MiB. See [The memory hint](#the-memory-hint).
	- The window buffers grow by about 22 bytes a pixel. Of those, 8 are the full-window Rgba16Float texture the GL path draws into before the flip.
	- A 6000x4000 image is cut to 4096x2731 and costs 88 MiB rather than 32.
	- Opening Settings adds 12 MiB, which is given back when it closes.

- The same window on the Vulkan path, as on Wayland, at 2560x1440, from the allocator:

	| Allocation                                    | MiB
	| :-------------------------------------------- | ---:
	| Each scrim texture, 8 bytes a pixel           | 30.0
	| The same at 1920x1080                         | 16.9
	| The same at 1280x720                          |  7.5
	| Wallpaper, 2560x1440                          | 15.0
	| Wallpaper, 4096x2731                          | 44.0
	| Glyph atlases, 4                              |  2.5
	| In use                                        |  167
	| Reserved in blocks of 128+256+64+64           |  512
	| Driver's figure, without the dialogs' context |  506
	| Driver's figure, with it                      |  702

- What the numbers show:
	- The scrim is the largest part of a window, at 150 of 268 MiB at 2560x1440. Its textures exist whenever the halo or the outline is on. Turning only the halo off saves 30 MiB on X11 and doesn't save anything on Vulkan, where all 5 are made either way.
		- Since 2026-10-04 it's 52 MiB, and 36 with the halo off. See [A smaller scrim](#a-smaller-scrim).
	- The dialogs' kept context reserves a 128 MiB and a 64 MiB block from wgpu's allocator for less than 1 MiB of use. It also costs about 16 MiB of regular memory.
	- On Vulkan, most of the total is the allocator's unused space, with 167 MiB in use against 512 reserved. The allocator takes big blocks because wgpu's default memory hint is `Performance`.
	- With `MemoryHints::MemoryUsage` on both devices, the dialogs' context costs 18 MiB without a dialog open and 31 MiB with Settings open. The Vulkan window above came to 252 MiB instead of 702, context included. Settings still opened from it. Every device uses that hint now. See [The memory hint](#the-memory-hint).
	- The wallpaper costs about twice its texture on X11, which is 32 MiB for a 15 MiB texture and 88 for 44. In regular memory a 2560x1440 one costs 44 MiB. A 14 MiB copy the size of the decoded image stays mapped in the process, and the driver maps another 27 MiB. The 4096x2731 one costs 106 MiB. Why the copy stays isn't known yet.
	- On Vulkan, the wallpaper's 20 KB readback buffer for the lost-texture check makes the allocator keep a 64 MiB block of regular memory.

- Regular memory on X11, in MiB:

	| Part                                             | 1280x720      | 2560x1440
	| :----------------------------------------------- | ------------: | ------------:
	| Window with no wallpaper and an empty scrollback |            44 |            53
	| Wallpaper, 2560x1440                             |            45 |            44
	| Scrollback full, 10,000 lines                    | 25 (110 cols) | 54 (231 cols)

	- A scrollback cell is 24 bytes, for every column of every line, blank or not, per pane. The alt screen adds one more screen of cells.
	- Glyphs go straight into the GPU atlases, so there isn't a glyph cache in regular memory. The heap is 14 to 26 MiB, and most of that is the font list and shaped text. The font list had 1067 faces.

### The memory hint

- wgpu's default hint is `Performance`, which has the Vulkan and DX12 allocator take blocks of 128 to 256 MiB of graphics memory and 64 to 128 MiB of host memory. `MemoryUsage` starts at 8 and 4 MiB and grows to 64 and 32. GL and Metal ignore the hint.

- Since 2026-10-04 every device asks for `MemoryUsage`. That's the window's on every backend and the dialogs' kept context.

- Measured at 2560x1440 against a control with the default hint, in the same session, in MiB:

	| Part                                | Default | MemoryUsage
	| :---------------------------------- | ------: | ----------:
	| X11 process, Settings closed        |     453 |         273
	| X11 process, Settings open          |     465 |         285
	| Dialogs' kept context               |     201 |          21
	| Its allocator, reserved             |     192 |          12
	| Vulkan window plus the context      |     702 |         256
	| Vulkan window's allocator, reserved |     512 |         186

	- The X11 process without any kept context is 252 MiB with either hint, because the GL window ignores the hint. The context's figure is the difference. After a first Settings open the difference is 16 MiB.
	- The Vulkan window had 159 MiB in use in both runs.
	- The control read 453 where the table in [Measure first](#measure-first) has 469 for the same window. Each comparison here is within one session.

- Regular memory doesn't change with the hint. The kept context costs about 8 MiB of anonymous memory either way, 58.7 against 51.0 without a context. The 16 MiB in Measure first was the unique footprint, which moves with the page cache.

- Settings open time, from the key to the dialog's first frame (`SILK_DLGDBG=1`), 8 opens a run, 3 runs each:
	- Kept context, default hint: 108 ms for each of the first 3 opens, then a median of 62.
	- Kept context, `MemoryUsage`: 105 ms, then 66.
	- No kept context: about 230 ms every time.

- Frame times on the Vulkan path were measured under a 20 second scroll flood, 2 runs each. They were about 22 ms of render time and 1.9 ms of text preparation a frame with either hint, at the same frame rate. A control run with the card clocked up by another program read 18 ms, which is the spread between runs.

- Windows uses the same allocator through DX12 and wasn't measured.

### A smaller scrim

- The scrim has 5 textures the size of the window, each with four 16-bit floats, so 8 bytes a pixel and 40 in all.

- Most of them use one channel:
	- The text and cursor coverage layers use only alpha. One byte a pixel should do.
	- The 2 blur layers contain either blurred coverage or a distance. One 16-bit float each should do. A dark, soft halo might band at 8 bits, so test both.
	- The background color map uses all four channels. Its precision is worth testing too.

- That takes it from 40 bytes a pixel to about 10 to 16, before any change in size.

- The blur passes could run at half size, which is a quarter of the pixels. The blur already steps more than one pixel per tap at larger radii, so the loss may not show.
	- If it shows, Max silk keeps full size and the lower profiles use half size.
	- If it never shows, every profile uses half size.

- The text coverage is drawn by the text renderer, which writes color. A one-channel target needs the coverage pass to write white, and the shaders to read the red channel.

- It's checked against the old build at the same settings, in all 4 themes, dark and light, at a few radii. Legibility is the bar rather than a pixel count.

- Built 2026-10-04. Each layer keeps only what's read back from it, which comes to 13 bytes a pixel instead of 40:
	- The text coverage has four 8-bit channels. Only alpha is read, but the text renderer writes each glyph's color too, so one channel would need it to write white instead.
	- The cursor coverage has one 8-bit channel. The scrim draws its cursor quads white.
	- The 2 blur layers have one 16-bit float each. Both kinds of halo write red now.
	- The color map has four 8-bit channels, sRGB encoded by the scrim itself. Cell colors are opaque and come from sRGB bytes, so each one comes back exactly.
	- An sRGB texture format would do that encode on Vulkan. The GL path never turns sRGB writes on, so there it stored linear values and still decoded them on read, and the halo inside a reverse video bar came out dark.
	- With the halo off, the 2 blur layers are one pixel. All 5 already were with the outline off too.

- Measured at 2560x1440 against a control build, in the same session, in MiB:

	| Part                                  | Before | After
	| :------------------------------------ | -----: | ----:
	| X11 process                           |    273 |   175
	| X11 process, halo off                 |    243 |   159
	| X11 process, halo and outline off     |    123 |   123
	| Vulkan window, driver's figure        |    256 |   202
	| Vulkan window, halo off               |    256 |   138
	| Vulkan window, scrim in the allocator |    150 |    49
	| Vulkan window, all in use, allocator  |    159 |    58

	- So the scrim's share of the X11 window went from 150 MiB to 52, and to 36 with the halo off.
	- The Vulkan driver's figure moves less than the allocator's use, because the allocator reserves in blocks.
	- Regular memory didn't change.

- Checked against the control build at the same settings, on NVIDIA through GL and Vulkan, and on lavapipe. The cases were all 4 themes in dark and light, each scrim function, radii of 5, 8, and 20, a soft halo on a smooth light gradient in both modes, the cursor in the halo, and the outline alone. Differences are in sRGB levels out of 255:
	- GL: at most 1 on any pixel. Up to 1.6% of pixels changed in light mode and under 0.25% in dark.
	- Vulkan: at most 3, on the antialiased edge of the outline in light mode. Up to 2.4% of pixels changed in light mode, nearly all by 1, and almost none in dark.
	- lavapipe: at most 2.
	- The soft dark halo changed by at most 1 on any path, so it doesn't band any more than before.

### The wallpaper at window size

- Today the image is cut to 4096 on its long side, blurred on the CPU at that size, and uploaded whole. The GPU scales it to the window every frame.

- Prepare it at the size it's drawn at instead, by its fit. Scale, never crop, so the anchor and fit still work during a resize.

- After a resize, keep drawing the current texture, scaled. Once resizing has been still for a short wait, the worker prepares the image at the new size and swaps it in without a fade.

- The blur is in image pixels today. The new size changes that, so the blur is scaled with the image to keep today's look at the same window size.

- A blurred image needs less than the window's size. Detail finer than the blur is gone anyway, so the image can be held smaller by a factor tied to the blur, and the GPU scales it up.
	- At the default blur, that might cut it by 4 or more without any encoder.
	- A cap keeps a small blur from shrinking it too far.

- Built 2026-10-05, without the smaller still part. The image is still cut to 4096 first, and the blur's sigma is still in pixels of that cut.
	- The held size is the larger of the 2 scales from the cut to the window, for stretch as for zoom. So the picture keeps its proportions and the blur stays round. It's never bigger than the cut, so a picture smaller than the window is held whole, as before.
	- The shrink runs in linear light on the float copy, with a triangle filter that reads past the edge as the edge pixel. The blur is scaled with it, and so is the contrast mask's measure of how busy the picture is.
	- The blur reads past the edge as well. So a margin 3 sigma wide is shrunk from the image's own edge, blurred with the rest, and cut off after. Without it the outer rows came out up to 8 levels off.
	- The shader takes the picture's proportions from the cut rather than from the texture, so a zoom crop falls where it did.
	- A resize is followed half a second after the last size change. A request still working is left to finish, and its result is checked against the window's size when it comes in. A resize keeps the picture's summary, so the derived text colors stay put.

- Measured against a control build, in the same session, in MiB. Each figure is the window less the same window without a wallpaper, so it's the wallpaper's share. Regular memory is the unique footprint less the driver's libraries.

	| Picture, window                                 | GL before | GL after | Regular before | Regular after
	| :---------------------------------------------- | --------: | -------: | -------------: | ------------:
	| 9433x5306 photo, cut to 4096x2304, at 2560x1440 |        72 |       32 |             93 |            42
	| 2560x1440 pack image at 1280x800                |        32 |       12 |             45 |            16

	- A pack image at 2560x1440 is held whole, the same as before.
	- On Vulkan the photo's texture went from 36.0 to 15.0 MiB. The allocator kept the same 128 MiB of blocks, so the driver's figure stayed at 198.
	- Preparing the photo took 3.8 s whole, 1.5 s held for 2560x1440, and 1.0 s for 1280x800, on a busy machine. The blur is the slow part, and it runs on fewer pixels.

- Checked against the control build at the same window size, on GL and Vulkan, in dark and light, stretch and zoom, at 1280x800 and 2560x1440. The pictures were the built-in, a pack image, and 2 large photos. Differences are in sRGB levels out of 255:
	- A picture held whole: 0 changed pixels.
	- A picture held smaller, at the default settings: at most 1 on GL, on 2 to 23% of pixels. One light mode zoom had 639 pixels at 2 or 3, along a sharp edge in the photo. Vulkan was at most 2.
	- The photo at 100% visibility without a scrim: at most 2, on 14% of pixels.
	- A window resized and settled against one launched at that size: 1 level on under 0.3% of pixels, from the kept summary. The scaled picture shown before the swap was within 1 level of the one swapped in.
	- With the blur off, the look does change: up to 23 levels on 56% of pixels. The GPU used to draw a large picture by skipping pixels, and now they are averaged.
	- The first try shrank the sRGB bytes before the float copy, and it came out up to 4 off at the default settings and 5 at 100% visibility.

- Holding a blurred picture smaller still was tried 2026-10-05 and not built. The picture was held where the blur's sigma came to D held pixels, so the GPU scaled it up by sigma over D.
	- Inside the picture, at the default settings: at most 1 level at D of 4 or 3, and at most 2 at D of 2 or 1.5. Up to half the pixels changed by 1 in light mode.
	- Near the edge it was worse: up to 6 levels at D of 4 and 8 at D of 2, with a pack image at 2560x1440. The GPU's clamp flattens the outer half pixel of a held pixel several screen pixels wide. A one pixel border around the held picture should fix that.
	- At 100% visibility: up to 4 inside and 15 at the edge.
	- A pack image at 2560x1440 costs 32 MiB on GL at window size, 8 at D of 4, and about 0 at D of 2. As plain RGBA that's 2.3 MiB at D of 4 against BC1's 1.8 at full size, and 0.6 MiB at D of 2.
	- So at the default blur, holding it smaller makes compression unneeded. Compression only pays off where the blur is small or off. That choice is left to [Block compression for the wallpaper](#block-compression-for-the-wallpaper), which already picks by blur.

- Built 2026-10-06. The held size is the smaller of the window's and the blur's, where the sigma comes to 4 held pixels on High, Max and Custom, and 2 on Low, Standard and Remote. A blur lighter than that at full size never shrinks the picture below what the window shows.
	- A picture held by its blur keeps a one pixel border, cut from the blur's margin. The shader reads the inside and blends into the border at the edge, where the clamp used to flatten the outer half of each edge pixel.
	- A kept copy with a border has a key of its own, so a picture held by the window at a near size never uses it.
	- Growing the window doesn't prepare a picture held by its blur again, because its size stays the same.
	- Checked against the control build at the same window size, in sRGB levels:
		- With the blur off, or too light to matter: 0 changed pixels, dark and light, on GL and Vulkan.
		- At 4 pixels a sigma: at most 1 on GL, edge included, and 2 on Vulkan, except one pixel at 3 in light mode. A light mode zoom of a large photo had about 4,000 pixels at 2 to 4 along one sharp edge.
		- At 2 pixels a sigma, and at 100% visibility without a scrim: at most 1 on GL and 2 on Vulkan.
	- In a 2560x1440 window, the texture for a 2560x1440 picture went from 14.1 MiB to 2.3 at 4 pixels a sigma and 0.6 at 2. A 9433x5306 photo went from 14.1 to 5.8 at 4.

### Kept copies on disk

- Preparing a picture took 0.8 to 2.6 s in an optimized build, and most of that was the blur. A launch, a rotation back to a picture, a resize, and a wake from the idle release all prepared it from the file again.

- Since 2026-10-06 each prepared picture is kept on disk as a high quality JPEG, in the platform's cache folder. That's `~/.cache/silkterm/wallpaper`, `~/Library/Caches/silkterm/wallpaper` on macOS, and `%LOCALAPPDATA%\silkterm\cache\wallpaper` on Windows. `XDG_CACHE_HOME` moves it. A `--config`, or an `XDG_CONFIG_HOME` without an `XDG_CACHE_HOME`, keeps it beside that config, as with the data folder.
	- The key is built from everything that changes the stored pixels. That's the file, its length, its time and which file it is, the size it's cut to, the blur after the image's own tag, and the contrast mask settings. The held size names the copy. Light or dark mode is applied when the picture is drawn, so it isn't part of the key.
	- A request takes the nearest copy within 5% of the pixels it asks for. The window takes that as the size it asked for, so it doesn't ask again.
	- A copy keeps the summary of the picture it was made from, so the derived text colors are the same with or without it.
	- It's written after the picture is sent to the window, to a temp file that's renamed into place, because every process on the machine shares the folder. A copy that doesn't read back whole is removed and made again.
	- Past 256 MB the copies used longest ago go first.
	- A picture held whole with no blur and no mask is only a decode, so it isn't kept. Neither is one with any transparency, because JPEG doesn't support it.

- Measured with a non-LTO optimized build, at the default settings. The write happens after the picture is sent, so nothing waits on it.

	| Picture, window                          | Prepare | Read the copy | Write the copy | Copy
	| :--------------------------------------- | ------: | ------------: | -------------: | ------:
	| 2560x1440 photo, at 2560x1440            |  1.39 s |         28 ms |         118 ms | 231 KiB
	| 2560x1440 photo, at 1920x1080            |  0.76 s |         13 ms |          62 ms | 140 KiB
	| 9433x5306 photo, at 2560x1440            |  2.55 s |         28 ms |         112 ms | 334 KiB
	| 9433x5306 photo, at 1920x1080            |  1.99 s |         15 ms |          64 ms | 217 KiB
	| Starry Night from the pack, at 2560x1440 |  1.36 s |         33 ms |         115 ms | 448 KiB
	| Starry Night from the pack, at 1920x1080 |  0.78 s |         23 ms |          66 ms | 297 KiB

	- The JPEG decode alone was 6 to 24 ms over the same pictures and sizes.
	- A copy is within 4 levels of the prepared pixels, and under half a level on average. Quality 100 gains little, because the color conversion costs about half a level by itself. At a light blur, where the picture keeps hard edges, it's up to about 10.
	- Block compression is still in the queue. Kept BC data would skip the decode too, but a 2560x1440 copy reads back in about 30 ms as JPEG already.

- Since 2026-10-07 a picture the GPU gets as BC1 is kept on disk as those blocks, which is 1.8 MiB at 2560x1440, and it's uploaded again as it was stored. Unpacking it for the stand-in takes 6 to 11 ms. A picture held by its blur stays plain and is still kept as JPEG.
	- Older copies are in another format and are removed at the next prune, whatever their age.

### Block compression for the wallpaper

- GPUs read block-compressed formats directly. BC1 is half a byte a pixel and BC7 one byte, against 4 for plain RGBA.

- They work only for textures that don't change. A GPU can't draw into one, so this is for the wallpaper only.

- The image to compress is the one already prepared at window size and blur.
	- Since 2026-10-05 the window size part is built. Holding a blurred picture smaller still was measured then and built on 2026-10-06. At the default blur it beats BC1 at full size without any encoder. See [The wallpaper at window size](#the-wallpaper-at-window-size).

- Pick by blur and size:
	- A heavy blur: hold it smaller and skip compression. A quarter-size image in plain RGBA is already smaller than BC1 at full size.
	- Little or no blur: BC1. The wallpaper is dimmed behind text, so its artifacts shouldn't show.
	- BC7 where BC1 shows banding, if a test finds any.
		- Since 2026-10-07, BC7 by the performance profile. See [BC7 by profile](#bc7-by-profile).

- Encoding runs on the wallpaper worker once per image and per resize. A pure Rust encoder is preferred.

- A plain RGBA fallback is kept for an adapter without BC support, though every one checked has it.

- Executable size is a top priority, so the encoder's cost in bytes matters as much as its quality.

- Built 2026-10-07, with a BC1 encoder of our own in `bc1.rs` and without a new crate. The release binary went from 11,946,152 to 11,959,832 bytes, which is 13 KB more.
	- A picture held by the window gets BC1. One held by its blur stays plain, as does one with any transparency.
	- Each 4x4 block takes the better of 2 tries. One is the line its colors spread along, refined twice by least squares, and the other is one flat color matched through a table of endpoint pairs. A flat color comes back within a level, where rounding it to 5:6:5 is up to 4 off.
	- The texture is sRGB, so the sampler decodes it the same way it decodes the plain one. That's true on the X11 GL path too, because the wallpaper is only sampled there and never drawn into.
	- A BC texture is whole blocks. The picture is placed in the top left of a texture padded to a multiple of 4, the padding repeats its edge, and the shader scales its coordinates to the part that contains the picture.
	- Every device asks for BC when its adapter has it. Without it the picture is uploaded plain.
	- On GL the check for a lost texture reads back a block of the wallpaper, and GL can't copy a compressed texture out. A BC1 wallpaper skips that check. The sentinel textures and the VT watcher still detect a lost texture.
	- Encoding takes 20 to 41 ms at 2560x1440, on up to 4 threads, and 22 to 24 ms at 1920x1080.

- Measured against a control build, with a 2560x1440 photo and no blur in a 2560x1440 window, in MiB. A share is the window less the same window without a wallpaper.

	| Part                                          | Before | After
	| :-------------------------------------------- | -----: | ----:
	| The texture                                   |   14.1 |   1.8
	| Graphics memory on X11, the wallpaper's share |     32 |     1
	| Regular memory on X11, the wallpaper's share  |     46 |    11
	| Vulkan allocator, in use                      |   65.5 |  52.4

	- On Vulkan the driver's figure stayed at 198 MiB, because the allocator kept the same 128 MiB of blocks. Regular memory there was the same within the 2 MiB the runs vary by. A first try was 3 MiB over, because a second copy of the blocks for the disk cache stayed in the worker's heap once freed. The copy is shared now.
	- At the default blur the picture is held by its blur and stays plain, so nothing changed.

- How it looks, against the control at the same window size, on GL and Vulkan alike, in sRGB levels:
	- At the default blur, in dark mode and at Low: 0 changed pixels.
	- No blur, in dark mode: under a level on average, and at most 6 or 7 on the built-in, 13 on a large photo, and 32 around small colored stars in a star field.
	- No blur, in light mode: at most 12 to 17.
	- A light blur of 2: at most 4 in light mode and 7 in dark.
	- No blur at 100% visibility and no scrim: at most 65 around the stars, 1.3 on average.
	- A 1917x993 window, where every edge is padded: the edge rows and columns are no worse than the inside.
	- There weren't any contour bands. In the built-in's smooth gradients without blur, the 4x4 blocks show as faint steps in light mode, and plainly with the contrast stretched 4 times. That's the case BC7 was held back for, so it still needs more thought on the item.

- BC1 for a picture held by its blur was tried and not kept. It came out at most 3 to 5 levels off, but each block is drawn several screen pixels wide there, and with the contrast stretched 4 times the blocks showed as a grid. It would have saved 2 MiB at 4 held pixels a sigma and 0.5 at 2.

#### BC7 by profile

- Built 2026-10-07 in `bc7.rs`, also our own. High, Max silk, and Custom give a picture held by the window BC7, and Low, Standard, and Remote keep BC1, the same split as the blur's hold. `wallpaper.compression` in the config file takes "auto", "bc1", or "bc7", and either of the last two overrides the profile. There isn't a Settings row.
	- At the default blur the picture is held by its blur and stays plain on every profile, so this is for pictures without a blur or with a light one, from Custom or their own tags.
	- A profile change or a new value in the file prepares the picture again. A BC1 copy and a BC7 copy on disk have different keys, so neither is handed to a window that wants the other.
	- The release binary went from 11,952,432 to 11,960,800 bytes, which is 8 KB more.

- BC7 has 8 modes. Mode 6 is the one that matters here. It has one pair of colors a block at 7 bits plus a shared low bit, and 16 points between them where BC1 has 4 points between 5:6:5 ends.
	- An opaque picture needs both alpha ends at 255, which makes the low bit 1 and every color end odd. Even values are reached halfway between 2 odd ones, but 0 is out of reach. A black sky came back a level up all over, which was worse on average than BC1.
	- So a block that touches 0, and doesn't span more than 12 levels, also tries mode 5. Its ends are 7 bits with 0 and 255 in reach, and alpha is kept apart, but it has only 4 points.
	- Modes 0 to 3 and 7 split a block in 2 or 3 parts for sharp edges between several colors. A wallpaper doesn't have many of those edges, and those modes would cost several times the code and the encode time. Small stars of more than 2 colors are the one place it shows, as it did with BC1.
	- Unlike BC1, every decoder makes the same texels from a BC7 block, so what the unit tests unpack is what the GPU draws.

- Encoding takes 59 to 116 ms at 2560x1440 at full speed optimization, where BC1 took 15 to 44 in the same run. The release build is optimized for size, and there it was 83 to 163 ms against BC1's 21 to 59, with the machine busy. It runs on the wallpaper worker, and a kept copy skips it.

- In a 2560x1440 window, the texture is 3.5 MiB for a 2560x1440 photo, against 1.8 as BC1 and 14.1 plain. The Vulkan allocator's in use figure went from 52.4 to 54.3 MiB.

- How it looks against plain, at the same window size, in sRGB levels. GL is shown. Vulkan had the same maxes, adn means within about a tenth.

	| Scene, no blur                           | BC1 mean | BC1 max | BC7 mean | BC7 max
	| :--------------------------------------- | -------: | ------: | -------: | ------:
	| Built-in, dark mode                      |     0.24 |       6 |     0.08 |       6
	| Built-in, light mode                     |     0.49 |      12 |     0.17 |      12
	| Large photo, dark mode                   |     0.47 |      13 |     0.15 |      12
	| Earth, light mode, zoomed                |     0.17 |       9 |     0.03 |      10
	| Star field, dark mode                    |     0.55 |      32 |     0.16 |      30
	| Star field, light mode                   |     0.33 |      17 |     0.09 |      16
	| Star field, 100% visibility and no scrim |     1.26 |      65 |     0.36 |      62
	| Star field, light blur of 2, dark mode   |     0.35 |       7 |     0.09 |       4

	- The worst pixels are the same few in both, colored stars a block can't represent.
	- On the texture itself, in smooth areas of the 3 pictures with real gradients, the error jumps by 0.5 to 0.6 of a level across a block edge with BC7, against 1.3 to 1.5 with BC1. With the contrast stretched 8 times the built-in's gradients show the blocks in BC1 and look like the plain picture in BC7.
	- At the default blur, and at Low, nothing changed, with 0 changed pixels against the build before.

### The dialogs' kept GPU context

- Settings and About draw through a GPU context built once and kept for the life of the process. It uses about 52 MiB so Settings opens in 86 ms rather than 310 ms. See the [Settings dialog](20260930-145721_settings-dialog.md) design doc.
	- As measured, it uses about 200 MiB of graphics memory and 16 MiB of regular memory, and almost all of it is 2 blocks that wgpu's allocator reserves up front. With the `MemoryUsage` hint it's 18 MiB. See [Measure first](#measure-first).
	- With the hint every device uses since 2026-10-04, it's about 21 MiB of graphics memory and 8 MiB of anonymous memory. See [The memory hint](#the-memory-hint).

- With many windows, that's 52 MiB each, all the time, for a dialog that is rarely open.

- Options:
	- Drop it when the dialog closes, and keep it only while a dialog is open.
	- Share the main window's device instead of a second one, where the backend allows. That also saves a second driver context. On X11 the dialog's GL context can't outlive the window's, so this needs care there.
	- Keep it, and say so.

- As of 2026-10-04 it's kept, though that can still be reversed.
	- With the hint it costs about 21 MiB of graphics memory and 8 MiB of regular memory per process.
	- The idle release already lets its device go along with the window's.
	- If it were dropped on close, every open would take about 230 ms, against 66 to 105 ms with it kept.
	- Sharing the main window's device can't work on X11, where the window draws through GL. Elsewhere it would save about 20 MiB.
	- `WARM_DIALOG_GPU` in app.rs is still the one-line way back to building the context on each open.

- This was in the releasing resources doc's roadmap before.

### A shorter wait for a minimized window

- A minimized window and a covered one share "Minutes when hidden", which is now 1 by default. It was 30.

- The cost is the way back. Taking the card back takes about 25 ms on Linux, but 0.8 to 1.2 s on Windows with an RTX 2060. What a restored window shows on Windows in that second still needs to be checked.

- The release code only knows "hidden", which includes minimized, covered, and a window without a size. Wayland doesn't report any of them, so a window there still waits "Minutes otherwise".

- Built 2026-10-04 as a wait of its own for a minimized window, then taken back out on 2026-10-05 in favor of the shorter shared wait. See [Superseded](#superseded).

### Software rendering

- SilkTerm uses a software adapter only when no graphics card is found at launch. A card that is found but can't make a device, as when its memory is full, ends the launch, and an idle rebuild retries the same card.

- "Always use software rendering" is off by default. It asks wgpu for the fallback adapter, which is lavapipe or llvmpipe on Linux and WARP on Windows.
	- The performance profile follows it. Automatic already starts a software adapter at Low.
	- wgpu doesn't have a software adapter on macOS. The row is grayed there, with a tip.
	- The X11 transparency path uses its own GL context, and libGL picks its driver once per process. Whether a launch can steer it to Mesa's software driver needs checking. If not, the row is grayed under Transparency on X11.
	- A change takes effect at the next device build, which a rebuild can trigger, rather than only at relaunch.

- For the fallback, when a device can't be made at launch or at a rebuild, the software adapter is tried once before giving up.
	- A window that fell back stays on it until the next rebuild, which tries the card first again.
	- The title or Help > About says software rendering is in use, the same way it does now.
	- This is also a way out for the blank window under GPU load, where `vkCreateDevice` was seen failing.

- Built 2026-10-04, as `window.software_rendering`, on the Window tab under the idle rows for now.
	- On X11 the software device is Vulkan's lavapipe, drawn on the same window glutin made. libGL keeps the driver it loaded first, so Mesa's GL can't be used for a fallback or for a change made while running, and a second GL instance in the process panics. Steering libGL to Mesa at launch wasn't tried, because lavapipe handles both cases. lavapipe supports premultiplied alpha on that window, so Transparency still works and the row isn't grayed for it.
		- On an X server that can't make shared pixmaps, such as NVIDIA's, Mesa's software Vulkan presented through them anyway, and the window died of the X error that followed (2026100614510979). At launch on such a server the program sets `MESA_VK_WSI_DEBUG=noshm`, which sends lavapipe's frames with PutImage instead. Shells started from it inherit the variable.
	- Every device tries the setting's choice first and the other kind once. That's the window's device, the dialogs' kept context, and the one a dialog builds without it. If software is asked for and none is installed, the card draws and a message is printed to stderr once.
	- A change takes effect once no dialog is open. The window lets its device go and builds it again, and the dialogs' context goes with it.
	- A software device on a machine with a card isn't new hardware. The card keeps its rating, and the session steps down to Low without anything written, the way a remote screen takes Remote. Once it's back on the card, that step is removed.
	- A fallback prints the reason and the renderer on stderr. Help > About names the adapter of the device the window has now.
	- `SILK_REFUSE_CARD=<file>` makes every card refuse a device while the file is there, as a full one does.
	- `--software-rendering[=BOOL]` overrides the setting for one run and doesn't save anything. It goes on before the first device is made. On macOS it says there's no software renderer and the card draws.

- On macOS, wgpu only has its Metal backend. GL there needs ANGLE, and Vulkan needs MoltenVK, which runs on Metal itself. So a Mac without a Metal device, such as a virtual machine without graphics acceleration, doesn't have any adapter. SilkTerm shows an alert as well as a message on stderr, because a launch from Finder doesn't show stderr, and then it exits with 2. Whether to include a real fallback there still needs more thought (2026102209035801).

### The Resource use group

- "Free resources when idle" and its waits move into a group named "Resource use", with "Always use software rendering" beside them.

- "Free resources when idle" gets a warning mark. Its tip says it matters most on a card with little memory, or next to GPU-heavy programs, with many windows open that aren't all in view. Turn it off only if the graphics driver has trouble with it.

- A setting that stops a window from giving memory back gets a warning mark where it does so. Today that's Transparency on Windows, because a window in view with Transparency on is never let go. The mark shows only in the Windows build.

- A row that can't work on the running platform is grayed with a tip saying why, using the existing grayed-row tips. A row is hidden only in a build that can never use it, as with the Windows-only file association rows.

- Tips stay short, per the UI style guide.

- Built 2026-10-06. "Resource use" is the last group on the Window tab. It has "Free resources when idle", its 2 waits under it, and then "Always use software rendering".
	- The idle switch's mark shows on every platform, because a driver anywhere can have trouble with it.
	- Transparency keeps its one mark. In the Windows build the mark's tip adds that a window in view keeps its graphics memory. The `windows_warning` row field has the Windows text in place of `warning`.
	- On Wayland "Minutes when hidden" is grayed, because Wayland never tells a window it's minimized or covered. Its tip says the other wait is the one used.
	- "Always use software rendering" is left out of the macOS build, which can never use it. It was grayed there before.

## Alternative ideas

### Rejected

- Lossless compression on the GPU. Graphics APIs don't let a program store a texture losslessly in less space.

- An encoder with a C toolchain, such as `intel_tex_2`. A pure Rust one builds for every target without any extra setup.

- Block compression for the scrim or swapchain. A GPU can't render into a compressed format.

- 8-bit blur layers, tried 2026-10-04. They would save 2 more bytes a pixel, which is 7 MiB at 2560x1440. Against the control on GL, the distance functions changed 3 to 10% of pixels by 1 or 2 levels. The Gaussian function changed 11% by up to 6, seen as steps in the halo's fade.

- Half-size blur layers, tried 2026-10-04. They would save 3 more bytes a pixel, which is 10.5 MiB at 2560x1440, and three quarters of the blur's work. A half-size pixel counts as glyph when any of the 4 under it does, so the halo comes out heavier. At the default settings about a fifth of the pixels changed, mostly by 1 to 5 levels, and small counters in glyphs like @, # and $ filled solid, by up to 51. A soft halo changed by at most 3.
	- Under the plan above, the lower profiles get half size when the loss shows. The only lower profile with a halo is High, which keeps the same share of the radius so it looks like the same halo, so it stays full size until that's settled.

- One channel for the text coverage. glyphon writes each glyph's own color, so it would take a change to our glyphon branch to write white. It would save 3 more bytes a pixel. It hasn't been tried.

### Superseded

- A wait of its own for a minimized window, "Minutes when minimized", 1 by default, with a covered window keeping its 30. It was built 2026-10-04 and taken out on 2026-10-05, when "Minutes when hidden" went to 1 for both. A config that has its line loses it on the next launch.

## Research findings

- Block compression support, checked with Mesa 26.1.2:
	- lavapipe reports `textureCompressionBC`.
	- llvmpipe's GLES has `GL_EXT_texture_compression_s3tc_srgb`, `_rgtc`, and `_bptc`, which are the 3 that wgpu wants before it turns BC on for GLES.
	- WARP: D3D12 requires the BC formats on every device, WARP included.
	- Metal: wgpu turns BC on where Metal reports it. It hasn't been checked on a Mac yet.

- Pure Rust encoders: `texpresso` does BC1 to BC5, and `rusty_dds` does BC1 to BC7. Neither was measured for speed, quality, or binary size. An encoder of our own was built instead, as settled on 2026-10-06.
	- The same went for BC7 on 2026-10-07, with 2 of its 8 modes in about 8 KB of binary.

- The scrim's blur takes 25 taps a pass and spaces them more than a pixel apart once the radius passes about 4 pixels.

- The wallpaper's default blur is a sigma of 10, in pixels of the image after the 4096 cut.

## Roadmap

1. Measure.

2. Shrink the scrim and hold the wallpaper at window size. Settle the dialogs' context. Shorten the wait for a minimized window.

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

- "Drop "Minutes when minimized", and make "Minutes when hidden" 1 by default" (ID 2026100513581813)

- "Settings: a Resource use group, with warning marks" (ID 2026100418225506)

- "Block compression for the wallpaper" (ID 2026100418225507)

- "Wallpaper: optional BC7 in place of BC1" (ID 2026100705511507)

- "Wallpaper: keep resized copies on disk, oldest pruned first" (ID 2026100514211603)

- "The window doesn't paint while the GPU is busy or short on memory, and stays blank after the load ends" (ID 2026100312470535)

- "Release the GPU device after a long idle." (Opened 20260905-181131). Its open point, "figure out a way to reduce CPU and memory usage", moved here.

## Copyright and license

> Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]\
> Licensed under the [Creative Commons Attribution 4.0 International License](https://creativecommons.org/licenses/by/4.0/) (CC BY 4.0).\
> This document is provided as is, without warranty of any kind.
