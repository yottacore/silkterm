#!/usr/bin/env python3

##	- Purpose:
##		Per-monitor window size and font zoom, under quasi-random layouts and
##		steps, judged against a model of what the window should do. Each case
##		is a pure function of one seed:
##			- A headless sway layout of 2 to 4 outputs, each at its own
##			  resolution and scale, whole or fractional, placed edge to edge.
##			- The two switches, the last size anywhere, and sizes kept for some
##			  of the outputs, each with a font zoom, and now and then one for a
##			  monitor that isn't there.
##			- Native Wayland, or X11 through Xwayland.
##			- A launch on one output, maybe with a size or a font size on the
##			  command line, and maybe --remember-size with it, then moves,
##			  drags, drops with a sliver left on another output, resizes, font
##			  zooms, scale changes and relaunches.
##		After each step the model says which monitor the window should count
##		itself on, and which size and font zoom it should take there, and the
##		shell inside says what it got. Nothing may change while a drag is held.
##		At the end the sizes on disk have to be exactly the ones the user set,
##		less any set in a window with a size or a font size from the command
##		line and no --remember-size, which saves nothing.
##		Where a platform can't tell a window what the model knows, the model
##		names that limit and allows for it, and the run counts each one.
##	- Syntax: run.py [--bin PATH] [--seed N | --from N] [--secs S] [--wayland|--x11]
##	                 [--plan N] [-v] [--keep]
##		--seed N   that case alone, as a failure prints it; SILK_DPIRAND_SEED too
##		--from N   seeds from N on; by default from one taken off the clock
##		--secs S   run cases until S seconds are used, one at least;
##		           SILK_DPIRAND_SECS too (default 60)
##		--plan N   print the plans of N cases from --seed and run none
##		-v         print every case's steps, not just a failed one's
##		--keep     keep every case's folder, not just a failed one's
##	- Exit: 0 passed, 1 a case failed (its seed is printed), 2 bad arguments,
##	  3 nothing ran (no binary, sway, wtype, xdotool or software Vulkan).
##	- Test ID: EshVccz
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

import argparse
import json
import math
import os
import random
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from collections import Counter
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
sys.path.insert(0, str(HERE.parent))
import _testdir  # noqa: E402

LVP = Path("/usr/share/vulkan/icd.d/lvp_icd.json")
VPOINTER = HERE.parent / "monwl" / "_vpointer.py"

Rect = tuple[int, int, int, int]
Grid = tuple[int, int]

## Modes with a scale that divides them evenly, so every output has a whole
## size in the layout and there are no seams from rounding.
COMBOS: list[tuple[tuple[int, int], float]] = [
	((1920, 1080), 1.0), ((2560, 1440), 1.0), ((1280, 1024), 1.0), ((1080, 1920), 1.0),
	((1680, 1050), 1.0), ((2560, 1080), 1.0),
	((2560, 1600), 2.0), ((3840, 2160), 2.0), ((2880, 1800), 2.0), ((2560, 1440), 2.0),
	((2400, 1350), 1.5), ((2880, 1620), 1.5), ((1920, 1200), 1.5),
	((2400, 1350), 1.25), ((2560, 1600), 1.25),
]
SCALES = (1.0, 1.25, 1.5, 2.0)

## Steps, and how often each comes up.
KINDS = {"move": 3.0, "drag": 4.0, "resize": 2.0, "zoom": 2.0, "relaunch": 1.5, "rescale": 0.8, "fullscreen": 0.6}

## Known faults that a step may run into, each with its backlog item. A case
## that hits one passes and the run counts it; the entry goes when the item
## is fixed.
RESCALED = "a scale change moves the grid a few cells"
CAPPED = "sway holds a floating window to the size of the whole layout"
TOLD_NOTHING = "a Wayland window is told nothing of a move"
## Wayland and Xwayland show a window no button held on the compositor's title
## bar, so a move with the pointer away could be a drag held still. The look
## waits for the pointer back over the window, a key or focus.
WAITS = "a window that can't see the buttons waits for the person after a move"
## A resize that takes a Wayland window off an output is told to it once a
## frame at the new size is up, which in software can come after the size is
## saved, so it may still count the output it was on as the size was set.
LEFT_LATE = "a Wayland window is told it left an output once a frame at its new size is up"
KNOWN: dict[str, str] = {}


def whole_size(mode: tuple[int, int], scale: float) -> tuple[int, int] | None:
	w, h = mode[0] / scale, mode[1] / scale
	if not (w.is_integer() and h.is_integer()) or min(w, h) < 700 or max(w, h) < 1200:
		return None
	return int(w), int(h)


@dataclass
class Out:
	mode: tuple[int, int]
	scale: float
	at: tuple[int, int] = (0, 0)

	def size(self) -> tuple[int, int]:
		got = whole_size(self.mode, self.scale)
		assert got, (self.mode, self.scale)
		return got

	def rect(self) -> Rect:
		return (self.at[0], self.at[1], *self.size())


@dataclass
class Kept:
	cols: int
	rows: int
	zoom: int

	def __str__(self) -> str:
		return f"{self.cols}x{self.rows} zoom {self.zoom}"


@dataclass
class Step:
	kind: str
	to: int = 0
	pointer: bool = True
	style: str = ""
	nearby: int = -1
	share: float = 0.0
	along: float = 0.5
	hold: float = 0.0
	size: tuple[float, float] = (0.5, 0.5)
	zoom: int = 0
	cli: Grid | None = None
	font: int | None = None
	persist: bool = False
	scale: float = 1.0

	def __str__(self) -> str:
		back = "" if self.pointer else ", pointer stays away"
		match self.kind:
			case "move":
				return f"move to output {self.to + 1}{back}"
			case "drag":
				extra = f", {self.share:.0%} left on output {self.nearby + 1}" if self.style == "sliver" else ""
				held = f", held astride output {self.nearby + 1} for {self.hold:.1f} s" if self.hold else ""
				return f"drag to output {self.to + 1}, {self.style}{extra}{held}{back}"
			case "resize":
				return f"resize to {self.size[0]:.0%} x {self.size[1]:.0%} of its output{back}"
			case "zoom":
				return f"font zoom {'reset' if self.zoom == 0 else f'{self.zoom:+d}'}{back}"
			case "relaunch":
				return f"relaunch on output {self.to + 1}{pins(self.cli, self.font, self.persist)}"
			case "rescale":
				return f"output {self.to + 1} to scale {self.scale:g}"
			case "fullscreen":
				return "fullscreen and back"
		return self.kind


@dataclass
class Plan:
	seed: int
	arm: str
	outs: list[Out]
	remember: bool
	per_monitor: bool
	last: Kept
	fixed: Grid
	kept: dict[int, Kept]
	stale: Kept | None
	start: int
	cli: Grid | None
	font: int | None
	persist: bool
	steps: list[Step]

	def describe(self) -> list[str]:
		lines = [f"seed {self.seed}: {self.arm}, remember size {'on' if self.remember else 'off'}, per monitor {'on' if self.per_monitor else 'off'}, last size {self.last}"]
		for i, out in enumerate(self.outs):
			kept = self.kept.get(i)
			lines.append(f"  output {i + 1}: {out.mode[0]}x{out.mode[1]} at {out.scale:g}, at {out.at[0]},{out.at[1]}, " + (f"kept {kept}" if kept else "nothing kept"))
		if self.stale:
			lines.append(f"  kept for a monitor that isn't there: {self.stale}")
		lines.append(f"  opens on output {self.start + 1}{pins(self.cli, self.font, self.persist)}")
		lines += [f"  {n + 1}. {step}" for n, step in enumerate(self.steps)]
		return lines


def pins(cli: Grid | None, font: int | None, persist: bool) -> str:
	return ((f", --columns {cli[0]} --rows {cli[1]}" if cli else "") + (f", --font-size {font}" if font else "")
		+ (", --remember-size" if persist else ""))


def overlap(a: Rect, b: Rect) -> int:
	w = min(a[0] + a[2], b[0] + b[2]) - max(a[0], b[0])
	h = min(a[1] + a[3], b[1] + b[3]) - max(a[1], b[1])
	return max(w, 0) * max(h, 0)


## How long the two rectangles' shared edge is, when they touch side by side.
def shared_edge(a: Rect, b: Rect) -> int:
	if a[0] + a[2] == b[0] or b[0] + b[2] == a[0]:
		return min(a[1] + a[3], b[1] + b[3]) - max(a[1], b[1])
	if a[1] + a[3] == b[1] or b[1] + b[3] == a[1]:
		return min(a[0] + a[2], b[0] + b[2]) - max(a[0], b[0])
	return 0


## A grid that fits an output with room to spare, at a font zoom. Measured:
## about 10.7 px a column and 24.5 a row at the default font, a bit more for
## each step of zoom.
def room(out: Out, zoom: int) -> Grid:
	w, h = out.size()
	return max(30, int((w * 0.85 - 20) / (10.8 + 0.7 * zoom))), max(8, int((h * 0.8 - 40) / (24.5 + 1.1 * zoom)))


def some_grid(rng: random.Random, fit: Grid) -> Grid:
	return rng.randint(min(40, fit[0]), fit[0]), rng.randint(min(10, fit[1]), fit[1])


