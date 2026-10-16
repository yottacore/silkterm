# Scroll regression harness

Deterministic, headless check of the smooth-scroll behavior, driven off the permanent `SILK_SCROLLDBG` per-frame trace in `pane.rs`. It runs SilkTerm headless (the private `:98` Xvfb on X11, or a `cage` kiosk on Wayland), feeds it scenes that model how real full-screen apps repaint, and asserts each app scrolls the way it is *supposed* to right now.

Run it:

~~~sh
cicd/tests/scroll/run.bash            # deterministic scenes on X11 (gating)
cicd/tests/scroll/run.bash --wayland  # same scenes under a headless Wayland compositor (cage)
cicd/tests/scroll/run.bash --real     # + best-effort smoke of real less/nano/vim.tiny
cicd/tests/scroll/run.bash --help
~~~

cicd runs `run.bash` as part of stage 3 (skipped under `--quick`; reported as skipped on an environment miss, fatal on a measured regression). With `SCROLL_HARNESS_WAYLAND=1` in `config.bash` it runs a second time with `--wayland`, so the same scenes must pass on the Wayland backend too (proving the Linux binary scrolls identically on both engines). The Wayland pass self-skips where `cage` is not installed.

## X11 vs Wayland

The trace is read from stderr, not from pixels, so both engines run the exact same scenes and assertions. On X11 the app renders straight onto the private `:98` Xvfb; on Wayland each scene runs SilkTerm as the single client of a throwaway headless `cage` kiosk (software `pixman` compositor + software Vulkan), with `DISPLAY` unset so winit can only pick the Wayland backend. `cage` needs no real GPU or display.

## What each scene expects

Scenes self-scroll on a timer (no key injection - unreliable here), so the trace is deterministic. `analyze.py` reads it and checks:

| Scene           | shape (static bands)                                                                                                                                                                       | expected now
| :-------------- | :----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | :--------------------------------------------------------------------------------------------------------------------------------------
| less            | none top, 1 bottom                                                                                                                                                                         | slide, monotone (0 bounces)
| vim             | none top, 2 bottom                                                                                                                                                                         | slide, monotone (0 bounces)
| nano            | 1 top (title), 2 bottom                                                                                                                                                                    | slide, monotone (0 bounces)
| muffer          | 2 top (header), 1 bottom                                                                                                                                                                   | slide, monotone (0 bounces)
| tmux            | region above 1 status row, real linefeeds                                                                                                                                                  | slide off the engine's scroll count, sb 1, monotone
| altenter        | nano shape, entered mid-ease                                                                                                                                                               | still: frac 0 on every alt frame
| chrome          | transcript with a live block redrawn under it, normal screen                                                                                                                               | pinned: the block holds still while output eases, ob 4
| aptbar          | region over all but the last row on the normal screen, scrollback already full                                                                                                             | pinned: the lines still ease and the bar's row holds still, ob 1
| paste / pasteil | input box on a half-empty normal screen growing a line a step, repainted / insert-line                                                                                                     | popin: no step down ever slides (app_off never below 0)
| prompt          | a shell under ble.sh: a short command, then its two-row prompt drawn with insert- and delete-line under the cursor, sometimes redrawn in place, on a half-empty screen and then a full one | popin: the prompt shows where it lands, never sliding down out from under the output
| clear           | `clear`, `tput clear`, Ctrl+L's bytes, a reset and ble.sh's redraw of a long prompt after `clear`, over a full screen, scrollback full and then not                                        | instant: no normal-screen frame between the two alt-screen marks carries a fraction
| tmuxclear       | a clear inside tmux as tmux draws it with its status line on: the pane's rows scrolled away under the prompt's line, alt screen, after bursts that scroll the pane away whole              | wipe: between a region scroll of 3 and one of 5, no frame takes a step of 8 rows or more or slides past 4, and a burst before them slid

`altenter` is a different check: 400 lines of plain output, then the alt screen as soon as the trace shows them easing, so the ease is mid-flight. The scene reads its own trace for that, since a fixed gap missed the ease on a busy box. The alt grid has no scrollback, so the view has to be at rest the moment it swaps in; a leftover ease renders as the fraction wrapping through a whole cell once per line of backlog, which is the nano wobble. One frame is enough, since a still screen only builds when something changes. The scene is `scenes/altenter.bash`; a scene with a script of its own under `scenes/` runs that instead of `scene.bash`.

Title-bar apps (nano, muffer) slide again: `SLIDE_TOP_BAND_APPS = true` in `pane.rs`, with the reveal gap filled by the scrolled-off strip (the styled rows each step pushes out of the region), so the fill is exact and nothing repositions. If the toggle is ever turned back off, change the nano/muffer scenes from `slide` back to `hardcut` in `run.bash`.

The repaint scenes exercise the fingerprint detector; the `tmux` scene exercises the other path, where the engine records the scroll itself (count, region, and the rows that left) and nothing is inferred. Plain shell-output scrolling (`ls -lA`, a command finishing on the last line, a fast burst) is covered by the library tests in `cargo test` (`pane.rs`/`scroll.rs`): the advance comes from scrollback growth, or from that same engine count once the scrollback is full, and the easing-monotonicity checks there guard the rest.

## Exit codes

`0` all pass, `1` a real regression was measured, `3` nothing ran because something is missing (the binary, `python3`, the display or `cage`). cicd reports a `3` as skipped for that arm, never as OK.

A scene with too little trace or that never scrolled is a **skip** (an environment or timing miss), non-fatal unless `--strict`, but a run where no scene passed fails. A slide scene that scrolled and never slid fails. So does an `altenter` run where no output was easing when the alt screen took over, since it tested nothing. The one exception is when the scene waited 10 s for the ease and never saw it. It leaves a note saying so, and the run is a skip.

`clear` checks frames that should never be drawn moving, and a frame at rest prints nothing, so it brackets the clears with two short trips to the alt screen. The first scrolls down a line at a time and the second 9 lines at once, which is how the check tells the start from the end. A burst before the first trip has to be seen easing, or the run tested nothing and fails.

`tmuxclear` uses the same bracketing on the alt screen, where tmux keeps the outer terminal. Its marks are region scrolls of 3 and 5 rows, told apart by the trace's `sh`. Each clear goes over a screen filled in place, so only the clear can scroll.

`aptbar` prints past the scrollback's depth before its loop, because a full scrollback stops growing and the output ease then runs off the engine's own count. A pinned scene whose opening burst eased and whose loop did not is a failure, not a skip.
