<!-- markdownlint-disable MD007 -- Unordered list indentation -->
<!-- markdownlint-disable MD010 -- No hard tabs -->
<!-- markdownlint-disable MD041 -- First line in a file should be a top-level heading -->

<!-- TOC ignore:true -->
# Smooth scrolling

<!-- TOC ignore:true -->
## Table of contents

<!-- TOC -->

- [Summary](#summary)
- [Specification](#specification)
- [Goals](#goals)
	- [Non-goals](#non-goals)
- [Design](#design)
	- [Whole lines in the engine, a fraction in the renderer](#whole-lines-in-the-engine-a-fraction-in-the-renderer)
	- [Where a gesture comes to rest](#where-a-gesture-comes-to-rest)
	- [Easing new output](#easing-new-output)
	- [Rows that stay still while output eases](#rows-that-stay-still-while-output-eases)
	- [Windows and tabs that could not be seen](#windows-and-tabs-that-could-not-be-seen)
	- [The output chase](#the-output-chase)
		- [The curve, as specified](#the-curve-as-specified)
	- [The five settings](#the-five-settings)
	- [Full-screen programs](#full-screen-programs)
		- [The engine's scroll ledger](#the-engines-scroll-ledger)
		- [Fingerprints where the engine recorded nothing](#fingerprints-where-the-engine-recorded-nothing)
		- [Easing it into place](#easing-it-into-place)
		- [Filling the gap with the rows that left](#filling-the-gap-with-the-rows-that-left)
		- [What makes this hard](#what-makes-this-hard)
	- [The wheel in full-screen programs](#the-wheel-in-full-screen-programs)
	- [The scrollbar](#the-scrollbar)
- [Alternative ideas](#alternative-ideas)
	- [Unconsidered](#unconsidered)
	- [Rejected](#rejected)
	- [Superseded](#superseded)
- [Research findings](#research-findings)
- [Roadmap](#roadmap)
- [Related backlog issues](#related-backlog-issues)

<!-- /TOC -->

## Summary

SilkTerm moves text a pixel at a time instead of a whole line at a time. That goes for the wheel and the scrollbar, for new output pushing old text up, and for full-screen programs such as less, vim, nano and tmux that scroll part of the screen themselves. It is the reason the project exists. When this was started, no other terminal eased new output, and none animated full-screen programs.

## Specification

- The terminal's contents never change because of smooth scrolling. Only where the text is drawn changes.

- The wheel and the scrollbar move the text smoothly.
	- A wheel gesture always comes to rest on a whole line, and on the next line in the direction it was going.
	- A scrollbar drag or a click in the scrollbar's track comes to rest on the nearest line.
	- A jump back to the bottom sweeps home at full speed.

- New output slides up into place instead of jumping.
	- A little output, such as a prompt coming back, eases in gently.
	- Output that fits on one screen never goes faster than the Single-screen speed.
	- Once a screenful has gone by, the speed climbs until it keeps up with the output, however fast that is.
	- When output stops, the speed winds down smoothly and the last line settles in place.
	- At speed the view trails the output on purpose, by about the distance it needs to slow down. It never falls further behind than that.

- Rows a program keeps in place stay still while the text above them moves. Examples are a progress line, apt's status bar, tmux's status line and the input box of a chat-style program.

- Full-screen programs that scroll part of the screen get the same smooth motion as plain output. Their title and status rows stay still.

- New lines that only fill empty space at the bottom of the screen pop in. They do not slide down from behind the text above.

- A window, tab or pane that could not be seen does not animate what it missed. It jumps to the current state in one cut when it is shown again.

- The wheel scrolls the scrollback on the normal screen. In a full-screen program it sends arrow keys, unless the program asked for mouse events, in which case the program gets the wheel. Shift keeps the wheel for SilkTerm.

- Settings has one switch that turns all scroll animation off, and five sliders that shape output speed. On every slider, higher means faster, harder or crisper.

- A second switch turns off the slide for full-screen programs only.

## Goals

- Smooth motion for every kind of scroll a person watches: the wheel, the scrollbar, new output and full-screen programs.

- Output never piles up out of sight. The oldest complaint about smooth scrolling on terminals, and the reason it was dropped in the late 80s, is that fast output gets backlogged. SilkTerm has to start slow and gentle, then speed up as far as it needs to.
	- Example: `tail -f` on a log that prints one line now and then, and then suddenly a long, fast burst, and everything in between. Scrolling should stay smooth at the slow end and keep up at the fast end.

- Each setting does one thing a person can see, and no setting quietly changes what another one does.

- Nothing that stays still on screen should bounce, jump or leave a seam.

### Non-goals

- Changing what is in the terminal. The engine only knows whole lines, and that stays true.

- Guessing about programs that repaint the whole screen for every change. Those fall back to a plain cut where nothing can be measured reliably.

- Animating content that is already old. A catch-up after being hidden is a cut, not a slide.

## Design

### Whole lines in the engine, a fraction in the renderer

The engine (the `alacritty_terminal` crate) owns "where the grid is", in whole lines. The renderer owns a fractional offset laid over it. The crate's `display_offset` is a whole number, so smooth scrolling lives entirely in the renderer.

1. Keep a separate `visual_offset: f32` in the render layer, apart from the crate's integer `display_offset`.

1. On wheel input, set a target and ease `visual_offset` toward it each frame.

1. When `visual_offset` crosses a whole line, call `scroll_display(Delta(+/-1))` to move the grid by one line, and subtract `1.0` from `visual_offset` to keep the fractional remainder.

1. Draw the grid shifted down by `visual_offset * cell_height` pixels.

1. Draw one extra row at the top and the bottom, so partial rows fill the edges of the view during a fractional offset. The strip is clipped per pane.

The view never sits past the grid. The whole part of the offset is what the grid is scrolled by, and the fraction is drawn. So an offset beyond the scrollback would pin the whole part while the fraction kept wrapping, one whole-cell hop per line. That was the nano wobble: a burst still easing when the alt screen, which has no scrollback, took over. The offset is clamped to the scrollback instead. That stops the ease the moment a screen swaps, and it caps how far a fresh terminal's first output eases.

### Where a gesture comes to rest

A gesture rests on a whole line, and on the line it was heading for. A pixel-delta wheel leaves a fractional target, and parking there draws every row shifted by part of a cell. Rounding to the nearest line is the obvious way to settle that, and it is wrong at the end of a gesture. A scroll that stops nine tenths past a boundary goes all the way forward and then hops back. That reads as a glitch even though the travel is under a line. So the stop goes forward, in the direction the wheel was already turning. A scrollbar drag or a track click carries no direction and still rounds to nearest, which is what direct manipulation wants.

The ease curve is deliberately lopsided. A single exponential ease starts at peak speed on its first frame and crawls its last pixels in over a second. Both read wrong. Motion instead builds from rest through two stages: the visual position chases a leading stage, which chases the target. The stop is sharpened by a minimum closing speed over the final fraction of a line. Neither stage can overshoot, so the curve cannot bounce.

User navigation is exempt from the output chase below. Wheel and scrollbar keep a plain fixed ease. The Ease-in setting also paces the wheel ease's start, so both kinds of motion leave rest the same way.

### Easing new output

The same mechanism serves output. When new output pushes content up, `visual_offset` is set back by the lines that arrived and eased to zero, instead of snapping. Output scrolling is an animated target like wheel scrolling.

- The scrollback depth is sampled once per read from the program, not once per frame. A drop in depth means a clear, so everything left is new. An alt-screen switch or a resize starts the measurement over.

- There is one depth baseline, sampled both per read and per frame, and whichever comes first counts the growth. Two baselines let the same lines count twice, which showed as a hop down and an ease back.

- Once the scrollback is full, its depth stops growing. The engine then counts the lines it sends off the top of the screen, and that count carries plain output. See the ledger below.

- A row counts as scrolled only if its old position changed too. A command that prints the same output twice therefore shows the second copy in place, not sliding up from under the prompt.

- Sporadic output eases in over at least `scroll.output_ease_lines` lines, one by default. That floor is capped at 16 and by the history there is to ease through.

### Rows that stay still while output eases

Lines a program redraws in place at the bottom stay still while output eases. A progress line, apt's status bar or a live input block under a transcript did not move in the grid. But the ease shifted the whole pane, so they dropped a row with every new line and slid back up. That showed as a sharp horizontal seam at the top edge of those lines.

- The rows kept still are the ones below the rows a step actually moved, and one of them must be text that reads the same as before. Without that, the last chunk of plain output and a prompt coming back after a command would stay still too.

- Rows below a program's own scroll region are kept still on the engine's record alone, whatever they say.

- The band only grows while one ease runs. Steady output keeps a single ease going, so a band measured once could miss a block caught half drawn. A band that shrank would drop a kept row back into the moving text.

- A single progress line whose text changes on the same frame a new line arrives still drops for that step. Matching rows loosely enough to catch it also kept half-written output lines still.

- A click on a kept row while the ease runs maps to the moving view for that moment.

### Windows and tabs that could not be seen

Nothing that could not be seen eases. A minimized or covered window, and a tab that is not the one shown, build no frames while they are out of view. Whatever arrived meanwhile is a gap, not motion. Easing that gap in would say the wrong thing twice: it animates content that is already old, and it reads as output arriving right now. Coming back on screen is one instant cut instead. The flash that produces is the point, since it marks the update as catching up rather than happening. A window that is merely behind others is not frozen and keeps easing.

### The output chase

Catch-up speed is modeled as one curve on a time/speed graph, and each setting is a named segment of it. The curve starts and ends at zero. Each segment hands exactly one thing to the next: the point where it ended. In order:

- Ease-in lifts the speed from rest over its duration. It is the only segment that can leave zero.

- Ramp-up doubles the speed every one of its periods, toward whichever top applies.

- The top is either the Single-screen speed or unbounded. Single-screen applies while the burst's own first line is still on screen, so a short listing never races. Once a screenful has scrolled past, the ramp climbs exponentially until it keeps up.

- When the cap lifts mid-burst, Ease-in runs once more from the speed it found itself at, and then Ramp-up resumes.

- Ramp-down winds the speed down, and Ease-out brings it to rest.

The segments are straight lines and exponentials adjusted by time, rather than one smooth curve per segment. That is the approach audio and video production use. It is cheap to compute, and each setting stays a plain duration.

Winding down is the same curve traced backwards. Ramp-down is a braking curve computed from where Ease-out comes to rest. At any moment the speed may not exceed what could still be wound down, halving once per Ramp-down period, within the lines left to draw. Applied continuously, that one rule is both the reserve and the slowdown. At speed, the view trails the live output by a braking distance. The moment output stops, the speed rides the curve down and hands off to Ease-out exactly at the stopping band.

While a burst is in flight, the chase drives the view outright. It ends exactly where the stopping band begins, so it needs nothing handed to it and leaves nothing over. A prompt coming back after a command gets the same curve as a long listing.

The backlog is deliberately not capped in lines. The ramps bound the lag in time instead, to about one Ramp-down period at a steady rate. That is what makes the slow start possible.

#### The curve, as specified

This is the original specification the chase was built to. It still describes what the code does.

- General description:
	- Think of each setting as a specific segment of a graph on an X and Y axis.
	- The X-axis is time, the Y-axis is scroll speed.
		- The X-axis may be infinite (or at least unbounded) - say, running `cat /dev/random` then going on vacation.
		- The Y-axis may be infinite (or at least not strictly bounded) - with the same example as above, spitting out lines as fast as the CPU can run the kernel code.
	- The beginning and end of the curve necessarily sit at Y=0. Scrolling starts from stillness, and ends at stillness.
	- Some segment of the "curve" may be perfectly flat on the Y axis, and quite finite (i.e. capped at Y=[max single-screen speed]).
		- Possibly the whole curve, if output fits into a single screen.
	- We don't care about defining or modeling the overall "curve" - only the named segments within it.
	- **Each output-scroll-related setting defines a completely separate "function" (conceptually if not literally), that has extremely limited and precisely-defined influence over the next**.
		- With only a few exceptions, the one and only influence each setting has on the next, is that the *end* X/Y point of the previous function, determines exactly where the START point of the next is located. Those exceptions are documented in the "Parameters" section below.
	- At some point, the middle of the overall "curve" could turn from flat, to quickly ramp up to some nondeterministic, unbounded, virtual Y speed (i.e. when scrolling that was within a single screen, reaches the top of the terminal and must start speeding up to keep up with unlimited output). In that case:
		- The ease-in function takes over again, starting at that X and Y point. Except in this case, Y won't be 0.

- Parameters (all just defined segments of the/a "curve") - each one hands off complete control of scroll speed variability to the next, in this exact order:
	- "Ease-in":
		- This first "function" starts at Y=0 the first time, and describes how fast the speed initially jumps.
	- "Ramp-up":
		- Starts at exactly whatever X/Y "Ease-in" ended at. Can't be <=0, must be a positive slope.
		- Typically - but not necessarily - steeper than "ease-in". (But either way, it can't be <=0, so scroll speed will increase.)
		- This is a rare exception where the exact X/Y end point is not within its control. As mentioned earlier, the Y is defined by the next function in the chain, [max speed], which could be either [max single-screen speed], or [unbounded].
			- The X/Y starting point is defined by the previous function, and the Y ending point is defined by the *next* function. So it does not have full control over either 1) its duration, *or* 2) the length of its own line.
	- [Max speed]: A flat horizontal line in principle (and exactly horizontal when == [max single-screen scroll speed]).
		- [Max single-screen scroll speed] adjustment is in effect for as long as the top of the new output hasn't hit the top of the terminal yet.
		- [Unbounded]: as fast as the output needs to render, to keep up with output.
	- **Note**: The first two functions may or may not be invoked exactly and only one more time - *if* [max speed] was == [max single-screen scroll speed], *and* output now needs to accelerate to any speed faster than [max single-screen scroll speed]:
		- Second invocation of "Ease-in":
			- The second time starts not at Y=0 like the first time, but at Y=[Max single-screen scroll speed]. And again, still describes how fast the speed initially jumps from what it was before.
		- Second invocation of "Ramp-up":
			- Exact same formula, definition, constraints, and unique attribute as first invocation: Starts at exactly whatever X/Y the previous "Ease-in" ended at, and ends at the unbounded Y.
				- How does it know where "unbounded Y" is? Maybe it guesses a sane value, maybe it can see the rate of incoming data, or maybe it just punts and accelerates exponentially until it's reached.
				- As built: it accelerates exponentially until it keeps up.
	- "Ramp-down":
		- Once output ceases yet hasn't all rendered (because SilkTerm will hold a reserve buffer of at least 1 screen when running at top speed), the speed function hands off to "Ramp-down".
		- This starts at the precisely known X and Y handoff point on our time/speed curve.
		- It's almost the inverse of "Ramp-up", *except*:
			- Not only does it know its starting X, it also knows its exact starting Y.
			- It can't end arbitrarily on its own terms, but its end point *is* deterministic. It has to trace "Ease-out" *backwards* (can be pre-computed and stored in memory whenever "Ease-out" setting changes), to know exactly what Y value to end at and hand off to "Ease-out".
			- This adjustment, although not an exact mirror in calculation, "feels" just like the inverse of "Ramp-up".
	- "Ease-out":
		- Almost the inverse of 'Ease-in', at least visually - except that:
			- It's individually adjustable.
			- It must calculate backwards its starting X point, based on the inrushing known end of buffered content.
			- Its end point is *always* Y=0, and its X value can be calculated in real-time ahead of time. From there it can work backwards and tell (or be queried by) "Ramp-down", its own *exact starting* X and Y ahead of time, so that "Ramp-down" will know its own ending X/Y.
			- This adjustment, although not an exact mirror in calculation, "feels" just like the inverse of "Ease-in".

- Common behavior:
	- Typical scroll flow can take these routes - which don't/shouldn't need individual code paths, just for illustration:
		- Scenario 1: <1 screen of text, from the top:
			- "Instant" output.
		- Scenario 2: >1 screen of text, from the top:
			- First screen's worth of output appears "instantly". But once it needs to start scrolling up, then:
			- Ease-in has full control of speed. Then hands off to the ramp-up function. Then to unbounded speed. At some arbitrary point depending on output, the ramp-down function takes over, and finally ease-out.
		- Scenario 3: <1 screen of text, from the bottom (with a screen full of text above):
			- Ease-in begins with full control of speed from the start.
			- Then hands off to the ramp-up function.
			- Then to [maximum single-screen] speed.
			- At some arbitrary point when output ends, the ramp-down function takes over.
			- Finally the ease-out function.
		- Scenario 4: >1 screen of text, from the bottom (with a screen full of text above):
			- Ease-in begins with full control of speed from the start.
			- Then hands off to the ramp-up function.
			- Then to unbounded speed.
			- At some arbitrary point when output ends, the ramp-down function takes over.
			- Finally the ease-out function.
		- Other scenarios (e.g. output starts in the middle of the screen) can be inferred from those 4 scenarios.

### The five settings

The five settings that shape the chase are listed in the order they are watched, rather than grouped by how they work:

- Ease-in: how sharply the view leaves rest.

- Ramp-up: how hard it speeds up.

- Single-screen speed: the top speed while the burst still fits on screen.

- Ramp-down: how hard it brakes as the output runs out.

- Ease-out: how crisply it comes to rest.

All five run one way in the dialog: higher is faster, harder or crisper. Each is stored as a time in milliseconds, where more is gentler, and the dialog maps it onto a 1 to 100 scale that falls as the time grows. The ease ends are stored as durations, not speeds, so no slider reads backwards next to its partner. The shipped defaults read 50, 75, 75, 75 and 40.

These five are active lines in the shipped config, so a changed default reaches only a new config, or one where the line is edited or reset.

One "Smooth scrolling" switch (`scroll.smooth`) turns all scroll animation off at once: the wheel ease, the output ease and the full-screen slide. It leaves the five settings alone and grays their rows. Wheel lines and the scrollbar still apply either way. Every effect group in Settings has the same kind of master switch.

### Full-screen programs

Scrollback and output easing both have an easy signal: the wheel turns, or the scrollback grows. Full-screen ("alt-screen") programs such as less, vim, nano and tmux are the hard case. They own the screen. Most scroll a region of it with the terminal's own scroll commands, such as a linefeed at the bottom of a DECSTBM region or `CSI n S`, and the terminal throws the outgoing rows away because the alt screen keeps no scrollback. Some repaint whole lines in place instead, and then the grid just changes under us. Two mechanisms cover the two kinds, and the exact one is asked first.

`scroll.smooth_apps` turns this slide off on its own. It is on by default.

#### The engine's scroll ledger

The engine fork records every region scroll as it happens: which rows moved, by how many lines, and the rows the scroll pushed out. Each frame the pane reads it. That is the whole answer for anything that scrolls the terminal.

- The count is exact and uncapped, so a burst that replaces the screen between two frames is still one known number.

- The region says which rows stay still, and the outgoing rows are real content rather than a guess.

- This is what lets tmux ease at all, since it runs on the alt screen where there is nothing else to measure.

- The engine also counts every line it sends off the top of the screen into the scrollback. That count carries plain output once the scrollback is full. It covers a whole-screen scroll, a scroll of a region that starts at the top row, and a screen clear. apt's progress bar pins the last row under a region like that. The count is summed on its own, so a region scroll in the same read does not lose it.

- A recorded scroll that moved only blank rows is not eased. Nothing visible moved. A line editor such as ble.sh scrolls the blank rows under the cursor to make room for its prompt, and easing that drags the prompt down from behind the rows above.

- A scroll down that only pushes off rows that were blank to the bottom of the screen pops in. That is content making room for itself, such as an input box growing as a paste arrives. Easing it drops the new lines out from behind the rows above, last line first. The kept rows alone cannot tell this case, since less blanks its prompt row in the same write as its scroll back, so the last frame has to agree. `SLIDE_DOWN_INTO_ROOM` in `pane.rs` brings the slide back.

- One direction at a time. A scroll the other way starts the ledger over, and so does a scroll of a region that shares no row with the one in flight.

- A scroll of an overlapping region carries on. The ledger narrows to the rows both scrolls moved and keeps the rows that cross that edge. The pane takes the record each frame, with the region and direction left open while a slide is in flight, and clears it only at rest. nano is why: its edit window is rows 2 to 45, and whenever the line leaving it is blank, ncurses scrolls rows 2 to 46 instead. Starting over on that threw the slide away every other step of an up run.

#### Fingerprints where the engine recorded nothing

A program that repaints its lines with cursor moves, or ConPTY on Windows re-sending a scroll as a repaint, leaves no ledger entry. For those, every frame fingerprints each visible row with a hash of its characters. `scroll_shift_signed` looks for the vertical shift, up to 24 lines either way, that lines up the most rows, and enough of them must really have moved. An in-place status-line redraw lines up by position but did not move, so it cannot start a false slide. The bands it keeps still are measured the same way, as the unchanged rows at each end.

ConPTY sends region scrolls that way on the normal screen too. So the slide also runs on the normal screen while the view is following the bottom and the scrollback is not growing.

#### Easing it into place

The grid is already at the new position. To animate, the content is pushed back by the shift and that offset is eased to zero. It runs the same curve and the same five settings as plain output. A program scrolling its region by N lines looks exactly like N lines printed at a prompt, and a unit test keeps the two on one path.

#### Filling the gap with the rows that left

The gap the slide reveals cannot be redrawn from the model. The rows the ledger kept, or on the fingerprint path the rows captured styled a frame earlier, go into a strip. The strip is drawn welded to the sliding content's edge and rides the same eased offset, so the gap is always filled with real outgoing content, with its own cell backgrounds and scrim. The offset can never open more gap than the strip has. About three screens are kept, so a long burst eases through its tail.

- One row is pinned by reading, not by the region. A pager like less scrolls the whole screen and rewrites its prompt on the bottom row afterwards. That row reads the same after the scroll as before, so it is kept still even though the region says it moved. A blank row never qualifies. tmux scrolls first and draws the freed row a moment later, and a frame built in between would otherwise pin that row and make its new line pop in while the rest slides.

- The edge the strip fills from can have a row the recorded scroll does not account for. muffer repaints its "1 new message" pill over the last row of its transcript after each scroll. That row slid with the text while its old copy rode in the strip, so the pill showed twice. Such rows are counted from the edge, up to a quarter of the region, and only when the row past them moved as recorded. A frame repainted wholesale keeps nothing still. The strip then takes its rows from the frame before. The kept edge may grow during a slide, since a frame can be built between the scroll and the repaint.

- Rows under the slide that moved the same way, only further, slide with it, so a whole message moves as one.

A sliding frame is built from four parts:

- The scrolled-off strip, filling the revealed gap.

- The current middle region, sliding over it, clipped between the two bands.

- The title and status bands, drawn unshifted.

- The readability scrim, following the whole thing, strip included.

A full-screen program's start or end is an instant swap, not a scroll. The slide is canceled on the spot, and the detectors start over.

#### What makes this hard

- The stock engine has no scroll event and does not expose the program's scroll region. The ledger is our own addition to the fork. Without it, "a scroll happened, by N lines, with these fixed bands" has to be inferred, and the inference must reject false positives. An in-place redraw must not bounce.

- The off-screen content cannot be recovered from the grid. The ledger takes each row on its way out and leaves a spare in its place, which the grid resets as it would have reset the row. So keeping rows costs a full-screen program next to nothing. Only a row that stops moving while staying on screen is still copied, which happens when an overlapping region carries on. The fingerprint path still has to capture it styled a frame ahead.

- tmux draws lazily. On a burst it scrolls the outer terminal by the grid's whole advance and only then redraws, so the rows that leave are whatever it had drawn before, sometimes blank. Every terminal's scrollback gets the same stale rows. The strip is faithful to what tmux sent, not to what its pane has.

- tmux can only scroll a pane that spans the full width. Side-by-side panes are repainted, so they fall to the fingerprint path and mostly cut.

- The fixed bands mean three regions have to tile with no gap and no overlap.

- All of it is sub-line and per frame, on the same fractional renderer and scrim pass, under a redraw loop that cannot trust X11 redraw requests.

### The wheel in full-screen programs

The wheel sends cursor keys only on the alt screen, with alternate-scroll on and no mouse mode. The normal screen always gets the smooth scrollback, in proportion to the notches turned. When a program has turned on mouse tracking, the wheel is reported to it, one event per line and capped. Shift keeps the wheel local.

### The scrollbar

- A scrollbar floats over each pane's right edge and never changes the grid, so turning it on or off, or changing its width, reflows nothing. It is 16 DIP wide by default, from 4 to 64.

- It fades while the view is idle at the bottom, and comes back on a scroll or when the pointer nears it. It stays up the whole time the view is parked in the scrollback. Always visible is a setting.

- Dragging the handle scrolls, and a click on the track pages that way. A dragged handle follows the pointer exactly while the text eases in behind it.

- Full-screen programs keep no scrollback, so they get no scrollbar.

- With the minimap on, there is still one scrollbar, at the far right over the map's edge. The minimap marker and the scrollbar handle are pinned together during either drag.

## Alternative ideas

### Unconsidered

- Turn smooth scrolling off when a program is seen writing to the screen directly. Raised in "When running `sudo apt update`, the progress bar at the bottom bounces" (Opened 20260628-083740). The ledger fixed the case that raised it.

### Rejected

- Snap the output ease during a burst of lines. It stopped apt's status bar bouncing, but a command's output arrives in one burst under a millisecond, so every multi-line output snapped. Any burst threshold above one frame breaks the feature. Backlog: "Smooth scrolling is broken" (Opened 20260628-083740).

- Cap the backlog at 16 lines and drive the speed from how full it is. Any real burst filled the cap in about a tenth of a second, and after that the view rode the raw output rate and the speed settings did nothing visible.

- The chase as a speed cap on the plain navigation ease. It only acted while the ease was the faster of the two, so a short advance got the navigation ease instead, which no setting reaches, and then picked back up at the sharpened stop. A returning prompt stalled every time. Backlog: "A prompt coming back after a command slides in oddly" (Opened 20260905-124907).

- Relax the speed only during a lull. In practice it never fired, the Ramp-down setting did nothing visible, and stops from speed were cliffs.

- Smooth sigmoid-family curves for each segment, and the straight-line options that adjust height or length rather than time. Straight and exponential segments adjusted by time were chosen. See [The curve, as specified](#the-curve-as-specified).

- A single exponential ease for the wheel. It starts at peak speed and crawls its last pixels.

- Rounding a wheel gesture to the nearest line. It hops back at the end of a gesture.

- Forking the Alacritty application. See the Alacritty design doc.

### Superseded

- "Initial scroll speed", the slow starting speed that sped up under a burst. It fed four mechanisms at once, so every slider seemed to move every other one. The chase's Ease-in took over leaving rest. Backlog: "Smooth-scroll enhancement" (Opened 20260628-083740).

- "In-view output speed" (`scroll.inview_tau_ms`). It is now the chase's Single-screen speed (`scroll.single_screen_tau_ms`). Backlog: "Scroll-on-output enhancement: One additional setting" (Opened 20260629-110720).

- Filling the gap with the whole previous frame. Its fill could trail the ease and moved at every new capture, which read as a pulsing shadow under a title bar. The scrolled-off strip replaced it. Backlog: "The Notorious "Bouncing Shadow in Wobbly Nano" bug" (Opened 20260707-182523).

- Inferring the advance at a full scrollback by matching row fingerprints, first strictly, then by best coverage. The engine's own count replaced both on the normal screen. Backlog: "Some output, like debug output will bounce badly" and "Severe bug: `flatpak update` output bounces wildly" (Opened 20260723-135701).

- Leaving full-screen programs to jump a line at a time, as too fragile to animate. The ledger and fingerprint paths now slide them. Backlog: "In `nano`, scrolling isn't smooth" (canceled 20260713-142351).

- The wheel sending cursor keys whenever either the alt screen or alternate-scroll was on. It recalled shell history at a bare prompt. Backlog: "Native keybindings for `less` don't work" (Opened 20260628-083740).

- "The engine does not expose the scroll region." True of the stock crate, and the reason apt's status bar bounced. The ledger in the fork records it now. Backlog: "When running `sudo apt update`, the progress bar at the bottom bounces" (Opened 20260628-083740).

## Research findings

- A slow renderer hides per-frame scroll bugs. A software renderer takes several scroll steps per frame where a real GPU takes one. Reproduce by feeding the recorded bytes in a unit test.

- nano's ncurses switches its scroll region between rows 2 to 45 and rows 2 to 46, depending on whether the line leaving is blank.

- ble.sh makes room for its prompt with insert-line and delete-line on the blank rows under the cursor. The engine reports that as a region scroll.

- less rewrites its prompt row after each scroll, and blanks it in the same write as a scroll back.

- apt keeps its status bar under a scroll region that starts at the top row, so its log lines grow the scrollback.

- tmux scrolls the outer terminal first and draws afterwards, and repaints rather than scrolls a pane that is not full width.

- ConPTY on Windows re-sends a region scroll as a repaint in place.

- Copying each outgoing row took about a third off the engine's parse speed on the alt screen. Swapping in a spare row brought it to about 5%, and a test keeps it under 10%.

- The scroll harness scenes (less, vim, nano, muffer) slide the same under Wayland as under X11.

- At the start of the project, WezTerm, kitty, foot, Alacritty, GNOME Terminal and Konsole all snapped to whole rows.

## Roadmap

- Stacked tmux panes printing at once: only one slides, and the other jumps. The fix is a ledger entry and a slide per region. Backlog: "With two tmux panes stacked and both printing, only one pane slides at a time" (deferred, Opened 20260911-113647).

- Side-by-side tmux panes still cut, since tmux repaints them.

- A per-program list that turns smooth scrolling off while that program runs. The scroll part could be done per pane. Backlog: "Config file: For each feature listed below, allow user to list programs" (deferred, Opened 20260708-115155).

- A single progress line that changes on the same frame a new line arrives still drops for that step.

## Related backlog issues

- "Huge quality-of-life improvement: Re-thought-throug, rationalized scroll-on-output settings refactor" (closed 20260804-084202)

- "Smooth-scroll enhancement" (Opened 20260628-083740)

- "Scroll-on-output enhancement: One additional setting" (Opened 20260629-110720)

- "A single boolean option to disable/enable smooth scrolling, without changing other settings" (closed 20260802-123859)

- "Need scrollbars. (Disable in Settings.)" (Opened 20260731-115810)

- "Does not work very well under tmux" (Opened 20260826-123553)

- "A prompt coming back after a command slides in oddly" (Opened 20260905-124907)

- "Muffer: When several lines of text are entered, upon hitting "Enter" key" (Opened 20260923-114755)

- "New text added to a screen with enough room to not have to scroll up" (Opened 20260919-121522)

- "nano:" (Opened 20260911-124508)

- "The dreaded "Nano Bounce Bug" is back" (Opened 20260709-115247)

- "The Notorious "Bouncing Shadow in Wobbly Nano" bug" (Opened 20260707-182523)

- "A wheel gesture can end up moving backwards about one line" (Opened 20260813-091542)

- "Windows: no smooth-scrolling in full-screen / scroll-region apps" (closed 20260719-191037)

- "Smooth scrolling is broken" (Opened 20260628-083740)

- "When running `sudo apt update`, the progress bar at the bottom bounces" (Opened 20260628-083740)

- "In `nano`, scrolling isn't smooth" (canceled 20260713-142351)

- "With two tmux panes stacked and both printing, only one pane slides at a time" (deferred, Opened 20260911-113647)

- "Config file: For each feature listed below, allow user to list programs" (deferred, Opened 20260708-115155)
