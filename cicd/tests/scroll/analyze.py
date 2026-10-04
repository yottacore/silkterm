#!/usr/bin/env python3

##	History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

"""Analyze a SILK_SCROLLDBG trace for one scroll scenario.

Reads the per-frame trace on stdin (the `SCROLLDBG ...` lines SilkTerm writes to
stderr when SILK_SCROLLDBG is set) and checks the invariant the scenario is
supposed to hold:

  --mode slide    : the smooth alt-screen slide must engage (app_off != 0) and the
                    content must move monotonically - no bounce (a delta against the
                    scroll direction). Optionally the static-top-band count must match
                    --expect-st (0 for less/vim, which have no title bar).
  --mode hardcut  : the app has a static top band (nano/muffer) so the slide is
                    deliberately disabled - the shift is still detected but app_off
                    must stay 0 across every frame (a plain page redraw).
  --mode pinned   : output easing on the normal screen under a live block redrawn
                    in place (muffer's shape). The block must be held still:
                    most easing frames carry ob == --expect-sb, and none more.
  --mode still    : the alt screen took over while plain output was still easing.
                    There is no scrollback behind it, so the view must be at rest
                    on the spot: frac stays 0 on every frame. A leftover ease shows
                    up as the fraction wrapping through a whole cell once per line
                    (the nano wobble). One frame is enough - a still screen builds
                    only when something changes. The output must be seen easing
                    before the swap, or there was nothing to stop and the check
                    tested nothing.
  --mode popin    : content growing down into blank rows on a half-empty screen
                    (an input box taking a paste). That is room, not a scroll, so
                    a step down must never slide: app_off never goes below 0.
                    Steps back up may slide. No step down at all is a skip.

Exit codes: 0 pass, 1 real regression (a genuine violation with data), 2 skip
(not enough trace / the scene never scrolled - an environment/timing miss, not a
code regression). The runner treats 2 as non-fatal unless --strict. A slide scene
that scrolled but never slid is a regression, not a skip.

The bounce metric reconstructs a reference line's screen position as
`app_off - cumulative_shift`: while easing, app_off shrinks with the grid held, so
the position glides one way; a step adds `shift` to both the grid advance and
app_off, so the position stays continuous across steps. Any reversal is a bounce.
"""

import argparse
import re
import sys