## Outputs edge to edge: each one put against a side of one already placed,
## sharing at least 200 px of edge, so a pointer can cross.
def place(rng: random.Random, outs: list[Out]) -> None:
	rects: list[Rect] = [(0, 0, *outs[0].size())]
	for out in outs[1:]:
		w, h = out.size()
		for _ in range(200):
			ax, ay, aw, ah = rng.choice(rects)
			side = rng.choice("RLDU")
			if side in "RL":
				x = ax + aw if side == "R" else ax - w
				y = rng.choice([ay, ay + ah - h, ay + (ah - h) // 2, rng.randint(ay - h + 200, ay + ah - 200)])
			else:
				y = ay + ah if side == "D" else ay - h
				x = rng.choice([ax, ax + aw - w, ax + (aw - w) // 2, rng.randint(ax - w + 200, ax + aw - 200)])
			cand = (x, y, w, h)
			if all(overlap(cand, r) == 0 for r in rects) and shared_edge(cand, (ax, ay, aw, ah)) >= 200:
				rects.append(cand)
				break
		else:
			rects.append((max(r[0] + r[2] for r in rects), 0, w, h))
	left, top = min(r[0] for r in rects), min(r[1] for r in rects)
	for out, r in zip(outs, rects, strict=True):
		out.at = (r[0] - left, r[1] - top)


def neighbors(outs: list[Out], i: int) -> list[int]:
	return [j for j in range(len(outs)) if j != i and shared_edge(outs[i].rect(), outs[j].rect()) >= 200]


def make_plan(seed: int, arm: str | None = None) -> Plan:
	rng = random.Random(seed)
	picked = rng.choice(["wayland", "wayland", "x11"])
	arm = arm or picked
	outs = [Out(*rng.choice(COMBOS)) for _ in range(rng.choice([2, 2, 3, 3, 3, 4]))]
	## two outputs alike now and then, so one key names both
	if len(outs) > 2 and rng.random() < 0.25:
		outs[-1] = Out(outs[0].mode, outs[0].scale)
	place(rng, outs)
	remember = rng.random() < 0.9
	per_monitor = rng.random() < 0.85
	smallest = min(outs, key=lambda o: o.size()[0] * o.size()[1])
	last_zoom = rng.choice([0, 0, 0, 1, 2, -1])
	last = Kept(*some_grid(rng, room(smallest, last_zoom)), last_zoom)
	kept: dict[int, Kept] = {}
	for i, out in enumerate(outs):
		if rng.random() < 0.7:
			zoom = rng.choice([0, 0, 1, 2, 3, -1, -2])
			kept[i] = Kept(*some_grid(rng, room(out, zoom)), zoom)
	stale = Kept(*some_grid(rng, room(smallest, 0)), 1) if rng.random() < 0.3 else None
	fixed = some_grid(rng, room(smallest, 0))
	start = rng.randrange(len(outs))
	cli = some_grid(rng, room(smallest, 0)) if rng.random() < 0.1 else None
	font = rng.randint(11, 18) if rng.random() < 0.08 else None

	steps: list[Step] = []
	layout = [Out(o.mode, o.scale, o.at) for o in outs]
	here = start
	for _ in range(rng.randint(3, 7)):
		kind = rng.choices(list(KINDS), weights=list(KINDS.values()))[0]
		others = [j for j in range(len(layout)) if j != here]
		if kind == "move":
			steps.append(Step("move", to=rng.choice(others), pointer=rng.random() < 0.7))
		elif kind == "drag":
			to = here if rng.random() < 0.1 else rng.choice(others)
			style = rng.choices(["clean", "sliver"], weights=[0.55, 0.45])[0]
			step = Step("drag", to=to, style=style, along=rng.random(), pointer=rng.random() < 0.85)
			near = neighbors(layout, to)
			if style == "sliver" and near:
				step.nearby = rng.choice(near)
				step.share = rng.uniform(0.06, 0.35)
			elif style == "sliver":
				step.style = "clean"
			if rng.random() < 0.25 and near:
				step.hold = rng.uniform(1.0, 2.5)
				step.nearby = step.nearby if step.nearby >= 0 else rng.choice(near)
			steps.append(step)
		elif kind == "resize":
			steps.append(Step("resize", size=(rng.uniform(0.35, 0.9), rng.uniform(0.35, 0.9)), pointer=rng.random() < 0.75))
		elif kind == "zoom":
			steps.append(Step("zoom", zoom=rng.choice([1, 1, 2, -1, -1, 0]), pointer=rng.random() < 0.5))
		elif kind == "fullscreen":
			steps.append(Step("fullscreen"))
		elif kind == "relaunch":
			cli2 = some_grid(rng, room(smallest, 0)) if rng.random() < 0.1 else None
			font2 = rng.randint(11, 18) if rng.random() < 0.07 else None
			steps.append(Step("relaunch", to=rng.randrange(len(layout)), cli=cli2, font=font2))
		else:
			i = here if rng.random() < 0.6 else rng.randrange(len(layout))
			out = layout[i]
			fits = []
			for s in SCALES:
				size = whole_size(out.mode, s)
				if s == out.scale or not size:
					continue
				grown = (out.at[0], out.at[1], *size)
				if all(overlap(grown, o.rect()) == 0 for j, o in enumerate(layout) if j != i):
					fits.append(s)
			if not fits:
				continue
			step = Step("rescale", to=i, scale=rng.choice(fits))
			layout[i] = Out(out.mode, step.scale, out.at)
			steps.append(step)
			continue
		if kind in ("move", "drag", "relaunch"):
			here = steps[-1].to
	## Drawn apart, so a seed from before it was added makes the same case,
	## but for this.
	more = random.Random(f"{seed}/remember-size")
	persist = bool(cli or font) and more.random() < 0.5
	for step in steps:
		step.persist = bool(step.cli or step.font) and more.random() < 0.5
	return Plan(seed, arm, outs, remember, per_monitor, last, fixed, kept, stale, start, cli, font, persist, steps)


## The generator still makes every kind of case. Runs before any case, and
## needs no compositor.
def generator_holes() -> list[str]:
	seen: Counter[str] = Counter()
	for seed in range(400):
		plan = make_plan(seed)
		seen[plan.arm] += 1
		seen["fractional"] += any(not o.scale.is_integer() for o in plan.outs)
		seen["mixed scales"] += len({o.scale for o in plan.outs}) > 1
		seen["twins"] += len({(o.mode, o.scale) for o in plan.outs}) < len(plan.outs)
		seen["4 outputs"] += len(plan.outs) == 4
		seen["stale entry"] += plan.stale is not None
		seen["remember off"] += not plan.remember
		seen["per monitor off"] += not plan.per_monitor
		seen["size pinned"] += plan.cli is not None or any(s.cli for s in plan.steps)
		seen["font pinned"] += plan.font is not None or any(s.font for s in plan.steps)
		seen["remember size"] += plan.persist or any(s.persist for s in plan.steps)
		for step in plan.steps:
			seen[step.kind] += 1
			seen[f"drag {step.style}"] += step.kind == "drag"
			seen["held astride"] += step.hold > 0
			seen["pointer stays away"] += not step.pointer
			seen["zoom reset"] += step.kind == "zoom" and step.zoom == 0
	want = ["wayland", "x11", "fractional", "mixed scales", "twins", "4 outputs", "stale entry", "remember off", "per monitor off",
		"size pinned", "font pinned", "remember size", *KINDS, "drag clean", "drag sliver", "held astride", "pointer stays away", "zoom reset"]
	return [w for w in want if seen[w] == 0]


class NoRigError(Exception):
	pass


class CheckError(Exception):
	pass


@dataclass
class Win:
	output: str
	rect: Rect
	deco: int


@dataclass
class Model:
	store: dict[str, Kept]
	last: Kept
	remember: bool
	per_monitor: bool
	fixed: Grid
	belief: str | None = None
	grid: Grid = (0, 0)
	zoom: int = 0
	size_pinned: bool = False
	font_pinned: bool = False
	## Wayland: the outputs the window is on, in the order it entered them. A
	## group entered at one time has no order the window can see.
	entered: list[list[str]] = field(default_factory=list)

	def kept(self, key: str | None) -> Kept:
		if key and self.per_monitor and key in self.store:
			return self.store[key]
		return self.last

	## What a look that names `key` leaves the window at.
	def after_look(self, key: str | None) -> tuple[str | None, Grid, int]:
		if key is None or key == self.belief:
			return self.belief, self.grid, self.zoom
		if not (self.remember and self.per_monitor) or self.size_pinned:
			return key, self.grid, self.zoom
		kept = self.kept(key)
		return key, (kept.cols, kept.rows), self.zoom if self.font_pinned else kept.zoom

	## As `config::remember_window` keeps it: the last size anywhere first, and
	## a monitor seen for the first time starts from that.
	def save(self, key: str | None, grid: Grid | None, zoom: int | None) -> None:
		cols, rows = grid or (self.last.cols, self.last.rows)
		self.last = Kept(cols, rows, self.last.zoom if zoom is None else zoom)
		if key is None or not (self.remember and self.per_monitor) or (grid is None and zoom is None):
			return
		was = self.store.get(key, self.last)
		cols, rows = grid or (was.cols, was.rows)
		self.store[key] = Kept(cols, rows, was.zoom if zoom is None else zoom)


class Rig:
	def __init__(self, plan: Plan, work: Path, binary: Path) -> None:
		self.plan = plan
		self.work = work
		self.binary = binary
		self.procs: list[subprocess.Popen[bytes]] = []
		self.app: subprocess.Popen[bytes] | None = None
		self.ptr: subprocess.Popen[str] | None = None
		## where the pointer was last moved to, in the layout
		self.pointer_at = (0, 0)
		self.oks = 0
		## A socket path has to fit in 108 bytes.
		run = work / "run"
		if len(str(run)) > 70:
			run = Path(tempfile.mkdtemp(prefix="silkdpi.", dir="/tmp"))
			self.short_run: Path | None = run
		else:
			run.mkdir(mode=0o700)
			self.short_run = None
		self.run = run
		base = {k: v for k, v in os.environ.items() if k not in ("DISPLAY", "WAYLAND_DISPLAY", "WAYLAND_SOCKET", "SWAYSOCK")}
		base["XDG_RUNTIME_DIR"] = str(run)
		self.base = base
		self.wl: dict[str, str] = {}
		self.xdisplay = ""
		self.home = work / "home"
		self.grid_file = work / "grid.txt"
		self.said = work / "said.txt"
		self.saidpos = 0
		self.unread: list[str] = []

	def start(self) -> None:
		conf = self.work / "sway.conf"
		lines = []
		for i, out in enumerate(self.plan.outs):
			lines.append(f"output HEADLESS-{i + 1} resolution {out.mode[0]}x{out.mode[1]} position {out.at[0]} {out.at[1]} scale {out.scale:g}")
			lines.append(f"workspace {i + 1} output HEADLESS-{i + 1}")
		lines += [f"xwayland {'enable' if self.plan.arm == 'x11' else 'disable'}", "default_floating_border normal",
			"for_window [all] floating enable", "focus_follows_mouse no", "mouse_warping none"]
		conf.write_text("\n".join(lines) + "\n")
		env = dict(self.base, WLR_BACKENDS="headless", WLR_HEADLESS_OUTPUTS=str(len(self.plan.outs)), WLR_RENDERER="pixman", WLR_LIBINPUT_NO_DEVICES="1")
		with (self.work / "sway.log").open("w") as log:
			sway = subprocess.Popen(["sway", "-c", str(conf)], env=env, stdout=log, stderr=subprocess.STDOUT)
		self.procs.append(sway)
		sock = None
		for _ in range(100):
			found = list(self.run.glob("sway-ipc.*.sock"))
			if found and (self.run / "wayland-1").exists():
				sock = found[0]
				break
			time.sleep(0.1)
		if not sock:
			raise NoRigError("sway did not start")
		self.wl = dict(self.base, WAYLAND_DISPLAY="wayland-1", SWAYSOCK=str(sock))
		if self.plan.arm == "x11":
			## sway listens for X clients before Xwayland runs, and starts it on the first
			for _ in range(100):
				ss = subprocess.run(["ss", "-xlpH"], capture_output=True, text=True, check=False).stdout
				for line in ss.splitlines():
					hit = re.search(r"/tmp/\.X11-unix/X(\d+)", line)
					if f"pid={sway.pid}," in line and hit:
						self.xdisplay = ":" + hit.group(1)
				if self.xdisplay:
					break
				time.sleep(0.1)
			if not self.xdisplay:
				raise NoRigError("no Xwayland display from sway")
		w, h = self.extent()
		self.ptr = subprocess.Popen(["python3", "-I", str(VPOINTER), str(w), str(h)], env=self.wl, stdin=subprocess.PIPE,
			stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
		assert self.ptr.stdout
		if self.ptr.stdout.readline().strip() != "ready":
			raise NoRigError("no virtual pointer")
		self.pointer("move", w - 5, h - 5)

	def extent(self) -> tuple[int, int]:
		rects = [o["rect"] for o in self.outputs()]
		return max(r["x"] + r["width"] for r in rects), max(r["y"] + r["height"] for r in rects)

	def close(self) -> None:
		self.stop_app()
		if self.ptr:
			if self.ptr.stdin:
				self.ptr.stdin.close()
			self.ptr.wait(5)
		for proc in reversed(self.procs):
			proc.terminate()
			try:
				proc.wait(5)
			except subprocess.TimeoutExpired:
				proc.kill()
		if self.short_run and self.short_run.name.startswith("silkdpi."):
			shutil.rmtree(self.short_run, ignore_errors=True)

	def msg(self, *args: str, kind: str = "") -> object:
		cmd = ["swaymsg", "-r"] + (["-t", kind] if kind else []) + list(args)
		out = subprocess.run(cmd, env=self.wl, capture_output=True, text=True, check=False).stdout
		return json.loads(out) if out.strip() else None

	def outputs(self) -> list[dict[str, Any]]:
		got = self.msg(kind="get_outputs")
		return [o for o in got if o.get("active")] if isinstance(got, list) else []

	def pointer(self, *words: object) -> None:
		assert self.ptr and self.ptr.stdin and self.ptr.stdout
		if words[0] == "move":
			self.pointer_at = (int(str(words[1])), int(str(words[2])))
		self.ptr.stdin.write(" ".join(str(w) for w in words) + "\n")
		self.ptr.stdin.flush()
		if self.ptr.stdout.readline().strip() != "ok":
			raise CheckError(f"the pointer did not answer: {words}")

	## Ctrl and one key, to the window: a virtual keyboard on Wayland, and XTEST
	## in Xwayland, where a virtual keyboard's keys go missing. Focus redraws the
	## window, and a debug build drawing in software can be busy for most of a
	## second, so the keyboard comes well before its key. A keyboard that came
	## and went while the window was busy was never seen.
	def ctrl_key(self, key: str) -> None:
		self.msg(f"[pid={self.app.pid if self.app else 0}] focus")
		time.sleep(0.5)
		if self.plan.arm == "x11":
			subprocess.run(["xdotool", "key", "--clearmodifiers", f"ctrl+{key}"], env=dict(self.base, DISPLAY=self.xdisplay),
				capture_output=True, check=False)
			time.sleep(0.3)
			return
		typed = subprocess.run(["wtype", "-s", "1500", "-M", "ctrl", "-k", key, "-m", "ctrl", "-s", "500"], env=self.wl,
			capture_output=True, text=True, check=False)
		if typed.returncode:
			raise CheckError(f"wtype failed: {typed.stderr.strip()}")

	## The window's focus and a Shift tap: a person back at it by keyboard.
	def nudge(self) -> None:
		self.msg(f"[pid={self.app.pid if self.app else 0}] focus")
		time.sleep(0.5)
		if self.plan.arm == "x11":
			subprocess.run(["xdotool", "key", "shift"], env=dict(self.base, DISPLAY=self.xdisplay), capture_output=True, check=False)
			return
		typed = subprocess.run(["wtype", "-s", "1500", "-M", "shift", "-m", "shift", "-s", "500"], env=self.wl,
			capture_output=True, text=True, check=False)
		if typed.returncode:
			raise CheckError(f"wtype failed: {typed.stderr.strip()}")

	def launch(self, cli: Grid | None, font: int | None, persist: bool) -> None:
		args = [str(self.binary), "--shell", f"python3 -I {HERE / '_size.py'} {self.grid_file}"]
		if cli:
			args += ["--columns", str(cli[0]), "--rows", str(cli[1])]
		if font:
			args += ["--font-size", str(font)]
		if persist:
			args += ["--remember-size"]
		env = dict(self.wl if self.plan.arm == "wayland" else self.base, LIBGL_ALWAYS_SOFTWARE="1", VK_ICD_FILENAMES=str(LVP),
			VK_LAYER_MESSAGE_ID_FILTER="VUID-vkCmdCopyBufferToImage-pRegions-00173", LP_NUM_THREADS="2",
			HOME=str(self.home), XDG_CONFIG_HOME=str(self.home / ".config"), XDG_DATA_HOME=str(self.home / ".local/share"),
			XDG_CACHE_HOME=str(self.home / ".cache"), XDG_STATE_HOME=str(self.home / ".local/state"), SILK_MONDBG="1")
		if self.plan.arm == "x11":
			env["DISPLAY"] = self.xdisplay
		with self.said.open("a") as said:
			self.app = subprocess.Popen(args, env=env, stdout=subprocess.DEVNULL, stderr=said)
		for _ in range(200):
			if self.window():
				return
			time.sleep(0.1)
		raise CheckError("the window never showed")

	## Only the process launched here, and only while it is still this binary.
	def stop_app(self) -> None:
		app, self.app = self.app, None
		if not app or app.poll() is not None:
			return
		try:
			if Path(f"/proc/{app.pid}/exe").resolve() != self.binary.resolve():
				return
		except OSError:
			return
		app.send_signal(signal.SIGTERM)
		try:
			app.wait(10)
		except subprocess.TimeoutExpired:
			app.kill()
			app.wait(5)

	def window(self) -> Win | None:
		if not self.app:
			return None
		tree = self.msg(kind="get_tree")
		found: list[Win] = []

		def walk(node: dict[str, Any], output: str) -> None:
			if node.get("type") == "output":
				output = node["name"]
			for child in node.get("nodes", []) + node.get("floating_nodes", []):
				if self.app and child.get("pid") == self.app.pid:
					r = child["rect"]
					found.append(Win(output, (r["x"], r["y"], r["width"], r["height"]), child["deco_rect"]["height"]))
				walk(child, output)

		if isinstance(tree, dict):
			walk(tree, "")
		return found[0] if found else None

	## The shell's "rows columns width height", the last two in pixels.
	def size_lines(self) -> list[list[int]]:
		try:
			text = self.grid_file.read_text()
		except OSError:
			return []
		return [[int(v) for v in line.split()] for line in text.splitlines() if len(line.split()) == 4]

	def grid_lines(self) -> list[str]:
		return [f"{s[0]} {s[1]}" for s in self.size_lines()]

	## The shell's own rows and columns, as columns and rows.
	def grid(self) -> Grid | None:
		lines = self.size_lines()
		return (lines[-1][1], lines[-1][0]) if lines else None

	## The size of a cell in pixels, as the terminal tells the shell, on the
	## last line or on line `at`.
	def cell(self, at: int = -1) -> tuple[int, int] | None:
		lines = self.size_lines()
		if not -len(lines) <= at < len(lines) or not all(lines[at]):
			return None
		rows, cols, width, height = lines[at]
		return width // cols, height // rows

	def trace(self) -> list[str]:
		lines, self.unread = self.unread, []
		try:
			with self.said.open() as said:
				said.seek(self.saidpos)
				text = said.read()
				self.saidpos = said.tell()
		except OSError:
			return lines
		## "[mon 1.23s] take ...", kept as "[mon] take ..."
		return lines + [re.sub(r"^\[mon [0-9.]+s\]", "[mon]", line) for line in text.splitlines() if line.startswith("[mon ")]

	def state(self) -> tuple[Kept, dict[str, Kept]]:
		path = self.home / ".local/state/silkterm/state.shcl"
		values: dict[str, str] = {}
		stack: list[str] = []
		for raw in path.read_text().splitlines() if path.exists() else []:
			text = raw.strip()
			if not text or text.startswith("#"):
				continue
			depth = len(raw) - len(raw.lstrip("\t"))
			del stack[depth:]
			name, _, value = text.partition(":")
			if value.strip():
				values[".".join([*stack, name])] = value.strip()
			else:
				stack.append(name)
		def num(name: str, missing: int = 0) -> int:
			return int(values.get(name, missing))

		last = Kept(num("window.remembered_columns", -1), num("window.remembered_rows", -1), num("window.remembered_font_zoom"))
		monitors: dict[str, Kept] = {}
		for k in values:
			hit = re.fullmatch(r"window\.monitors\.([^.]+)\.columns", k)
			if hit:
				at = f"window.monitors.{hit.group(1)}"
				monitors[hit.group(1)] = Kept(num(f"{at}.columns", -1), num(f"{at}.rows", -1), num(f"{at}.font_zoom", -99))
		return last, monitors


def trace_field(line: str, name: str) -> str:
	hit = re.search(rf"\b{name}=(\S+)", line)
	return hit.group(1) if hit else "-"


def key_of(arm: str, out: dict[str, Any], scale: float) -> str:
	if arm == "x11":
		return f"{out['rect']['width']}x{out['rect']['height']}_100pct"
	mode = out["current_mode"]
	return f"{mode['width']}x{mode['height']}_{round(scale * 100)}pct"


## The grids a scale change can leave a window at that keeps its size on the
## desktop, from a cell and scale to another. A font's cell doesn't grow in
## step with the scale, so the grid goes by how far the cell grew against
## the scale: at font zoom -2 a cell is 9x18 at 100% and 19x38 at 200%, and
## 24 rows became 22 (2026101513385002). One more either way for the margins,
## the chrome and the part of a cell the window had over.
def drift_band(grid: Grid, was: tuple[tuple[int, int], float], now: tuple[tuple[int, int], float]) -> set[Grid]:
	(cell_was, scale_was), (cell, scale) = was, now
	spans = []
	for n, c0, c1 in zip(grid, cell_was, cell, strict=True):
		lo, hi = sorted((n, n * c0 * scale / (scale_was * c1)))
		spans.append(range(max(1, math.floor(lo) - 1), math.ceil(hi) + 2))
	return {(c, r) for c in spans[0] for r in spans[1]}


class Case:
	def __init__(self, plan: Plan, work: Path, binary: Path) -> None:
		self.plan = plan
		self.rig = Rig(plan, work, binary)
		self.log: list[str] = []
		self.limits: Counter[str] = Counter()
		self.known: Counter[str] = Counter()
		self.model = Model({}, plan.last, plan.remember, plan.per_monitor, plan.fixed)
		self.scale = 1.0
		self.drag_scales: set[float] = set()
		self.drag_touched: set[str] = set()
		self.drag_primaries: set[str] = set()
		self.drag_names: dict[str | None, str] = {}
		## the outputs the window was on when last looked for
		self.touched: list[str] = []
		self.font: int | None = None
		self.stale: str | None = None
		## a look put off until the pointer, a key or focus comes back
		self.waiting = False
		## a look while fullscreen, whose size is taken once back
		self.owed: list[str] = []
		## monitors whose size was last set at a font size from the command line
		self.refonted: set[str] = set()
		## the monitor a kept size was just taken for, at a font from the
		## command line
		self.atfont: str | None = None
		self.cells: dict[tuple[float, int | None, int], tuple[int, int]] = {}
		## the window saves the sizes the user sets: not with a size or a font
		## size from the command line, unless with --remember-size as well
		self.saves = True
		## the line of the shell's sizes, and the scale, a step began at
		self.was: tuple[int, float] = (-1, 1.0)

	def note(self, text: str) -> None:
		self.log.append(text)

	## Whether a scale change leaves the window's size to the system, for a
	## monitor's own size to take over: the grid may then drift a few cells.
	def drifts(self) -> bool:
		return self.plan.remember and self.plan.per_monitor and not self.model.size_pinned

	## The scale a cell the shell is told was drawn at: an X11 window's is 1.
	def drawn_at(self, scale: float) -> float:
		return scale if self.plan.arm == "wayland" else 1.0

	## Whether `got` is where a scale change can have moved `grid` to since
	## the step began, by the cell the shell is told now.
	def drifted_to(self, got: Grid | None, grid: Grid, scales: set[float]) -> bool:
		was = self.rig.cell(self.was[0])
		now = self.rig.cell()
		if not got or not was or not now:
			return False
		return any(got in drift_band(grid, (was, self.was[1]), (now, self.drawn_at(s))) for s in scales)

	def outs(self) -> dict[str, dict[str, Any]]:
		return {o["name"]: o for o in self.rig.outputs()}

	def write_config(self) -> None:
		plan = self.plan
		outs = self.outs()
		lines = ["performance:", "\tautomatic: false", '\tprofile: "custom"', "wallpaper:", "\tenabled: false", "scroll:", "\tminimap:",
			"\t\tenabled: false", "cursor:", "\tblink: false", "window:", f"\tremember_size: {str(plan.remember).lower()}",
			f"\tremember_per_monitor: {str(plan.per_monitor).lower()}", f"\tremembered_columns: {plan.last.cols}",
			f"\tremembered_rows: {plan.last.rows}", f"\tremembered_font_zoom: {plan.last.zoom}"]
		if not plan.remember:
			lines += [f"\tcolumns: {plan.fixed[0]}", f"\trows: {plan.fixed[1]}"]
		store: dict[str, Kept] = {}
		for i, kept in sorted(plan.kept.items()):
			out = outs[f"HEADLESS-{i + 1}"]
			key = key_of(plan.arm, out, out["scale"])
			store.setdefault(key, kept)
		if plan.stale:
			store.setdefault("1366x768_100pct", plan.stale)
		if store:
			lines.append("\tmonitors:")
			for key, kept in store.items():
				lines += [f"\t\t{key}:", f"\t\t\tcolumns: {kept.cols}", f"\t\t\trows: {kept.rows}", f"\t\t\tfont_zoom: {kept.zoom}"]
		cfg = self.rig.home / ".config/silkterm"
		cfg.mkdir(parents=True, exist_ok=True)
		(cfg / "config.shcl").write_text("\n".join(lines) + "\n")
		self.model.store = dict(store)

	## Where the window is and which output most of it is on, as sway has it.
	def where(self) -> tuple[Win, dict[str, dict[str, Any]], list[str], list[str]]:
		win = self.rig.window()
		if not win:
			raise CheckError("the window is gone")
		outs = self.outs()
		areas = {name: overlap(win.rect, (o["rect"]["x"], o["rect"]["y"], o["rect"]["width"], o["rect"]["height"])) for name, o in outs.items()}
		touched = [n for n, a in areas.items() if a > 0]
		most = max(areas.values(), default=0)
		primary = [n for n, a in areas.items() if a == most and a > 0]
		self.touched = touched
		return win, outs, touched, primary

	def track_entered(self, touched: list[str]) -> None:
		groups = [[n for n in g if n in touched] for g in self.model.entered]
		groups = [g for g in groups if g]
		seen = {n for g in groups for n in g}
		new = [n for n in touched if n not in seen]
		if new:
			groups.append(new)
		self.model.entered = groups

	## The keys a look may name now: the one the requirements want, and any a
	## platform limit allows, each with its reason.
	def names(self, outs: dict[str, dict[str, Any]], primary: list[str]) -> dict[str | None, str]:
		arm = self.plan.arm
		allowed: dict[str | None, str] = {}
		for name in primary:
			allowed[key_of(arm, outs[name], outs[name]["scale"] if arm == "wayland" else 1.0)] = "requirement"
		if arm == "x11" or not primary:
			return allowed
		scale = outs[primary[0]]["scale"]
		first = self.model.entered[0] if self.model.entered else []
		alike = sum(outs[n]["scale"] == scale for n in self.touched if n in outs)
		for f in first:
			if f in primary:
				continue
			if outs[f]["scale"] == scale:
				allowed.setdefault(key_of(arm, outs[f], scale), "same-scale outputs look alike to a Wayland window")
			elif alike > 1:
				allowed.setdefault(None, "more than one output the window is on is at its scale")
		return allowed

	## Waits for the window to settle on one of `allowed`, then checks it
	## stays there and matches the model.
	def settle(
		self, what: str, allowed: dict[str | None, str], looks: bool, rescaled: bool = False, rounding: str = RESCALED, look_wait: float = 3.0,
		unseen: str = TOLD_NOTHING,
	) -> None:
		model = self.model
		if not looks:
			allowed = {None: allowed.get(None, unseen)}
		## outputs touched on the way, which a look after the drop may mix in
		allowed = {**self.drag_names, **allowed}
		outcomes = {k: model.after_look(k) for k in allowed}
		grids = {o[1] for o in outcomes.values()}
		rescaled = rescaled and self.drifts() and model.grid in grids
		scales = {o["scale"] for o in self.rig.outputs()}

		def drift(g: Grid | None) -> bool:
			return rescaled and self.drifted_to(g, model.grid, scales)

		time.sleep(2.2)
		## A window on no output yet keeps looking, for up to 10 s after it opens.
		if looks:
			seen: list[str] = []
			deadline = time.monotonic() + look_wait
			while time.monotonic() < deadline and not any(t.split()[1] in ("arrive", "stay") for t in seen):
				seen += self.rig.trace()
				time.sleep(0.2)
			time.sleep(0.3)
			seen += self.rig.trace()
			self.rig.unread = seen + self.rig.unread
			## At a font from the command line a kept size is taken in its
			## columns and rows where they fit there, else at the kept
			## window's size.
			for t in seen:
				kind = t.split()[1]
				named_at = None if trace_field(t, "key") == "-" else trace_field(t, "key")
				if not model.font_pinned or kind not in ("take", "atfont") or named_at not in outcomes or named_at == model.belief:
					continue
				self.atfont = named_at
				if kind == "atfont":
					cols, rows = (int(v) for v in trace_field(t, "grid").split("x"))
					was = outcomes[named_at]
					outcomes[named_at] = (was[0], (cols, rows), was[2])
					grids.add((cols, rows))
		deadline = time.monotonic() + 8
		while self.rig.grid() not in grids and not drift(self.rig.grid()) and time.monotonic() < deadline:
			time.sleep(0.1)
		got = self.rig.grid()
		mark = len(self.rig.grid_lines())
		time.sleep(2.0)
		later_lines = self.rig.grid_lines()[mark:]
		later = set(later_lines)
		trace = self.rig.trace()
		looked = [t for t in trace if t.split()[1] in ("arrive", "stay")]
		takes = [t for t in trace if t.split()[1] == "take"]
		saves = [t for t in trace if t.split()[1] == "save"]
		win, outs, touched, primary = self.where()
		if saves:
			raise CheckError(f"{what}: saved a size nobody set: {saves}")
		win_now = self.rig.window()
		lw, lh = self.rig.extent()
		if win_now and (win_now.rect[2] >= lw - 4 or win_now.rect[3] + win_now.deco >= lh - 4):
			grids |= {(c, r) for c in range(1, max(g[0] for g in grids) + 1) for r in range(1, max(g[1] for g in grids) + 1)}
		## A window drawing in software can be seconds late with a frame, and
		## what waits on one comes late with it: the look after a scale change,
		## and so the take after the drift (2026101614100002). One more step to
		## a grid it may end at is that, and it has to stay there.
		if got is not None and later_lines:
			seen = [f"{got[1]} {got[0]}", *later_lines]
			rows, cols = (int(v) for v in seen[-1].split())
			if sum(a != b for a, b in zip(seen, seen[1:], strict=False)) == 1 and ((cols, rows) in grids or drift((cols, rows))):
				self.note(f"      went on from {got} to {(cols, rows)}")
				got, later = (cols, rows), {seen[-1]}
		self.note(f"    {what}: on {win.output} {win.rect}, most on {','.join(primary) or '-'}, grid {got}, looks {[trace_field(t, 'key') for t in looked]}")
		if got is None or (got not in grids and not drift(got)):
			raise CheckError(f"{what}: at {got}, want one of {sorted(grids)} ({', '.join(f'{k}: {v}' for k, v in allowed.items())})")
		if len(later) > 1 or (later and later != {f"{got[1]} {got[0]}"}):
			raise CheckError(f"{what}: did not hold at {got}: {sorted(later)}")
		arrived = [t for t in looked if t.split()[1] == "arrive"]
		named: str | None = trace_field(arrived[-1], "key") if arrived else None
		if named == "-":
			named = None
		if named is not None and named == model.belief:
			named = None
		if named is None and None not in allowed:
			## it stayed, so it still counts itself where it did
			if model.belief not in allowed:
				raise CheckError(f"{what}: still counts itself on {model.belief}, want one of {list(allowed)}")
			allowed = {None: allowed[model.belief]}
			outcomes = {None: model.after_look(None)}
		if named not in allowed:
			raise CheckError(f"{what}: counted itself on {named}, want one of {list(allowed)}")
		key, grid, zoom = outcomes[named]
		## sway holds a floating window to the size of the whole layout
		lw, lh = self.rig.extent()
		capped = (win.rect[2] >= lw - 4 or win.rect[3] + win.deco >= lh - 4) and got[0] <= grid[0] and got[1] <= grid[1]
		if grid != got and capped:
			self.allow(CAPPED, named)
			grid = got
		drifted = rescaled and grid == model.grid
		if grid != got and not (drifted and drift(got)):
			raise CheckError(f"{what}: named {named} but is at {got}, not {grid}")
		if grid != got:
			self.allow(rounding, named)
			grid = got
		if takes and named is not None:
			taken = trace_field(takes[-1], "zoom")
			if taken != "-" and int(taken) != zoom:
				raise CheckError(f"{what}: took font zoom {taken}, want {zoom}")
		self.allow(allowed[named], named)
		## A window that wasn't told it moved still counts itself where it was,
		## and saves there, until it looks again.
		if named is not None:
			self.stale = None
		elif allowed[None] != "requirement":
			self.stale = allowed[None]
		model.belief, model.grid, model.zoom = key, grid, zoom
		self.scale = outs[primary[0]]["scale"] if primary else self.scale
		self.check_font(what)
		self.check_atfont(what)

	## A kept size taken at a font from the command line keeps its columns
	## and rows if the window fits the screen the size is kept for at that
	## font, else the kept window's size, which is other columns and rows.
	## Judged from the cell the shell is told, and only where the answer is
	## clear of what the window's frame and rounding could move.
	def check_atfont(self, what: str) -> None:
		key, self.atfont = self.atfont, None
		cell = self.rig.cell()
		got = self.rig.grid()
		if key is None or not cell or not got or not re.fullmatch(r"\d+x\d+_\d+pct", key):
			return
		kept = self.model.kept(key)
		win, outs, _, primary = self.where()
		scale = outs[primary[0]]["scale"] if primary and self.plan.arm == "wayland" else 1.0
		frame = (win.rect[2] * scale - got[0] * cell[0], win.rect[3] * scale - got[1] * cell[1])
		want = (kept.cols * cell[0] + frame[0], kept.rows * cell[1] + frame[1])
		w, h = (int(v) for v in key.partition("_")[0].split("x"))
		fits = want[0] <= w * 0.96 and want[1] <= h * 0.96
		past = want[0] > w * 1.04 or want[1] > h * 1.04
		self.note(f"      at the font from the command line as {got}, kept {kept}, {want[0]:.0f}x{want[1]:.0f} there on {w}x{h}")
		if fits and got != (kept.cols, kept.rows):
			raise CheckError(f"{what}: a kept {kept} fits {key} at the font from the command line, but came out at {got}")
		if past and got == (kept.cols, kept.rows):
			raise CheckError(f"{what}: a kept {kept} is {want[0]:.0f}x{want[1]:.0f} at the font from the command line, past {key}, and stayed")

	## Waits out a save the user asked for, and checks it was for the monitor
	## the window counts itself on, and that this is the one it is on.
	def user_save(self, what: str, allowed: dict[str | None, str], zoom: int | None) -> None:
		if not self.saves:
			self.user_unsaved(what, allowed, zoom)
			return
		model = self.model
		deadline = time.monotonic() + 8
		saves: list[str] = []
		trace: list[str] = []
		time.sleep(1.5)
		while time.monotonic() < deadline:
			trace += self.rig.trace()
			saves = [t for t in trace if t.split()[1] == "save"]
			if saves:
				break
			time.sleep(0.2)
		time.sleep(1.0)
		trace += self.rig.trace()
		saves = [t for t in trace if t.split()[1] == "save"]
		takes = [t for t in trace if t.split()[1] == "take"]
		got = self.rig.grid()
		win, _, _, primary = self.where()
		self.note(f"    {what}: on {win.output} {win.rect}, grid {got}, saves {[(trace_field(s, 'key'), trace_field(s, 'grid')) for s in saves]}")
		if takes:
			raise CheckError(f"{what}: took a kept size over the user's: {takes}")
		## one for each key press of a zoom that outlasts the wait before a save
		if not saves or not got or len({trace_field(t, "key") for t in saves}) > 1:
			raise CheckError(f"{what}: want saves for one monitor, got {saves}")
		key = trace_field(saves[-1], "key")
		saved = got
		## At a font size from the command line the grid is kept for the font
		## the next launch has: the same when it fits the screen there, else
		## the grid that font shows in the window now.
		refont = [t for t in trace if t.split()[1] == "refont"]
		if self.font is not None and refont:
			cols, rows = (int(v) for v in trace_field(refont[-1], "grid").split("x"))
			saved = (cols, rows)
			self.note(f"      kept for the normal font as {saved}")
		elif refont:
			raise CheckError(f"{what}: kept a grid for another font with none on the command line: {refont}")
		if trace_field(saves[-1], "grid") != f"{saved[0]}x{saved[1]}":
			raise CheckError(f"{what}: saved {trace_field(saves[-1], 'grid')} at {got}")
		if trace_field(saves[-1], "zoom") != ("-" if zoom is None else str(zoom)):
			raise CheckError(f"{what}: saved font zoom {trace_field(saves[-1], 'zoom')}, want {zoom}")
		if [t for t in trace if t.split()[1] == "arrive"]:
			raise CheckError(f"{what}: looked for its monitor again: {trace}")
		## A Wayland window looks as the user sets a size, in case it moved
		## without being told, and keeps the size it has (2026101413420431).
		found = [t for t in trace if t.split()[1] == "found"]
		if found:
			named = trace_field(found[-1], "key")
			if named not in allowed:
				raise CheckError(f"{what}: found itself on {named}, want one of {list(allowed)}")
			model.belief, self.stale = named, None
		if key != (model.belief or "-"):
			raise CheckError(f"{what}: saved for {key}, but counts itself on {model.belief}")
		if model.belief not in allowed:
			if not self.stale:
				raise CheckError(f"{what}: saved for {key}, want one of {list(allowed)}")
			## only one that can't name the output it's on keeps the one it left
			if self.plan.arm == "wayland" and None not in allowed:
				raise CheckError(f"{what}: saved for {key}, where it was before a move it wasn't told of, want one of {list(allowed)}")
			self.allow(self.stale, model.belief)
		else:
			self.allow(allowed[model.belief], model.belief)
		model.save(model.belief, saved, zoom)
		if model.belief:
			if self.font is not None:
				self.refonted.add(model.belief)
			else:
				self.refonted.discard(model.belief)
		model.grid = got
		if zoom is not None:
			model.zoom = zoom
		self.check_font(what)

	## A window opened with a size or a font size from the command line saves
	## nothing the user sets, unless --remember-size came with it
	## (2026101422190936). It still looks for its output as a save would.
	def user_unsaved(self, what: str, allowed: dict[str | None, str], zoom: int | None) -> None:
		model = self.model
		trace: list[str] = []
		deadline = time.monotonic() + 8
		time.sleep(1.5)
		while time.monotonic() < deadline and not any(t.split()[1] == "unsaved" for t in trace):
			trace += self.rig.trace()
			time.sleep(0.2)
		time.sleep(1.0)
		trace += self.rig.trace()
		got = self.rig.grid()
		win, _, _, _ = self.where()
		unsaved = [t for t in trace if t.split()[1] == "unsaved"]
		self.note(f"    {what}: on {win.output} {win.rect}, grid {got}, not saved {[(trace_field(t, 'key'), trace_field(t, 'grid')) for t in unsaved]}")
		for event, why in (("save", "saved it"), ("take", "took a kept size over it"), ("arrive", "looked for its monitor again")):
			hit = [t for t in trace if t.split()[1] == event]
			if hit:
				raise CheckError(f"{what}: a size set with options from the command line, and the window {why}: {hit}")
		if not unsaved or not got:
			raise CheckError(f"{what}: no sign of the size the user set: {trace}")
		found = [t for t in trace if t.split()[1] == "found"]
		if found:
			named = trace_field(found[-1], "key")
			if named not in allowed:
				raise CheckError(f"{what}: found itself on {named}, want one of {list(allowed)}")
			model.belief, self.stale = named, None
		if trace_field(unsaved[-1], "key") != (model.belief or "-"):
			raise CheckError(f"{what}: kept for {trace_field(unsaved[-1], 'key')}, but counts itself on {model.belief}")
		model.grid = got
		if zoom is not None:
			model.zoom = zoom
		self.check_font(what)

	## The font zoom, judged by the cell size the shell is told rather than by
	## what the window says it took: one scale and zoom always give one cell
	## size, and at one scale a bigger zoom never gives a shorter cell, and two
	## steps more a taller one.
	def check_font(self, what: str) -> None:
		cell = self.rig.cell()
		if not cell:
			return
		scale = self.scale if self.plan.arm == "wayland" else 1.0
		key = (scale, self.font, self.model.zoom)
		was = self.cells.setdefault(key, cell)
		if was != cell:
			raise CheckError(f"{what}: a cell of {cell} at scale {scale:g} and font zoom {key[2]}, and {was} before")
		for (other_scale, other_font, zoom), other in self.cells.items():
			if (other_scale, other_font) != (scale, self.font) or zoom == key[2]:
				continue
			lo, hi = (other, cell) if zoom < key[2] else (cell, other)
			if lo[1] > hi[1] or (abs(zoom - key[2]) > 1 and lo[1] == hi[1]):
				raise CheckError(f"{what}: a cell of {cell} at font zoom {key[2]}, but {other} at zoom {zoom}")

	def allow(self, reason: str, named: str | None) -> None:
		if reason == "requirement":
			return
		if reason in KNOWN:
			self.known[KNOWN[reason]] += 1
			self.note(f"      known: {reason} ({KNOWN[reason]}), named {named}")
			return
		self.limits[reason] += 1
		self.note(f"      allowed: {reason}, named {named}")

	## The pointer over the window first, as a hand on the mouse would be. On
	## Wayland that is a look, settled before the user's own change. A look put
	## off after a move goes ahead on the pointer, or on a key.
	def before_user(self, what: str, step: Step) -> None:
		if self.waiting and not step.pointer:
			_, outs, _, primary = self.where()
			self.rig.nudge()
			self.settle(f"{what} (a key first)", self.names(outs, primary), True)
			self.waiting = False
			return
		if not step.pointer or (self.plan.arm == "x11" and not self.waiting):
			return
		_, outs, _, primary = self.where()
		self.pointer_back()
		self.settle(f"{what} (pointer over it)", self.names(outs, primary), True)
		self.waiting = False

	## A fullscreen window's size is never saved, and leaving fullscreen puts
	## back the size it had. Leaving can bring the pointer over the window,
	## which is a look. A look that finds another monitor while fullscreen
	## takes its size once back. True when a size was taken, for the look's
	## settle to judge.
	def fullscreen_saves(self, what: str) -> bool:
		time.sleep(1.5)
		trace: list[str] = []
		deadline = time.monotonic() + 4
		while time.monotonic() < deadline:
			trace += self.rig.trace()
			if any(t.split()[1] == "save" for t in trace):
				break
			time.sleep(0.2)
		time.sleep(0.5)
		trace += self.rig.trace()
		_, outs, _, primary = self.where()
		names = self.names(outs, primary)
		took = any(t.split()[1] == "take" for t in trace)
		if any(t.split()[1] == "later" for t in trace):
			self.owed = [t for t in trace if t.split()[1] == "arrive"][-1:]
			self.note(f"      looked while fullscreen, to take a size once back: {self.owed}")
		if took:
			self.rig.unread += self.owed + [t for t in trace if t.split()[1] in ("arrive", "stay", "take")]
			self.owed = []
		for t in trace:
			event, key = t.split()[1], trace_field(t, "key")
			if event == "arrive" and not took and not self.owed:
				if key not in names and not self.stale:
					raise CheckError(f"{what}: counted itself on {key}, want one of {list(names)}")
				self.model.belief, self.stale = key, None
			elif event == "keep" and self.drifts():
				raise CheckError(f"{what}: kept its size on {key} where a monitor's own size takes over")
			elif event == "save":
				raise CheckError(f"{what}: saved a size nobody set: {t}")
		return took

	def held_still(self, what: str, mark: int, start: Grid) -> None:
		lines = self.rig.size_lines()
		grids = {(c, r) for r, c, _, _ in lines[mark:]}
		if grids <= {start}:
			return
		## the lines go back for the look after the drop
		trace = self.rig.trace()
		self.rig.unread = trace
		took = [t for t in trace if t.split()[1] == "take"]
		if took:
			raise CheckError(f"{what}: took a size while still held: {took}")
		was = self.rig.cell(self.was[0])

		def drifted(at: int) -> bool:
			rows, cols, _, _ = lines[at]
			cell = self.rig.cell(at)
			if not was or not cell:
				return False
			return any((cols, rows) in drift_band(start, (was, self.was[1]), (cell, self.drawn_at(s))) for s in self.drag_scales)

		if len(self.drag_scales) > 1 and self.drifts() and all(drifted(at) for at in range(mark, len(lines))):
			self.allow(RESCALED, None)
			return
		raise CheckError(f"{what}: resized while still held: {sorted(grids)}, trace {trace}")

	## Out of the window's way for a step that leaves the pointer be: a corner of
	## the layout away from the window and the output's top left, where a resize
	## puts it.
	def park_pointer(self) -> None:
		win, outs, _, primary = self.where()
		w, h = self.rig.extent()
		r = outs[primary[0]]["rect"] if primary else outs[win.output]["rect"]
		keep_off = [win.rect, (r["x"], r["y"], r["width"], r["height"])]
		corners = [(5, 5), (w - 5, 5), (5, h - 5), (w - 5, h - 5)]
		far = max(corners, key=lambda c: min(abs(c[0] - k[0] - k[2] // 2) + abs(c[1] - k[1] - k[3] // 2) for k in keep_off))
		self.rig.pointer("move", *far)

	## The pointer rests on the window, or off every output, where sway puts it
	## back on one it chooses.
	def under_pointer(self, outs: dict[str, dict[str, Any]]) -> bool:
		win = self.rig.window()
		dot = (*self.rig.pointer_at, 1, 1)
		on_some = any(overlap(dot, (o["rect"]["x"], o["rect"]["y"], o["rect"]["width"], o["rect"]["height"])) for o in outs.values())
		return not on_some or bool(win and overlap(dot, win.rect))

	def pointer_back(self) -> None:
		win = self.rig.window()
		if not win:
			return
		w, h = self.rig.extent()
		corner = (w - 5, h - 5) if overlap(win.rect, (w - 10, h - 10, 10, 10)) == 0 else (5, 5)
		self.rig.pointer("move", *corner)
		time.sleep(0.3)
		x, y = self.reachable(win.rect)
		self.rig.pointer("move", x, y)
		## a hand moves on, and only that tells an X11 window the pointer is back
		self.rig.pointer("move", x + 4, y + 2)

	## The middle of the part of a window on the output most of it is on. A
	## window can hang off the layout, and a point off it sent the pointer
	## somewhere else, so the window never saw it come back (seed 400012).
	def reachable(self, rect: Rect) -> tuple[int, int]:
		best, area = None, 0
		for o in self.outs().values():
			r = (o["rect"]["x"], o["rect"]["y"], o["rect"]["width"], o["rect"]["height"])
			if overlap(rect, r) > area:
				best, area = r, overlap(rect, r)
		if best is None:
			return rect[0] + rect[2] // 2, rect[1] + rect[3] // 2
		x0, y0 = max(rect[0], best[0]), max(rect[1], best[1])
		x1, y1 = min(rect[0] + rect[2], best[0] + best[2]), min(rect[1] + rect[3], best[1] + best[3])
		return (x0 + x1) // 2, (y0 + y1) // 2

	## The window follows the pointer. Where it has been is kept, for the
	## outputs a look after the drop may mix in.
	def glide(self, frm: tuple[int, int], to: tuple[int, int]) -> None:
		for i in range(1, 11):
			self.rig.pointer("move", frm[0] + (to[0] - frm[0]) * i // 10, frm[1] + (to[1] - frm[1]) * i // 10)
			time.sleep(0.05)
			_, outs, touched, primary = self.where()
			self.track_entered(touched)
			if primary:
				self.drag_scales.add(outs[primary[0]]["scale"])
			self.drag_touched |= set(touched)
			self.drag_primaries |= set(primary)

	## A look may come between two places the glide was seen at, so any output
	## the window touched on the way may have been its first, against any it
	## was mostly on.
	def drag_mixes(self, outs: dict[str, dict[str, Any]]) -> None:
		if self.plan.arm != "wayland":
			return
		for p in self.drag_primaries & outs.keys():
			scale = outs[p]["scale"]
			for f in self.drag_touched & outs.keys() - {p}:
				if outs[f]["scale"] == scale:
					self.drag_names.setdefault(key_of("wayland", outs[f], scale), "same-scale outputs look alike to a Wayland window")

	## Where the window's top left goes for a drop on output `to`.
	def drop_point(self, step: Step, win: Win, outs: dict[str, dict[str, Any]]) -> tuple[int, int]:
		r = outs[f"HEADLESS-{step.to + 1}"]["rect"]
		tx, ty, tw, th = r["x"], r["y"], r["width"], r["height"]
		w, h = win.rect[2], win.rect[3] + win.deco
		x = tx + int(max(tw - w - 20, 0) * step.along) + 10
		y = ty + int(max(th - h - 20, 0) * (1 - step.along)) + 10 + win.deco
		if step.style == "sliver" and step.nearby >= 0:
			n = outs[f"HEADLESS-{step.nearby + 1}"]["rect"]
			nx, ny, nw, nh = n["x"], n["y"], n["width"], n["height"]
			cut_x, cut_y = int(w * step.share), int(h * step.share)
			if nx + nw == tx:
				x = tx - cut_x
			elif tx + tw == nx:
				x = nx - w + cut_x
			elif ny + nh == ty:
				y = ty - cut_y + win.deco
			elif ty + th == ny:
				y = ny - h + cut_y + win.deco
			if nx + nw == tx or tx + tw == nx:
				lo, hi = max(ty, ny), min(ty + th, ny + nh)
				y = max(lo, min(y - win.deco, hi - h)) + win.deco
			else:
				lo, hi = max(tx, nx), min(tx + tw, nx + nw)
				x = max(lo, min(x, hi - w))
		return x, y

	def astride(self, step: Step, win: Win, outs: dict[str, dict[str, Any]]) -> tuple[int, int]:
		t = outs[f"HEADLESS-{step.to + 1}"]["rect"]
		n = outs[f"HEADLESS-{step.nearby + 1}"]["rect"]
		w, h = win.rect[2], win.rect[3]
		if n["x"] + n["width"] == t["x"] or t["x"] + t["width"] == n["x"]:
			edge = t["x"] if n["x"] + n["width"] == t["x"] else n["x"]
			lo, hi = max(t["y"], n["y"]), min(t["y"] + t["height"], n["y"] + n["height"])
			return edge - w // 2, max(lo, min((lo + hi) // 2 - h // 2, hi - h)) + win.deco
		edge = t["y"] if n["y"] + n["height"] == t["y"] else n["y"]
		lo, hi = max(t["x"], n["x"]), min(t["x"] + t["width"], n["x"] + n["width"])
		return max(lo, min((lo + hi) // 2 - w // 2, hi - w)), edge - h // 2 + win.deco

	def run(self) -> None:
		plan, rig, model = self.plan, self.rig, self.model
		rig.start()
		self.write_config()
		self.relaunch(Step("relaunch", to=plan.start, cli=plan.cli, font=plan.font, persist=plan.persist), first=True)
		for n, step in enumerate(plan.steps):
			what = f"step {n + 1}, {step}"
			self.note(f"  {what}")
			before_scale = self.scale
			self.was = (len(rig.size_lines()) - 1, self.drawn_at(before_scale))
			self.drag_names = {}
			self.drag_scales = set()
			self.drag_touched = set()
			self.drag_primaries = set()
			if not step.pointer and step.kind != "drag":
				self.park_pointer()
			if step.kind == "move":
				rig.msg(f"[pid={rig.app.pid if rig.app else 0}] move container to output HEADLESS-{step.to + 1}")
				time.sleep(0.3)
				_, outs, touched, primary = self.where()
				self.track_entered(touched)
				after_scale = outs[primary[0]]["scale"] if primary else before_scale
				if step.pointer:
					self.pointer_back()
				self.after_move(what, outs, primary, step.pointer, after_scale != before_scale)
			elif step.kind == "drag":
				self.drag(what, step, before_scale)
			elif step.kind == "resize":
				## sway resizes about the middle, so the window goes back to the
				## output's top left after, to stay on it.
				win, outs, _, primary = self.where()
				r = outs[primary[0]]["rect"] if primary else outs[win.output]["rect"]
				self.before_user(what, step)
				_, outs, _, primary = self.where()
				before = {k: LEFT_LATE for k in self.names(outs, primary)} if plan.arm == "wayland" else {}
				pid = rig.app.pid if rig.app else 0
				rig.msg(f"[pid={pid}] resize set {int(r['width'] * step.size[0])} {int(r['height'] * step.size[1])}")
				_, outs, touched, primary = self.where()
				self.track_entered(touched)
				resized_on = self.names(outs, primary)
				rig.msg(f"[pid={pid}] move absolute position {r['x'] + 10} {r['y'] + 10 + win.deco}")
				model.size_pinned = False
				## The move can take it off an output it had a sliver on, and the
				## window names its output as the resize comes, before the move or
				## after it.
				_, outs, touched, primary = self.where()
				self.track_entered(touched)
				self.user_save(what, {**before, **resized_on, **self.names(outs, primary)}, None if model.font_pinned else model.zoom)
			elif step.kind == "zoom":
				self.before_user(what, step)
				_, outs, _, primary = self.where()
				target = 0 if step.zoom == 0 else model.zoom + step.zoom
				if step.zoom == 0:
					rig.ctrl_key("0")
				else:
					for _ in range(abs(step.zoom)):
						rig.ctrl_key("equal" if step.zoom > 0 else "minus")
				if target == model.zoom:
					self.settle(what, {None: "requirement"}, False)
					continue
				model.font_pinned = False
				self.user_save(what, self.names(outs, primary), target)
			elif step.kind == "relaunch":
				self.relaunch(step)
			elif step.kind == "fullscreen":
				if self.waiting:
					self.before_user(what, step)
				pid = rig.app.pid if rig.app else 0
				## A Wayland window moved without being told finds out where it
				## is from the pointer coming over it, here just before it goes
				## fullscreen, so it looks while fullscreen.
				hovered = plan.arm == "wayland" and self.stale is not None
				if hovered:
					over, _, _, _ = self.where()
					rig.pointer("move", *self.reachable(over.rect))
				rig.msg(f"[pid={pid}] fullscreen enable")
				took = self.fullscreen_saves(f"{what} (fullscreen)")
				rig.msg(f"[pid={pid}] fullscreen disable")
				if self.fullscreen_saves(f"{what} (back)") or took:
					_, outs, _, primary = self.where()
					self.settle(what, self.names(outs, primary), True)
				else:
					self.settle(what, {None: "requirement"}, False)
				if hovered:
					self.park_pointer()
			elif step.kind == "rescale":
				rig.msg(f"output HEADLESS-{step.to + 1} scale {step.scale:g}")
				rig.pointer("extent", *rig.extent())
				time.sleep(0.3)
				_, outs, touched, primary = self.where()
				self.track_entered(touched)
				after_scale = outs[primary[0]]["scale"] if primary else before_scale
				looks = plan.arm == "wayland" and after_scale != before_scale
				if looks:
					self.pointer_back()
					self.waiting = False
				allowed = self.names(outs, primary)
				if not looks and plan.arm == "x11":
					allowed = {None: "an X11 window is told nothing when the layout shifts under it"}
				## sway tells the window under a resting pointer that the pointer
				## left and came back as the layout changes, which is a look
				## (2026101422190941)
				reentered = plan.arm == "wayland" and not looks and self.under_pointer(outs)
				if reentered:
					allowed = {**allowed, None: TOLD_NOTHING}
				self.settle(what, allowed, looks or reentered or plan.arm == "x11", after_scale != before_scale)
				self.within_reach(what)
		rig.stop_app()
		time.sleep(0.5)
		last, monitors = rig.state()
		want_last = model.last
		if last != want_last:
			raise CheckError(f"the last size on disk is {last}, want {want_last}")
		if monitors != model.store:
			raise CheckError(f"the sizes on disk are {monitors}, want {model.store}")

	## An output that shrinks with a scale change can leave the window's title
	## bar or middle over a gap, where no pointer can go, so a drag would grab
	## nothing (2026101614100000). A person would move it back by key, so the
	## window goes to the middle of its output, with the pointer away. The
	## monitors changed since the window last looked, so it may look at once
	## rather than wait for the person.
	def within_reach(self, what: str) -> None:
		win, outs, _, primary = self.where()
		spots = [(win.rect[0] + win.rect[2] // 2, win.rect[1] - win.deco // 2), (win.rect[0] + win.rect[2] // 2, win.rect[1] + win.rect[3] // 2)]
		rects = [(o["rect"]["x"], o["rect"]["y"], o["rect"]["width"], o["rect"]["height"]) for o in outs.values()]
		if all(any(overlap((*spot, 1, 1), r) for r in rects) for spot in spots):
			return
		before_scale = outs[primary[0]]["scale"] if primary else self.scale
		what = f"{what}, then moved back in reach by key"
		self.note(f"  {what}")
		self.was = (len(self.rig.size_lines()) - 1, self.drawn_at(before_scale))
		self.park_pointer()
		self.rig.msg(f"[pid={self.rig.app.pid if self.rig.app else 0}] move position center")
		time.sleep(0.3)
		_, outs, touched, primary = self.where()
		self.track_entered(touched)
		rescaled = (outs[primary[0]]["scale"] if primary else before_scale) != before_scale
		waits = self.waiting or self.plan.arm == "x11" or rescaled
		self.settle(what, {**self.names(outs, primary), None: WAITS if waits else TOLD_NOTHING}, True, rescaled)
		self.waiting = waits

	def drag(self, what: str, step: Step, before_scale: float) -> None:
		rig = self.rig
		win, outs, _, _ = self.where()
		grab = (win.rect[0] + win.rect[2] // 2, win.rect[1] - win.deco // 2)
		dx, dy = grab[0] - win.rect[0], grab[1] - win.rect[1]
		rig.pointer("move", *grab)
		rig.pointer("down")
		mark = len(rig.grid_lines())
		start = rig.grid() or self.model.grid
		self.drag_scales = {before_scale}
		self.drag_names = {}
		at = grab
		if step.hold:
			x, y = self.astride(step, win, outs)
			self.glide(at, (x + dx, y + dy))
			at = (x + dx, y + dy)
			time.sleep(step.hold)
		x, y = self.drop_point(step, win, outs)
		self.glide(at, (x + dx, y + dy))
		time.sleep(0.3)
		self.held_still(what, mark, start)
		rig.pointer("up")
		time.sleep(0.3)
		_, outs, touched, primary = self.where()
		self.drag_mixes(outs)
		self.track_entered(touched)
		after_scale = outs[primary[0]]["scale"] if primary else before_scale
		if step.pointer:
			time.sleep(1.0)
			self.pointer_back()
		self.after_move(what, outs, primary, step.pointer, after_scale != before_scale or len(self.drag_scales) > 1)

	## A move looks once the pointer is back over the window. With the pointer
	## away it doesn't: an X11 window was told it moved and puts the look off
	## until the person is back, as does a Wayland one whose scale changed.
	def after_move(self, what: str, outs: dict[str, dict[str, Any]], primary: list[str], back: bool, rescaled: bool) -> None:
		waits = self.waiting or self.plan.arm == "x11" or rescaled
		self.settle(what, self.names(outs, primary), back, rescaled, unseen=WAITS if waits else TOLD_NOTHING)
		self.waiting = waits and not back

	def relaunch(self, step: Step, first: bool = False) -> None:
		rig, model, plan = self.rig, self.model, self.plan
		what = "launch" if first else f"relaunch on output {step.to + 1}"
		rig.stop_app()
		saved = [t for t in rig.trace() if t.split()[1] == "save"]
		if saved:
			raise CheckError(f"{what}: the window saved a size nobody set as it closed: {saved}")
		rig.msg(f"focus output HEADLESS-{step.to + 1}")
		## a Wayland window opens at scale 1 and learns its own after
		self.was = (len(rig.size_lines()), 1.0)
		rig.launch(step.cli, step.font, step.persist)
		model.size_pinned = step.cli is not None
		model.font_pinned = step.font is not None
		self.font = step.font
		self.saves = step.persist or not (step.cli or step.font)
		model.belief = None
		model.entered = []
		self.stale = None
		self.waiting = False
		self.owed = []
		time.sleep(0.5)
		win, outs, touched, primary = self.where()
		self.track_entered(touched)
		trace = rig.trace()
		opened = [t for t in trace if t.split()[1] == "open"]
		if not opened:
			raise CheckError(f"{what}: no open line in the trace")
		## what came after the open is the look's, for settle
		rig.unread = trace[trace.index(opened[0]) + 1:]
		## The open itself: where it is not known yet on Wayland, and where the
		## pointer is on X11, which may be another monitor than the window's.
		grid_at_open = trace_field(opened[0], "grid")
		model.belief = None if trace_field(opened[0], "key") == "-" else trace_field(opened[0], "key")
		cols, rows = (int(v) for v in grid_at_open.split("x"))
		model.grid = (cols, rows)
		model.zoom = int(trace_field(opened[0], "zoom"))
		if step.cli and model.grid != step.cli:
			raise CheckError(f"{what}: opened at {model.grid}, not the {step.cli} asked for")
		if not plan.remember and not step.cli and model.grid != plan.fixed:
			raise CheckError(f"{what}: opened at {model.grid}, not the set {plan.fixed}")
		if plan.remember and not step.font:
			want = model.kept(model.belief)
			if model.zoom != want.zoom:
				raise CheckError(f"{what}: opened at font zoom {model.zoom}, want {want.zoom}")
			if not step.cli and model.grid != (want.cols, want.rows):
				raise CheckError(f"{what}: opened at {model.grid} on {model.belief}, want {want}")
		atfont = [t for t in trace[:trace.index(opened[0])] if t.split()[1] == "atfont"]
		expect: Grid | None = None
		opened_on = model.belief
		if plan.remember and step.font and not step.cli:
			want = model.kept(model.belief)
			expect = (want.cols, want.rows)
			if atfont:
				cols, rows = (int(v) for v in trace_field(atfont[-1], "grid").split("x"))
				expect = (cols, rows)
			if model.grid != expect:
				raise CheckError(f"{what}: opened at {model.grid} on {model.belief}, want {expect} from {want}")
		elif atfont:
			raise CheckError(f"{what}: opened a kept size for another font with none on the command line or a size: {atfont}")
		if not plan.remember:
			model.zoom = 0
		self.note(f"  {what}: opened at {model.grid} zoom {model.zoom}, counted on {model.belief}")
		self.scale = outs[primary[0]]["scale"] if primary else 1.0
		## A Wayland window opens at 1 and learns its scale after.
		first_scale = outs[primary[0]]["scale"] if primary and plan.arm == "wayland" else 1.0
		self.settle(what, self.names(outs, primary), True, first_scale != 1.0, look_wait=12.0)
		## or the grid it opened at, where the look kept it
		if expect and model.grid == expect and opened_on:
			self.atfont = opened_on
			self.check_atfont(what)
		## A size set at a font size from the command line, on this monitor,
		## fits it at the normal font: in its columns and rows if they fit,
		## else at the size the window was.
		if not step.font and model.belief in self.refonted and plan.per_monitor:
			win, outs, _, primary = self.where()
			out = outs[primary[0]]["rect"] if primary else outs[win.output]["rect"]
			if win.rect[2] > out["width"] or win.rect[3] > out["height"]:
				raise CheckError(f"{what}: a size set at a font size from the command line opened at {win.rect[2]}x{win.rect[3]}, "
					f"past the {out['width']}x{out['height']} output")


def run_case(seed: int, arm: str | None, binary: Path, folder: Path, keep: bool) -> tuple[bool, Case]:
	plan = make_plan(seed, arm)
	work = Path(tempfile.mkdtemp(prefix=f"dpirand.{seed}.", dir=folder))
	case = Case(plan, work, binary)
	ok = True
	try:
		case.run()
	except CheckError as e:
		case.note(f"  FAIL {e}")
		ok = False
	finally:
		case.rig.close()
	if ok and not keep:
		shutil.rmtree(work, ignore_errors=True)
	else:
		case.note(f"  kept {work}")
	return ok, case


def stop_now(signum: int, frame: object) -> None:
	raise KeyboardInterrupt


def main() -> int:
	parser = argparse.ArgumentParser(add_help=True)
	parser.add_argument("--bin", type=Path)
	parser.add_argument("--seed", type=int, default=None)
	parser.add_argument("--secs", type=float, default=None)
	parser.add_argument("--plan", type=int, default=0)
	parser.add_argument("-v", "--verbose", action="store_true")
	parser.add_argument("--keep", action="store_true")
	parser.add_argument("--from", type=int, default=None, dest="start")
	arms = parser.add_mutually_exclusive_group()
	arms.add_argument("--wayland", action="store_const", const="wayland", dest="arm")
	arms.add_argument("--x11", action="store_const", const="x11", dest="arm")
	args = parser.parse_args()
	sys.stdout.reconfigure(line_buffering=True)  # type: ignore[union-attr]
	## so a case cut short still takes its compositor and window down with it
	signal.signal(signal.SIGTERM, stop_now)

	holes = generator_holes()
	if holes:
		print(f"FAIL the case generator no longer makes: {', '.join(holes)}")
		return 1
	env_seed = os.environ.get("SILK_DPIRAND_SEED", "")
	one = args.seed if args.seed is not None else (int(env_seed) if env_seed else None)
	if args.plan:
		base = one if one is not None else 0
		for seed in range(base, base + args.plan):
			print("\n".join(make_plan(seed, args.arm).describe()))
		return 0

	binary: Path | None = args.bin
	if binary is None:
		target = Path(os.environ.get("CARGO_TARGET_DIR", "target"))
		target = target if target.is_absolute() else ROOT / target
		binary = next((p for p in (target / "debug/silkterm", target / "release/silkterm") if p.is_file()), None)
	if binary is None or not binary.is_file():
		print("  skip: no SilkTerm binary (build it first, or pass --bin)")
		return 3
	for tool in ("sway", "swaymsg", "wtype", "xdotool", "ss"):
		if not shutil.which(tool):
			print(f"  skip: no {tool}")
			return 3
	if not LVP.is_file():
		print(f"  skip: no software Vulkan ({LVP})")
		return 3
	print(f"  binary: {binary}")
	secs = args.secs if args.secs is not None else float(os.environ.get("SILK_DPIRAND_SECS", "") or 60)
	folder = _testdir.use()
	base = one if one is not None else args.start if args.start is not None else int(time.time() * 1000) % 1_000_000_000
	print(f"  seeds from {base}")
	deadline = time.monotonic() + secs
	failed: list[int] = []
	ran = 0
	limits: Counter[str] = Counter()
	known: Counter[str] = Counter()
	kinds: Counter[str] = Counter()
	seed = base
	while ran == 0 or (one is None and time.monotonic() < deadline):
		started = time.monotonic()
		try:
			ok, case = run_case(seed, args.arm, binary, folder, args.keep)
		except NoRigError as e:
			print(f"  skip: {e}")
			_testdir.end(3)
			return 3
		ran += 1
		limits.update(case.limits)
		known.update(case.known)
		kinds.update(s.kind for s in case.plan.steps)
		took = time.monotonic() - started
		print(f"  {'ok  ' if ok else 'FAIL'} seed {seed} ({case.plan.arm}, {len(case.plan.outs)} outputs, {len(case.plan.steps)} steps, {took:.0f} s)")
		if not ok or args.verbose:
			print("\n".join("    " + line for line in case.plan.describe() + case.log))
		if not ok:
			failed.append(seed)
			print(f"    rerun: cicd/tests/dpirand/run.py --seed {seed}")
		seed += 1
	print(f"  {ran} cases, {sum(kinds.values())} steps: " + ", ".join(f"{k} {n}" for k, n in sorted(kinds.items())))
	for reason, n in sorted(limits.items()):
		print(f"  allowed {n}x: {reason}")
	for item, n in sorted(known.items()):
		print(f"  known {n}x: {item}")
	status = 1 if failed else 0
	print(f"{len(failed)} failed: seeds {' '.join(map(str, failed))}" if failed else "all passed")
	_testdir.end(status)
	return status


if __name__ == "__main__":
	sys.exit(main())

##	History:
##		- 20261014 JC: Created.
##		- 20261014 JC: Fullscreen and back saves nothing, and a size set at a font
##		  size from the command line is kept for the normal font.
##		- 20261015 JC: A size set after a move the window wasn't told of is for the
##		  output it's on, and a scale change under a resting pointer may look.
##		- 20261015 JC: A kept size used at a font size from the command line keeps
##		  its columns and rows where they fit there, else the kept window's size.
##		- 20261016 JC: A window with a size or a font size from the command line
##		  saves nothing unless --remember-size comes with it, and a grid moved by
##		  a scale change is judged from the cells the shell is told.
##		- 20261016 JC: A window a scale change leaves out of reach is moved back by
##		  key, a grid may take one late step to where it ends, and a take's drift
##		  is judged from the cells.
##		- 20261016 JC: A take's grid has to hold through the scale it brings.
