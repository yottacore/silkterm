<!-- markdownlint-disable MD007 -- Unordered list indentation -->
<!-- markdownlint-disable MD010 -- No hard tabs -->
<!-- markdownlint-disable MD041 -- First line in a file should be a top-level heading -->

<!-- TOC ignore:true -->
# Unicode, fonts and emoji

<!-- TOC ignore:true -->
## Table of contents

<!-- TOC -->

- [Summary](#summary)
- [Specification](#specification)
- [Goals](#goals)
	- [Non-goals](#non-goals)
- [Design](#design)
	- [Shaping with fallback](#shaping-with-fallback)
	- [Fitting glyphs to the grid](#fitting-glyphs-to-the-grid)
	- [Picking the family](#picking-the-family)
	- [Color emoji](#color-emoji)
	- [Engine limits](#engine-limits)
- [Alternative ideas](#alternative-ideas)
	- [Rejected](#rejected)
	- [Superseded](#superseded)
- [Research findings](#research-findings)
- [Roadmap](#roadmap)
- [Related backlog issues](#related-backlog-issues)

<!-- /TOC -->

## Summary

A terminal is a grid of cells, and fonts do not know that. SilkTerm picks one monospace family for every weight, falls back glyph by glyph to other installed fonts for anything that family lacks, and fits every such glyph to the cells the terminal gave it. Color emoji are painted by SilkTerm itself, since the text library cannot read the current color font format.

## Specification

- Every character draws. A character the terminal font lacks comes from another installed font. A character no installed font has shows whatever the system's last resort draws, and installing a font that covers it is the fix.

- Text stays on the grid. Every character, from any font, sits in the cells the terminal gave it: one for a narrow character and two for a wide one.

- Regular, bold and italic come from one monospace family, so a bold word is exactly as wide as the regular one.

- Color emoji draw in color. `text.color_emoji` turns that off.

- A character Unicode presents as text, such as a check mark or the copyright sign, draws in the cell's text color, never in an emoji font's own colors.

- A glyph from another font sits on the same baseline as the text beside it.

- The font is found from one search order, the same on every platform:
	- the desktop's monospace family, when "Use system font" is on;
	- then `font.family`, a comma-separated list;
	- then the desktop's monospace family, when "Use system font" is off;
	- then a built-in list, which is also what a fresh config gets;
	- then whatever the generic monospace name finds, only if nothing above is installed.

- Every family in the built-in list has a real bold face.

- The font size follows the desktop's fixed-width size unless a size is set. Ctrl+Minus, Ctrl+Plus and Ctrl+0 change it a pixel at a time.

- A cell takes at most nine combining marks.

- A control character, a tab included, draws as a single blank cell.

- A pane is always at least two columns wide, so a wide character always fits.

## Goals

- CJK, emoji, math symbols and right-to-left scripts draw instead of empty boxes, with the grid intact.

- The same build and config pick the same font on every platform.

- No setting for fallback fonts. Other terminals and editors have none, and there is nothing to tune.

### Non-goals

- Proportional text. The grid is the grid.

- Matching weights across fonts. Two marks from two different fonts can differ a little in weight, and which fonts are involved depends on the machine.

- A bundled font. The installed fonts are used.

## Design

### Shaping with fallback

Pane text is shaped with per-glyph font fallback (`Shaping::Advanced`), so a character the terminal font lacks comes from another installed font while the monospace alignment stays. Measuring the cell uses the plain path (`Shaping::Basic`), since it only needs the terminal font. An earlier version of the text library hung on real output with fallback on. The version in use has a bounded fallback loop and was stress-tested.

A character the terminal font does not carry goes through `fill_glyph`, which shapes it on its own and hands the pane a buffer to place. Two things follow that nothing else in the renderer has to think about.

- The fallback face has its own baseline. Its glyph is shifted onto the baseline of the text beside it.

- A fallback emoji face may carry a character Unicode presents as text. Such a character is not painted by the emoji face, so it takes the cell's color. Real emoji are unaffected.

A color emoji face rasterizes to nothing through the text library. So an empty raster is taken as "this face draws nothing", and the glyph is shaped again through the generic monospace chain, which picks a face that does rasterize. The color path below then paints the real emoji.

Each fallback glyph is shaped once per pane, keyed by character, bold and italic, and tinted per cell. That took fallback work from about 16% of CPU on a full screen to 0.2%.

### Fitting glyphs to the grid

- The cell width is the text's real rendered pitch, not rounded. Everything placed on the grid, from the cursor and cell backgrounds to fallback glyphs, is positioned by multiplying that width by the column. A rounded width drifted by a fraction of a pixel per column, so the cursor sat further past the text the longer the line got.

- A character rides the shared row layout only when the font's own width for it agrees with the number of columns the terminal gave it. A monospace font is free to carry a double-width character at single width, and the default font does so for 53 of them, several common emoji and fullwidth punctuation among them. Laid out from the font, such a character took one column where the grid gave it two, and everything after it on the row drew a column to the left. Anything that disagrees is drawn on its own, fitted to its box, the same path characters missing from the font take. Those emoji draw in color as a result.

- A glyph drawn on its own is scaled and centered in its cell box, so an over-wide fallback cannot spill onto its neighbor. It is clipped to the pane's content.

- A control character, a tab included, draws as a one-cell space so the row stays on the grid.

### Picking the family

One monospace family is pinned for every weight, because the shaper picks the best face per query and would otherwise let a bold run end up in a different family than the regular run beside it. Bold asks for the boldest weight the pinned family has, so it cannot escape into a proportional bold fallback.

The search order in the Specification is the same on every platform. The setting only reorders that list and never shortens it. An earlier version dropped `font.family` entirely while following the desktop font. The same build and the same config then picked different fonts depending on the platform, and a configured list could be silently ignored. Every list is now always walked. A family that is not installed falls through to the next one, and the configured list still has effect as a fallback.

Platforms differ only in what they report, not in the rules applied to it. Windows has a system font size but no monospace family, so following the family there does nothing, and the search starts at `font.family` without a special case. A switch with nothing behind it reads as inert, so the Settings checkbox grays out and says why. The same goes for a desktop with no font setting at all, which is why the check asks what was detected rather than which platform is running.

On Linux the desktop's own settings store is asked first, and the other fills in: xfconf on Xfce, gsettings elsewhere. gsettings answers on any box with GNOME's schemas installed, an Xfce box included, and a key nobody set comes back as the schema default. So asking it first on Xfce gave Cantarell 11 and Monospace 11, whatever the desktop was set to.

The built-in list is last for a reason. The generic monospace query below it is a lottery over installed fonts, and its winner may have no bold face. That throws bold runs into an arbitrary, often proportional, fallback whose advances cannot be snapped to the cell grid. Every entry in the built-in list has a real bold face: Monaspace Argon, Fira Code, JetBrains Mono, Cascadia Mono, Consolas, Ubuntu Mono, SF Mono, Menlo and Courier New. When that list changes, the outgoing value is recorded, so an existing config still carrying it word for word is refreshed on the next launch. A list the user edited is theirs and is left alone.

The size half of "Use system font" works on Windows too. An explicit `font.size` beats the desktop's size. The size hotkeys change the size for the window, since all panes share one set of text metrics.

### Color emoji

The text library reads only the older color glyph table (COLR v0), and every current color emoji font has only the newer one (COLRv1). So SilkTerm paints color glyphs itself, in `coloremoji.rs`. It walks the paint graph and renders it through a small 2D back end: transforms, clip and layer stacks, solid, linear, radial and sweep fills, and Porter-Duff and blend compositing. The result goes to the renderer's color atlas as a per-cell image fitted to the cell box. Characters with no color glyph are untouched and take the monochrome fallback path.

The color glyph cache keeps images by glyph and pixel size. It discards only images no recent frame has touched, and if everything in it is still in use it grows, which is bounded by what fits on screen. An earlier cache emptied itself completely when full, and threw away images the frame being drawn still needed.

The two glyph caches, text and color, are cleared together, and a missing raster is skipped rather than stopping the frame.

### Engine limits

- The engine keeps up to nine combining marks per cell. That change is carried on the engine fork until upstream releases it. See the Alacritty design doc.

- A pane is at least two columns, the engine's documented minimum, so a wide character always has room.

- An RTL override character is kept in a window title, because refusing it would also refuse the joiners that tie multi-part emoji together.

## Alternative ideas

### Rejected

- Settings for fallback fonts. There is nothing a person would tune.

- Batching fallback glyphs into one draw. Not worth 1.6% of a frame. See the Speed design doc.

- "Use system font" as inert on Windows. Only the family half is inert. The size half works.

### Superseded

- The plain shaping path for pane text. Replaced by per-glyph fallback. Backlog: "Some unicode glyphs don't render".

- The desktop's monospace family, else generic monospace, as the whole default. Replaced by the one search order and the built-in list. Backlog: "Use system monospace font by default" and "The font fallback stack is only partly implemented".

- Pinning the monospace advance on the text buffer. The measured, unrounded cell width replaced it. Backlog: "There are weird spacing issues with the cursor".

- Emoji as monochrome outlines. They are painted in color now. Backlog: "Graphical emoji render as monochrome outlines instead of color".

- A heavy check mark beside a light cross in the prompt. It is the light pair now. Backlog: "The '✘' an '✓' on the git prompt look weird in powershell".

## Research findings

- The default font carries 53 double-width characters at single width.

- A screen filled with fallback symbols is cheaper than ordinary text, 10.9% of a core against 24.7%, since each leaves a blank placeholder in the shaped row.

- Other terminals show the same emoji fonts in color because their rasterizers read COLRv1.

- gsettings answers with schema defaults on an Xfce box, so it must be asked second there.

## Roadmap

- The fallback order has not been checked on Windows with a fresh config. Backlog: "The font fallback stack is only partly implemented".

- A font size per pane. All panes in a window share one set of text metrics, so it needs a renderer per pane. Backlog: "Hotkeys to increase and decrease font size".

## Related backlog issues

- "Some unicode glyphs don't render, most likely due to inadequate font coverage rather than a bug" (closed 20260629-214404)

- "Use system monospace font by default" (closed 20260629-214404)

- "There are weird spacing issues with the cursor" (closed 20260629-214404)

- "Graphical emoji render as monochrome outlines instead of color" (closed 20260727-014507)

- "The font fallback stack is only partly implemented, and resolves differently per platform for the same build" (Opened 20260727-014507)

- "Crash: a screen filled with distinct emoji aborts the terminal" (closed 20260728-074118)

- "Bug: Editing a line at any point on the prompt, that has one or more emojis in it" (Opened 20260802-002500)

- "Windows: font, scrolling and virtual-workspace problems" (Opened 20260721-130036)

- "The '✘' an '✓' on the git prompt look weird in powershell"

- "When the terminal is completely is full of text, it's slows noticeably" (Opened 20260708-191010)

- "Hotkeys to increase and decrease font size" (Opened 20260722-100516)

- "Font size should be able to be increased, even when using system font" (Opened 20260722-100516)

- "Windows fonts look too small even at 100% scale" (canceled 20260817-120024)
