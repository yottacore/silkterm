<!-- markdownlint-disable MD007 -- Unordered list indentation -->
<!-- markdownlint-disable MD010 -- No hard tabs -->
<!-- markdownlint-disable MD033 -- No inline html -->
<!-- markdownlint-disable MD055 -- Table pipe style [Expected: leading_and_trailing; Actual: leading_only; Missing trailing pipe] -->
<!-- markdownlint-disable MD041 -- First line in a file should be a top-level heading -->

<!-- TOC ignore:true -->
# SilkTerm design

<!-- TOC ignore:true -->
## Table of contents

<!-- TOC -->

- [Goal](#goal)
- [Feature designs](#feature-designs)
- [Architecture](#architecture)
	- [Language / Stack Decision](#language--stack-decision)
	- [Logical code organization](#logical-code-organization)
	- [API (alacritty_terminal)](#api-alacritty_terminal)
	- [Smooth scrolling](#smooth-scrolling)
	- [Minimap](#minimap)
	- [Text readability scrim](#text-readability-scrim)
	- [Minimum contrast (2026-08-30)](#minimum-contrast-2026-08-30)
	- [Dark text on a light background (2026-09-21)](#dark-text-on-a-light-background-2026-09-21)
	- [How much of the wallpaper is on screen (2026-09-20)](#how-much-of-the-wallpaper-is-on-screen-2026-09-20)
	- [Text colors from the wallpaper (2026-09-20)](#text-colors-from-the-wallpaper-2026-09-20)
	- [Performance profiles (2026-09-03)](#performance-profiles-2026-09-03)
	- [Font fallback stack](#font-fallback-stack)
	- [Hyperlinks](#hyperlinks)
	- [What a double-click grabs (2026-08-26)](#what-a-double-click-grabs-2026-08-26)
	- [Selecting past the edge of the screen (2026-09-20)](#selecting-past-the-edge-of-the-screen-2026-09-20)
	- [Measurements and display scaling](#measurements-and-display-scaling)
	- [Attention colors and dialog chrome](#attention-colors-and-dialog-chrome)
	- [Groups and sub-groups in the Settings dialog](#groups-and-sub-groups-in-the-settings-dialog)
	- [The color picker (2026-09-20)](#the-color-picker-2026-09-20)
	- [Saved themes](#saved-themes)
	- [The shell list and how it is filled](#the-shell-list-and-how-it-is-filled)
	- [What a pane's shell inherits](#what-a-panes-shell-inherits)
	- [A prompt is offered to bash, never installed (2026-08-30)](#a-prompt-is-offered-to-bash-never-installed-2026-08-30)
	- [Opening files from Explorer on Windows (2026-09-30)](#opening-files-from-explorer-on-windows-2026-09-30)
	- [One tip system, four places that draw it (2026-08-30)](#one-tip-system-four-places-that-draw-it-2026-08-30)
	- [Render Loop Sketch](#render-loop-sketch)
	- [Output notices under a flood](#output-notices-under-a-flood)
	- [The About box says how long the session has been up (2026-09-20)](#the-about-box-says-how-long-the-session-has-been-up-2026-09-20)
	- [A character handed to the window is typing (2026-09-09)](#a-character-handed-to-the-window-is-typing-2026-09-09)
	- [What untrusted input may not do (2026-09-09)](#what-untrusted-input-may-not-do-2026-09-09)
	- [The fuzzer (2026-09-09)](#the-fuzzer-2026-09-09)
	- [Environment](#environment)
	- [Startup and slow external resources](#startup-and-slow-external-resources)
	- [Letting the GPU go on a long idle (2026-09-17)](#letting-the-gpu-go-on-a-long-idle-2026-09-17)
	- [Configuration format](#configuration-format)
	- [Variables in a setting (2026-08-30)](#variables-in-a-setting-2026-08-30)
	- [Command-line options](#command-line-options)
- [Delivery (CI/CD, branches, releases)](#delivery-cicd-branches-releases)
	- [A tab can be named by hand, and the window title follows the tab (2026-08-30)](#a-tab-can-be-named-by-hand-and-the-window-title-follows-the-tab-2026-08-30)
	- [A terminal running with administrator or root rights says so (2026-09-09)](#a-terminal-running-with-administrator-or-root-rights-says-so-2026-09-09)
	- [Tabs report what they are running, and where (2026-08-21)](#tabs-report-what-they-are-running-and-where-2026-08-21)
	- [A tab is as wide as its own label needs (2026-08-23)](#a-tab-is-as-wide-as-its-own-label-needs-2026-08-23)
	- [PowerShell gets the same prompt bash does (2026-08-21, reworked 2026-08-30)](#powershell-gets-the-same-prompt-bash-does-2026-08-21-reworked-2026-08-30)

<!-- /TOC -->

## Goal

GUI terminal emulator for Debian/X11/Compiz with pixel-by-pixel smooth scrolling, both:

- Animated easing on output (new text appears).

- Smooth scrollback navigation with wheel.

No existing Linux terminal does animated smooth-scroll on output. (Verified: WezTerm, kitty, foot, Alacritty, GNOME Terminal, Konsole all snap to cell rows.)

## Feature designs

Each of these has its own design doc, which is the source of truth for that feature.

- [Smooth scrolling](design_docs/20260930-144720_smooth-scrolling.md)

## Architecture

### Language / Stack Decision

Rust + `alacritty_terminal` crate (not a fork of Alacritty repo).

Rationale:

- `alacritty_terminal` crate (v0.15.0 at design time; v0.26 as built) provides PTY + full VT/ANSI parser + grid state as a standalone library. Inherit the two hardest, correctness-critical pieces.

- Do not `git fork alacritty` - its renderer is built to snap to cells and maintainers reject smooth scroll by design. Forking = fighting architecture + merge debt. Crate = clean dependency, build only the renderer.

- Two crates are nonetheless patched, through `[patch.crates-io]` in the workspace manifest, and both follow one rule: a branch under `jim-collier` named for the release it sits on, holding that published release plus our change and a test for it. Naming the release is what lets an older lock keep resolving, and starting from the published source rather than the upstream branch keeps the delta to what has actually been read. `alacritty_terminal` carries the scroll ledger and four smaller fixes; `x11-clipboard` keeps the copied text when a stale `SelectionClear` arrives, which was the defect behind copies that silently stopped working (2026-09-20).

- Renderer: `wgpu` (or `glium` as fallback). Glyph atlas + cell draw.

Rejected alternatives:

- Go (`aminal`, custom): Difficult due to dearth of existing plumbing options; parser is the hard part.

- Zig + libvterm + raylib: viable but less ecosystem glue than Rust path.

- Python: Excluded (not compiled).

### Logical code organization

SilkTerm implements an event-loop-driven renderer over a retained terminal model - closer to a game's update/render loop than to a widget framework. Three logical roles:

- Model (the only source of truth). Each pane embeds an `alacritty_terminal::Term`: the integer character grid, scrollback, cursor, and the full VT/ANSI parser. A per-pane background thread reads the child process's PTY, feeds the bytes into that `Term`, and wakes the UI thread. Global tunables live in one swappable `Settings` (an atomic `Arc`) that every layer reads. Nothing else caches grid contents.

- View (rebuilt every frame, pulled - never pushed). There is no retained widget tree. Each frame every visible pane snapshots its grid into draw data: styled text runs for the glyph renderer, plus solid quads for cell backgrounds, the cursor, and the selection. The GPU renderers draw that. Smooth scroll is a view-only idea layered on top: the model only knows whole lines, the renderer interpolates a fractional offset between them. Chrome (menu bar, tab bar, context menus, dialogs) is drawn the same immediate-mode way.

- Controller (event routing). winit delivers all input to one `ApplicationHandler`. Keystrokes become PTY bytes for the focused pane (or drive an open menu/dialog instead); the mouse drives selection, focus, divider-drag, pane reorder, and menus. Input never edits the grid directly - it goes to the child, the child replies, and the model updates on the next PTY read.

The spine of the program is a single ownership tree:

~~~text
App  (winit ApplicationHandler)
+- State                      Main window
|  +- Gfx                     GPU backend: native wgpu surface, a glutin GL context
|  |                            on X11, or a composited DX12 surface on Windows
|  |                            (the two per-pixel-transparency paths)
|  +- renderers               Text (glyphon) + rects + bg image + scrim
|  \- Tabs                    The tab list + active index
|     \- PaneManager          One per tab: a binary split tree (Node::Split / Leaf)
|        \- Pane              A leaf: layout rect, selection, per-pane state
|           \- TermInstance   Alacritty Term + its PTY-reader thread
\- DialogWin?                 Optional pop-out window (Settings / About),
                                self-contained with its own Gfx + text renderer
~~~

So a window is a list of tabs, a tab is a split tree of panes, a pane wraps one terminal; pop-out dialogs are independent sibling windows.

Frame loop: a PTY read or a user event marks the app dirty or starts an animation. `about_to_wait` renders when something is dirty or animating, and otherwise waits. A render advances the scroll easing, snaps the grid to the nearest whole line, and redraws each pane from current model state. Frames are driven from `about_to_wait` rather than redraw requests, because `request_redraw` is unreliable under X11/Compiz here. Timed wakes, such as a parked cursor's resume or a minimap compose that owes another, are read after the frame, since drawing is what sets them.

### API (alacritty_terminal)

(As designed against 0.15.0; the build tracks the current release - 0.26 as of 2026-07. Signatures below are the stable core that carried over.)

- `Term::scroll_display(Scroll)` - moves viewport by whole lines. `Scroll` enum: `Delta(i32)`, `PageUp`, `PageDown`, `Top`, `Bottom`.

- `grid.display_offset()` - integer line offset from bottom = current viewport position.

- Grid cell iteration (`iter_visible` / indexing) = render source.

- `config::Scrolling` = history limit + line multiplier only. not animation. Ignore for smooth scroll.

Critical constraint: crate's `display_offset` is integer lines. No fractional scroll in crate. Smooth scroll lives entirely in the renderer.

Sharing the terminal with the reader thread: the reader holds the terminal across a whole read cycle, so the renderer cannot simply take it every frame without stalling. It also cannot merely try and give up. The reader reclaims it immediately, and an impatient try can lose forever, which showed up as a pane frozen for seconds during heavy output. The rule is to try first, and after a couple of frames of getting nowhere, wait properly. Waiting is bounded, because it reserves the terminal ahead of the reader's next cycle. Trying is not bounded at all.

### Smooth scrolling

The engine keeps whole lines, and the renderer draws a fractional offset over them. The wheel, the scrollbar, new output and full-screen programs all ease by moving that offset. New output runs through the output chase, a speed curve made of five named segments, one per setting. Full-screen programs are read from a scroll ledger in the engine fork, with row fingerprints as the fallback.

Full design: [Smooth scrolling](design_docs/20260930-144720_smooth-scrolling.md).

### Minimap

An optional sidebar showing the whole scroll buffer in miniature, in the spirit of the Sublime Text / VS Code minimap. On by default since 2026-09-17, once the column learned to step aside for full-screen programs.

Where it sits:

- Per pane, not per window. Scrollback belongs to a pane, so in a split each pane carries its own map.

- The map owns a real column inside the pane's rect - it never overlays the text. Turning it on costs terminal columns, and the PTY resizes like any other layout change. A pane too narrow to spare the room gets no column at all; the column may never take more than half a pane.

- Left to right: terminal text, then the preview. The regular scrollbar sits at the pane's far right edge, over the edge of the preview, the same overlay it is without a map. An earlier design kept a second, always-visible bar beside the preview. The two showed the same thing, so that one was removed.

- The configured width is the whole column's.

The mapping, which is the decision everything else rests on:

- The whole buffer - history plus screen - always maps linearly onto the column, top-anchored, oldest first. The editors slide their minimap once the document outgrows it; this one never does.

- The map stops where the eased text has reached, not at the live bottom of the buffer. Under a flood the view sits behind the newest output by however far the output ease is holding it, and drawing past that puts lines in the column that are not on screen. What is held back is the output the view has not come down to yet: new lines add to it as they arrive, and the view gives it back as it reaches them. So a scroll or a jump to the bottom never shortens the column by itself, and typing during a flood does not turn the trim off. The extent is settled when the picture is composed, so the marker is never measured against a picture nobody drew, and a map that came out short asks for another compose and follows the ease down. One line is always kept, so the column never disappears.

- An earlier pass read the same sentence the other way and stopped the map at the last screen row with output, so the blank rows under a short prompt took no track. That is out again. The blank rows are part of the buffer.

- Two narrower rules were tried before that one and each broke the other's case (2026-09-20). Trimming whenever the view is following the bottom reads a jump to the bottom as output, since the view eases in the same way, and shortens the map by the whole distance the gesture has left to travel. Trimming only while the output chase owns the motion misses the rest of a flood after a single keystroke, because a keystroke aims the view at the bottom and that flag does not clear while output keeps arriving. Both are one-line readings of the scroll position from outside the scroll model, which carries the chase's undrained backlog and a gesture's remaining travel in one number. Counting the unreached lines inside the model is what settles both, since only it can tell the two apart.

- With a short buffer, lines draw at a capped height (1.5 px at 1x) and the preview just does not reach the bottom of the column yet. That cap is scaled but not rounded to whole pixels, on purpose: a line has to be able to sit at a fraction of one, or its ink falls inside a single pixel row and a page goes back to reading as a slab.

- With a deep buffer, lines go sub-pixel and blend down, so the map compresses instead of scrolling. At the default 10,000-line scrollback a line is a fraction of a pixel; colored regions still read as bands, which is most of the point.

- The marker is measured at the map's own pitch, over the lines the picture draws rather than over the whole buffer. Anything else puts it above or below the text it stands for whenever the two differ, which is every moment the trim above is holding the map short.

- The marker carries a floor on its height so a deep buffer still leaves something to grab. The thumb takes the same span, never its own. Where the floor makes the marker taller than the rows it stands for, it grows both ways from their middle, so it still reads as pointing at them.

What a line looks like:

- Strokes, not glyphs. Per cell: a run of the cell's fg color where there is ink, over the cell's bg where it differs from the default. Hues survive, so errors, prompts and diffs stay findable from across the room.

- How much of its cell a character inks varies with the character, from a quarter for a period or a comma up to the full amount for a hash, a block or an em-wide letter. A flat share is what made a run of text read as one bar. The weights are eyeballed from a monospace face rather than measured, since the map is a hint and the face in use is not known where the raster runs. A cell that carries its own background still paints solid whatever is in it; only how far its color pulls toward the foreground moves.

- Across a line, coverage adds up, so a short or indented line reads as one. Down the column, color is averaged over only the lines that have ink, so a lone red line among blanks keeps its color rather than fading into them.

- A cell is spread over a tent a pixel each side, and so is a line, rather than each being clipped to the pixel it happens to fall in. Neither grid lines up with the pixels, and at the ratios a column runs at - about 100 cells into 90 px, about one line per pixel - clipping leaves the two grids beating against each other. That draws a comb across the column and broad bands down it, neither of which is in the text. The wider filter costs about a sixth more per compose and it is what makes a page of repeated output read as the text it came from.

- Under about 0.6 px per line the line filter goes back to clipping. A pixel there already averages more than a whole line, the gap below is switched off and the column is even anyway, while the wider filter would cost three times as much - and that is the deep buffer where a compose is already the expensive one.

- How bright a pixel row gets is how much ink actually fell in it, so a mostly blank stretch reads dimmer than a solid page. That is what makes density legible from a distance. One inked line among many would otherwise almost vanish, so a pixel never falls below a set share of the strongest line in it.

- A line does not fill its own height. The gap above and below is what stops a page of text reading as one block. At the capped height the ink is a band narrower than a pixel, so it falls across two pixel rows at part strength rather than filling one, which is what a page of text looks like from a distance. Below about half a pixel there is no room for a gap and the line is taken whole, with the two ramped between so the map does not change brightness as a growing buffer crosses that point.

- The column steps aside while a full-screen program runs, and the text gets its width back. Such a program draws on its own screen, which has no scroll buffer behind it, so the map would show a rectangle at the top of an otherwise empty column.

- Which programs are the exception is a setting rather than a rule, because there is no way to tell from the outside whether a full-screen program is one the map could usefully follow. By default it names a pager and the two multiplexers.

Interaction:

- The marker drags like a thumb and rides the scroll target, so it tracks the pointer exactly. A drag works out from where it grabbed rather than from where the marker was last drawn: the height floor means the drawn top is a rounded reading of the position, and reading it back would move the view on a press that never moved.

- A click elsewhere in the column centers the view there, eased the same way a scrollbar drag settles. The bottom of the map stands for the lines the trim is holding back as well as the last one it drew, so a click there means the newest output.

- The wheel over the column scrolls the buffer, same as over the text - including under an app that is tracking the mouse, since there is no cell under the pointer to report.

Alt screen:

- The column stays, so the PTY is not resized every time an app flips screens. The preview shows the screen itself, with no marker and no thumb - there is nothing to scroll.

Cost when off:

- Truly off: no column, no cache, no per-frame work. The whole feature hangs off one config check, and the cache is freed the moment the column goes away.

Settings and chrome:

- A "Minimap" toggle and a width slider on the Movement tab, under the scrollbar cluster, plus a View-menu item. The marker reuses the scrollbar's thumb color, which is why the scrollbar color rows sit with the palette rather than under the scrollbar switch.

How it is built:

- `minimap.rs` owns the line cache, the raster, the mapping and the hit tests. `pane.rs` carves the rect and routes events. Drawing is one textured quad per pane plus overlay quads for the marker and thumb.

- Each line rasterizes once into a fixed-width pixel row, at the first compose after it enters history, since history lines never change; the live screen rows re-raster at each compose. A build between composes only counts the new lines. It runs while holding the lock the PTY reader waits on, and under a flood most lines leave history before any compose would show them, so rasterizing them as they arrived cost about half the terminal's speed. A screen swap, a resize and a width change drop the cache. Sitting scrolled back with a full scrollback is the one case where nothing reports how many lines were pushed, so a changed newest-history line is taken as the sign the cache has fallen behind, and it rebuilds whole at a bounded rate.

- A compose reads at most about 4 ms worth of lines from the grid. A deep scrollback that was just rebuilt, or turned over by a flood, holds far more than that: reading 100,000 lines took 150 ms, and the terminal stood still for all of it. Past the limit, one line in every so many is drawn and stands in for the lines around it. Later composes spend what is left of the limit replacing the stand-ins, newest first, since the oldest are the first to leave. Once output stops, the map ends up the one every line makes. Until then a pixel row can show a neighbor's picture of a line, which under a flood nobody can tell.

- A compose that only replaced stand-ins redoes just the pixel rows they fall in, and the screen's. Each pixel row is worked out on its own, so that matches a whole compose.

- Redrawing the whole image is pixel work on the cached rows, and it needs no lock. Over a deep scrollback it is still most of a second at a million lines, so the rows go to a thread of their own for it, and the finished image is swapped in when they come back. The builds carry on counting meanwhile. A small one is cheaper to do in place.

- The composed image uploads as a texture the size of the column, so texture size limits and the GL context's VRAM-loss re-upload both stay non-issues.

- Under a flood every pixel of the map moves on every line, so a recompose is throttled rather than run per frame. A compose the throttle defers schedules a timed wake, not an animation flag - marking the window animating would bring it straight back, find the throttle still closed, and spin at the frame rate. A compose can owe the next one, and that wake is only known once its frame is drawn, so the event loop looks for it again after drawing.

- The throttle is at least 90 ms, and at least twenty times what the last ordinary compose took, the part on its own thread included. A whole redraw after a screen swap, a resize or a resync costs far more, and waiting twenty times that left the map still for seconds, so it does not count. A column that changes size composes at once instead of waiting the throttle out, so the image is never left at a size the column no longer has. A whole compose still grows with the scrollback, which can be a million lines, so a fixed interval could not bound its share at every depth. At the default depth under a flood, the map on costs about 6% of throughput.

- Memory is about 5 MB per pane at the default scrollback and a 120 px column, freed while the map is off.

- The marker and the scrollbar thumb say the same thing, so dragging either one pins both to the pointer. Off a drag they both ride the eased position and move with the content. Letting only the dragged one ride the pointer left the other trailing the ease the whole way down, which read as the second one lagging.

- A handle let go stays where it was dropped until the text arrives, rather than being handed straight back to the eased position. The text is still on its way at that moment, so handing it back sent the handle backward the way it came and then crawling forward again - a bounce, on the one gesture where the user has said exactly where they want to be. The hold ends when the two agree, or at once if anything else moves the target, since from there the handle belongs to the content again.

### Text readability scrim

A bg-colored backing behind glyphs so text stays legible over a busy background image or a near-transparent terminal. The scene's text is rendered to a coverage texture, turned into a halo, and composited under the crisp text, colored per-pixel so each glyph's backing takes its own cell's bg color. The outline is drawn in the same composite from the crisp coverage, so it works with the halo off, and the halo's blur is skipped then. The cursor is a separate coverage texture so it can join the halo and the outline as independent toggles.

The halo shape is selectable ("Scrim function"), because a plain Gaussian blur is a poor legibility backing. It is a round kernel, so as the radius grows the backing rounds off and the corners of a solid block recede. A square of text then reads as sitting on a separate round blob rather than an even plate. Four functions are offered:

- **Dilate**. The backing grows the same distance from every edge as a square (Chebyshev distance), so corners stay full. The most solid/boxy look.

- **SDF** (default). The backing grows by true round (Euclidean) distance with full corners: round like the old blur, but the corners no longer pull in. This is the described ideal.

- **DT** (distance transform). The same Euclidean distance rendered as a solid plate with a crisp feathered lip, rather than a soft glow. A highlighter-style backing.

- **Gaussian [ugly]**. The legacy separable blur, kept as a baseline to compare against.

The distance functions share one engine: a separable, exactly-Euclidean distance transform bounded to the halo radius. It takes a per-column 1D distance, then a row combine. That is cheap - two passes, no jump-flood - and reads either metric off the same field. Independently, a "Scrim falloff" curve shapes how the backing fades with distance: Sigmoid, Half-normal, Linear, Logarithmic, or Exponential. It applies both as the blur kernel weight and as the distance-path transfer. Falloff and function are orthogonal: the function decides the halo's shape, the falloff its fade. The falloff is named for the curve it draws rather than for a blur, since the same word otherwise names both a shape and a fade. A bell curve's outer half is a half-normal, and a smoothstep is a sigmoid. Every curve is normalized to reach zero at the halo's outer edge, so a halo ends where its radius says it does.

A third knob, "Strength", decides how bold the finished halo is: each 20% doubles its opacity, up to five doublings at 100%. Because the doubled value is clamped, the halo's core saturates first and the solid part grows outward along the falloff. So the backing thickens into a plate rather than merely brightening, and it still stops at the radius. At 0 the halo is exactly as the function and falloff built it. Light mode takes some of that back, for the reason below.

The shipped values are a radius of 8 px and a strength of 20%. Both were raised when the exponential falloff was made twice as steep, since a curve that drops away sooner has to start further out and heavier to finish in about the same place. The cheaper profiles keep the same share of the radius they always had, so they still look like the same halo built with fewer taps.

### Minimum contrast (2026-08-30)

Programs pick text colors for a terminal they cannot see. One that assumes a light background writes near-black text, and on a dark one it disappears. So a floor is enforced on how close text may come to the color behind it, and anything under it is moved away: lighter on a dark background, darker on a light one.

The comparison is against the cell's own background color, not against what a pixel behind the glyph actually shows. Per-pixel would mean the wallpaper, the blur, the scrim and the cell color all at once, in the shader, and it would give one word two colors across a gradient. The cell color is also the right answer in practice: a cell carrying its own background paints it solid, and one on the default background gets a scrim halo of exactly that color, with the wallpaper already pulled most of the way toward it.

Lightness is measured in Oklab rather than as a WCAG ratio. That ratio's constant term swamps the dark end, so two near-blacks score respectably while being invisible, which is the whole case this is for. The move changes Oklab L alone and leaves a and b, so hue and saturation survive and colors stay told apart: a lifted navy is still navy. It goes to whichever side the text is already on, unless that side has no room left before white or black, in which case it goes the other way. Pale text on a merely light background is the case that needs the flip.

The default floor is 45%, which puts previously invisible text at roughly 2.8:1 against a black background. Lower settings measure out as doing nothing visible at all. Two things are deliberately exempt. Text set to exactly its background color is left hidden, since that is how the hidden attribute works and how a program conceals a password. And ANSI black on a dark background is not exempt, even though it is invisible by definition - a program using it as a foreground has made the mistake this setting is for.

Every built-in theme's own foreground clears the floor on its own, which is checked at build time. A theme whose body text needed lifting would mean the floor was repainting the thing it is measured against.

The block cursor is a second background. It is drawn as a plate at 55% under the glyph, and the glyph keeps its own color, so the text on it has to clear the same floor against the plate as blended over the theme's background. That is checked for every built-in theme and mode the same way. A cursor at the text's own brightness fails it outright, which is what the monochrome themes shipped with. In a light theme the rule also sets how dark the text has to be: the plate sits between the text and the background, and a paler foreground leaves no room for one that both shows as a block and carries the text.

Over a light background the plate is drawn at 80% instead. A linear-light blend barely moves a light ground, so at 55% even a black cursor could not take the plate much more than 0.2 Oklab L off the paper. The stronger alpha goes only as far as the text on the plate still clears the floor, so a saved theme or an overridden cursor picked for the old plate keeps about the plate it had. The shipped light themes' text was darkened to make the room, and their plates now sit 0.25 to 0.28 off the background, against 0.20 to 0.42 in dark mode. Colors taken from the wallpaper follow the same rule, since the derived text is never paler than the theme's.

### Dark text on a light background (2026-09-21)

The color pipeline works in linear light, and glyph coverage is blended there too. A pixel the rasterizer says is half covered comes out at about three quarters brightness either way round. On a dark background that is a strong edge. On a light one it is barely a quarter of the ink the eye expects, so the thin parts of every stroke fade and a light theme reads a weight lighter than the same font in a dark one. Bold survives because most of its pixels are fully covered.

What the text should look like is settled first. Almost every other program blends text in sRGB, and that is the weight a font is drawn and hinted for, so the target is the pixel an sRGB blend of the pair would have produced.

The fix reaches it without blending there. glyphon's fragment shader is given the text color, its background and an amount, all in the params uniform, and it bends coverage so the finished pixel comes out on that target: blend the pair in sRGB at the reported coverage, decode, and read off how far between the two the answer sits. That fraction is the alpha. The output is still linear and the surface is still encoded exactly once, which is what the color pipeline contract is about - the rejected alternative was a second encode of the output inside the text pass, not arithmetic that reads the sRGB curve.

`text.dark_on_light` says how much of the correction to apply, defaulting to 1.0, and it is applied only where the text is darker than the background behind it. Light on dark is already heavy enough and correcting that side would thin it. The comparison is Oklab lightness, the same measure minimum contrast uses.

The setting runs to 2.0 rather than stopping at the blend. Everything up to 1.0 is a correction and 1.0 is the whole of it; above that is taste, and it is there because how heavy text ought to look is partly the display and the font. It fills the counters of small letters if pushed, which the config comment says.

The first version of this was a coverage exponent, and the exponent was the wrong curve rather than the wrong number. Matching an sRGB blend needs roughly 0.53 at a quarter coverage, 0.35 at a half and 0.18 at three quarters, so no single value fits: one that filled the stems smudged the faint edge pixels, and one that left the edges alone left the stems pale. Measured on the shipped light theme, the exponent it shipped with was about 20 sRGB levels light on a three-quarter covered pixel. The correction here has no such number in it - at full amount every pixel carries the ink the rasterizer reported.

One alpha has to serve all three channels, so the pair reaches the shader as sRGB grays of its own brightness. A glyph in some other color - an ANSI red, say - takes the same curve, which measures up to about 20 levels off on its partly covered pixels, always toward more ink. Correcting per glyph would mean encoding each glyph color in the shader for a difference smaller than the one being fixed.

The pair is decided once per render pass, not per glyph, because one pass draws the whole window and a uniform is what the shader can read. The main window's pass carries the terminal's own pair, so in a light theme the menu and tab labels - which stay on dark chrome in both modes - are corrected along with everything else, in the wrong direction. That is accepted: it is a small strip, and giving the chrome its own pass would cost a second renderer and a second atlas to fix a few hundred pixels. The Settings dialog is a separate context and decides on its own panel colors.

### How much of the wallpaper is on screen (2026-09-20)

The visibility slider is an authored amount - a person moved it - and what the renderer wants is a linear-light alpha. Those are not the same thing, in two separate ways, and both used to show.

The first is the mode. sRGB's curve is steep near black and flat near white, so the same alpha covers a lot of visible ground over a dark background and almost none over a light one. Measured at the shipped 10%, a picture's own contrast came out at 6.4 sRGB levels of spread in dark mode and 0.6 in light: the same setting, and the picture was simply gone.

What the slider means is settled first: **this much of the picture's own contrast reaches the screen**. Over a black background a linear blend delivers exactly that, because black leaves the blend a pure scale of the encoded picture and a scale cannot touch contrast. That is why dark mode has never needed any of this, and the closed form `alpha^(1/2.4)` says how much it delivers. A dark theme whose background is not black delivers less, and the same expression says how much less.

Light mode cannot deliver it with a blend at all, so it does not use one. It mixes the background and the picture in a power curve at the amount dark mode's blend would have delivered. The curve is a pure power rather than sRGB's own, because sRGB's `- 0.055` term does not cancel: over black the power curve makes the mix exactly the linear blend it replaces, so the two modes are one rule with dark mode as its black-background case, and sRGB's would have lifted dark mode's black by eight levels. Measured after the change, light mode's spread was 6.4 against dark mode's 6.4, and at half visibility 12.5 against 12.5.

A mix needs the background color, which a hardware blend cannot supply, so the wallpaper pass writes the pane fill itself and is clipped to the pane. The divider slits between panes keep their own color rather than taking a faint tint from the picture, which is the one thing that changes there.

The second is the picture. At one setting a bright photo glares where a dark one is barely there, because the slider says how much of the picture to mix in rather than how far to move the background. `wallpaper.even_visibility` holds every picture to the same displacement: one further from the background than the shipped pack's median is drawn at less than the number says, and one closer at more. How bright a picture reads is its overall level and its bright end together, half each, because glare comes from the bright end - a night sky with a sun in it is not a dark picture to look at. The correction fades out as the slider rises and is gone at 100%, since that is where the picture has to be drawn as it is.

It reaches dark mode too, which is the point of it: at a 10% slider the pack's brightest picture went from a mean of 58.8 to 48.3 and its darkest from 0.8 to 2.6. In light mode the rule reads from the other side, because there it is the dark picture that stands out: the same two went from 26 and 93 sRGB levels of displacement to 63 and 52. Setting it to 0 restores the old behavior exactly, which the rig confirms pixel for pixel.

The scrim's halo is the one thing here still calibrated by measurement rather than derived. It has the same asymmetry pointing the other way - in light mode it is a pale plate on a darkened field, which is the same move in the direction the eye notices most - but that composite blends against the destination through the pipeline's blend state and cannot read it, so there is nothing to solve against. Its alpha is scaled down until it covers the same ground dark mode's does, which works out at about a doubling and a half whatever the visibility is set to, and it stops at a quarter of what was asked for so the plate cannot stop doing its job. That moved 43% of the pixels around a screenful of text by an average of 10 sRGB levels, and the text still read clearly.

Everything here measures with a transfer curve taken on Rec.709 luma. Luma because a linear-light alpha blend is affine in it, so one number stands in for a whole composite. A curve because linear light is not what the eye reads; the sRGB transfer tracks CIE L* closely enough for the scrim, and the pure power is what makes the mix exact. Oklab lightness was measured and rejected: it has no linear toe, so it reads a near-black background as far more separable than it is.

The pipeline contract holds throughout. The wallpaper pass still emits linear, and only the mix inside it happens in a curve. Nothing downstream - the scrim, the text, the cursor, and whatever the GPU effects epic adds - sees anything but linear light.

### Text colors from the wallpaper (2026-09-20)

A switch on the Themes tab that takes the text and cursor colors from the picture behind them instead of from the theme. On by default, since the wallpaper is on by default too and the derived text is never dimmer than the theme's own - so it can only help, and with no picture up it does nothing at all. While it is on, the Foreground and Cursor rows gray out, and nothing derived this way is written to the file - a rotation would otherwise rewrite the config every few minutes, and the colors would outlive the picture they came from.

Two halves, decided separately. Harmony and legibility are unrelated problems, and one number cannot answer both: a complement at the same lightness as its ground is the least readable pairing there is, which is where the shimmer at the edge of vivid opposites comes from.

- **Lightness** comes from how bright the background actually gets. This is the half that does the work, and the obvious approach is the wrong one: averaging the image says nothing useful, because a photo's brightness varies from cell to cell and text readable over a dark sky vanishes into a cloud. The text is placed the contrast floor away from the field's bright end - the 95th percentile of what the cells behind it are, once the picture has been composited over the theme's background at its visibility setting.

- **Hue** comes from the picture's own dominant hue, turned to its complement and held to a gentle tint. A mean color cannot supply it either, and for a different reason: opposite hues cancel, so a picture full of color averages to gray and the hue of that gray is noise. Of the 104 shipped wallpapers, 17 average to something that faint, and one of them reads 174 degrees away from the hue that is all over it. The hue is taken from a chroma-weighted histogram instead, the way a picture's color is normally found.

- **The cursor** takes a further third of the circle, which is where every built-in theme's cursor already sits against its foreground. Its plate is a second background the text has to clear the floor on, so the plate is placed exactly the floor away from the text - the furthest it can get from the picture while still carrying a glyph - and the cursor that draws it is found from there.

Three limits, said here rather than left to be discovered.

- **It cannot guarantee the floor, and does not pretend to.** Measured over the shipped pack, one foreground clears a 45% gap on every image at the shipped 10% visibility, on about two thirds at 35%, and on a fifth at 100%. Past that no color exists: the picture's own bright end is already inside the floor of white. The derived color takes the best position available and the text scrim covers the rest, which is the job the scrim already had. Nothing else is switched on behind the user's back to make up the difference.

- **A dark picture never dims the text.** The theme says how bright its text should be and the picture may only ask for more. Without that floor a near-black wallpaper answers mid-gray text, which reads as the wallpaper spoiling the theme rather than serving it.

- **The theme still decides which side the text sits on.** A light theme keeps dark text however dark the picture is. Flipping polarity from a photograph would stop it being the theme that was chosen.

The chroma is capped low for every theme, which is the one place a theme's own identity is deliberately overridden. Carrying a monochrome theme's saturation to a complementary hue turns Matrix's green into flat yellow - the cast is decoration and the lightness is the legibility, so the cast is what gives way. A picture with almost no color in it keeps the theme's own hue instead, since there is nothing there to complement.

The grayed rows show the user's own colors, not the derived pair. A row a performance profile governs shows the profile's value, because there is no other way to see it; a color is different, since it is on screen behind the dialog. So the rows say what comes back when the switch goes off, and the live copy's derived pair never reaches the dialog, the file, or a saved theme.

The work splits across two threads. The wallpaper worker already holds the finished pixels, so it reduces the picture to six numbers there. The colors themselves are worked out from those numbers wherever the live settings are, which is what lets a theme change re-color the text with no second decode. Luma is what gets summarized rather than lightness, because luma survives being composited over a background color later and lightness does not.

### Performance profiles (2026-09-03)

One setting decides how much the look may cost, so a slow machine is a choice on one tab rather than a dozen switches on four.

- Five profiles, in the order they cost: Custom, Max silk, High, Low, Standard terminal. Max silk is every effect at its shipped setting. High shortens the ease-in, ease-out and single-screen stretches of a scroll and gives the text halo a cheaper shape with a shorter reach. Low also drops the halo and the cursor animation, and leans on the outline instead; it keeps the wallpaper, which is decoded once and costs nothing per frame, and smooth scrolling. Standard terminal is a plain terminal: no smooth scrolling, no wallpaper, no halo, no outline, no animation. No profile draws an outline over a pixel wide; Low once used two, which read as a heavy stroke rather than as the thin edge the outline is for.

- A sixth, Remote (temporary), is Standard terminal under another name and is never written to the file. It is put on for a remote screen at launch and taken off again at the next launch unless that one is remote too. It can also be switched by hand, from the Profile dropdown or from "Temporary remote display mode" on the View menu, and either way it lasts the session. The stored profile waits underneath it.

- A profile sits on top of the stored settings rather than in them. The file and the dialog keep the user's own values. When settings go live the profile overwrites the fields it governs and keeps the originals beside them, and every write path puts them back before anything reaches the file. Choosing Custom is a profile that governs nothing, so it restores everything.

- In the dialog the governed rows show the profile's values rather than the user's own, and their flyover says so. This is display only, so Apply writes the user's values underneath.

- It leads the Silk tab, first in the dialog, with text readability and the scrolling feel under it. Those are the two sections it governs most of, so the switch and its effects are on one screen. Wallpaper and cursor rows stay on their own tabs and are grayed there. That still makes eight tabs, one past the guide's ceiling.

- Automatic is the default, and the first pick is measured rather than guessed. Naming the adapter was not enough: an integrated chip is not a slow one, so it started at Max silk and stayed there.
	- A run the display stalls gives no rating (2026-09-18). A monitor asleep paces every frame at about one a second, whatever is drawn. The run read that as a hopeless machine and saved Standard terminal, which has no wallpaper, for every launch after. Simply not saving on a stall would test a truly slow machine at every launch. So when a rung runs more than four times over its budget, Standard is timed once: a slow machine draws that well enough and gets Standard, and if Standard stalls as well, the display is what is pacing the frames. Then nothing is saved, the session goes back to the profile it had, the banner says the display was not drawing at its usual rate, and the next launch tests again. The display power state is not read, for the reason given under the watch below.

- A governed row still takes input, and changing one takes the profile to Custom (2026-09-20). The rows used to be grayed, which meant every tweak started with a trip to the Silk tab to find a dropdown, and the flyover could only say where that tab was. Changing a setting is a clear enough statement that the profile is no longer wanted, so it is read as one: the values on screen become the user's own, the profile becomes Custom and "Choose automatically" goes off. The values on screen are what is kept, not the older ones the profile had been hiding, because the edit was made against what could be seen. Picking Custom from the dropdown is still the other way in, and that one does bring the older values back - the difference is that a pick says "my settings" and an edit says "this, but with that changed". While a profile is showing, a governed row offers no revert arrow, since what it shows is not a value the user set. Remote (temporary) is no exception here: it governs, so an edit under it drops the session override and the stored profile with it, or the new value would be covered up by one or the other.

- The Profile dropdown stays live while automatic is on, and naming one switches automatic off (2026-09-19). It used to be grayed, so taking the machine's choice back meant finding the switch first and then the dropdown, in that order, with the dropdown showing the answer being argued with the whole time. Naming a profile is the clearest statement there is that the choice is no longer the machine's, so it is read as one. Without that, a pick made with automatic still on would be overwritten at the next launch with nothing on screen to say why. Remote (temporary) is the exception: it lasts this session only and says nothing about what the machine should settle on, so it leaves the switch as it was.

- What the machine is gets hashed - the processor and its usable core count, the graphics adapter, and installed memory to the nearest GiB - and that hash is what the profile is written down against. The parts that need no adapter are read on a worker at launch, since nothing before the first frame wants them. A different hash is a different machine and gets rated again; the same hash leaves the profile where it was left. "Check for hardware change" under Performance switches the check off for a machine already rated, and "Re-test next run" under it asks for one more rating regardless, then clears itself once that launch has started one.
	- The rating is written into the settings file line by line, so a file with a line that cannot be read still keeps it, and the rest of the file is left as it was. A file that reads clean but has nowhere a line can go, such as one with no Performance section, gets what a save from the Settings dialog would write. Either way the write is refused if any other setting would load differently at the next launch. That is judged on the text the next launch would leave, after every rewrite it makes before it reads the file: the wallpaper heading repair, the conversion of a file from before the nested layout, the move of `shell.default` into the shell list, and the renames and refreshes (2026-09-18). Two of those once looked at how a line was written as well as at its value, the quotes around an old default font list and the indent of a commented-out heading above a renamed setting, and a save changes both. Judging only the renames missed the conversion, which copies a font list with its quotes for the refresh after it to read. Adding missing settings is left out, since it only adds lines the program owns and runs whether or not a rating was written.
	- Those steps read no quotes or indents now (the saving contract under Configuration format), so nothing known can make the check refuse. It stays for the next step that does, and a test hands it one.
	- A rating is also refused where the next launch would not keep it. That launch writes a file from before the nested layout afresh and carries no rating over, so the rating goes in one launch later.
	- When the rating cannot be kept (the file is open in another program, or cannot be written), the banner says so before it comes down, and the test runs again at the next launch.
	- A build from before 2026-09-10 and a later one, launched in turn on one settings file, test at every switch, since the rating version is part of the hash.
	- Rejected: a separate rating record in the data directory. It is a second copy of state the settings file already holds, and a hand-cleared `rated_hardware` would stop forcing a new rating, which the template comment promises.
	- Rejected: retrying a write the busy check deferred. It costs a scan of every process's open files on a timer, for a case the banner now explains.

- A remote screen is not rated and nothing is written for it. Every frame is encoded and shipped over a network, so the graphics card says nothing about what the person sees, and a benchmark on it would only flatter the machine; the session runs under the Remote profile instead and the console's rating stays as it was. An adapter with no card behind it goes to Low, untimed, decided before anything renders.

- Anything else is timed. The window comes up whole, the wallpaper appears, and then a banner takes the window while three rungs are measured in turn: Max silk, High, Low, each put live and given up to about a second of full-rate frames. The first whose median frame period fits the display's refresh budget is the answer. The window keeps drawing underneath the banner, dimmed, because what is being timed is worth seeing; it takes no input, because a keystroke would change the measurement. Standard terminal is never timed - it is what is left when Low misses. A rung several times past the budget ends the run outright, since no profile below it changes the per-pixel work by that much, and that is also the case that would otherwise take longest to measure.

- The display is still watched afterwards. When the median frame over a window of eased frames runs half again past the refresh period, the profile steps down one rung until SilkTerm restarts. The refresh period is the monitor's the window is on now, read again a few times a second, so a window dragged to another monitor is judged by that one. Frames paced under the old one are dropped rather than counted. Nothing is written, and the next launch starts from the rated profile again. The watch stops at Low. Low keeps the wallpaper, which costs nothing per frame, and Standard terminal turns off the eased frames being measured, so a step there could never be checked again. Only a window with focus is counted. A frame several times past the budget is not counted at all, because a monitor asleep under the NVIDIA driver paces a GL client at 1 fps, and an idle gap is not a frame either. Eases more than 30 seconds apart start a new window, so a verdict comes from one sitting. It never steps back up within a session, because a lighter profile renders less, so a fast run under it says nothing about the heavier one. A hand pick or a measured rating lifts the step, and a hand pick with automatic off stays put.
	- This reverses the earlier rule that the step was written down. A written step made one window's misreading every later window's setting, with no way back while automatic was on. It took a 60 Hz desktop with a discrete card down to Standard terminal overnight, and the wallpaper with it.
	- Ratings written before this change are redone once, since a written step cannot be told apart from a measured answer. The rating version is part of the hardware hash. A machine with "Check for hardware change" off keeps what it has until "Re-test next run" is used.
	- Rejected: write the step and ask for a rating at the next launch. That is a banner after every hiccup.
	- Rejected: write it but stop at Low. One stall would still change every later window.
	- Rejected: read the X11 display power state. It is platform-specific, and it misses every other kind of stall, such as a suspend in mid-ease or a card taken by another program.

- Blur quality is not part of a profile yet. The backlog item for it stands on its own, and a profile could drive it later.

### Font fallback stack

One monospace family is pinned for every weight, because the shaper picks the best face per query and would otherwise let a bold run end up in a different family than the regular run beside it.

Which family that is comes from a single search order, the same on every platform:

- the OS monospace family, when "use system font" is on

- then the configured `font_family`, a comma-separated stack

- then the OS monospace family, when "use system font" is off

- then a built-in stack, which is also what a fresh config is written with

- then, only if none of the above is installed, whatever the generic monospace name resolves to

The setting only reorders that list; it never truncates it. An earlier version dropped `font_family` entirely while following the OS font. The same build and the same config then resolved differently depending on the platform, and a configured stack could be silently ignored. Every list is now always walked. A family that is not installed simply falls through to the next one, and the configured stack still has effect as a fallback.

Platforms differ only in what they report, not in the rules applied to it. Windows has a system font size but no monospace family, so following the family there is a no-op and resolution starts at `font_family` without a special case. A toggle with nothing behind it reads as inert, so the Settings checkbox grays out and says why. The same holds for a desktop with no font setting configured at all, which is why the check asks what was detected rather than which platform is running.

On Linux the desktop's own settings store is asked first and the other one fills in: xfconf on Xfce, gsettings elsewhere. gsettings answers on any box with GNOME's schemas installed, an Xfce box included, and a key nobody set comes back as the schema default, so asking it first on Xfce gave Cantarell 11 and Monospace 11 whatever the desktop was set to.

The built-in stack is last for a reason. The generic monospace query below it is effectively a lottery over installed fonts, and its winner may ship no bold face. That ejects bold runs into an arbitrary, often proportional, fallback whose advances can't be snapped to the cell grid. Every entry in the built-in stack carries a real bold face. When that stack changes, the outgoing value is recorded, so an existing config still carrying it verbatim is refreshed on the next launch. A stack the user edited is theirs and is left alone.

### Hyperlinks

- URLs in the output are clickable. A link must carry a scheme from a fixed list - http, https, ftp, ftps, sftp, ssh, file, mailto - rather than being guessed from shape. That keeps false positives near zero, since output is full of words with colons and slashes in them. It is also the whole of the security story: a scheme outside the list is not a link, so it can never be handed to the desktop. Bare `www.` prefixes and bare file paths were considered and left out for the same reason.

- Punctuation is trimmed the way a reader would. A full stop or comma after a URL belongs to the sentence, and so does a closing bracket the URL is sitting inside. One the URL itself opened is part of it. A URL that wraps across rows is one link, found from either half.

- Hovering underlines, Ctrl+click opens. The underline appears on a plain hover with no modifier, since a link the user cannot see is a link they will not try. Opening needs Ctrl so it can never be confused with selecting. The press arms and the release opens, so a slipped press can be dragged off to cancel. A right-click on a link puts "Open link" and "Copy link" at the top of the menu, and only there.

- An app that is watching the mouse itself owns the pointer, so nothing underlines over it - holding Shift asks for the local behavior instead, the same bypass selection already uses. The right-click menu continues to win over such an app, as all our chrome does.

- Links open through the desktop's own handler by default, with a configurable program to override it. Deciding what a URL means is the desktop's job, not a terminal's.

### What a double-click grabs (2026-08-26)

- A double-click asks three questions in order, and the first one that answers wins: is this a shape we can name, is it inside a matched pair, is it a word. Word selection was the only rule for a long time and it cannot handle a path with a space in it, because a space is what ends a word.

- The shapes are URLs and file URIs, drive paths (`C:\...`), UNC paths, absolute posix paths, and `~/` paths. Each has to start at an anchor a reader would recognize, with only whitespace, a quote or an opening bracket in front of it. Among the options considered, that was preferred over "anything that is not obviously a word", which reads `and/or` as a path.

- Git remotes and scp targets are shapes as well, in the `[user@]host:path` form. This one was added because a git prompt writes the remote inside brackets beside its status marks, and the matched-pair rule then handed back the marks along with it. Narrowing the pair rule was considered and rejected, since selecting a quoted phrase whole is wanted and was asked for separately. A host needs a dot and an alphabetic ending, and the path needs a separator and a letter in its first segment, which is what keeps `build:release/x` and `notes.txt:12/34` out.

- A remote is the one shape a file extension does not end. A prompt writes the branch after the repository as `repo.git:dev`, and that whole field is what a reader sees as one thing, so stopping at `.git` would leave the branch as a dead patch that selects the brackets instead.

- Where a path ends is two heuristics, both picked for what they refuse. A space is crossed only when a path separator turns up soon after, so a folder name with spaces stays whole while a path followed by a sentence does not swallow it. And the run stops at a file extension, which is what leaves a `:120:5` line number behind.

- A trailing full stop, comma or bracket comes off the same way it does for a link. The two share the trimming idea but not the code, since a path may hold characters a URL may not.

### Selecting past the edge of the screen (2026-09-20)

- A drag held past the top or bottom of its pane scrolls the view that way and keeps selecting, so a selection can run further than what fits on screen. A pointer outside the pane is pulled to the nearest edge cell rather than ignored, which also means a drag that strays into a neighboring pane still belongs to the one it started in.

- The speed is the larger of two answers: how far past the edge the pointer is, and how long it has been held there. Distance alone is the obvious rule and it is the one that feels right, but a maximized window has its top edge against the top of the screen, so there is nowhere left to push the pointer - that window could only ever creep. The hold reaches the same top speed in two seconds.

- It creeps rather than standing still right at the edge, since picking up one more line is the common case and a drag that starts fast overshoots it. The top speed is capped: a pointer flung off the screen should not cross the whole buffer before the button comes up.

### Measurements and display scaling

- Every measurement in the interface is written once, in device-independent pixels, and turned into real ones only when it is drawn. A DIP is a ninety-sixth of an inch, so a border, a gap or a checkbox is the same physical size on any screen. Nothing is written in raw pixels any more - the terminal grid itself is the only thing sized in them, and that follows the font.

- Where the conversion happens differs by surface, and the difference is deliberate.

	- A pop-out dialog is solved end to end in DIP and converts once, where its layout meets its window. It owns its whole coordinate space, so one boundary is enough and a stray conversion inside would scale something twice.

	- The main window's chrome converts at each measurement instead. Menu bar, tab bar, menus, focus ring and pane gap all share a coordinate space with the terminal grid, which is in real pixels by nature, so there is no boundary to put a conversion on.

- A dialog already open follows a scale change in place, rather than being rebuilt (2026-09-19). Dragging it to a monitor at another scale, or changing the desktop's scaling under it, moves only the boundary: the text context rasterizes at the new size and the chrome is measured again, and the layout below is already in DIP, so it is the same size on screen with the clicks where they look. A rebuild would have been a few lines, since reopening was the one thing that worked before, but what a reopen carries is the tab and the scroll - so every unapplied edit would have gone the moment the window crossed a monitor edge, which is worse than the wrong size. The size kept for the rest of the session is in DIP for the same reason, so a reopen on another monitor is the same apparent size and not the same count of pixels. About and the notice follow one too (2026-09-20), by a different route: neither can be resized and neither holds a layout to adjust, so each keeps what it was built from and is laid out again from scratch at the new scale.

- Neither half of the boundary moves on its own, so a scale change sets both (2026-09-20). The window toolkit keeps the logical size, which for an ordinary window means the new physical size arrives straight after - but a maximized or tiled window keeps its physical size and sends nothing at all, and the dialog would then draw at the new scale inside the size it had before. So the dialog is told what the window really measures rather than waiting to be told, and the window is asked for that size held to what the screen can still hold, since a screen holds fewer DIP at a higher scale.

- **A measurement TAKEN in real pixels must convert the constant beside it, not the other way about.** Text is measured against the font, which is real pixels by nature; the clear space that goes around it is written in DIP. Adding the two as they stand and dividing the sum at the dialog's boundary shrinks the constant by the scale factor - so at 2x a tab's title had half the clear space its own box allowed for and sat flush against the right edge, and above that it ran past it. Every such site converts the constant where it is used, exactly as the main window's chrome does. There is one rule for it, so the four places that size the dialog's columns cannot drift apart.

- Conversion rounds to whole pixels. A rule or a hairline that fell between two of them would come out soft, and the one-pixel gap between panes is the extreme case: on a screen scaled below 1x, rounding alone would take it to nothing, so a measurement asked to be visible never rounds away.

- A raw-pixel measurement is invisible at 1x and only thins out as the scale factor rises, which makes this the kind of mistake nobody sees on the machine they wrote it on. So the scale factor can be overridden from the environment (`SILK_SCALE`), and a high-DPI layout can be looked at on an ordinary display. Off X11 there is no other way to ask for one.

### Attention colors and dialog chrome

- A theme carries two attention colors rather than one, because they answer different questions. **Highlights** marks several things at once: the live pane's ring, slider handles, revert arrows, the default button. It therefore stays calm enough to appear many times on a screen. **Focus** marks the single control the keyboard is on, so it is the more vivid of the pair and sits well away from its partner in hue. Every theme keeps its two well apart, because a theme that let them converge would draw "look at this" and "you are here" in the same color.

- The dialog's own accents follow the theme. They used to be a fixed blue while the theme's attention color was something else entirely, so the panel could not agree with the terminal it belonged to. The pressed-button fill is that color mixed back toward the panel, which is what makes a pressed button read as pressed rather than as the focused one.

- A focused field shows one outline, not two. The ring sits exactly on the box's own outline and the box stands its border down. Where the focused thing is not a box at all, such as a checkbox or a slider handle, the ring sits a little outside it instead.

- Tabs sit on a recessed **Gutter** strip and stand on the rule that closes it off, the way tabbed interfaces generally read. The current tab is a lighter gray rather than an accent: "you are here" is not the same job as "look at this". Above the rows there is no heading repeating the tab's own name, since the strip has said it already.

- Controls whose label does not explain them carry a line of flyover help. One that is grayed out explains why instead, that being the more urgent question at the time. The text wraps to the panel rather than being clamped to its edge, so neither a longer sentence nor a larger interface font can push it out of view.

### Groups and sub-groups in the Settings dialog

- Settings are organized two ways. A **group** is a titled section with a rule under it and clear space above. A **sub-group** has no title of its own. It is a control followed by the controls that depend on it, whose labels step right so the run reads as belonging to the leader. A master switch and the things it governs is the shape this exists for.

- Only labels move. Every control keeps the one column it shares with every other row, because a settings list is scanned down that column and a control that wandered with its label would break it. A sub-group is therefore free of any bookkeeping. It is read off the indentation rather than declared a second time, so the leader and its members cannot disagree about who belongs to what.

- A fraction stored as a decimal is shown as a whole percent. Nobody thinks in 0.35, and the file is a different audience from the dialog. The decimal is what the renderer wants and what a hand-edited config should keep. The two directions are exact inverses, so reverting one gives back its own default rather than a hair off it.

- The tabs follow what a person is looking at rather than what the code calls it: Silk, Background, Text, Cursor, Movement, Themes, Window, Shell. Settings that describe one subject sit together even when they are implemented in different places. The cursor's shape, its animation and whether it joins the text halo are all "cursor" to the person changing them.

### The color picker (2026-09-20)

- A color chip opens a picker: a saturation and brightness square, a hue strip beside it, six value boxes, and Cancel and OK. The hex field on the row stays where it is. Typing a known hex is faster than hunting for it, and a picker is for the case where the value is not known yet.

- The box holds the color as hue, saturation and brightness rather than as the three bytes. Dragging to the bottom of the square leaves black, which says nothing about hue, and dragging to the left edge leaves a gray, which says nothing about saturation either. Reading the model back off the bytes each frame would send both markers home the moment the color reached an edge. The bytes are derived from the model, and the trip back the other way is exact for every color, so nothing drifts on the way in.

- Changes go straight to the row behind the box, and Cancel puts back what the row held when it opened. That makes the chip, and the window under the dialog, the preview: there is no second copy of the value to get out of step, and the one thing to undo is one assignment.

- The square and the strip are drawn by the renderer's own quad shader, as two modes of it. A gradient built from flat quads would be thousands of them for one square, and a picker is not worth a texture upload per hue.

- The square mixes toward the hue in sRGB, not in linear light. Every other color in the program is handed to the GPU linear, and mixing toward white there gives a square nobody would recognize as a color picker: the pale half swamps everything else. So the square's quad carries its hue in sRGB and the shader encodes the result itself. That is the one exception, and it is written down where the quad is declared.

- Six value boxes: red, green and blue as whole percents, then brightness, saturation and a hex value. No hue box. The strip is the hue control, and the other five can already name any color between them. Percents rather than 0 to 255 because every other fraction in the dialog shows as a whole percent, and a settings dialog should not switch units halfway down.

- The chip is a focus stop of its own, so a Color row has two: the chip, then the hex field. Walking onto the chip opens nothing, and Space or Enter opens the picker. Without that the picker would be the one thing in the dialog a keyboard could not reach.

- Inside the box the arrows adjust whatever holds focus: the square by a hundredth of its range, the strip by a hundredth of a turn, a value box by a hundredth of its own range. That is the same step every number box in the dialog already takes.

### Saved themes

- A theme the user saves is stored **whole**: both variants, the ten palette colors and the sixteen ANSI colors, rather than as a base theme plus the differences.

	- Saving, renaming and deleting all become one operation on one config subtree.

	- A stale color cannot survive under a name that no longer sets it.

	- A saved theme is self-contained enough to hand to someone else.

- What identifies a saved theme in the file is a slug that never changes, with the display name stored beside it. A rename therefore rewrites one line instead of moving a subtree, and the `theme` setting keeps holding a name a person would recognize.

- **Nothing records "this theme has unsaved changes".** A per-color override that disagrees with the theme is that record, and it already lives in the config file. So the Save button is right after a restart, with no flag to keep in step. Saving folds the overrides into the theme and drops them, which is also what makes the button go quiet again.

- While an override is in place the Theme dropdown says `[unsaved]` rather than naming a palette the colors have moved away from. It is display only, derived from the same test the Save button uses, so nothing extra is stored. The list underneath is unchanged and still highlights the theme the edits started from, which is both how to see what they started from and how to discard them: pick it again and its colors come back.

- A saved theme may take a built-in's name and stand in for it. That gives "customize a built-in" an obvious home, and deleting the saved copy puts the built-in back rather than leaving the name pointing at nothing. Built-ins themselves cannot be renamed or deleted.

- Picking a theme takes on its colors wholesale rather than keeping the previous theme's tweaks on top. A picker that visibly changed nothing on every color that had been edited would read as broken, and those tweaks belonged to the theme being left behind.

### The shell list and how it is filled

- The shells a new tab can be started with are one list, stored in the config as `shells.<key>` with a title, a command, an active flag, a comment, and the date a scan last found the program installed. File order is list order, which is also menu order. It is a plain part of the config, so it can be hand-edited, and the Settings dialog's "Shell" tab is an editor for something that already works - the list came first on purpose.

- **The list names the default shell: its first switched-on entry.** There was a separate `shell.default` setting saying the same thing, and two places claiming to name one shell can only ever disagree; one rule that is visible in the list is worth more than a second field. A config that carried the old setting has that entry moved to the top of the list, once, and the line removed - the value was the user's own statement of which shell they meant, so it is carried rather than dropped. Finding which entry it names is the same identity question the scan asks, not a string compare: the old setting was routinely a bare name where the scan had already stored the full path to the same file, and comparing the two as text put a SECOND copy of the user's default shell at the top of their own list, where the top is what "default shell" now means. An initial population is led by the shell the user logs in with, which is what makes the default right without their having said anything.

- Finding installed shells is background work that starts a few seconds after the wallpaper is really on screen - not merely the window. Both are off-thread and both are slow in the same way, so overlapping them puts a stall in the one moment anybody is looking: the gap between the window appearing and the picture arriving in it. A wallpaper that never answers (a share on a dead mount) cannot hold the scan off forever - there is a deadline past which it runs anyway, since a terminal with no shells in its menu is worse than a terminal with no picture behind its text. It stats every directory on PATH and, on Windows, reads the registry - any of which can be a mount or a hive that answers slowly - so none of it may sit between launch and the first frame. It runs on its own thread and the result is folded in when it arrives, the same shape the wallpaper pipeline uses.

- **What a scan may do to the list is deliberately lopsided.** It may add a shell it found, and it may switch off one whose program has gone - keeping the entry, its title, its flags and its place, since a shell that is merely uninstalled is not a shell the user stopped wanting. It may never switch one back on, and never rewrite a command line. A scan cannot tell a program that came back from a switch somebody turned off on purpose, and the cost of guessing wrong runs one way: quietly re-enabling something the user disabled is worse than leaving them one tick to undo.

- **A save of the list works from the file, not from what the window loaded.** Each window reads the list when it starts, and nothing watches the file, so another window may have saved since. A window that was behind used to take out only the entries it knew about and write its own list back after them. A shell another window had just found stayed on top of the whole list and became the default shell. Now the file is the third side of every save. What another window added stays, at the end, and what it removed stays gone. An entry this window did not change keeps the file's copy. The window's own order is written only when it moved something itself.

- Two shells count as one entry when they run the same program with the same arguments. Which program that is has to be resolved rather than compared as text, because the same shell is written several ways (`bash`, `/bin/bash`) and, on Windows, three environments ship a program called `bash` and they are not the same shell. Where a stored entry resolves nowhere at all, a bare name match is enough - that is what lets a reinstall re-arm the disabled entry it belongs to instead of sitting beside it as a second copy.

- **The order a fresh list arrives in is stated outright, in one place, rather than falling out of the sequence the looking happens to run in.** Each find is put in a group and the whole set is sorted once at the end. On unix the user's own login shell leads - nothing may sort above it, since the top of the list is what "default shell" means - then the modern cross-platform shells, the language REPLs, and the rest of the POSIX family. Windows has no user shell, so it is stated instead: PowerShell 7, the modern shells, the WSL distributions, the three POSIX-environment bashes, PyCmd, the language REPLs, Windows Cmd, and last the two Windows PowerShell 5 entries - the ones you reach for when something needs them rather than the one you open a terminal to get. Groups that hold shells of equal standing sort alphabetically inside themselves; groups that hold one shell built several ways keep a curated order, which is why MSYS2's full bash is offered above the mini one Git comes with.

- The login shell's twin sits directly below it and starts without reading its startup files. Each shell spells that its own way (`--norc`, `--no-rcs`, `--no-config`, `-NoProfile`), so the flag is per shell and the twin only exists where there is one. Only the login shell gets a twin; every shell having one would double a list nobody asked to be long. It arrives switched OFF: it is what you reach for when your own rc file is the thing you are debugging, not a second copy of your shell in the menu every day. `cmd.exe` is deliberately not on that table even though it has such a flag (`/d`, no AutoRun): it is what Windows reports as the command processor, so it is the login shell on every Windows box, and a second "Command Prompt" in everyone's menu costs more than a rarely-set AutoRun key is worth.

- WSL distributions are read from the registry, never by asking `wsl.exe`. A WSL2 distribution lives in a virtual disk, and listing what is installed must not be the thing that boots a virtual machine - that would be slow, surprising, and arguably a security problem for the user. Each distribution is offered whole, running its own default shell; anyone who wants a particular shell inside one edits the entry to say so. Its generation is part of its name (`WSL2; Ubuntu`), because that is the whole difference between two rows that would otherwise read identically, and the WSL2 ones are offered above the WSL1 ones. Both are offered where both exist: a WSL1 distribution is installed and usable, and hiding one because a newer-generation one sits beside it is not a call a scan gets to make. The generation is a bit in the distribution's registry flags - the `Version` value beside it is the registration format's version and reads 2 for a WSL1 distribution just as happily.

- **The Shell tab is the one place allowed to write the list, and a scan that arrives while it is open is folded into it rather than fought with.** Everywhere else the dialog carries the live list through untouched on Apply: a dialog that opened before a scan arrived would otherwise write back the empty list it copied then, emptying the menu for the rest of the session while the file on disk still had every shell in it. The tab needs to write it, so instead the scan is folded into both of the dialog's copies - the edited one, so the user sees what turned up, and the baseline, so the fold does not read as an edit they made. Because a scan only ever appends and switches off, folding it into work already done cannot undo any of it.

- The grid edits every field in the row rather than through a popup: it costs fewer clicks and reuses the field machinery the dialog already has. Four columns are fixed-width and the command takes whatever width is left, since it is the one value that is routinely too long to read at a glance. "Last seen" is read-only - it is the program's own note about the entry, and it is what makes a switched-off shell explicable. The command is required, which is enforced in the two places it can be broken: emptying the field leaves the stored command standing, and an entry that never got one is dropped on the way out of the dialog rather than written as a shell that names nothing to run.

- **Reordering is a mouse gesture on a grip, not four buttons.** Every line carries a drag handle at its left edge and the list reorders under the pointer as it travels, rather than on release - the line being dragged is the line that is seen to move, which is the whole reason to prefer a grip over arrows. It costs four Tab stops per line, and that is the trade taken knowingly: reordering has no keyboard equivalent now. The grip is therefore not a stop at all, since a focus ring on a control that Space cannot work would be worse than no ring.

- **Remove sits between the command and the date, and is drawn in red.** It is the one control in the dialog that destroys something, so it is deliberately kept off the right-hand edge that a pointer travels down on its way to the checkboxes. The red is chrome rather than a theme color: "this deletes something" is a fixed meaning, and a theme whose accent happened to be red would say it about every control at once.

- **How "where is this shell now" is answered has two halves: what the OS can see, and what the shell says - and the second wins.** Unix reads the link at `/proc/<pid>/cwd`. Windows has no equivalent and no API that reports another process's directory, so it is read out of the shell's own process memory, where SetCurrentDirectory keeps it; the result is checked for still being a directory first, so a layout that ever moved would degrade to "don't know" rather than to a wrong directory. Neither can see a shell that keeps its own idea of where it is - PowerShell's `Set-Location` never tells the OS - which is why the shell is also given a way to say so directly, in the escape sequence every terminal reads for this (OSC 7, and the ConEmu OSC 9;9 spelling that Windows Terminal documents). A report is preferred to the OS answer because it comes from the one that knows; a report naming a directory that is not here, or a machine that is not this one, is dropped and the OS answer stands.

- **The snippet is put into PowerShell profiles automatically, and what that licenses is deliberately narrow.** Asking every user to paste a block into a file before new tabs open in the right place is a poor trade when the block can be put there for them - but writing to somebody else's shell profile has to earn it. So: only a profile that reports nothing at all is touched (our marker or anyone else's OSC 7 / OSC 9;9 means it is in hand); it is appended to, never rewritten, after a copy is saved beside it; the marker makes a second launch a no-op; deleting the block is how it is switched off, and nothing puts it back; the prompt is not replaced, only wrapped where there is no other hook; and a shell whose execution policy would refuse to load the profile is left alone with a line saying why, because a file the shell cannot read is worse than no file. One setting switches the whole thing off before it starts.

- **The listening is done by wrapping the PTY, not by forking the VT parser.** The sequences arrive as bytes and the parser we use handles neither, so the obvious route was a fork of it. The route taken instead is that the PTY is an interface rather than a concrete type: a wrapper sits in front of the real one and scans what it reads on the way past, leaving the byte stream untouched. It costs a single pass looking for one byte, and it means no second fork to carry.

- **Where a shell starts is answered by four things in a fixed order, and the setting is the last of them.** A `--directory` on the command line wins outright; failing that a new tab, pane or window inherits the directory of the pane it came from; a SilkTerm that a shell launched keeps the directory that shell was in; and only what is left over - a launch from the desktop, a menu or a shortcut, where the inherited directory is an accident of whoever started us - reads `shell.startup_directory`. Its default is the home variable somebody on that platform would type (`$HOME`, `%USERPROFILE%`), because a setting whose default is a blank box says nothing about what may be put in it.

- The test for "did a shell launch us" is whether standard input is a terminal, and it is the same question on both platforms. Asking about parent processes would be more direct and costs a process-table walk on Windows, which is not something to put on the path to the first frame. Measured there: launched from a console the standard handles are the console's, launched the way Explorer and the Start menu do it they are null. A release build owns no console of its own either way, so the window-handle and attach-to-parent answers are both wrong for this question.

- Removing an entry asks first, and moving one does not. Doing the opposite undoes a move; nothing undoes a removal.

### What a pane's shell inherits

- A pane's shell inherits the environment SilkTerm itself was launched with. That is deliberate for anything the user set - an activated virtualenv, a PATH they added, a variable they exported before starting the terminal - and it is what makes a terminal opened from a shell behave as a continuation of that shell.

- It is wrong for the bookkeeping a shell keeps for itself. PowerShell 7 prepends its own module directories to the module search path that every version of PowerShell shares, so a Windows PowerShell 5.1 pane opened anywhere below one resolves PSReadLine to PowerShell 7's copy rather than its own and is not allowed to load it - the pane then starts with an error and no line editing. The execution-policy variable is the same shape: one shell sets it and everything that shell starts inherits it, so a pane can run under a policy nobody chose for it.

- So a short list of shell-private variables is put back to what a freshly launched program would see, read from the machine at startup, and everything else is passed through untouched. Among the options it was decided that a narrow list is the only one that holds up: replacing the whole environment would discard the user's own exports, which is the one thing inheriting exists for, and editing the polluted value in place - dropping the entries that belong to the other shell - would depend on where that shell happens to be installed.

- This is not a defect in SilkTerm, and the fix is not a workaround for one. The same thing happens to a command prompt launched from PowerShell 7 with nothing of ours involved. But a pane should start the way it would from the desktop, and the terminal is the only place that can decide that once for every shell it opens.

- The list is not split by platform. PowerShell runs on Linux and macOS too and mutates the same variable there, so two installs side by side collide the same way; and the launching shell's `cd -` target is stale on every platform, since a pane opens somewhere else. What is deliberately left out is the class people reach for first - an activated virtualenv or conda environment - which a user wants a pane to keep, and which could not be removed honestly in any case, because the matching PATH edits would stay behind and leave the pane half-activated.

- Unix constrains the list in a way Windows does not. There is no call that says what a freshly launched program would see - that answer is composed by PAM, the session manager and the login shell between them and is never recorded - so the unix arm can only DROP a variable, never restore it. A name may therefore join the list only if a desktop session never sets it. That holds for all three today, and it is the rule to check before adding a fourth.

### A prompt is offered to bash, never installed (2026-08-30)

- SilkTerm includes x9ps1-git, a git-aware bash prompt, and hands it to the bash panes it starts. It shows the branch, whether the tree is clean, and how far ahead or behind its tracking branch it is. It was on by default until 2026-09-16, and is off now.

- Among the ways to deliver it, it was decided to set `PROMPT_COMMAND` in the pane's environment. bash picks that up as a shell variable, and the user's own rc files run afterwards. Nothing is written into anyone's `.bashrc`, there is nothing to uninstall, and it cannot follow the user into a shell SilkTerm did not start.

- It was meant to give way to a prompt set in `.bashrc`, and never did. `PROMPT_COMMAND` sets `PS1` before every prompt, so it replaces one from the rc files. Only a `PROMPT_COMMAND` of the user's own wins.
	- Yielding to any `PS1` was decided against. Debian's `/etc/bash.bashrc` and its default `.bashrc` both set one, so the prompt would never show there.
	- Instead, when it is on it always wins, and it is off by default. The Settings row names it and says where it comes from, so turning it on is a choice made knowing it replaces the rc's prompt.

- The alternative considered was the PowerShell approach: append a block to the rc file. That was rejected here because the PowerShell case has no other option - PowerShell cannot report its directory any other way - while bash has one that touches nothing. A prompt is also a matter of taste in a way a directory report is not, so the reversible answer wins.

- The script is written beside the config the first time a bash pane opens, and rewritten whenever it differs from the compiled-in copy, so an updated SilkTerm carries an updated prompt. The pane runs it through `$BASH`, which is bash's own path - no dependency on `PATH` and no execute bit needed.

- x9ps1-git is a separate MIT project of the same author. The in-repo copy is a vendored copy of its `bin/x9ps1-git`, and will go stale on its own if nobody looks - the version it carries is in its own header.

- PowerShell gets the same prompt, ported rather than shared, and delivered the other way. See below for why the two halves cannot use one mechanism.

### Opening files from Explorer on Windows (2026-09-30)

- SilkTerm opens `.bat`, `.cmd`, `.ps1` and `.vbs` files, and folders, through per-user file associations. It is not Windows' default terminal.

- Being the default terminal is ruled out for good. Windows starts the console program first and then hands its live session to the terminal through COM, and only Windows Terminal's console host can do the handing. So it needs Windows Terminal installed, a COM server inside SilkTerm, and a second way into the pty backend. Associations cover what a person actually double-clicks, for a small part of the work.

- Registering overrides only the `open` verb of each type's program ID, its label and its command, in `HKCU\Software\Classes`. Windows reads HKCU over HKLM a value at a time, so the type keeps its icon, its other verbs and its description. It also adds a `SilkTerm.<ext>` entry under Open with, which is the only way in when the user picked an app for the type there.

- What each value held before is saved under `HKCU\Software\SilkTerm\Associations`, with the value we wrote. Putting back restores only a value still holding ours, so a change somebody made since is left alone. Registering again puts back and saves over, so the path follows the build.

- The command is `silkterm --keep-open --open "%1" %*`. `--open` picks the host from the file type and takes the rest of the line as the file's arguments. A batch file is started as itself, since CreateProcess hands it to `cmd.exe` quoted the way cmd wants. A `.ps1` uses the same execution-policy step as Windows' own "Run with PowerShell".

- A dogfood build registers the launcher's link beside its versions folder rather than the versioned copy, which gets renamed.

- The Settings rows act at once and hold no config value, since the registry is the state. They exist only in the Windows build.

### One tip system, four places that draw it (2026-08-30)

- Flyover help comes up in four places: a Settings row, a link in the About box, a tab in the strip, and a menu item. Two renderers and two fonts are involved, so the drawing was never going to be shared.

- What is shared is everything else, and it lives in `tip.rs`: how long the pointer rests before a tip appears, how the text is broken to fit a width, and where the box goes relative to what it describes. A tip that answered faster in one place than another would read as a different kind of thing, which is the reason the delay in particular is one number.

- There are two placement rules, not one, and which applies is a property of what is being described. A Settings row's tip goes under the control, flipping above it when there is no room - a footer button's tip clamped into the bottom edge would sit on the buttons it explains. A menu row's tip goes beside the popup instead, because a box under the row would cover the rows the reader is choosing between.

- The tab's tip goes away after 30 seconds, and comes back only once the pointer has left the tab and returned (2026-09-22). The pointer is often left on the strip after picking a tab, and the tip then covered the top of the pane for as long as it sat there. `window.tab_tip_max_s` sets the time, and 0 keeps it up. The other tips close when the pointer moves on, which it does almost at once in a menu or a dialog.

- A menu row gets a tip only when its label does not already say what it does. Copy and New tab explain themselves; Paste Selection, Read-only and Bare window do not. A tip on every row is noise a reader learns to skip past, which costs the ones that matter.

- A tip's box is its own color, derived from the chrome it hangs off rather than taken from it (2026-09-20). The shipped menu background is the inactive tab's own bytes, so a tab tip filled with it drew as a tab that had grown downward, and a menu row's tip had the same fault against the popup beside it. Both now fill with the menu background lifted by the step the strip puts between an inactive and an active tab, then warmed, since every tab color leans faintly blue. The border and the text come off the same two colors. The dialogs already did this with the button shade against their panel.
	- Not two more colors on the Themes tab, which is what the request suggested. Twelve colors are editable and each has one job, and the chrome already answers this kind of question with derived shades - menu hover, border and separator are all shades of the menu color, so a custom menu color stays coherent. A tip that could be set apart from its own menu would be one more pair to keep readable.
	- The benchmark banner draws in the same overlay pass and keeps the menu colors. It is a modal notice over a dimmed window, not flyover help.

### Render Loop Sketch

- Frame: advance lerp -> cross-boundary check -> sync crate offset -> translate render -> draw cells (+overscan rows).

- Need: glyph atlas (rasterize font once, cache cells), cell metrics (width/height in px), vsync via wgpu surface.

### Output notices under a flood

- A pane's PTY reader finishes a read cycle roughly every 900 bytes when output is pouring in, and each cycle used to become its own window event. On 32 MiB of output that is about 20,000 events, and it was decided that the window should take delivery of at most one at a time: the notice carries nothing, so the window reads the grid as it stands whenever it gets round to one, and a queue of twenty identical notices only ever produced twenty identical reads.

- Measured on the Windows box, the folding costs nothing and saves a great deal: throughput is unchanged (11.8 against 11.7 MB/s over four alternating pairs) while the process burns a third less CPU and the window thread less than half - the 2.5 seconds that used to go into the operating system's message queue was more than parsing and drawing put together.

- The notice is re-armed before the window acts on it, so a read cycle that arrives mid-handling posts a fresh one rather than being dropped. That ordering is the whole safety argument, and it is what a unit test pins.

### The About box says how long the session has been up (2026-09-20)

- The reading is taken as the box opens rather than ticking while it is on screen. A dialog redrawing once a second to move a number nobody is watching costs a wake a second for as long as it is up, and the answer is only interesting to the nearest minute. Close it and open it again for a fresh one.

- The clock runs from a mark set at the top of `main`, not from the first time anything asks. Starting it on first read would have reported how long the About box had been reachable.

### A character handed to the window is typing (2026-09-09)

- Windows lets a program give a window a character outright instead of pressing a key for it. The touch keyboard does this for characters the layout has no key for, and so do text expanders and some accessibility tools. It arrives as a key the layout cannot name, carrying only the text it stands for.

- Every reader of a key event looks at which key it was, so an unnamed one reached nothing at all - typed text, hotkeys, menu accelerators, the Settings dialog's fields and the tab rename box alike. The key is filled in from the text once, where the event arrives, rather than in each of those places.

- Nothing else produces an unnamed key that carries text, so there is no second meaning to weigh. A key the layout did name is left alone: its text is a representation of the key rather than typing, and Enter carrying a carriage return is the case that would go wrong.

- Modifiers still apply, so an injected `c` while Control is held sends the control code, the same as typing it would. Windows composes the character without consulting the layout or the modifiers, so an argument exists for ignoring them here - but a character behaving differently from the same character typed is the worse surprise, and nobody has reported the other way round.

### What untrusted input may not do (2026-09-09)

Almost everything a terminal handles came from somewhere else. Bytes arriving from a pane's program may have traveled a long way first - a log file, a build server, a remote host over ssh - and the program printing them need not be the one that wrote them. So the rule is that nothing a pane shows may change what the terminal does, and these surfaces get held to it explicitly.

- The terminal must not type on a program's behalf. A few sequences ask the terminal a question, and the answer goes back down the pty where the shell reads it as if it had been typed. An answer that could carry a line ending would submit itself, and one that could carry the program's own text would let the program choose the command. So every reply is a fixed shape built from a number, and the terminal answers no question whose answer would be somebody else's text. There is no way to read back a window title, and a request to read the clipboard is ignored rather than answered.

	- Pinned by `a_program_cannot_make_the_terminal_type` in `term.rs`, which drives a real terminal through generated escape sequences and then asks it every question it knows how to ask.

- A link must carry a scheme from the list, or it is not a link. That is what stops `javascript:` and its relatives from reaching the desktop's handler, and it is why detection is by scheme rather than by shape. The URL then goes to the platform's own opener as one plain argument, never through a shell.

	- Pinned by `a_hostile_scheme_never_becomes_a_link` and `a_url_reaches_the_opener_exactly_as_it_was_printed` in `links.rs`.

- A paste must not be able to step outside its brackets. When the application has asked for bracketed paste, an escape character in the payload would end the bracket early and everything after it would arrive as keystrokes - which is how a paste runs a command nobody typed. Escapes are removed. Unbracketed, the application cannot tell a pasted byte from a typed one, so a line break has to be the single carriage return the Enter key sends.

	- Pinned by `a_bracketed_paste_cannot_be_closed_from_inside` and the fuzz target beside it in `pane.rs`.

- A window title is text and nothing else. The desktop draws it on a task bar, in a window list and in an alt-tab switcher, all of which treat it as plain text. Control characters are removed where the title arrives, once, rather than at each of the places it is later shown. A right-to-left override survives, which does let a title read back to front - that is the same reordering any file name can ask for, and refusing it would also refuse the joiners that hold an emoji together.

	- Pinned by `a_title_arrives_as_plain_text` in `tabtitle.rs`.

- A reported directory is not automatically a directory. A shell says where it is with an escape sequence, and the answer both names the tab and decides where the next pane starts. A payload carrying a control character is refused, since it would reach the tab strip and the title. Whether the path is absolute is asked separately, at the one gate on what becomes a working directory - a relative path resolves against wherever the terminal itself was started, somewhere nobody can see, and a posix path on Windows names a real directory in the wrong filesystem.

	- Pinned by `a_reported_directory_carrying_a_control_character_is_refused` in `cwd.rs` and `a_pane_only_ever_starts_in_an_absolute_directory` in `term.rs`.

- A program may set the clipboard, but only from the pane in use (2026-09-15). This is the one place output changes something outside the pane, and it is allowed because tmux, editors over ssh and muffer's auto-copy all copy this way. A pane in the background or in another tab is ignored, so a log scrolling past in one cannot swap out what the next paste holds. A store over a megabyte is dropped whole rather than clipped. Reading the clipboard is still refused, and text a program put there goes through the paste rule above on its way back.

	- Pinned by `a_program_can_set_the_clipboard_but_never_read_it` in `term.rs`.

### The fuzzer (2026-09-09)

Ten targets, each sitting beside the code it hammers in a `mod fuzz` inside that module's tests. They are ordinary tests, so a plain `cargo test` runs every one at a fraction of a second, and the pipeline sets a budget per target for a real soak. The engine is `source/src/fuzz.rs`; the corpus and its notes are under `cicd/tests/fuzz-corpus/`.

- Generated by grammar, not guided by coverage. Every surface here is grammar-shaped - escape sequences, config lines, URLs, image chunk headers - and a generator that knows the grammar reaches deep states in a few hundred cases where bit flips need millions. Coverage-guided fuzzing would also mean a nightly toolchain and sanitizers, so it would run on one platform out of four and not in the ordinary test run at all.

- Bit flips run as well, over a small corpus of realistic inputs. A well-formed generator never emits a truncated multi-byte character or a chunk header that lies about its own length, and those are where a parser's edges are.

- One seed is the whole reproduction recipe. Every case is a pure function of a u64, and a failure reports the seed to re-run it with. Seeds run in order from zero, so a longer budget only ever adds cases - nothing a short run covered is dropped by a long one.

- Library code is in scope, which is most of the point. The escape-sequence target drives a real terminal from the engine crate, so it fuzzes the parser and the grid model rather than only our own code around them; the config target goes through the config-format crate the same way.

- What a target asserts is a property, not an expected output. A parser that returns nothing is a fine answer; a parser that returns a span running past the end of the row it was given is not. That is what makes a random case worth generating at all.

Three defects came out of building it, all fixed with it: a program could put control characters into the window title, a reported directory could too, and a reported directory that was relative would have started a pane somewhere nobody chose. Each also has a plain unit test, which is the cheaper place to keep it.

### Environment

- Target: Debian. The primary dev/reference environment is X11 (Compiz), but one Linux binary runs native on both X11 and Wayland. winit selects the backend at runtime, and X11/Wayland/GL are all loaded on demand. Windows and macOS are targets too, all with x86_64 and ARM64 variants.

	- The X11 path also uses a glutin GL context for per-pixel background transparency, because wgpu can't drive an ARGB surface on X11. Wayland uses the plain wgpu surface, which already does premultiplied alpha. Everything else - chrome, text, scrollback slide, background image + blur + scrim - is the shared native path on both.
	- On Windows, transparency means presenting through the desktop compositor: a DX12 swapchain on a DirectComposition visual, with no redirection surface under the window. A swapchain made straight from the window only composites opaque, and the backend picked by default varies per machine, so DX12 is pinned whenever the setting is on. Both are fixed at window creation, so the setting takes effect on the next launch there.

	- Wayland coverage: smooth scrolling is identical on both engines. The scroll regression harness runs its scenes a second time under a headless `cage` kiosk (`run.bash --wayland`). Per-pixel transparency and dialog stacking on Wayland are not yet exercised (follow-ups).

- Pixel-precise input: touchpad gives true pixel deltas; notched mouse wheel snaps to lines (clamp/accumulate notch deltas into smooth target).

### Startup and slow external resources

- Nothing on the path from launch to the first frame may read an external resource that isn't needed to draw that frame. A wallpaper folder can be a network share, a synced collection or anything else that answers slowly or times out, and a terminal that waits for it is a terminal that hasn't opened yet.

- The wallpaper is the whole of that category today: scanning the rotation folder, reading the shuffle history, decoding the image, blurring and contrast-flattening it, and reading its layout tags. All of it runs on a worker thread. The window opens and the shell starts immediately, and the wallpaper appears when it is ready. That visible gap is an accepted trade, since the alternative is a window that may never open at all.

- Each request gets its own thread rather than sharing a long-lived worker. A request stuck on a dead mount can never be canceled, so a shared worker would leave every later request stuck behind it. A stale result is discarded on arrival, and a thread stuck on a read costs almost nothing. One doing real work does not, so a superseded worker asks between its stages whether it is still the newest and gives up when it is not, before the float copy and the blur where most of the memory and time go.
	- A rotation tick that finds a request still working sends nothing. Sending would only retire the one in flight, and once an image took longer to prepare than the interval, every rotation was retired before it arrived: the picture never changed and the retired workers ran on, several gigabytes at once. The tick is remembered and served by the arriving result, or by a rotation sent then when the result was not one. So an interval shorter than a preparation rotates at the pace of the preparation, with one worker at a time, and the setting reads as "at least this many seconds".

- The config file itself is a deliberate exception. Window size, font metrics and theme all come from it, and the window is held hidden until it can open at its final size. Reading it later would only trade a small local read for a visible resize flash.

- The same shape is intended for shell discovery when that arrives: draw first, scan for installed shells afterwards, fold in what was found.

### Letting the GPU go on a long idle (2026-09-17)

- Off by default. Switched on, a window that has sat unused lets its GPU device go, with everything uploaded to it, and takes it back the moment it is used again. The shells run on and the grid keeps up; only drawing stops. The case is many windows open for days, each holding a device, a swapchain, two glyph atlases, the scrim's textures and a wallpaper the whole time.

- Unused means no input, no focus change and no output from any pane while the window can be seen. Output into a hidden window does not count, or a program printing in a minimized window would hold the device for good. Two waits, both in minutes on the Window tab: a shorter one for a window that is minimized, or covered where the desktop reports it, and a longer one for a window that is only unfocused, since that one may be on a second screen being read. A window with focus and on screen never lets go.

- It comes back on any sign of life: a key, a click, the pointer entering, focus, a hidden window being shown, a shell printing, or the desktop asking for a repaint. Output into a hidden window does not bring it back; that waits for the reveal, the way the frozen-window rule already works.

- Held off while a dialog is open, since on X11 the dialog's context cannot outlive the terminal's, and while a hardware rating is owed or running.

- What is kept is what a rebuild starts from: the wgpu instance, and on X11 the GL framebuffer config the window was made with. The instance rather than a fresh one, because a GL instance's teardown terminates an EGL display the glutin context may share, and because on the other backends the adapter enumeration it holds is the slow part of a cold start. The dialogs' warm context keeps its instance and adapter the same way and lets only its device go: on NVIDIA, every Vulkan instance destroyed left two descriptors open.

- The fonts and metrics stay, since layout and input still need them. Gone with the device: the rasterized glyphs, the shaped chrome and the wallpaper, which is decoded again on the way back, as after a VT switch.

- Measured on the Linux box under software GL, on a private display: about 3 ms to let go, about 25 ms to take back, and no CPU at all while released. Under the NVIDIA driver, on Wayland and on Windows the numbers are not taken yet.

- Memory found on the way: glibc lets its mmap threshold rise with each large buffer freed, after which a wallpaper's decode is carved out of the worker thread's arena and stays resident there once freed, and `malloc_trim` never shrinks an arena that is not the main one. Every window kept the first decode's 50 MB for life, and each rebuild kept 40 MB more. The threshold is pinned at 4 MB now, so an image buffer comes from the OS and goes back to it. Launch memory dropped by about 60 MB with a wallpaper.

- The same release and rebuild heals a window after a return to its console from a text one, twice: at once, and again three seconds later, after the X server has set the mode. The older fix rebuilt only the glyphs and the wallpaper, and each thing added to the device since then was one more that a switch could leave spoiled.

- The window title says so (2026-09-18). "(resource conservation mode)" while released, "(restoring resources ...)" until the wallpaper is back, since the device itself returns too fast to see, then "(resources restored)" for five seconds. Any rebuild shows it, a return from a text console included, and it goes on a `--title` too, since it is news about the window rather than part of its name.

- Rejected: dropping the uploads and keeping the device. The device and its context are the fixed cost the feature exists to remove, and the uploads are the smaller half.

- Rejected: disabling the feature under transparency. The X11 GL path survives the teardown, since the ARGB visual belongs to the window and a new context on the kept config binds to it.

### Configuration format

- The user config uses SHCL (the sister project), replacing TOML. The file is `config.shcl`. The reference parser is a single zero-dependency crate, so dropping `toml`, `toml_edit` and `serde` made the shipped binary smaller rather than larger.

- The deciding property is forgiveness. A malformed line yields a diagnostic and is skipped, so one bad value costs only its own setting. Strict TOML could instead fail the whole document and sink every setting to its default. Forgiveness let two workarounds be deleted outright: a retry loop that blanked offending lines and reparsed, and a rewrite pass for leading-dot floats, which are valid here.

- Values are typed by the reader, not the file, so there is nothing to get wrong in the syntax and a value is stored back exactly as written.

- shcl 3.0 reads a backslash outside double quotes as itself, where 2.x read it as an escape, and 2.x wrote Windows paths that way (2026-09-24). A file whose footer has no `Format` line is taken as 2.x, and the launch has shcl rewrite it to read the same before any other step parses it. The new footer carries that line, so it happens once. A footer somebody rewrote keeps their wording, and shcl's own stamp goes under it.

- With `remember_size` on, the size written down is an ordinary window's. A fullscreen or maximized window is not a size to come back to, so neither is remembered: unfullscreening would otherwise leave every later launch opening at the size of the screen. The window's own columns and rows stay as they were, and a resize by hand still replaces them.

- A number given on the command line is held to the range of the setting it stands for, the same range the file's copy of that setting is held to. A count of rows or columns is also held to what the graphics device can draw, since the window is a texture and a refusal there ends the launch rather than the setting.

### Variables in a setting (2026-08-30)

- A setting that names a path or a program is text SilkTerm reads. No shell ever sees it, so nothing else would expand a variable written there.

- Three spellings are read, all of them on every platform: `$NAME` and `${NAME}` from bash, `%NAME%` from cmd, `$env:NAME` and `${env:NAME}` from PowerShell. A leading `~` works too. Which shell somebody prefers should not decide whether their config file works, and a file carried between machines keeps working.

- A few names that mean one thing under two spellings are paired: HOME with USERPROFILE, USER with USERNAME, TMPDIR with TEMP and TMP. Only names with a real counterpart are listed, since a guess would be worse than an empty expansion that can be seen.

- An unset name expands to nothing, the way a shell does it.

- A command is the exception. It is split into arguments first, then only the program name is expanded; every argument after it goes through exactly as written.
	- The program name is expanded because nothing else would. SilkTerm starts it directly, so there is no shell in between to read a `%ProgramFiles%`. Splitting first also keeps a variable holding `C:\Program Files\...` as one argument.
	- The arguments are not expanded because the program itself reads them, and they already mean something to it. `cmd /k prompt $P$G` sets a cmd prompt, `bash -c 'echo $FOO'` wants bash's own `$FOO`, and a `%` in a unix path is a percent sign. Substituting any of those hands the program a word nobody typed, with no way to write a literal.
	- Every word was expanded until 20260916, which is what broke those three. Reading only the local platform's spelling was tried first and dropped the same day: it fixed the cross-platform collisions, left the local ones, and cost a carried config for nothing.

- `shell.startup_directory` defaults to the home variable in the platform's own spelling: `%USERPROFILE%` on Windows, `$HOME` elsewhere. Both are read on both platforms, so a file carried to another machine still finds home. It was `~` for part of 20260916.

### Command-line options

- Most options describe a window to open: a hierarchy of tabs and panes built with create/select verbs, with look and behavior cascading window -> tab -> pane.

- A second, much smaller family only prints something and exits. `--help`, `--syntax`, `--about`, `--donate` and `--version` never open a window, never read a config, and never touch a layout.

- Those flags are accepted in any position. The rest of the grammar cares a great deal about order, but answering a request for the help with a complaint about where the flag was written would be absurd.

- Output written for a person is padded with a blank line above and below, so it sits clear of the shell prompts either side of it. `--version` is the exception, and it exists to be captured by a script, so it stays a single flush line. `--ver` and `-v` are the same flag.

- Every build carries a build number, because a version cannot identify one. Two dogfood builds of the same release share a version, so a report of "it still does this on beta3" names something that could be any of a dozen binaries. The number is whole minutes elapsed since 2000 began, written in Crockford base 32 in lowercase: five characters until 2063, it sorts in the order the builds were made, and it decodes back to the minute one was built. Crockford's alphabet leaves out i, l, o and u, so nothing read off a screen and typed into a bug report can come back as a different character.
	- It appears in `--version`, in `--about`, in Help > About, and in the release notes.
	- The pipeline pins one number for a whole run, so the four cross builds of a release report the same build rather than four made minutes apart. Without that the release notes could not name one.
	- Unchanged sources keep the number they had. The binary did not change, so neither should what it calls itself.
	- The release notes take the number out of the artifact being published rather than computing it again, so the notes cannot name a build nobody can download.

- Where a shell starts is decided by four things, most deliberate first: `--directory` on the command line, then the directory inherited from the pane a new tab/pane/window came from, then the directory SilkTerm itself was launched from (only when a shell launched it), then the `shell.startup_directory` setting. `--directory` cascades window -> tab -> pane exactly as `--shell` does, so the flag that names a shell and the flag that says where it starts behave alike.

- `--about` reports what a bug report needs: version, build number, which of the cross builds this is, and the GPU the renderer picked. It asks the graphics stack for an adapter but never builds a device, which is the expensive half. A box with no usable adapter loses three lines and still prints the rest.

- On Windows a release build owns no console of its own, so printing has to join the one that launched it. This happens only on the paths that print and exit; a terminal window that held a console would die with the shell that started it.

- The contract on saving is that a user's comments and blank-line grouping survive. Layout may be tidied, meaning indentation and quotes that are not needed, but a value is never rewritten. The shipped template is deliberately spelled the way a save would spell it, so the first save is a no-op rather than a reflow of the file we just wrote.
	- So a save must never change what the next launch loads. The renames and refreshes a launch applies before it reads the file decide only on what a save keeps: a value as it reads, in whichever quotes; a setting's place in its blocks as the file format reads it, with comment lines left out; and a commented line where a save would put it. Before this, a single-quoted old default font list was left alone until the first save put double quotes around it, and a setting under a commented-out heading was renamed only once a save had moved the heading.
	- Rejected: a save that writes every untouched line back as it was typed. That is a second writer beside the format's own, with placement rules of its own to get right. Also rejected: refusing a save that would move a value, since the person saving can neither see why nor fix it.
	- Adding missing settings at launch keeps the same promise (2026-09-18). A setting typed deeper than its block needs still loads, until an active line is added above it and takes it for a child. Such a line is moved out to its block's own depth, which a save would do anyway. Where that is not enough, or a line would become unreadable, nothing is added and the launch says why.

- A save writes through a temp file and a rename, never in place, so a crash mid-save cannot leave a truncated config. If Windows takes the old file off its name and then cannot put the new one there, the new settings are written at the name directly, rather than leaving no file. If loading had to drop a line it could not place, the save is refused rather than quietly deleting it. One changed setting is not worth a line someone wrote.
	- A refused save is said on screen, since stderr reaches nobody on Windows and nobody who started SilkTerm from a menu (2026-09-18). Windows shows its own message box. Elsewhere a small window drawn like About stands in for one, since Linux has no message box every desktop carries. It names the file and the lines, and says changes are used now but not kept.
	- Saves nobody asked for, such as a resize, a menu switch or shells found at launch, are said once a session for each file, or every resize would raise it again. An OK or Apply in Settings is answered every time, and OK then closes, since trying again cannot help. A file open in another program still keeps Settings open, because trying again can.
	- Editing only the lines a save changes, the way the rating does, would let most saves through. That was held for shcl 3.0, which did not change it: a line it cannot place still counts as lost, so the save is still refused (2026-09-24).
	- Saves go through shcl's line-keeping save since 2026-09-25, so a line nobody changed keeps its quotes and indent. shcl still writes the whole file where it cannot keep the lines, and it always does for a file with a dropped line, so the refusal stays.

- Settings opens on the file as it is now, not on what the window loaded (2026-09-22). Several windows share one file, and one that saved after this window loaded would otherwise not show here. The file then gets only what was edited in the dialog, and the window takes everything that differs from what it runs, so OK is also when another window's change arrives.
	- The file is read when the dialog opens, not watched. Opening Settings changes nothing on screen, and Cancel leaves the window as it was.
	- The session's own choices are folded in first, the way Reload config folds them: the command line's font and colors, a wallpaper named for the session, the Remote profile.

- The template's sections follow the Settings dialog's tabs, in the same order, so a person who has learned one has learned the other. That order reaches a new file only. An existing config keeps whatever order it has, since the machinery that adds new settings places them but never moves what is already there.

- The file is organized as nested blocks, tab-indented, mirroring how the settings relate: `wallpaper` holds its children, with `rotate` and `contrast_mask` nested inside it. A setting can also be written as a single dotted line (`wallpaper.opacity: 0.1`) and reads identically. The block form is just the canonical spelling.

	- Each setting stands in its own blank-line-delimited section, comments directly above it: a short title, a description, and a range line where one applies. A commented-out line shows the built-in default and carries a `## Default` marker.

	- The in-place add/refresh passes stay line-oriented. They resolve each line's full path from the indentation around it, so a new setting is inserted inside the right block, beside its siblings.

- No migration from the old TOML configs: a fresh file is generated with defaults.

- When the flat naming gave way to nested blocks, an old config converts wholesale rather than being rewritten in place. The old file is kept alongside as a backup, a fresh current-format file is written, and every value the user had set carries over to its new place. Rewriting flat lines into blocks would have shredded the old file's comments. This way settings survive and the file's documentation is current.

- A file an earlier build converted with its image written onto the `wallpaper:` heading is repaired where it is at launch. The value moves to `image:`, unless the file already names an image there, which is the later choice and is kept while the heading's value is dropped. No other line changes, but the file is written back with LF line endings, as a conversion writes it. It is not converted again, because that needs a backup name, and that build had used most of them converting the same file at every launch. Values that build dropped are not read back from the backups, since a backup is older than anything saved after it.

- A config carries the commented default lines it was first given, so when a default changes those lines start describing the old behavior. Such a line is refreshed to the current default. The file may be corrected about what the program does on its own, but never about a value that was set by hand. A line the user activated, or annotated, is therefore left alone.

- Starting over is a rename rather than a delete. `--reset-config` moves the file aside and lets the next launch write a fresh one, so the previous settings stay recoverable.

- Some defaults are better inferred from the config directory than stated in the file. A folder of wallpapers sitting in the expected place is taken as wanting them rotated, without a setting to turn it on and without writing anything back. The inference yields to anything explicit: a wallpaper named in the config, or one given on the command line for a single run.
	- The folder setting's default names that place, the way somebody on the platform would type it: `%LOCALAPPDATA%\silkterm\wallpaper`, `$HOME/Library/Application Support/silkterm/wallpaper`, or `$XDG_CONFIG_HOME/silkterm/wallpaper`. It used to be blank, and a blank box says nothing about what goes in it. The default is looked up as "the usual place" rather than expanded. So it still finds the older `wallpapers` and `backgrounds` spellings and a pack left beside the config on Windows, and it follows `--config` and `XDG_CONFIG_HOME` whether or not that variable is set. An empty value means the same.
	- Settings has one "File or folder" box for both. A named image wins at run time, so the box shows one whenever it is set. Otherwise it follows Rotate folder: the folder with it on, the image with it off. Which of the two a field edits is fixed when the field opens, so emptying it on the way to typing something else does not switch it.

- With the wallpaper switched on, something is always shown. The shipped image stands in whenever nothing else supplies one: no image named, an empty rotation folder, a file that will not open. A folder that does hold images owns the picture instead, so the stand-in only appears once the folder has been read and found wanting. Which means any request that could leave the window bare has to read the folder first, rather than assume the last pick is still in hand.
	- The one exception is asking for none. `--wallpaper-file` or `--wallpaper` with no value shows no picture for the session, with or without a rotation folder. Before, it showed the stand-in or nothing depending on a folder the command never named.

- A wallpaper named on the command line, at launch or while running, turns the wallpaper on for the session even when the file has it off. Naming one is a deliberate choice. A performance profile that turns the wallpaper off still wins, for both flags alike, since the profile goes on after them. Reload config keeps it, along with the font and colors the command line gave at launch.

- A wallpaper image can carry its own layout and look in its XMP metadata, under a `wallpaper` namespace named for what the tags describe rather than for this program, so any tool can write them. `Fit` and `Anchor` are absolute, since how an image should be cropped is a property of the image. `Opacity` and `Blur` are absolute too, in the same units as the two settings, and replace them for that image. The shipped pack carries the program defaults on every image, so the two sliders only reach untagged images until the switch is turned off; that trade was accepted so an image's look means the same thing everywhere. Each pair has its own switch in Settings, on by default, and a missing or unreadable tag always falls back to the setting rather than failing the image.

## Delivery (CI/CD, branches, releases)

Guiding constraint: GitHub is dumb git hosting plus optional release storage, nothing more. No hosted CI, no Actions, as few third-party tools as possible; the whole pipeline runs locally (`cicd/cicd.bash`).

- Merge gate: `cicd.bash --gate` (fmt check, clippy with warnings as errors, tests) runs as the `pre-push` hook for pushes to main. This is the local stand-in for a hosted CI workflow. Pushes to dev and feature branches are not gated, since a branch is tested before it merges into dev. The gate reads the commit being pushed, from a throwaway worktree checked out at that commit, rather than the working tree: the two are routinely different here, and a fix still sitting uncommitted would otherwise carry the push. Cargo writes to the usual output directory, so only this crate is rebuilt there, not its dependencies.

- Version-bump guard: the same `pre-push` hook blocks a push to main unless its `source/Cargo.toml` version is a strict increase over the version already on main, by full semver precedence including prerelease ordering. So a release merge can't ship the same-or-lower version. It also requires the README Release badge to match that version, the same check `release.bash` makes, just earlier. It skips on the first main push and on branch deletes, and is overridable with `--no-verify` / `SKIP_GATE=1`.

- Branch flow: feature branches merge `--no-ff` into `dev` (the integration target). `main` is release-only: merging dev into main cuts a release.

- Releases: `cicd/utility/release.bash` tags the merge `v<version>` and can push the tag and attach the artifacts to a GitHub Release as plain uploads. The version comes from `source/Cargo.toml` alone. The tag is read from it and the build stamps from it, so they can never disagree. Version and README badge get bumped on dev before the release merge; nothing is ever committed directly on main.

- Build matrix (buildable from this Linux x86_64 box): Linux x86_64 (native) and, via `cargo-zigbuild` + `zig`, Linux ARM64, Windows x86_64 (mingw), Windows ARM64. macOS and BSD are deferred, since cross-building them needs an Apple SDK (osxcross, license-gated) or a FreeBSD sysroot, neither present here. The debug build is what the tests and profiler run against, and the optimized release builds feed packaging and dogfooding. ARM64 targets are on by default, since zig cross-builds aren't emulated and so are not much slower, and they drop out with `--no-arm`.

- Windows x86_64 toolchain for releases: the gnu (mingw) build is the canonical shipped Windows x86_64 binary, and the msvc build is deliberately left out of the Linux-cut release. The gnu build cross-builds from this Linux box in the same run as everything else, and is self-contained. msvc can only be built on Windows, since it needs `link.exe`, so folding it in would mean a Windows->Linux binary hand-off. Since the msvc build was made crt-static it no longer offers end users anything gnu doesn't. Its remaining edges, PDB/WinDbg debugging and a standard ABI, are dev-side only. `cicd-win.ps1` still builds msvc on Windows for local dogfooding and debugging. Two things would reopen this: Authenticode code-signing, whose natural home is Windows and would pull Windows-installer finalization onto that box; or evidence that mingw binaries trip antivirus reputation heuristics enough to matter. `makensis` itself is host-agnostic, so building the Windows installers on Linux is a non-issue independent of this choice.

- Packaging (pipeline stage 6, when `--quick` is not passed): built from the stage-5 release binaries, never rebuilt. Linux -> `.deb` (cargo-deb) and `.rpm` (cargo-generate-rpm) per arch, driven by `[package.metadata.deb]` / `[package.metadata.generate-rpm]` in `source/Cargo.toml`. Windows -> one self-contained NSIS installer `.exe` per arch (`cicd/packaging/windows/installer.nsi.in` + `makensis`). It upgrades an existing install in place by running the old uninstaller first, and needs no bundled runtime because the binary links only system DLLs. RPM versions can't contain `-`, so `1.0.0-beta1` is emitted as `1.0.0~beta1`. AppImage/Flatpak and the deferred macOS `.dmg` / BSD packages are future work.

- Dogfood delivery: every build the pipeline makes is installed under a fixed name in a synced app dir for the platform it targets (`.../exec/app/{linux,mswin,macos}/`), with the icon and a sidecar naming the build beside it. That is the only place a launcher looks. It replaces an older arrangement where each box also read the build host's clone over the network, which meant a probe, three bounded waits and a whole class of "why did the shortcut hang" that no longer exists. The cost is that a build reaches another box at Dropbox's pace rather than immediately, which for dogfooding is not a cost.

- One launcher, three platforms: `utility/n8runterm.ps1` is the whole implementation, and `runterm` (bash, for Linux and macOS) and `runterm.cmd` do nothing but find it and pass the arguments on. The two previous launchers were the same program written twice, and they had drifted apart in ways that only showed when one box behaved differently from the other. PowerShell 7 is now a requirement for launching a dogfood build on any platform, which is the price of having one implementation to keep right.

- Where copies live: a versions folder beside a `silkterm` symlink pointing at the newest. Anything that names the program - a name on PATH, a menu entry, an icon - points at the symlink and so never needs rewriting when a build arrives. The folder is GFS-rotated on every launch: newest and oldest always, then the last couple, then the newest of each ended day, week, month and year, capped per tier so a box that builds daily still keeps something from months back. At most ten copies, at least five, and it stops at 1 GB in between. A copy that is running is never deleted, so a window open on an old build keeps its binary.

- A menu entry runs the launcher, never the terminal. A shortcut naming a build pins that build forever, which is how the entry went stale before. The launcher writes and refreshes the entry itself, from the paths it already knows, so it cannot drift from where things actually are. Its icon comes from beside the symlink - a `.png` on Linux, the symlinked binary's own icon on Windows.

- Artifact naming (stable; download links depend on it): `<exe>-<version>-<os-arch>[.exe]` for binaries, `<exe>-<version>-<os-arch>.{deb,rpm}` and `<exe>-<version>-<os-arch>-setup.exe` for packages, plus `<exe>-<version>-sha256sums.txt` (covers binaries and packages), all collected into `cicd/artifacts/release/`.

- Pinning: `rust-toolchain.toml` pins rustc/clippy/rustfmt and the cross targets. The cargo-installed helpers (cargo-deny, cargo-zigbuild, cargo-deb, cargo-generate-rpm) and makensis are pinned in `cicd/tool-pins.txt`, which both pipelines read, with a non-gating drift warning. Dependency freshness is a periodic local `cargo update` pass, and cargo-deny advisories flag anything urgent in every run.

- Reproducible builds (2026-09-24): one commit built in two folders gives the same bytes for all four published targets. The path map keeps the folder out, the build number comes from the commit's time on a clean tree, and the Windows linkers are told not to write the link time into the PE header: `--no-insert-timestamp` for mingw, `-Brepro` for zig, which refuses the other. `cicd/utility/repro-check.bash` is the check. The msvc build that `cicd-win.ps1` makes for Windows boxes is not published and still carries its link time.

- README badges: static shields only (release, license, minimum Rust). No CI badge, since there is no hosted workflow to point one at.

- Wallpaper gallery on GitHub Pages: `docs/` is served from main, and holds one self-contained page - a thumbnail grid whose tiles open the wallpaper full size in place, with prev/next paging, a filter box and per-image provenance. A README cannot do this: GitHub renders no scripting and strips image maps, so a single contact sheet has no clickable tiles and there is no way to page through anything. It stretches the guiding constraint above and does so knowingly - Pages here is branch-served static files with no Actions workflow, GitHub builds nothing, and if it were switched off tomorrow the only casualty would be one README link. The page carries thumbnails only (about 1.4 MiB) and fetches each full image from the pack already in the repository, so the 60 MiB of wallpapers is never stored twice. Both it and the README contact sheet come out of `cicd/utility/wallpaper-gallery.bash`, which is deliberately one entry point: two rendered artifacts from one pack go stale together or not at all.

### A tab can be named by hand, and the window title follows the tab (2026-08-30)

Double-clicking a tab renames it in place. The strip has always drawn what the shell is doing, which is right most of the time and wrong when several tabs are running the same thing in the same tree.

- The edit starts with what the tab already says, all of it selected, so typing replaces it and any other key edits it. Enter or Tab keeps the change, Escape drops it, and a click anywhere else keeps it. Selection, Home and End, and paste all work; a pasted newline becomes a space, since a tab is one line high.

- Committing a blank title, or one that matches what the tab would have said on its own, puts it back to naming the shell. Those are the two ways out of a hand-typed title, and neither needs a control of its own.

- Titles need not be unique. Two tabs called the same thing is a thing people do on purpose.

- The rename is keyed by the tab's position, so opening or closing a tab ends it - committed on an open, dropped on a close.

The window title is now assembled in one place, and always starts with the application name. A dogfood build says which one it is, since the pool holds several and they are otherwise indistinguishable in the taskbar.

- After the name comes, in order: a title typed on the tab, else the title the running program asked for, else what the tab says about the shell. So a program that renames the window (an editor, a build tool) reaches the title bar without touching the tab, and a hand-typed tab title outranks it.

- A title that is only the name or path of a program is passed over, so the next source down answers - normally the tab's own label. A Windows console names a new window after the program it starts, so a shell that sets no title of its own arrives carrying its own image path. Measured for cmd and for pwsh on two machines.

- A console that names the program and then the command it is running keeps the command, since that half says something. The name has to be a full path for that to apply, which is what a console writes; a bare name on the left would eat the file in vim's "build.bat - VIM".

- The extensions that count are `.exe`, `.bat` and `.cmd`. Not `.com`: far more titles end in a hostname or a directory than in one of the three DOS-era programs that still use it.

- A tab carrying a blank title lets the program's title through, and with neither the title is just the application name - blank means "defer", not "show nothing". A blank typed into the rename box goes back to automatic naming instead, so a blank tab title comes from an empty `--title` on the command line.

- A `--title` given on the command line is the whole answer, verbatim. It is an explicit request for exactly that string. The one thing it cannot drop is the rights below.

Which of the three parts a tab names is now the user's to choose, and so is whether the window title falls back to the tab at all (2026-09-20). Four switches on the Window tab, all on as shipped.

- A part that is switched off is dropped before the ladder of shortenings is built, not cut out of the finished label. A tab with the directory off therefore has no path rung to give up, and the rungs it does have are measured against what will really be drawn.

- Switching all three off leaves the tab naming its shell. A tab with no text cannot be told from the one beside it, so there is a floor under this and the shell's name is it.

- The tab's flyover still names all three whatever the switches say. That is the point of turning one off: the text is out of the strip, not out of reach.

- Turning the window title's switch off drops both of the tab's answers, the name typed on it and the text it works out for itself, since both are the tab talking. A title the running program asked for still comes through, and a `--title` is untouched.

A title the running program asks for can name the tab too (2026-09-20), which is a fifth switch beside the four above, also on as shipped.

- The order is the window title's, one step down: a name typed on the tab wins, then the program's title, then the text the tab works out for itself. The two were already a pair - the window title falls back to the tab - so having them disagree about what a program said would only have been confusing.

- It reads the program's title through exactly the same filter the window title does. A Windows console's own decoration comes off it, and a title that is only the name or path of a program says nothing on either.

- The title sits above the tab's ladder of shortenings rather than inside it. A program's title has no shorter forms of its own, so folding it in would have thrown away every rung longer than whatever the program happened to say, and a narrow tab would jump straight from a full title to the shell's initials. Above the ladder, a tab too narrow for the title falls through the forms it works out for itself, and the floor is still the shell's name.

- On by default because the window title already prefers a program's title by default, and a tab that disagreed with the title bar above it would read as a bug. The switch is there for a shell that retitles on every prompt, which turns a strip of useful labels into a row of the same `user@host` text.

- The tab's flyover carries a "Program title" line when there is one, whatever the switch says, the same as the parts above.

### A terminal running with administrator or root rights says so (2026-09-09)

The window title starts with "Administrator: " on Windows and "Root: " elsewhere. Windows already spells its own elevated console title bars that way, so that word is kept rather than invented.

- No flag turns it off, `--title` included. The absence of the word has to mean something, and a marker a flag can remove means nothing.

- It goes on the window title only. Tab titles are left alone, and so are the Settings and About windows: neither is a taskbar entry of its own, and the terminal window they belong to already says it.

- The rights are the terminal process's own, read once on the way to the first window. A pane that elevates itself afterwards, `sudo -s` for instance, is not covered - the title would be claiming something about the wrong process. On unix the test is the effective user id, so a setuid binary reports the rights it actually holds rather than the account that started it.

- An elevated Windows console writes the same word in front of the first title it sends, which was measured over a pseudoconsole on two machines. That is the title naming the program the console started, so both halves say nothing: the word comes off, and the program name is then dropped by the rule above. Without that step the word hides the name and the whole path is shown, which is the original complaint with rights on top.

- A title the program sets afterwards is not decorated - measured in the same run, where a title set to "build" arrived as "build". So the word cannot arrive twice from a console. It can still be typed twice, and a title already starting with it is not given another.

- Only the exact word about to be put back is removed, and only on Windows. Nothing on unix decorates a title, so taking anything off there could only destroy somebody's own text - which means a program running as root that titles itself "Root: something" will show the word twice, and that is the right answer, because the second one is its own.

- A console speaking another language writes another word, and no list of words could cover them all. So the title is also matched against the program the pane was started with. When a Windows path to that program follows a word, the word comes off in any language. Only the file name is compared, because a console spells the folders its own way. A word that holds a path separator, or one in front of some other program, is left alone.

### Tabs report what they are running, and where (2026-08-21)

A tab used to say the application's own name on Windows and the shell's process name on unix. It now reports the shell by its FRIENDLY name - the one the Shells list carries, which is the name the user gave it - followed by what that shell is doing: the command in the foreground, or the last one it ran, or, having run nothing, the directory it is in.

- The shell a pane runs is resolved once, when the pane is spawned. Leaving it as "whatever the default shell is" let the answer change under a running pane, since the background scan fills the list seconds after launch and the Shells tab reorders it - so a pane could be labeled with a shell it was not running.

- The path is shortened by an ellipsis eating the middle a directory at a time, so what is left keeps its real names. Only when that has run out do the directories above the current one drop to their initials, which on any path deeper than a couple of levels never happens - the ellipsis has already covered more ground than a column of single letters would. Two things survive every step, because they are what distinguish a location from a command - the anchor it starts from and the separator it ends with. Windows keeps its drive letter and gets no `~`, since neither shell there prints one.
	- This reverses the original order (2026-09-20), which was PyCmd's: initials first, ellipsis only where it was shorter still. It came from nemo-anywhere, which had already been through the same argument. A middle left out reads as a place with a gap in it; a column of initials reads as neither the path nor anything else, and it gives up every name at once to save a few columns.

- Tab width is two percentages of the window rather than a fixed cap, so the extra text has room on a wide display while a lone tab still reads as a tab. See the entry below for what those two now mean.

- When the tabs stop fitting, the strip shows a page at a time rather than shrinking them to nothing. The wheel over the tab bar turns the page, and switching tabs brings the new one onto it.

- A hover tip carries what the tab cannot: the shell's name, the command line behind it, whatever is running now, the whole path, and how long the tab has been open. It reads as a table - one `key: value` per line, every value starting in the same column - which is why it is the one piece of chrome drawn in the TERMINAL font rather than the interface one: the column is made of spaces, and spaces align nothing in a proportional face. A value carrying a space or a quote is quoted, so its edges are never in doubt; the quote picked is the one the value does not already contain, the same habit the config file has, so a Windows command line reads inside single quotes instead of fighting its own double ones. What is derived rather than quoted - the clock reading, and the note that no directory was reported - stays bare, since quoting those would say they were data.

### A tab is as wide as its own label needs (2026-08-23)

Tabs used to divide the bar evenly between a minimum and a maximum percentage of the window, so every tab was the same width whether it had anything to say or not. They now size themselves.

- The first percentage is the REGULAR width: what a tab is when nothing is pushing on it. It is a target, not a share - three tabs on a wide bar sit at it and leave the rest of the bar empty, rather than a couple of them stretching across the window.

- A tab whose label wants more room grows past it, up to the maximum. A crowded bar pushes every tab back below it. Everyone reaches the regular width before anyone grows past it, so a long path can never cost another tab its ordinary size, and under crowding each tab gives up the same fraction rather than the last few being starved.

- The tab in front is the exception (2026-09-20). It takes what the row can spare before any other tab grows past its ordinary width, and the maximum does not apply to it, so it spells its label out wherever there is room. The room it leaves is no use to tabs already at the cap, and it is the one being read. The tabs behind it give way for it, each dropping to whatever rung of its own ladder still fits, and none of them goes below the regular width. With the shipped 10% regular width that can leave an inactive tab saying only its shell's name; raising the regular width is what buys them a path back. Taken from nemo-anywhere, whose tab row had the same problem.

- Defaults are 10% regular and 100% maximum. The old pair (8% and 26%) made sense when the bar was divided evenly; a maximum now only says how far one tab may grow when it has the room, which is worth allowing in full for a window holding a single tab.

- The floor a tab may not shrink past is its own shortest label - a short form of the shell's name and nothing else. Tabs past that point become a page.

- What a tab says now gives way in a fixed order, rather than only the path shortening: the shell's name shortens first, then the running command's name is truncated, then the path abbreviates, then the command goes, then the path, and what is left is the shortest form of the shell's name. The path is shown alongside the command now, where before a tab running something said only what it was running.

- Short shell names are hand-picked for the shells we ship ("Windows Cmd" reads "Cmd", "PowerShell 7" reads "PS 7") and derived for anything renamed, since nothing mechanical arrives at "Cmd" from "Windows Cmd". A derived name keeps its distribution rather than its family ("WSL2; Ubuntu" reads "Ubuntu") and marks a variant with a star, so "Zsh" and "Zsh*" at least say that one of them is not the ordinary one.

### PowerShell gets the same prompt bash does (2026-08-21, reworked 2026-08-30)

The integration block sets a prompt, but only where the prompt is still the one PowerShell comes with, identified by the help link its own definition carries. Anything anybody else installed is left alone.

It began as a prompt that named the version, because two PowerShells look alike at a prompt. It now reads the same as the bash prompt described above: version, time, user, host, path, and in a git working tree the remote, the branch, and two marks for committed and level with the upstream. A PowerShell pane and a bash pane should look like the same terminal.

Two decisions came out of the port.

- It lives in the block rather than in a script beside the config, which is where the bash prompt lives. A prompt is drawn after every command, and a script would mean a process per prompt - cheap on unix, not on Windows. The block is already kept up to date in place, so it carries updates just as well as a file would.

- The block stays plain ASCII, and the check, cross and arrow are written as code points. A file with no byte-order mark is read as ANSI by Windows PowerShell 5.1, which would mangle a literal glyph on the one version that cannot be told otherwise.

- The console is put on UTF-8 at load. That is not what lets the prompt draw its glyphs, since PowerShell writes the prompt as wide characters and the code page has no say in it. What it buys is the decoding of output from `git` itself, where a branch name outside ASCII would otherwise arrive wrong.

- The second line is bare where the bash prompt puts an arrow. The `>` already says where the typing goes, and the arrow the bash version uses is a code point few fonts carry.

- The check is U+2713 rather than the U+2714 the bash prompt uses. U+2714 has an emoji presentation, so it is drawn by a color font in its own color and ignores the reverse-video the mark is set in. U+2713 is not an emoji code point at all, so it takes the color the way the cross beside it does.

Cost was the other thing the port had to answer, since three `git` calls per prompt is invisible on unix and not on Windows. The search for the working tree is done in the shell rather than by asking git, so a directory outside a repository costs no process at all, and inside one a single `git status --porcelain=v2 --branch` answers branch, clean and upstream together. The remote URL is read once per repository and remembered.

The block is also kept up to date in place from then on, between its two markers. It gains things over time, and an install that only ever appended would leave everyone who already had it on the first version forever. That edit is safe only because the region is delimited by markers we wrote - which is exactly the signal the stored shell list lacks, and why that list may still only ever be added to.