TRACE = re.compile(
    r"SCROLLDBG f=(\d+) pane=(\d+) sh=(-?\d+) app_off=(-?[\d.]+) "
    r"slide_sh=(-?[\d.]+) st=(\d+) sb=(\d+) frac=([\d.]+)"
    r"(?: alt=(\d))?(?: ob=(\d+))?"
)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--mode", required=True, choices=["slide", "hardcut", "still", "pinned", "popin"])
    ap.add_argument("--expect-st", type=int, default=-1)
    ap.add_argument("--expect-sb", type=int, default=-1)
    ap.add_argument("--label", default="scene")
    ap.add_argument("--eps", type=float, default=0.02)
    # frames traced before the scene's loop began, which the check must not judge
    ap.add_argument("--skip-frames", type=int, default=0)
    a = ap.parse_args()

    frames = []
    for line in sys.stdin:
        m = TRACE.search(line)
        if m:
            frames.append(
                {
                    "sh": int(m.group(3)),
                    "app_off": float(m.group(4)),
                    "st": int(m.group(6)),
                    "sb": int(m.group(7)),
                    "frac": float(m.group(8)),
                    "alt": int(m.group(9)) if m.group(9) is not None else 1,
                    "ob": int(m.group(10) or 0),
                }
            )

    frames = frames[a.skip_frames:]

    def out(tag, msg):
        print(f"[ {tag} {a.label}: {msg} ]")

    if a.mode == "pinned":
        easing = [f for f in frames if f["alt"] == 0 and f["frac"] > a.eps]
        if len(easing) < 10:
            # The opening burst eased, so the renderer is up and the loop's lines cut.
            if a.skip_frames >= 10:
                out("FAIL", f"only {len(easing)} easing frames in the loop, after "
                           f"{a.skip_frames} before it: the output is not easing")
                return 1
            out("SKIP", f"only {len(easing)} easing frames (GL warmup / timing?)")
            return 2
        over = [f for f in easing if f["ob"] > a.expect_sb]
        if over:
            out("FAIL", f"band wider than the block on {len(over)} frame(s) "
                       f"(ob={over[0]['ob']}, block {a.expect_sb}): output was held")
            return 1
        held = sum(1 for f in easing if f["ob"] == a.expect_sb)
        if held * 5 < len(easing) * 4:
            out("FAIL", f"block held on only {held} of {len(easing)} easing frames")
            return 1
        out("PASS", f"block held still on {held} of {len(easing)} easing frames")
        return 0

    if a.mode == "still":
        swap = next((i for i, f in enumerate(frames) if f["alt"] == 1), None)
        if swap is None:
            out("SKIP", "no trace frames (GL warmup / timing?)")
            return 2
        # the trace only prints a normal-screen frame that carries a fraction
        if not any(f["alt"] == 0 and f["frac"] > a.eps for f in frames[:swap]):
            out("FAIL", "no output was easing when the alt screen took over, so nothing was tested")
            return 1
        frames = [f for f in frames if f["alt"] == 1]
        moving = [f for f in frames if f["frac"] > a.eps]
        if moving:
            worst = max(f["frac"] for f in moving)
            out("FAIL", f"view still easing on the alt screen: {len(moving)} of "
                       f"{len(frames)} frame(s) carry a fraction (max {worst:.3f})")
            return 1
        out("PASS", f"landed at rest: frac 0 across {len(frames)} frame(s)")
        return 0

    if len(frames) < 5:
        out("SKIP", f"only {len(frames)} trace frames (GL warmup / timing?)")
        return 2
    if not any(f["sh"] != 0 for f in frames):
        out("SKIP", f"scene never scrolled (no sh!=0 across {len(frames)} frames)")
        return 2

    engaged = [f for f in frames if abs(f["app_off"]) > a.eps]

    if a.mode == "popin":
        down = [f for f in frames if f["sh"] < 0]
        if not down:
            out("SKIP", f"no step down across {len(frames)} frames")
            return 2
        slid = [f for f in frames if f["app_off"] < -a.eps]
        if slid:
            worst = min(f["app_off"] for f in slid)
            out("FAIL", f"slid down into empty rows on {len(slid)} frame(s) "
                       f"(app_off {worst:.3f})")
            return 1
        out("PASS", f"popped in: {len(down)} steps down, none slid")
        return 0

    if a.mode == "hardcut":
        if engaged:
            worst = max(abs(f["app_off"]) for f in engaged)
            out("FAIL", f"expected hard-cut but slide engaged on {len(engaged)} "
                       f"frame(s) (max app_off={worst:.3f})")
            return 1
        out("PASS", f"hard-cut: scrolled, app_off stayed 0 across {len(frames)} frames")
        return 0

    # slide mode
    if not engaged:
        out("FAIL", f"scrolled but never slid (app_off stayed 0 across {len(frames)} frames)")
        return 1
    if a.expect_st >= 0:
        bad = [f for f in engaged if f["st"] != a.expect_st]
        if bad:
            out("FAIL", f"expected st={a.expect_st} while sliding but saw st={bad[0]['st']}")
            return 1
    if a.expect_sb >= 0:
        bad = [f for f in engaged if f["sb"] != a.expect_sb]
        if bad:
            out("FAIL", f"expected sb={a.expect_sb} while sliding but saw sb={bad[0]['sb']}")
            return 1

    # bounce: pos = app_off - cumulative shift; check it never reverses direction
    cum = 0.0
    pos = []
    for f in frames:
        cum += f["sh"]
        pos.append(f["app_off"] - cum)
    active = [i for i, f in enumerate(frames) if abs(f["app_off"]) > a.eps]
    lo, hi = active[0], active[-1]
    seg = pos[lo : hi + 1]
    net = seg[-1] - seg[0]
    want = -1 if net < 0 else 1
    reversals = 0
    worst = 0.0
    for i in range(1, len(seg)):
        d = seg[i] - seg[i - 1]
        if abs(d) > a.eps and (1 if d > 0 else -1) != want:
            reversals += 1
            worst = max(worst, abs(d))
    if reversals:
        out("FAIL", f"{reversals} content bounce(s) during slide "
                   f"(max {worst:.3f} cells against the scroll direction)")
        return 1

    out("PASS", f"slide engaged {len(engaged)}f, st ok, monotone (0 bounces)")
    return 0


if __name__ == "__main__":
    sys.exit(main())

##	History:
##		- 20260706 JC: Created.
