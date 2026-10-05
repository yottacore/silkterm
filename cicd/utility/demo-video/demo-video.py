#!/usr/bin/env python3

##	Purpose:
##		Record the SilkTerm demo video and README gif: drives a real SilkTerm on a
##		private Xvfb (never :0) inside a decorated window, types at a realistic pace
##		(variable wpm, occasional fixed typos), lays down keyboard/mouse foley
##		synced to the actual input timestamps, overlays per-segment narration, and
##		encodes the deliverables. Two recordings from one script, each maxing out
##		its format:
##		  video: 1920x1080@60 h265, font 1.5x the defined size, with audio
##		  gif:   960x540@50 native, defined font size, optimized palette, silent
##		The app is rendered ON THE GPU via VirtualGL (vglrun -d egl); on plain
##		llvmpipe the Xvfb caps it near 10fps and the scroll judders, which no
##		capture rate or frame-averaging can fix (the frames aren't there to blend).
##		On the GPU it paints as fast as vblank lets it, so the run PINS the app's
##		frame rate to the capture rate (SILK_MAX_FPS, see PROFILES) rather than
##		guessing at it. The window size is passed at LAUNCH (--pixel-width/height), never
##		resized after: the VGL EGL present latches the surface size at creation
##		(the app's xcb event connection bypasses VGL's Xlib interposer), so a
##		post-launch xdotool resize leaves a stale-offset blit (clipped video /
##		band-at-top gif). The outro comment goes gray via a prompt flag (no
##		ble.sh - it drops the odd first keystroke and breaks commands).
##		Narration lives in a black band above the window (BAND px of bare root,
##		the window is placed below it) - plain yellow text, no box, so nothing
##		ever covers the terminal. The band is static, which costs a gif almost
##		nothing (the encoder only stores what changes between frames). The window
##		decoration is generated at record time (a square-cornered dark theme
##		recolored slate blue-gray) so it reads as chrome against both the black
##		band and the terminal's own colors.
##		Settings changes shown mid-run (the cursor ones) go through the app's
##		control socket, so they arrive live with nothing typed on camera.
##		Both profiles start opaque on a plain black background (no image); the
##		closing scenes turn the built-in wallpaper on through the control socket.
##		Screens are cleared between scenes except where the next command is meant
##		to push the previous output up - a cleared screen means the typing that
##		follows changes few pixels, which is most of what keeps the gif small.
##		Gif size is set almost entirely by the scrolling scenes: measured per 5s
##		window, scrolling ran 20-40x the byte cost of everything else, so their
##		LENGTH and the WIDTH of the rows in motion are the only real levers (palette
##		size, dither and lossy were all measured and are not worth their artifacts).
##		Hence: short listings with narrow columns, a brief build, one direction of
##		wheel, and no full-screen-app scene at all.
##	Syntax:
##		demo-video.py [--profile video,gif] [--segments a,b,...] [--seed N]
##		              [--keep-work] [--no-rotate] [--no-asset] [--display :98]
##		              [--out-dir DIR]
##		Env: SILK_BIN overrides the binary (default release/silkterm under
##		CARGO_TARGET_DIR, or REPO/target).
##	Notes:
##		AV sync needs no calibration: before the app launches, the bare root is
##		flashed white (xsetroot) at a recorded wall-clock time; the bright frame
##		is found in the capture afterwards, anchoring every event epoch to video
##		time exactly. Sound assets + licenses live in ./sounds/ (see LICENSES.txt).
##	History: at bottom.

##	Copyright (c) 2026 Bubbles
##	Licensed under The MIT License (MIT). Full text at:
##		https://mit-license.org/
##	SPDX-License-Identifier: MIT

import argparse
import colorsys
import getpass
import json
import math
import os
import pwd
import random
import re
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import time
import wave
from collections.abc import Callable
from pathlib import Path
from typing import Any, TypedDict

import numpy as np
from scipy import signal as spsig

ME_DIR   = Path(__file__).resolve().parent
REPO     = ME_DIR.parents[2]                  # github/cicd/utility/demo-video -> github
PRIVATE  = REPO.parent / "private" / "demo-video"
SOUNDS   = ME_DIR / "sounds"

SR         = 48000                            # audio mix rate
BANNER_TTF = "/usr/share/fonts/truetype/lato/Lato-Semibold.ttf"
BANNER_FG  = "0xFFD866"                       # warm yellow, on the black band above the window
LEAD_S     = 0.8                              # quiet lead-in kept before the first segment
TAIL_HOLD_S  = 4.5                            # freeze the final frame this long at the end...
TAIL_BLACK_S = 2.0                            # ...then a fully black screen this long
TAIL_EXTRA   = TAIL_HOLD_S + TAIL_BLACK_S     # total appended tail (added at encode, not captured)
FOLEY_LAG  = 0.03                             # foley sits this far after the key event (the app
                                              # paints the glyph a frame or two later; sound-to-
                                              # picture reads tighter than sound-to-keypress)

# The faux window fills the frame below the narration band: a black border shows
# around it, and BAND px of bare root above it carry the captions. FRAME_* are the
# decoration extents (left,right,titlebar,bottom in px) - the client is sized so
# the outer frame sits BORDER px inside the left/right/bottom edges and BAND px
# below the top.
BORDER   = 8
FRAME_L, FRAME_R, FRAME_T, FRAME_B = 2, 2, 32, 2

# The decoration is built at record time from a square-cornered dark theme (its
# parts are flat one-color SVGs, so a color swap is the whole job) and dropped
# in the WM's own throwaway HOME - nothing is installed system-wide, and the
# WM's session gets every XDG folder inside that HOME too (see wm_env). Slate
# blue-gray: it has to read as chrome next to mint terminal text and warm yellow
# captions, while separating the window from the black border and black band.
WM_BASE_THEME = "Material-Black-Pistachio"  # SQUARE corners (opaque top-left)
WM_THEME      = "SilkDemo"
DECO_BG       = "#5c6a8a"                   # active titlebar + frame
DECO_BG_OFF   = "#3c4557"                   # inactive
DECO_GLYPH    = "#e8eefa"                   # button glyphs
DECO_TEXT     = "#eef3fb"                   # title text

# cap_fps == out_fps == what the app paints, and all three are now WELDED: the
# run exports SILK_MAX_FPS=cap_fps (see app_env), which pins the app's
# animation loop to that rate and takes vblank out of the picture.
#
# The welding is the whole point, and the shipped gif is what proved it matters.
# x11grab samples the X screen on a fixed clock and gets whatever frame is
# current, so the step between two samples is always a whole number of the app's
# frames. Unless the app's rate divides the capture rate evenly, that number is
# not constant: it was painting 60 (vblank on a 60Hz host) into a 50fps capture,
# so one frame in six was dropped and every fifth stored frame carried two frames
# of travel. Measured on the shipped gif, a scroll reads -11 -11 -22, -10 -10 -19,
# -8 -8 -8 -7 -14, +5 +5 +5 +5 +10: an exact doubling on a strict period, at every
# speed and in both directions. Speed-independence is what rules the app's own
# scrolling out - a scroll defect does not double itself identically at 3px/frame
# and at 11px/frame. That regular hitch is what reads as the picture jumping, and
# no amount of scroll tuning could ever have removed it.
#
# Pinning also removes a hidden dependency nobody would have looked for: without
# it the source rate is the RECORDING HOST'S refresh rate, so the same script on
# a 144Hz box beats against both profiles.
#
# A gif stores each frame's delay in whole centiseconds, so its rate must be
# 100/n: 50 is 2cs. The video has no such constraint.
class Profile(TypedDict):
	size: tuple[int, int]
	cap_fps: int
	out_fps: int
	mono_pt: float
	ui_pt: int
	banner_fs: int
	band: int
	audio: bool
	banner_min: float

PROFILES = {
	"video": Profile(
		size=(1920, 1080), cap_fps=60, out_fps=60, mono_pt=19.5, ui_pt=11,
		banner_fs=38, band=112, audio=True, banner_min=4.0,
	),
	"gif": Profile(
		size=(960, 540), cap_fps=50, out_fps=50, mono_pt=13, ui_pt=10,
		banner_fs=24, band=60, audio=False, banner_min=3.0,
	),
}

def log(msg: str) -> None:
	print(f"[demo] {msg}", flush=True)

def run(cmd: list[str], **kw: Any) -> subprocess.CompletedProcess[Any]:
	return subprocess.run(cmd, check=True, **kw)

def out_of(cmd: list[str]) -> str:
	return subprocess.run(cmd, check=True, capture_output=True, text=True).stdout


##•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
##	Recorder: display/app/capture lifecycle + the event/banner logs

def run_user() -> str:
	# gui-headless.bash names its run folder the same way; USER is unset under
	# cron and in some ssh contexts
	return os.environ.get("USER") or pwd.getpwuid(os.getuid()).pw_name

def target_dir() -> Path:
	# cargo is run from the repo, so a relative CARGO_TARGET_DIR hangs off it
	return REPO / os.environ.get("CARGO_TARGET_DIR", "target")

def gpu_prefix(e: dict[str, str]) -> list[str]:
	"""Command prefix that puts the app on a real GPU, editing env `e` as needed.

	Without one, llvmpipe caps the app near 10fps on Xvfb and the scroll judders,
	which no capture rate can fix afterwards. Two hosts, two answers: VirtualGL on
	a normal Linux box, and on WSL the d3d12 Mesa driver, which reaches the Windows
	GPU directly and needs no 3D X server (there is no vglrun there anyway).
	"""
	if shutil.which("vglrun"):
		return ["vglrun", "-d", "egl"]
	if Path("/dev/dxg").exists() and any(Path("/usr/lib").glob("*/dri/d3d12_dri.so")):
		e.update(MESA_LOADER_DRIVER_OVERRIDE="d3d12", GALLIUM_DRIVER="d3d12")
		log("GPU: d3d12 (WSL)")
		return []
	log("WARNING: no GPU path found - software GL, the scroll will judder")
	e["LIBGL_ALWAYS_SOFTWARE"] = "1"
	return []


class Rec:
	def __init__(self, args: argparse.Namespace, profile: Profile) -> None:
		self.p        = profile
		self.size     = profile["size"]
		self.band     = profile["band"]         # black narration strip above the window
		self.cap_fps  = profile["cap_fps"]
		self.out_fps  = profile["out_fps"]
		self.display  = args.display
		self.num      = self.display.lstrip(":")
		self.auth     = f"/tmp/cicd-gui-headless-{run_user()}/Xauthority-{self.num}"
		self.bin      = os.environ.get("SILK_BIN") or str(target_dir() / "release/silkterm")
		self.work     = Path(tempfile.mkdtemp(prefix="silk-demo-"))
		self.home     = self.work / "home"
		self.wmhome   = self.work / "wmhome"    # the WM's HOME: theme + its own xfconf
		self.keep     = args.keep_work
		self.events: list[tuple[float, str]] = []    # (epoch, kind) kind: key:NAME / mouse:NAME
		self.banners: list[tuple[float, float, str]] = []    # (epoch_start, epoch_end, text)
		self.cuts: list[tuple[float, float]] = []    # (epoch_start, epoch_end) left out of the finished take
		self.app: subprocess.Popen[bytes] | None = None
		self.ff: subprocess.Popen[bytes] | None = None
		self.wm: subprocess.Popen[bytes] | None = None
		self.win      = ""      # the app's X window id, once it is up
		self.raw      = self.work / "raw.mkv"
		self.launch_e = 0.0     # wall-clock epoch the app was started
		self.flash_e  = 0.0     # wall-clock epoch of the white sync flash
		self.flash_vt = 0.0     # video time of the sync flash, found at encode
		self.t0_e     = 0.0     # wall-clock epoch where trimmed content starts
		self.seg_marks: dict[str, float] = {}    # segment name -> wall-clock epoch it started

	def env(self) -> dict[str, str]:
		e = dict(os.environ)
		e.update(DISPLAY=self.display, XAUTHORITY=self.auth, LIBGL_ALWAYS_SOFTWARE="1")
		# Setting DISPLAY is not enough to move a winit app onto our private Xvfb:
		# it prefers Wayland whenever WAYLAND_DISPLAY is set, so on any Wayland
		# session (WSLg included) the window opens on the real desktop instead and
		# nothing here can find it. Everything we launch belongs on the X display.
		for k in ("WAYLAND_DISPLAY", "XDG_SESSION_TYPE"):
			e.pop(k, None)
		return e

	def cut_s(self) -> float:
		return sum(e - s for s, e in self.cuts)

	# an epoch as it falls in the finished take, with every cut before it taken
	# out. One inside a cut lands on the cut's start.
	def net(self, epoch: float) -> float:
		gone = 0.0
		for s, e in self.cuts:
			if epoch >= e:
				gone += e - s
			elif epoch > s:
				gone += epoch - s
		return epoch - gone

	def xdo(self, *a: str) -> None:
		subprocess.run(["xdotool", *a], env=self.env(), check=False,
			stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

	def _frame_extents(self, win: str) -> tuple[int, int, int, int]:
		# _NET_FRAME_EXTENTS = left, right, top, bottom (px). Falls back to the
		# theme's known extents if the WM hasn't set the hint yet.
		r = subprocess.run(["xprop", "-id", win, "_NET_FRAME_EXTENTS"],
			env=self.env(), capture_output=True, text=True, check=False)
		m = re.search(r"=\s*(\d+),\s*(\d+),\s*(\d+),\s*(\d+)", r.stdout)
		if not m:
			return FRAME_L, FRAME_R, FRAME_T, FRAME_B
		return int(m[1]), int(m[2]), int(m[3]), int(m[4])

	def _client_xy(self, win: str) -> tuple[int, int]:
		r = subprocess.run(["xwininfo", "-id", win], env=self.env(),
			capture_output=True, text=True, check=False).stdout
		x = re.search(r"Absolute upper-left X:\s*(-?\d+)", r)
		y = re.search(r"Absolute upper-left Y:\s*(-?\d+)", r)
		if not x or not y:
			raise RuntimeError(f"xwininfo gave no position for window {win}")
		return int(x[1]), int(y[1])

	def place_window(self, win: str) -> None:
		# nudge the window so its OUTER frame sits BORDER px inside the left edge
		# and just under the narration band. xdotool's move semantics vs the
		# reparenting frame are fuzzy, so measure the real frame-outer after each
		# move and correct by the residual (converges in a step or two).
		want_y = BORDER + self.band
		target = [BORDER, want_y]
		for _ in range(4):
			self.xdo("windowmove", win, str(target[0]), str(target[1]))
			time.sleep(0.25)
			left, _right, top, _bottom = self._frame_extents(win)
			cx, cy = self._client_xy(win)
			dx, dy = BORDER - (cx - left), want_y - (cy - top)
			if abs(dx) <= 1 and abs(dy) <= 1:
				break
			target[0] += dx
			target[1] += dy

	def make_theme(self) -> str:
		# Recolour the base theme into the demo's slate decoration, into the WM's
		# own HOME. Every frame part is a flat <rect fill="..."> so the swap is a
		# string replace; the button glyphs are separate files and keep their shape.
		base = Path("/usr/share/themes") / WM_BASE_THEME / "xfwm4"
		if not base.is_dir():
			log(f"WARNING: theme {WM_BASE_THEME} not installed - using the WM default")
			return "Default"
		dst = self.wmhome / ".themes" / WM_THEME / "xfwm4"
		dst.parent.mkdir(parents=True, exist_ok=True)
		shutil.copytree(base, dst, dirs_exist_ok=True)
		for svg in dst.rglob("*.svg"):
			text = svg.read_text()
			svg.write_text(text.replace("#09090a", DECO_BG)
				.replace("#1a1c1e", DECO_BG_OFF).replace("#a3a3a3", DECO_GLYPH))
		rc = dst / "themerc"
		rc.write_text(re.sub(r"(?m)^(active_text(_shadow)?_color)=.*",
			rf"\1={DECO_TEXT}", rc.read_text()))
		return WM_THEME

	def start_display(self) -> None:
		# each profile records at its own resolution, so cycle the display; the WM
		# is ours (not gui-headless --wm) so xfconf can point it at the generated
		# theme - the window's real decoration is what frames the shot
		gh = str(REPO / "cicd/utility/gui-headless.bash")
		e = dict(os.environ, CICD_HEADLESS_DISPLAY=self.display,
			CICD_HEADLESS_SIZE=f"{self.size[0]}x{self.size[1]}x24")
		subprocess.run([gh, "stop"], env=e, capture_output=True, check=False)
		run([gh, "start"], env=e)
		self.start_wm(self.make_theme())
		time.sleep(2.0)
		# pure black so the thin border framing the window reads as black, not a tint
		subprocess.run(["xsetroot", "-solid", "#000000"], env=self.env(), check=False)

	def wm_env(self) -> dict[str, str]:
		# xfconfd keeps its channels under XDG_CONFIG_HOME, not HOME, so with the
		# desktop's own one inherited a recording wrote its theme, title font and
		# buttons over the real desktop's. Every base folder goes inside wmhome.
		e = self.env()
		e["HOME"] = str(self.wmhome)
		for var, sub in (("XDG_CONFIG_HOME", ".config"), ("XDG_DATA_HOME", ".local/share"),
				("XDG_CACHE_HOME", ".cache"), ("XDG_STATE_HOME", ".local/state"),
				("XDG_RUNTIME_DIR", "run")):
			d = self.wmhome / sub
			d.mkdir(parents=True, exist_ok=True, mode=0o700)
			e[var] = str(d)
		return e

	def start_wm(self, theme: str, wm: str = "xfwm4 --compositor=off --vblank=off") -> None:
		# its own session, so stop_wm can end the bus and xfconfd with it. The
		# child keeps its own copy of the log, so ours closes once it has started.
		with (self.work / "wm.log").open("w") as wm_log:
			self.wm = subprocess.Popen(["dbus-run-session", "--", "sh", "-c",
				f'xfconf-query -c xfwm4 -p /general/theme --create -t string -s "{theme}"; '
				'xfconf-query -c xfwm4 -p /general/title_font --create -t string -s "Lato Bold 10"; '
				'xfconf-query -c xfwm4 -p /general/button_layout --create -t string -s "O|HMC"; '
				f"exec {wm}"],
				env=self.wm_env(), stdout=wm_log,
				stderr=subprocess.STDOUT, start_new_session=True)

	def stop_wm(self) -> None:
		# dbus-run-session dies on SIGTERM and leaves its bus, xfconfd and the WM
		# running under init. They all stay in its process group, so end that.
		if not self.wm:
			return
		for sig in (signal.SIGTERM, signal.SIGKILL):
			try:
				os.killpg(self.wm.pid, sig)
			except ProcessLookupError:
				break
			deadline = time.time() + 5
			while time.time() < deadline and self.wm_survivors():
				time.sleep(0.1)
			if not self.wm_survivors():
				break
		try:
			self.wm.wait(timeout=5)
		except subprocess.TimeoutExpired:
			pass
		self.wm = None

	def wm_survivors(self) -> bool:
		if not self.wm:
			return False
		self.wm.poll()                         # reap the leader, or it counts as alive
		try:
			os.killpg(self.wm.pid, 0)
			return True
		except ProcessLookupError:
			return False

	def stop_display(self) -> None:
		self.stop_wm()
		gh = str(REPO / "cicd/utility/gui-headless.bash")
		e = dict(os.environ, CICD_HEADLESS_DISPLAY=self.display)
		subprocess.run([gh, "stop"], env=e, capture_output=True, check=False)

	def start_capture(self) -> None:
		self.raw = self.work / "raw.mkv"
		with (self.work / "ffmpeg.log").open("w") as ff_log:
			self.ff = subprocess.Popen([
				"ffmpeg", "-hide_banner", "-loglevel", "error",
				"-progress", str(self.work / "ffprogress.txt"),
				"-f", "x11grab", "-framerate", str(self.cap_fps),
				"-video_size", f"{self.size[0]}x{self.size[1]}", "-i", self.display,
				"-c:v", "libx264", "-preset", "ultrafast", "-qp", "0",
				"-pix_fmt", "yuv444p", str(self.raw)],
				env=self.env(), stdin=subprocess.DEVNULL, stderr=ff_log)
		# flash only once frames are actually flowing - a slow-opening ffmpeg
		# would otherwise miss the sync flash and break the whole AV anchor
		prog = self.work / "ffprogress.txt"
		deadline = time.time() + 30
		while time.time() < deadline:
			if prog.exists() and re.search(r"(?m)^frame=([1-9]\d*)", prog.read_text()):
				break
			time.sleep(0.3)
		else:
			raise RuntimeError("x11grab produced no frames (see ffmpeg.log)")
		time.sleep(0.8)
		subprocess.run(["xsetroot", "-solid", "white"], env=self.env(), check=False)
		self.flash_e = time.time()
		time.sleep(0.25)
		subprocess.run(["xsetroot", "-solid", "#000000"], env=self.env(), check=False)  # black border behind the window
		self.mouse_park()       # X parks the pointer mid-screen; get it out of frame
		time.sleep(0.4)

	def stop_capture(self) -> None:
		if self.ff:
			self.ff.send_signal(signal.SIGINT)
			try:
				self.ff.wait(timeout=30)
			except subprocess.TimeoutExpired:
				self.ff.kill()
			self.ff = None

	def app_env(self) -> dict[str, str]:
		e = self.env()
		e.pop("LIBGL_ALWAYS_SOFTWARE", None)      # the app runs on the GPU (see gpu_prefix)
		# the pop-out dialogs (Settings/About) are static wgpu/Vulkan windows; pin
		# them to lavapipe so they don't chase a GPU Vulkan surface Xvfb can't present
		# gray prompt, rose user, sand host. The trailing bit grays whatever is TYPED
		# after the prompt WHEN a flag file exists - that's how the outro comment goes
		# gray ("as if ble.sh") without ble.sh, which drops the odd first keystroke.
		gray_flag = ("\\[$(test -f \"$HOME/.silk-gray\" && "
			"printf '\\033[38;5;245m')\\]")
		# pin the app's animation rate to the rate we grab at, so every capture
		# tick falls on exactly one painted frame (see PROFILES). Off outside the
		# recorder, where vblank does the pacing.
		e.update(SILK_MAX_FPS=str(self.cap_fps),
			SHELL="/bin/bash", HOME=str(self.home),
			XDG_CONFIG_HOME=str(self.home / ".config"),
			PATH=f"{self.home}/bin:{os.environ['PATH']}",
			VK_ICD_FILENAMES="/usr/share/vulkan/icd.d/lvp_icd.json",
			PS1="\\[\\e[38;2;224;144;158m\\]juno\\[\\e[38;2;150;156;162m\\]@"
				"\\[\\e[38;2;222;178;134m\\]vela\\[\\e[38;2;150;156;162m\\]:\\w\\$ "
				"\\[\\e[0m\\]" + gray_flag,
			HISTFILE="/dev/null")
		return e

	def launch_app(self, shell_cmd: str) -> None:
		e = self.app_env()
		cmd = [self.bin, "--config", str(self.home / ".config/silkterm/config.shcl"),
			"--shell", shell_cmd]
		cmd = gpu_prefix(e) + cmd
		# a decorated (non-fullscreen) window: xfwm4 draws the full frame + the
		# titlebar with buttons, which is the "fake decoration" the shot wants.
		# The window fills the view below the narration band - a BORDER-px black
		# frame around it, BAND px of black above. The client is sized so the outer
		# decoration fits what is left. That size goes in at LAUNCH (--pixel-width/
		# height) and the window is never resized after - the VGL EGL present
		# latches the surface size at creation, so a post-launch resize breaks the
		# blit (moving is fine, which is how place_window nudges it into place).
		width, height = self.size
		cw = width - 2 * BORDER - FRAME_L - FRAME_R
		ch = height - 2 * BORDER - self.band - FRAME_T - FRAME_B
		cmd += ["--pixel-width", str(cw), "--pixel-height", str(ch)]
		self.launch_e = time.time()
		with (self.work / "silk.log").open("w") as silk_log:
			self.app = subprocess.Popen(cmd, env=e, cwd=str(self.home),
				stdout=silk_log, stderr=subprocess.STDOUT)
		deadline = time.time() + 60
		win = ""
		while time.time() < deadline and not win:
			r = subprocess.run(["xdotool", "search", "--class", "silkterm"],
				env=self.env(), capture_output=True, text=True, check=False)
			win = r.stdout.split()[0] if r.stdout.strip() else ""
			time.sleep(0.5)
		if not win:
			raise RuntimeError("silkterm window never appeared (see silk.log)")
		self.win = win
		self.place_window(win)
		time.sleep(4.0)                           # GPU GL bring-up + first frames
		self.xdo("windowactivate", win)
		time.sleep(0.3)
		self.mouse_park()

	def kill_app(self) -> None:
		if self.app:
			self.app.terminate()
			try:
				self.app.wait(timeout=5)
			except subprocess.TimeoutExpired:
				self.app.kill()
			self.app = None

	# --- event log -------------------------------------------------------------
	def ev(self, kind: str) -> None:
		self.events.append((time.time(), kind))

	def mouse_park(self) -> None:
		# the very bottom-right pixel: the arrow's hotspot is its tip, so the whole
		# glyph draws past the screen edge and no pointer is left in frame
		self.xdo("mousemove", str(self.size[0] - 1), str(self.size[1] - 1))

	def cleanup(self) -> None:
		self.stop_capture()
		self.kill_app()
		self.stop_display()
		if not self.keep and self.work.exists():
			shutil.rmtree(self.work, ignore_errors=True)


##•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
##	Typing engine

# qwerty neighbors for plausible typos
NEIGH = {
	"a": "sq", "b": "vn", "c": "xv", "d": "sf", "e": "wr", "f": "dg", "g": "fh",
	"h": "gj", "i": "uo", "j": "hk", "k": "jl", "l": "k", "m": "n", "n": "bm",
	"o": "ip", "p": "o", "q": "wa", "r": "et", "s": "ad", "t": "ry", "u": "yi",
	"v": "cb", "w": "qe", "x": "zc", "y": "tu", "z": "x",
}
# char -> XT scancode: the key bank has one unique slice per physical key, so
# every key thocks with its own sample; a shifted symbol thocks with its base
# key, same as a real board
_SHIFTED = dict(zip('!@#$%^&*()_+{}:"<>?~|', "1234567890-=[];',./`\\"))
_SCAN = {c: 2 + i for i, c in enumerate("1234567890-=")}
_SCAN |= {c: 16 + i for i, c in enumerate("qwertyuiop[]")}
_SCAN |= {c: 30 + i for i, c in enumerate("asdfghjkl;'")}
_SCAN |= {c: 44 + i for i, c in enumerate("zxcvbnm,./")}
_SCAN |= {"`": 41, "\\": 43, " ": 57}
KEY_CODES = {"SPACE": 57, "ENTER": 28, "BACKSPACE": 14, "TAB": 15,
	"ESC": 1, "ESCAPE": 1, "UP": 57416, "DOWN": 57424, "LEFT": 57419,
	"RIGHT": 57421, "PGUP": 3657, "PGDN": 3665}

def key_sound(ch: str) -> str:
	c = _SHIFTED.get(ch, ch.lower())
	return f"key:{_SCAN.get(c, 30)}"          # unknown falls back to 'a'

def keysym_sound(keysym: str) -> str:
	if len(keysym) == 1:
		return key_sound(keysym)
	return f"key:{KEY_CODES.get(keysym.upper(), 30)}"

class Typist:
	def __init__(self, rec: Rec, rng: random.Random) -> None:
		self.rec = rec
		self.rng = rng
		self.wpm = rng.uniform(120, 160)

	def _delay(self) -> float:
		# per-char delay from current wpm, lognormal jitter; wpm drifts as it would
		self.wpm += self.rng.uniform(-8, 8)
		self.wpm = max(100.0, min(220.0, self.wpm))
		d = 12.0 / self.wpm                      # 60 / (5 * wpm)
		return d * self.rng.lognormvariate(0.0, 0.22)

	def _emit(self, ch: str) -> None:
		# timestamp AFTER the send so the xdotool spawn latency never skews the
		# foley; the event epoch is the moment X actually got the key
		if ch == " ":
			self.rec.xdo("key", "--clearmodifiers", "space")
			self.rec.ev("key:SPACE")
		else:
			subprocess.run(["xdotool", "type", "--delay", "0", "--", ch],
				env=self.rec.env(), check=False,
				stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
			self.rec.ev(key_sound(ch))

	def _backspace(self, n: int) -> None:
		for _ in range(n):
			time.sleep(self.rng.uniform(0.09, 0.16))
			self.rec.xdo("key", "--clearmodifiers", "BackSpace")
			self.rec.ev("key:BACKSPACE")

	def type(self, text: str, typos: float = 0.006, wpm: float | None = None) -> None:
		if wpm is not None:
			self.wpm = wpm
		# ensure the terminal has focus before the first keystroke: after a dialog
		# closes the first char can race the focus handoff and drop (which turned
		# "silkterm" into "ilkterm" and broke the wallpaper command)
		self.rec.xdo("windowactivate", self.rec.win)
		time.sleep(0.3)
		i = 0
		while i < len(text):
			ch = text[i]
			time.sleep(self._delay() * (1.6 if ch == " " else 1.0))
			# an expert's slip: wrong neighbor, maybe one more char, catch it, fix it
			if ch.lower() in NEIGH and self.rng.random() < typos:
				wrong = self.rng.choice(NEIGH[ch.lower()])
				self._emit(wrong)
				extra = 0
				if self.rng.random() < 0.4 and i + 1 < len(text) and text[i + 1] != " ":
					time.sleep(self._delay())
					self._emit(text[i + 1])
					extra = 1
				time.sleep(self.rng.uniform(0.22, 0.45))   # the "oops" beat
				self._backspace(1 + extra)
				time.sleep(self.rng.uniform(0.08, 0.2))
				self._emit(ch)
				if extra:
					time.sleep(self._delay())
					self._emit(text[i + 1])
				i += 1 + extra
				continue
			self._emit(ch)
			i += 1

	def enter(self) -> None:
		time.sleep(self.rng.uniform(0.15, 0.4))
		self.rec.xdo("key", "--clearmodifiers", "Return")
		self.rec.ev("key:ENTER")

	def key(self, keysym: str, sound: str | None = None) -> None:
		self.rec.xdo("key", "--clearmodifiers", keysym)
		if sound is None:
			sound = keysym_sound(keysym)
		if sound:
			self.rec.ev(sound)

	def keys(self, keysym: str, n: int, hz: float = 8.0, sound: str | None = None) -> None:
		# repeated taps (arrow scrolling); slight cadence wobble
		for _ in range(n):
			self.key(keysym, sound)
			time.sleep(max(0.03, self.rng.uniform(0.8, 1.2) / hz))

	def hold(self, keysym: str, count: int, hz: float = 55.0, first_sound: str | None = None) -> None:
		# a held key, faked as fast discrete repeats (Xvfb has no autorepeat, so a
		# real keydown/keyup delivers just one press): one click on the first
		# press, silence for the rest - reads as press-and-hold
		if first_sound is None:
			first_sound = keysym_sound(keysym)
		if first_sound:
			self.rec.ev(first_sound)
		self.rec.xdo("key", "--clearmodifiers", "--repeat", str(count),
			"--delay", str(int(1000 / hz)), keysym)

	def cmd(self, text: str, settle: float = 1.0, typos: float = 0.006, wpm: float | None = None) -> None:
		self.type(text, typos, wpm)
		self.enter()
		time.sleep(settle)


##•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
##	Mouse

class Mouse:
	def __init__(self, rec: Rec, rng: random.Random) -> None:
		self.rec = rec
		self.rng = rng
		self.pos = (rec.size[0] - 1, rec.size[1] - 1)

	def move(self, x: int, y: int, dur: float = 0.6) -> None:
		x0, y0 = self.pos
		steps = max(6, int(dur * 40))
		for i in range(1, steps + 1):
			t = i / steps
			t = t * t * (3 - 2 * t)              # smoothstep
			self.rec.xdo("mousemove", str(int(x0 + (x - x0) * t)), str(int(y0 + (y - y0) * t)))
			time.sleep(dur / steps)
		self.pos = (x, y)

	def click(self, quiet: bool = False) -> None:
		self.rec.xdo("click", "1")
		self.rec.ev("mouse:CLICK_Q" if quiet else "mouse:CLICK")

	def double(self) -> None:
		self.rec.ev("mouse:CLICK")
		time.sleep(0.11)
		self.rec.ev("mouse:CLICK")
		self.rec.xdo("click", "--repeat", "2", "--delay", "110", "1")

	def drag(self, x1: int, y1: int, x2: int, y2: int, dur: float = 0.9) -> None:
		self.move(x1, y1, 0.5)
		self.rec.ev("mouse:CLICK")
		self.rec.xdo("mousedown", "1")
		time.sleep(0.15)
		self.move(x2, y2, dur)
		time.sleep(0.1)
		self.rec.ev("mouse:CLICK_Q")
		self.rec.xdo("mouseup", "1")

	def park(self) -> None:
		self.rec.mouse_park()
		self.pos = (self.rec.size[0] - 1, self.rec.size[1] - 1)

	def wheel(self, up: bool, n: int, hz: float = 7.0) -> None:
		for _ in range(n):
			self.rec.ev("mouse:WHEEL")
			self.rec.xdo("click", "4" if up else "5")
			time.sleep(self.rng.uniform(0.8, 1.2) / hz)


##•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
##	Banner bookkeeping

class Banner:
	# every caption sits in the band above the window, so there is no position to
	# choose any more - only the text and the span it covers
	def __init__(self, rec: Rec, text: str) -> None:
		self.rec, self.text = rec, text

	def __enter__(self) -> "Banner":
		self.start = time.time()
		return self

	def __exit__(self, *exc: object) -> None:
		self.rec.banners.append((self.start, time.time(), self.text))

# housekeeping between scenes that the finished take leaves out: it is recorded
# like anything else, then dropped at encode, sound included
class Cut:
	def __init__(self, rec: Rec) -> None:
		self.rec = rec

	def __enter__(self) -> "Cut":
		self.start = time.time()
		return self

	def __exit__(self, *exc: object) -> None:
		self.rec.cuts.append((self.start, time.time()))


##•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
##	Scene content: recording fonts, config, the synthetic desktop, home tree

def write_dconf(home: Path, profile: Profile) -> None:
	# the app reads both recording fonts through gsettings; with XDG_CONFIG_HOME
	# on the fake home a compiled dconf db is all it takes. Chrome/dialogs get a
	# clean modern sans; the terminal gets the defined mono at the profile's size.
	src = home.parent / "dconf-src"
	src.mkdir(exist_ok=True)
	(src / "ifc.txt").write_text(
		"[org/gnome/desktop/interface]\n"
		f"font-name='Lato {profile['ui_pt']}'\n"
		f"monospace-font-name='Monaspace Argon Semi-Bold {profile['mono_pt']}'\n")
	dst = home / ".config" / "dconf"
	dst.mkdir(parents=True, exist_ok=True)
	run(["dconf", "compile", str(dst / "user"), str(src)])

def write_config(home: Path, profile: Profile) -> None:
	# mirrors the real defined config. Both profiles start opaque on plain black,
	# which needs wallpaper.fallback_builtin OFF - it defaults on, and while it is on there
	# is no "no wallpaper" state to start from (an unset image IS what shows the
	# built-in one). Image opacity stays at the 0.10 default.
	#
	# rotate.enabled stays OFF too: rotation adopts a wallpaper folder on its
	# own, and a folder suppresses the built-in image, so the reveal could show
	# something else or nothing at all.
	#
	# What is pinned here and what is deliberately absent:
	#  - grid, font and margin stay pinned even where they equal a default, so a
	#    later default change cannot reflow a scene that was timed against them.
	#  - the scrim and the scroll feel are NOT pinned, so the demo always shows
	#    what ships. Pinning them is how the old gif ended up advertising a halo
	#    and an outline no build had used for weeks.
	#  - cursor.size.width IS pinned, and to the shipped block on purpose: it is
	#    the before half of seg_cursor, which narrows it to a bar on camera.
	cfgdir = home / ".config" / "silkterm"
	cfgdir.mkdir(parents=True, exist_ok=True)
	(cfgdir / "config.shcl").write_text('''font.use_system_family: true
performance.automatic: false
performance.profile: custom
font.line_height_scale: 1.22
window.margin: 8.0
window.remember_size: false
window.columns: 160
window.rows: 48
transparency.enabled: false
wallpaper.fallback_builtin: false
wallpaper.rotate.enabled: false
wallpaper.opacity: 0.10
wallpaper.default_fit: zoom
wallpaper.blur: 10.0
text.scrim.enabled: true
cursor.size.height: 100
cursor.size.width: 100
cursor.animation: pulse_vertical
cursor.animation_resume_s: 1
cursor.blink_rate_ms: 500
selection.word_separators: "=,|:\\"' ()[]{}<>"
scroll.scrollback: 10000
scroll.wheel_lines: 3.0
scroll.alt_scroll_lines: 3.0
scroll.output_ease_lines: 1.0
scroll.smooth_apps: true
theme: SilkTerm
theme_mode: dark
''')

RUST_SCROLL = '''// smooth output easing: nudge the visual offset toward rest, never snap
use crate::grid::Grid;

pub struct Scroll {
	visual:  f64,
	target:  f64,
	backlog: u32,
	tau_ms:  f32,
}

impl Scroll {
	pub fn new(tau_ms: f32) -> Self {
		Self { visual: 0.0, target: 0.0, backlog: 0, tau_ms }
	}

	pub fn nudge_output(&mut self, grew: u32) {
		self.backlog = (self.backlog + grew).min(MAX_BACKLOG);
		self.target = 0.0;
	}

	pub fn step(&mut self, dt_ms: f32) -> bool {
		let tau = self.effective_tau(dt_ms);
		let k = 1.0 - (-dt_ms / tau).exp();
		self.visual += (self.target - self.visual) * k as f64;
		(self.visual - self.target).abs() > SETTLE_EPS
	}

	fn effective_tau(&self, dt_ms: f32) -> f32 {
		// a burst ramps the ease speed so the view keeps up, then relaxes
		let load = self.backlog as f32 / MAX_BACKLOG as f32;
		self.tau_ms * (1.0 - 0.8 * load.min(1.0))
	}
}
'''

# a believable generic home: enough entries that `ls -lA` runs past the bottom
HOME_DIRS = ["Desktop", "Documents", "Downloads", "Music", "Pictures", "Videos",
	"bin", "projects",
	".cache", ".config", ".gnupg", ".local", ".mozilla", ".ssh", ".vim"]
HOME_DOTFILES = [(".bash_aliases", 361), (".bash_logout", 220), (".bashrc", 3526),
	(".curlrc", 74), (".dircolors", 4291), (".gitconfig", 412),
	(".inputrc", 289), (".profile", 807),
	(".tmux.conf", 1184), (".vimrc", 1204), (".wgetrc", 118), (".Xresources", 688)]
HOME_FILES = [("backup-2025.tar.gz", 1483477621), ("notes.md", 8412),
	("photo-kyoto.jpg", 3318554), ("pulsar-flame.svg", 96214),
	("resume.pdf", 188416), ("shopping.txt", 973), ("soundtrack.flac", 38119433),
	("todo.md", 2101)]

def write_tree(rec: Rec, rng: random.Random) -> None:
	home = rec.home
	proj = home / "projects" / "pulsar"
	src = proj / "src"
	src.mkdir(parents=True)
	(proj / "docs").mkdir()
	(proj / "assets").mkdir()
	(proj / "Cargo.toml").write_text(
		'[package]\nname = "pulsar"\nversion = "0.4.1"\nedition = "2024"\n')
	(proj / "README.md").write_text("# pulsar\n\nA tiny GPU particle toy.\n")
	(proj / "LICENSE").write_text("MIT\n")
	(src / "scroll.rs").write_text(RUST_SCROLL * 5)
	(src / "main.rs").write_text('fn main() {\n\tpulsar::run();\n}\n')
	(src / "render.rs").write_text(RUST_SCROLL)

	for name in HOME_DIRS:
		(home / name).mkdir(parents=True, exist_ok=True)
	for name, size in HOME_DOTFILES + HOME_FILES:
		f = home / name
		f.touch()
		os.truncate(f, size)

	# `ls` wrapper: the listing flags live here rather than in an alias typed on
	# camera - everyone has their ls aliased, so showing it being set says nothing.
	# Owner and group are omitted (-gG) and the timestamp is bare: every column a
	# row carries is width the gif pays for on each scrolled line, and none of them
	# are what the scene is about. A real listing would also print the real
	# username, so map it to the fake one.
	bind = home / "bin"
	bind.mkdir(exist_ok=True)
	user = getpass.getuser()
	wrapper = bind / "ls"
	# The colours come from the wrapper too. Without LS_COLORS in the environment
	# ls colours directories and nothing else, so a listing's reds and magentas
	# depended on whoever's shell started the recorder - the same run rendered
	# differently on two boxes. Ask dircolors for the stock database instead.
	wrapper.write_text('#!/bin/dash\neval "$(dircolors -b)"\nexport LS_COLORS\n'
		"/usr/bin/ls -lAgG --color"
		" --group-directories-first --time-style=+%H:%M"
		f' "$@" | sed "s/{user}/juno/g"\n')
	wrapper.chmod(0o755)
	(bind / "silkterm").symlink_to(rec.bin)
	# pin nano to no-softwrap so a config line stays on one screen row
	(home / ".nanorc").write_text("unset softwrap\nunset breaklonglines\n")
	# ctrl+l clears the SCROLLBACK too, not just the screen (readline's
	# clear-display sends ESC[3J on top of the usual clear). Plain clear-screen
	# leaves the old output in history, and splitting a pane rewraps it straight
	# back into view - the panes scene opened onto the build log it had just
	# cleared. readline reads this file regardless of bash's --norc.
	(home / ".inputrc").write_text('"\\C-l": clear-display\n')

	# the closing scene over the wallpaper: true color, attributes, scripts,
	# double-width kanji and katakana, emoji and box drawing in a handful of short
	# lines (a dense screenful would cost the gif far more than it says).
	# The bar sweeps the whole hue circle the short way round - dark purple, up
	# through the bright middle, down to dark red - so it reads as a rainbow rather
	# than the single-axis ramp it used to be. Computed here, not in dash: integer
	# shell arithmetic cannot do a hue sweep, and the stops never vary anyway.
	stops: list[str] = []
	for i in range(36):
		frac = i / 35
		red, green, blue = colorsys.hsv_to_rgb(
			285.0 * (1.0 - frac) / 360.0,          # purple -> blue -> green -> red
			1.0,
			0.35 + 0.65 * math.sin(math.pi * frac))   # dark at both ends, bright between
		stops.append(f"'{round(red * 255)};{round(green * 255)};{round(blue * 255)}'")
	show = bind / "showcase"
	show.write_text(f'''#!/bin/dash
for c in {" ".join(stops)}; do printf '\\033[48;2;%sm  ' "$c"; done
printf '\\033[0m\\n\\n'
printf '  \\033[1m24-bit color\\033[0m    \\033[3mitalic\\033[0m    '
printf '\\033[1;38;2;255;216;102mbold color\\033[0m    \\033[7m reverse \\033[0m\\n\\n'
printf '  日本語 忍者 桜 猫   タ ッ ネ ホ   Ελληνικά  Кириллица  العربية\\n'
printf '  🤔 🍰 🎉 😀   ┌─┬─┐ ╔═╦═╗ ▁▂▃▄▅▆▇█\\n\\n'
''')
	show.chmod(0o755)

	# build.sh: cargo-flavoured output, paced in movements rather than at random.
	# The scroll speed is not a constant - it leaves rest gently, doubles while the
	# backlog grows, tops out, then rides a braking curve down to a slow stop.
	# Output that arrives at one rate only ever shows one point on that curve, so
	# the earlier version (a line every so often, with a quarter chance of a pause)
	# never left the gentle end and the whole middle of the curve went unseen. The
	# pacing below walks the curve end to end, and the long silence at movement 4
	# is what makes the wind-down visible at all: the view is still travelling when
	# the output stops, and has to brake and stop on its own.
	crates = [
		"proc-macro2", "quote", "syn", "unicode-ident", "libc", "bitflags",
		"smallvec", "log", "cfg-if", "once_cell", "memchr", "either",
		"itertools", "regex-syntax", "aho-corasick", "hashbrown", "indexmap",
		"equivalent", "serde", "serde_derive", "ryu", "itoa", "thiserror",
		"anyhow", "bytemuck", "raw-window-handle", "wayland-client",
		"x11-dl", "calloop", "wgpu-types", "naga", "spirv", "gpu-alloc",
		"gpu-descriptor", "renderdoc-sys", "wgpu-hal", "wgpu", "winit",
		"glam", "cosmic-text", "swash", "skrifa", "zeno", "ttf-parser",
		"rustybuzz", "glyphon", "pulsar-render",
	]
	def ver() -> str:
		return f"{rng.randint(0, 3)}.{rng.randint(1, 30)}.{rng.randint(0, 9)}"
	lines = ["#!/bin/dash", 'g="\\033[1;32m"; y="\\033[1;33m"; b="\\033[1;34m"; r="\\033[0m"']
	def comp(c: str) -> None:
		lines.append(f'printf "   ${{g}}Compiling${{r}} {c} v{ver()}\\n"')
	lines.append('printf "   ${g}Compiling${r} pulsar workspace\\n"')
	feed = iter(crates)

	# 1 - well apart: each line eases and stops before the next arrives, so this is
	#     the gentle end of the curve on its own, one line at a time
	for _ in range(5):
		comp(next(feed))
		lines.append("sleep 0.45")
	# 2 - the gaps close: a line now arrives before the last has settled, so the
	#     speed ramps instead of restarting from rest each time
	for gap in ("0.34", "0.27", "0.21", "0.16", "0.13", "0.10", "0.08", "0.06"):
		comp(next(feed))
		lines.append(f"sleep {gap}")
	# 3 - no gaps at all: a sustained burst, which is the only thing that lifts the
	#     speed past its single-screen cap and makes the view trail the live bottom
	for c in feed:
		comp(c)
	# 4 - silence, and this is the point of the whole scene: nothing more arrives,
	#     so the view has to brake down the ramp and stop by itself, in view
	lines.append("sleep 1.3")
	# 5 - coda: a couple of slow lines, which start gently again from rest
	lines += [
		'printf "${y}warning${r}: unused variable: ${b}lift${r}\\n"',
		'printf "  ${b}-->${r} src/render.rs:141:9\\n"',
		'sleep 0.5',
		'printf "   ${g}Compiling${r} pulsar v0.4.1\\n"',
		'sleep 0.9',
		'printf "    ${g}Finished${r} release [optimized] in 12.31s\\n"',
	]
	sh = proj / "build.sh"
	sh.write_text("\n".join(lines) + "\n")
	sh.chmod(0o755)

def prep_content(rec: Rec, rng: random.Random) -> None:
	write_dconf(rec.home, rec.p)
	write_config(rec.home, rec.p)
	write_tree(rec, rng)


##•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
##	Talking to the running app (its control socket)

def ctl_socket(rec: Rec) -> Path | None:
	# the socket a running instance listens on - the same channel `silkterm
	# --reload-settings` uses from a shell inside the window. Named by pid; if the
	# launcher put a wrapper in between, take the one that appeared with this run
	# (an unrelated instance's socket is older).
	if not rec.app:
		return None
	run_dir = Path(os.environ.get("XDG_RUNTIME_DIR", "/tmp"))
	exact = run_dir / f"silkterm-ctl-{rec.app.pid}.sock"
	if exact.exists():
		return exact
	fresh = [p for p in run_dir.glob("silkterm-ctl-*.sock")
		if p.stat().st_mtime >= rec.launch_e - 2]
	return fresh[0] if len(fresh) == 1 else None

def ctl(rec: Rec, line: str) -> bool:
	sock = ctl_socket(rec)
	if not sock:
		log("WARNING: control socket not found - live settings change skipped")
		return False
	try:
		with socket.socket(socket.AF_UNIX) as s:
			s.connect(str(sock))
			s.sendall(line.encode() + b"\n")
			return s.recv(64).startswith(b"ok")
	except OSError as e:
		log(f"WARNING: control socket: {e}")
		return False

def set_cfg(rec: Rec, keys: dict[str, object]) -> bool:
	# a settings change, applied the way a settings change applies: rewrite the
	# keys and reload. Live, and nothing has to be typed on camera. Strings are
	# quoted so a value can never read as a comment or a bare word like none.
	#
	# Keys are dotted config paths, and each line's path is RESOLVED from an
	# indent stack of block headers rather than matched literally. That is not
	# defensive programming - the app rewrites this file into nested blocks the
	# first time it saves, so a literal `^cursor\.size\.width: ` matched the file
	# write_config wrote and then matched nothing at all for the rest of the run.
	# It failed silently, which cost a whole render to notice: the cursor never
	# changed shape and the panes scene never stilled its cursors.
	#
	# Hence the miss check below - a key that resolves to no line is a scene that
	# will quietly not happen, so it stops the run instead.
	cfg = rec.home / ".config/silkterm/config.shcl"
	lines = cfg.read_text().split("\n")
	stack: list[tuple[int, str]] = []
	seen: set[str] = set()
	for i, raw in enumerate(lines):
		body = raw.strip()
		if not body or body.startswith("#"):
			continue
		indent = len(raw) - len(raw.lstrip())
		while stack and stack[-1][0] >= indent:
			stack.pop()
		name, _, rest = body.partition(":")
		name = name.strip()
		if not rest.strip():                       # a block header, not a setting
			stack.append((indent, name))
			continue
		path = ".".join([n for _, n in stack] + [name])
		if path in keys:
			val = keys[path]
			if isinstance(val, bool):
				val = "true" if val else "false"
			elif isinstance(val, str):
				val = f'"{val}"'
			lines[i] = f"{raw[:indent]}{name}: {val}"
			seen.add(path)
	missing = sorted(set(keys) - seen)
	if missing:
		raise RuntimeError(f"set_cfg: no line for {missing} in {cfg}")
	cfg.write_text("\n".join(lines))
	return ctl(rec, "reload")


##•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
##	Segments (each takes the recorder, typist, mouse)

def wipe(r: Rec, t: Typist, settle: float = 0.8) -> None:
	# clear the screen between scenes: typing over an empty screen changes far
	# fewer pixels than typing over a full one, and that is most of what keeps the
	# gif down. Skipped where the next command is meant to push the old output up.
	r.xdo("windowactivate", r.win)
	time.sleep(0.25)
	t.key("ctrl+l", sound=key_sound("l"))
	time.sleep(settle)

def seg_ls(r: Rec, t: Typist, m: Mouse) -> None:
	# opens straight on the listing: the ls flags are baked into the wrapper rather
	# than aliased on camera, because watching someone set an alias sells nothing
	with Banner(r, "Silky-smooth output scrolling"):
		t.cmd("ls ~/", settle=3.0)
		time.sleep(0.7)
	# no wipe: the build output is meant to push this listing up

def seg_build(r: Rec, t: Typist, m: Mouse) -> None:
	with Banner(r, "Watch it adapt to any output speed"):
		t.cmd("cd projects/pulsar", settle=0.6, typos=0.0)
		# the script runs ~6.5s now (five paced movements, see write_tree) and the
		# settle has to outlast it, or the scene cuts away mid wind-down - which is
		# the half worth watching
		t.cmd("./build.sh", settle=7.0)
		time.sleep(0.7)
	# no wipe: the wheel scene scrolls back up through all of this

def seg_wheel(r: Rec, t: Typist, m: Mouse) -> None:
	# scrollback under the wheel - the same easing as the output scroll, driven
	# by hand. xdotool sends two wheel events per click here (winit fires on the
	# legacy button press AND release), so a few clicks cover a lot of lines. One
	# direction only: coming back down says nothing going up has not already said,
	# and a full-width listing in motion is the costliest thing in the gif. The
	# screen is then just cleared, with no remark - the scene is over at the top.
	with Banner(r, "Scroll back just as smoothly. Minimap at the right."):
		m.move(r.size[0] // 2, r.band + (r.size[1] - r.band) // 2, dur=0.5)
		m.wheel(True, 3, hz=3.2)
		time.sleep(0.9)
		m.park()
	wipe(r, t)

def seg_panes(r: Rec, t: Typist, m: Mouse) -> None:
	# still the cursor first, silently: three panes each pulsing their own cursor
	# pull the eye off the split, and every pulse is motion the gif pays for.
	# Two splits straight off the menu bar (Alt+P opens Panes, then the item's own
	# accelerator letter - V for vertical), all keyboard, no menu coordinates to
	# guess at. Splitting twice is what shows the auto-sizing. Closing them again
	# shows nothing new and took seconds, so that happens in a cut.
	set_cfg(r, {"cursor.animation": "none"})
	with Banner(r, "Split panes, sized for you"):
		r.xdo("windowactivate", r.win)
		time.sleep(0.3)
		# vertical then HORIZONTAL, not vertical twice. Two vertical splits leave
		# three columns, and at a third of the width the prompt very nearly fills
		# its pane - readline then redisplays it on a fresh line, which is a line
		# of new output, which the panes ease in like any other. The result was two
		# panes each sliding up a row a beat after they appeared, staggered, on an
		# otherwise empty screen: nothing else to look at, so it read as glitching.
		# At half width the prompt has room and no pane reprints. Splitting both
		# ways also shows the tree does both, which one direction twice does not.
		for accel in ("v", "h"):
			t.key("alt+p", sound=key_sound("p"))
			time.sleep(0.55)
			t.key(accel, sound=key_sound(accel))
			time.sleep(1.1)
		time.sleep(0.6)
	# each pane is a shell like any other, so `exit` is what leaves it. The pulse
	# comes back in here too: the reload takes ~1.2s to reach the screen, which
	# was most of the cursor scene's opening dwell when it happened on camera.
	with Cut(r):
		t.cmd("exit", settle=0.8, typos=0.0)
		t.cmd("exit", settle=0.8, typos=0.0)
		set_cfg(r, {"cursor.animation": "pulse_vertical"})
		wipe(r, t, settle=1.8)

def seg_cursor(r: Rec, t: Typist, m: Mouse) -> None:
	# the cursor is a setting, so switch it the way a setting switches - live,
	# through the control socket, with nothing typed on camera. An empty screen:
	# the cursor is the only thing moving on it.
	#
	# The scene opens on the pulsing block, already back on in the panes scene's
	# cut. Then shape and animation change together, block and pulse to a thin bar
	# that fades. The new animation waits out cursor.animation_resume_s before it
	# starts, so the second hold is the longer one.
	with Banner(r, "Cursor shape and animation, your pick"):
		r.xdo("windowactivate", r.win)
		time.sleep(1.6)
		set_cfg(r, {"cursor.size.width": 25, "cursor.animation": "phase"})
		time.sleep(3.2)

def seg_wallpaper(r: Rec, t: Typist, m: Mouse) -> None:
	# the image compiled into the binary, switched on like any other setting, so
	# this is exactly the out-of-the-box look with nothing typed. The reload
	# takes about a second to reach the screen, hence the longer hold.
	with Banner(r, "Advanced wallpaper support, with a default in the executable"):
		r.xdo("windowactivate", r.win)
		time.sleep(0.4)
		set_cfg(r, {"wallpaper.fallback_builtin": True})
		time.sleep(4.4)
	with Banner(r, "Text stays legible over any of it"):
		time.sleep(2.8)
	# no wipe from here on: the closing scenes build up the frame that the demo
	# ends on - wallpaper, color, then the sign-off

def seg_showcase(r: Rec, t: Typist, m: Mouse) -> None:
	# drop the flag the prompt watches for BEFORE this command runs, so the prompt
	# it returns to is already the gray one and the outro can type straight into
	# it - no bare Return just to draw a fresh prompt.
	(r.home / ".silk-gray").touch()
	with Banner(r, "24-bit color. Unicode. Over anything."):
		t.cmd("showcase", settle=2.6)
		time.sleep(0.8)

def seg_outro(r: Rec, t: Typist, m: Mouse) -> None:
	# the prompt grays whatever is typed after it while the flag file exists, so
	# the comment goes gray from the '#' on, as if ble.sh were installed - but with
	# plain reliable bash typing.
	with Banner(r, "github.com/yottacore/silkterm"):
		r.xdo("windowactivate", r.win)
		time.sleep(0.5)
		# a bare prompt above the sign-off and another below it (the one the Return
		# at the end of the comment leaves), so it sits on its own
		t.enter()
		time.sleep(0.6)
		t.cmd("# Smooth. Silky. ...SilkTerm.", settle=0.5, typos=0.0)
		time.sleep(3.0)
	# the rest of the linger is the encoder's freeze (TAIL_HOLD_S), which costs a
	# gif nothing - it stores a held frame as a no-change

# one script, both profiles (video and gif differ only in size/fonts/audio)
_SCRIPT: list[tuple[str, Callable[[Rec, Typist, Mouse], None]]] = [
	("ls",        seg_ls),
	("build",     seg_build),
	("wheel",     seg_wheel),
	("panes",     seg_panes),
	("cursor",    seg_cursor),
	("wallpaper", seg_wallpaper),
	("showcase",  seg_showcase),
	("outro",     seg_outro),
]
SEGMENTS = {"video": _SCRIPT, "gif": _SCRIPT}


##•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
##	Audio: process the key bank, mix the event log into a wav

SOUND_FILES = {
	"mouse:CLICK":    SOUNDS / "mouse/click.wav",
	"mouse:CLICK_Q":  SOUNDS / "mouse/click_quiet.wav",
}
KEYPACK = SOUNDS / "keys-oreo"     # mechvibes "EG Oreo": one recording, one slice per key
GAIN = {"key": 0.85, "mouse:CLICK": 0.5, "mouse:CLICK_Q": 0.36, "mouse:WHEEL": 0.5}

# the bank is quiet and slice loudness wanders ~6 dB; even each slice out to a
# consistent body presence (space/enter a touch prouder) but keep every key's
# own transient and timbre - that natural variety is the whole point of a
# per-key bank
KEY_BODY = {57: 0.085, 28: 0.085, 14: 0.07}

def shape_slice(s: np.ndarray, code: int) -> np.ndarray:
	rms = np.sqrt((s ** 2).mean()) + 1e-9
	s = s * (KEY_BODY.get(code, 0.062) / rms)
	n_in, n_out = int(SR * 0.001), int(SR * 0.006)
	s[:n_in] *= np.linspace(0.0, 1.0, n_in)[:, None]      # slice edges must not click
	s[-n_out:] *= np.linspace(1.0, 0.0, n_out)[:, None]
	peak = np.abs(s).max()
	if peak > 0.7:                                # keep one loud hit from owning the mix
		s *= 0.7 / peak
	return s.astype(np.float32)

def load_keypack(work: Path, cache: dict[str, np.ndarray]) -> None:
	cfg = json.loads((KEYPACK / "config.json").read_text())
	raw = work / "keypack.pcm"
	run(["ffmpeg", "-v", "error", "-y", "-i", str(KEYPACK / cfg["sound"]),
		"-ar", str(SR), "-ac", "2", "-f", "s16le", str(raw)])
	pcm = np.frombuffer(raw.read_bytes(), dtype=np.int16) \
		.astype(np.float32).reshape(-1, 2) / 32768.0
	for code, span in cfg["defines"].items():
		if not span:
			continue
		start, dur = span
		s = pcm[int(start * SR / 1000):int((start + dur) * SR / 1000)].copy()
		if len(s) < SR // 100:
			continue
		cache[f"key:{code}"] = shape_slice(s, int(code))
	for name, code in KEY_CODES.items():
		if f"key:{code}" in cache:
			cache[f"key:{name}"] = cache[f"key:{code}"]

def synth_wheel(sr: int) -> np.ndarray:
	# a soft scroll-wheel detent: a short muffled tick, much softer and darker
	# than a mouse click - a hair of noise on a low damped thonk, low-passed
	n = int(sr * 0.030)
	tt = np.arange(n) / sr
	noise = np.random.default_rng(3).standard_normal(n) * np.exp(-tt * 320)
	body = np.sin(2 * math.pi * 175 * tt) * np.exp(-tt * 150)
	mix = noise * 0.45 + body * 0.55
	sos = spsig.butter(2, 1700, btype="low", fs=sr, output="sos")
	mix = spsig.sosfilt(sos, mix)
	mix /= np.abs(mix).max() + 1e-9
	return np.stack([mix, mix], axis=1).astype(np.float32) * 0.28

def load_samples(work: Path) -> dict[str, np.ndarray]:
	cache: dict[str, np.ndarray] = {}
	for kind, path in SOUND_FILES.items():
		wav = work / (re.sub(r"[^A-Za-z0-9]", "_", kind) + ".wav")
		run(["ffmpeg", "-v", "error", "-y", "-i", str(path),
			"-ar", str(SR), "-ac", "2", "-f", "wav", str(wav)])
		with wave.open(str(wav), "rb") as w:
			data = np.frombuffer(w.readframes(w.getnframes()), dtype=np.int16)
		s = data.astype(np.float32).reshape(-1, 2) / 32768.0
		cache[kind] = s
	load_keypack(work, cache)
	cache["mouse:WHEEL"] = synth_wheel(SR)
	return cache

def build_audio(rec: Rec, work: Path, duration: float, rng: random.Random) -> Path:
	cache = load_samples(work)
	mix = np.zeros((int(duration * SR) + SR, 2), dtype=np.float32)
	for epoch, kind in rec.events:
		if any(s <= epoch < e for s, e in rec.cuts):
			continue
		t_rel = rec.net(epoch) - rec.t0_e + FOLEY_LAG
		if t_rel < -0.5 or t_rel > duration:
			continue
		s = cache.get(kind)
		if s is None:
			continue
		gain = GAIN.get(kind, GAIN.get(kind.split(":")[0], 0.8))
		gain *= rng.uniform(0.85, 1.05)           # stroke-force wobble; samples are raw
		samp = s
		at = int(max(0.0, t_rel) * SR)
		end = min(at + len(samp), len(mix))
		mix[at:end] += samp[: end - at] * gain
	peak = np.abs(mix).max()
	if peak > 0:
		mix *= min(0.40 / peak, 4.0)              # ~ -8 dBFS, bounded boost
	out = work / "audio.wav"
	with wave.open(str(out), "wb") as w:
		w.setnchannels(2)
		w.setsampwidth(2)
		w.setframerate(SR)
		w.writeframes((mix * 32767.0).astype(np.int16).tobytes())
	return out


##•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
##	Post: sync-flash location, motion-blur downsample, banners, encode

def check_drift(rec: Rec, video_end_e: float) -> None:
	dur = float(out_of(["ffprobe", "-v", "error", "-show_entries", "format=duration",
		"-of", "csv=p=0", str(rec.raw)]))
	expect = (video_end_e - rec.flash_e) + rec.flash_vt
	if abs(dur - expect) > max(0.5, expect * 0.02):
		log(f"WARNING: capture drift - raw {dur:.1f}s vs expected {expect:.1f}s; "
			"AV sync may be off (X server starved the grab loop?)")

def find_flash(raw: Path, work: Path) -> float:
	stats = work / "stats.txt"
	run(["ffmpeg", "-v", "error", "-t", "8", "-i", str(raw),
		"-vf", f"signalstats,metadata=print:key=lavfi.signalstats.YAVG:file={stats}",
		"-f", "null", "-"])
	best_t, best_y, pts = 0.0, -1.0, 0.0
	for line in stats.read_text().splitlines():
		mo = re.search(r"pts_time:([0-9.]+)", line)
		if mo:
			pts = float(mo.group(1))
		mo = re.search(r"YAVG=([0-9.]+)", line)
		if mo and float(mo.group(1)) > best_y:
			best_y, best_t = float(mo.group(1)), pts
	if best_y < 180:
		raise RuntimeError(f"sync flash not found (max YAVG {best_y})")
	return best_t

def esc_drawtext(work: Path, i: int, text: str) -> Path:
	f = work / f"banner{i}.txt"
	f.write_text(text)
	return f

# caption placement: centered in the black band above the window, so it never
# covers the terminal and needs no box behind it to stay readable
def banner_xy(rec: Rec) -> tuple[str, str]:
	return "(w-text_w)/2", f"({rec.band}-text_h)/2"

# a quick damped-spring vertical bounce for the pop-in / pop-out (~0.6s each): the
# caption springs in from just below its rest line, rings down, and springs back
# out as it fades. `base` is the rest y (may be an expr like "h-118").
def wobble_y(base: str, s: float, e: float, amp: int) -> str:
	win = 0.6
	ring = f"{amp}*exp(-6*T)*cos(2*PI*2.6*T)"
	win_in  = ring.replace("T", f"(t-{s:.3f})")
	win_out = ring.replace("T", f"({e:.3f}-t)")
	return (f"({base})"
		f"+if(between(t,{s:.3f},{s + win:.3f}),{win_in},0)"
		f"+if(between(t,{e - win:.3f},{e:.3f}),{win_out},0)")

def vf_chain(rec: Rec, work: Path, trim: float, dur: float, tail: bool = False) -> str:
	p = rec.p
	def to_vt(epoch: float) -> float:
		return rec.flash_vt + (epoch - rec.flash_e)
	# the GPU source is genuinely smooth, so just pin CFR at the delivery rate -
	# no frame-averaging needed (and none to fake, the frames are real)
	filters = [f"fps={rec.out_fps}"]
	if rec.cuts:
		gone = "+".join(f"between(t,{to_vt(s) - trim:.3f},{to_vt(e) - trim:.3f})"
			for s, e in rec.cuts)
		# select leaves the rate unset, and the tail's tpad then adds nothing
		filters.append(f"select='not({gone})',setpts=N/({rec.out_fps}*TB),"
			f"fps={rec.out_fps}")
	# resolve each banner's [s,e]; then clamp every end to the next banner's start
	# minus a gap, so only ONE banner is ever on screen (consecutive banners were
	# crossfading into an overlapping smear)
	spans: list[tuple[float, float, str]] = []
	for s_e, e_e, text in rec.banners:
		s = max(0.0, to_vt(rec.net(s_e)) - trim)
		e = max(s + p["banner_min"], to_vt(rec.net(e_e)) - trim)
		spans.append((s, e, text))
	spans.sort(key=lambda b: b[0])
	gap = 0.4
	for i in range(len(spans) - 1):
		s, e, text = spans[i]
		spans[i] = (s, min(e, spans[i + 1][0] - gap), text)
	amp = max(4, int(rec.band * 0.18))         # bounce stays inside the band
	x, base_y = banner_xy(rec)
	for i, (s, e, text) in enumerate(spans):
		if e <= s:
			continue
		tf = esc_drawtext(work, i, text)
		y = wobble_y(base_y, s, e, amp)
		# quick alpha pop (~0.15s) - the bounce carries the motion
		fade = f"clip((t-{s:.3f})/0.15,0,1)*clip(({e:.3f}-t)/0.15,0,1)"
		filters.append(
			f"drawtext=fontfile={BANNER_TTF}:textfile={tf}:fontsize={p['banner_fs']}:"
			f"fontcolor={BANNER_FG}:"
			f"x={x}:y='{y}':alpha='{fade}':enable='between(t,{s:.3f},{e:.3f})'")
	# no head/tail fades: the fade gradient is a fresh frame every step, which
	# bloats the gif enormously (palette churn + huge inter-frame deltas)
	# flatten to rgb24 so palettegen/paletteuse never see a stray alpha channel
	filters.append("format=rgb24")
	# end tail: hold the final frame (no motion) then a fully black screen. Only the
	# full-length outputs get it - not the looping highlight gif (default tail=False).
	if tail:
		filters.append(f"tpad=stop_mode=clone:stop_duration={TAIL_HOLD_S}")
		filters.append(f"tpad=stop_mode=add:color=black:stop_duration={TAIL_BLACK_S}")
	return ",".join(filters)

def encode_video(rec: Rec, work: Path, out_mp4: Path, video_end_e: float) -> Path:
	rec.flash_vt = find_flash(rec.raw, work)
	log(f"sync flash at video t={rec.flash_vt:.3f}s")
	check_drift(rec, video_end_e)
	trim = rec.flash_vt + (rec.t0_e - rec.flash_e)
	dur = video_end_e - rec.t0_e - rec.cut_s()
	vf = vf_chain(rec, work, trim, dur, tail=True)
	rng = random.Random(1)
	audio = build_audio(rec, work, dur, rng)   # tail is silent (freeze + black)
	run(["ffmpeg", "-v", "error", "-y",
		"-ss", f"{trim:.3f}", "-i", str(rec.raw), "-i", str(audio),
		"-t", f"{dur + TAIL_EXTRA:.3f}", "-vf", vf,
		"-c:v", "libx265", "-preset", "slow", "-crf", "20", "-pix_fmt", "yuv420p",
		"-tag:v", "hvc1", "-x265-params", "log-level=error",
		"-r", str(rec.out_fps), "-c:a", "aac", "-b:a", "160k",
		"-movflags", "+faststart", str(out_mp4)])
	return out_mp4

GIF_COLORS = 160        # one global palette; the wallpaper finale wants the headroom
# gifsicle's lossy LZW threshold. OFF on purpose: it only bought ~10%, and what it
# spends that on is ghost bars of the previous screen left in flat black areas -
# which in a terminal demo reads as the terminal itself misdrawing. Raise it only
# if a future scene list pushes the gif past what the README can carry.
GIF_LOSSY  = 0

def gif_pass(rec: Rec, work: Path, out_gif: Path, trim: float, dur: float, colors: int = GIF_COLORS, tail: bool = False) -> Path:
	vf = vf_chain(rec, work, trim, dur, tail=tail)
	pal = work / "pal.png"
	cut = ["-ss", f"{trim:.3f}", "-t", f"{dur + (TAIL_EXTRA if tail else 0.0):.3f}"]
	# ONE global palette (stats_mode=full) applied uniformly: stats_mode=diff +
	# diff_mode=rectangle mis-handled the big inter-frame jumps of fast scrolling
	# and left white/ghosted blocks. Ordered bayer stays temporally stable (error
	# diffusion shimmers and bloats a gif).
	run(["ffmpeg", "-v", "error", "-y", *cut, "-i", str(rec.raw),
		"-vf", f"{vf},palettegen=stats_mode=full:max_colors={colors}", str(pal)])
	run(["ffmpeg", "-v", "error", "-y", *cut, "-i", str(rec.raw), "-i", str(pal),
		"-lavfi", f"{vf}[x];[x][1:v]paletteuse=dither=bayer:bayer_scale=4",
		str(out_gif)])
	return out_gif

def gif_optimize(gif: Path) -> Path:
	# gifsicle squeezes the encoder's output further: -O3 re-cuts every frame to
	# the smallest changed rectangle, so the static band above the window and the
	# held tail frames cost near nothing. Skipped, with a note, when absent.
	if not shutil.which("gifsicle"):
		log("WARNING: gifsicle not found - gif left unoptimized")
		return gif
	before = gif.stat().st_size / (1 << 20)
	opt = gif.with_name(gif.stem + "-opt.gif")
	cmd = ["gifsicle", "-O3", "--no-warnings", "-o", str(opt), str(gif)]
	if GIF_LOSSY:
		cmd.insert(2, f"--lossy={GIF_LOSSY}")
	run(cmd)
	after = opt.stat().st_size / (1 << 20)
	log(f"gifsicle: {before:.1f} -> {after:.1f} MiB (lossy={GIF_LOSSY})")
	return opt

def encode_gif(rec: Rec, work: Path, out_gif: Path, video_end_e: float) -> Path:
	rec.flash_vt = find_flash(rec.raw, work)
	log(f"sync flash at video t={rec.flash_vt:.3f}s")
	check_drift(rec, video_end_e)
	trim = rec.flash_vt + (rec.t0_e - rec.flash_e)
	dur = video_end_e - rec.t0_e
	gif_pass(rec, work, out_gif, trim, dur, tail=True)
	return gif_optimize(out_gif)


##•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
##	Output placement + rotation (video and gif in their own dirs)

# The README carries the whole demo (it ends on the wallpaper + the black tail,
# which a cut-down highlight could never show), and 12 MiB is the stated ceiling
# for it. Scrolling is what sets the size - measured per second of finished gif
# at 50fps: a dense full-width scroll runs ~1.6 MiB/s, the build scene ~0.4, a
# near-static scene ~0.03. So if a render comes out over, the levers in order are the
# LENGTH of the wheel scene, the width of the rows in motion, and only then
# GIF_LOSSY. Dropping to 25fps halves it outright and is the last resort - the
# smoothness is the thing being demonstrated.
GIF_ASSET_MAX_MB = 12

def rotate(out_dir: Path, prefix: str, ext: str, no_rotate: bool) -> None:
	if no_rotate:
		return
	inc = REPO / "cicd/utility/include/gfs-rotate.bash"
	subprocess.run(["bash", "-c",
		f'source "{inc}" && gfs_rotate "{out_dir}" {prefix} {ext}'], check=False)

def place_video(mp4: Path, out_dir: Path, no_rotate: bool) -> None:
	out_dir.mkdir(parents=True, exist_ok=True)
	stamp = time.strftime("%Y%m%d-%H%M%S")
	dst = out_dir / f"silkterm-demo_{stamp}.mp4"
	shutil.copy2(mp4, dst)
	mb = dst.stat().st_size / (1 << 20)
	rotate(out_dir, "silkterm-demo", "mp4", no_rotate)
	log(f"video: {dst} ({mb:.1f} MiB)")

def place_gif(gif: Path, out_dir: Path, no_rotate: bool, no_asset: bool = False) -> None:
	out_dir.mkdir(parents=True, exist_ok=True)
	stamp = time.strftime("%Y%m%d-%H%M%S")
	dst = out_dir / f"silkterm-demo_{stamp}.gif"
	shutil.copy2(gif, dst)
	mb = dst.stat().st_size / (1 << 20)
	rotate(out_dir, "silkterm-demo", "gif", no_rotate)
	log(f"gif: {dst} ({mb:.1f} MiB)")
	if no_asset:                              # partial/tuning runs must not clobber it
		log("gif (README): skipped (--no-asset)")
	elif mb <= GIF_ASSET_MAX_MB:
		asset = REPO / "assets" / "demo.gif"
		shutil.copy2(gif, asset)
		log(f"gif (README): {asset} ({mb:.1f} MiB)")
	else:
		log(f"WARNING: gif is {mb:.1f} MiB (> {GIF_ASSET_MAX_MB}); assets/demo.gif "
			"left untouched - trim scenes or raise GIF_LOSSY")


##•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
##	Entry

def record(args: argparse.Namespace, name: str, seed: int) -> None:
	rng = random.Random(seed)
	rec = Rec(args, PROFILES[name])
	try:
		prep_content(rec, rng)
		rec.start_display()
		rec.start_capture()
		log(f"[{name}] capture running; launching app")
		# --norc/--noprofile skips even the system bashrc (which spews real paths on
		# this box); PS1 comes in via the environment
		rec.launch_app("/bin/bash --noprofile --norc -i")
		time.sleep(2.5)
		rec.t0_e = time.time() - LEAD_S

		t = Typist(rec, rng)
		m = Mouse(rec, rng)
		want = [s.strip() for s in args.segments.split(",") if s.strip()]
		for seg, fn in SEGMENTS[name]:
			if want and seg not in want:
				continue
			log(f"[{name}] segment: {seg}")
			rec.seg_marks[seg] = time.time()
			fn(rec, t, m)
		time.sleep(0.3)                       # brief settle; the 3s hold is added at encode (tpad)
		video_end_e = time.time()

		rec.stop_capture()
		rec.kill_app()

		if name == "video":
			out = rec.work / "demo.mp4"
			encode_video(rec, rec.work, out, video_end_e)
			place_video(out, Path(args.out_dir) / "video", args.no_rotate)
		else:
			out = rec.work / "demo.gif"
			gif = encode_gif(rec, rec.work, out, video_end_e)
			place_gif(gif, Path(args.out_dir) / "gif", args.no_rotate, args.no_asset)
		if rec.keep:
			log(f"[{name}] work dir kept: {rec.work}")
	finally:
		rec.cleanup()

def main() -> None:
	ap = argparse.ArgumentParser(description="Record the SilkTerm demo video + gif.")
	ap.add_argument("--display", default=os.environ.get("SILK_DEMO_DISPLAY", ":98"))
	ap.add_argument("--profile", default="video,gif", help="comma list: video,gif")
	ap.add_argument("--segments", default="", help="comma list; default all")
	ap.add_argument("--seed", type=int, default=None)
	ap.add_argument("--keep-work", action="store_true")
	ap.add_argument("--no-rotate", action="store_true")
	ap.add_argument("--no-asset", action="store_true",
		help="do not overwrite assets/demo.gif (for partial/tuning runs)")
	ap.add_argument("--out-dir", default=str(PRIVATE))
	args = ap.parse_args()

	seed = args.seed if args.seed is not None else int(time.time()) & 0xFFFF
	log(f"seed {seed}")
	for name in [p.strip() for p in args.profile.split(",") if p.strip()]:
		if name not in PROFILES:
			sys.exit(f"unknown profile: {name}")
		record(args, name, seed)

if __name__ == "__main__":
	main()


##	Script history:
##		- 20261005: type hints; the wm, ffmpeg and app logs are closed on our side
##		  once each child has started. Nothing recorded changes.
##		- 20260928: the wallpaper arrives through the control socket with
##		  nothing typed, under a new caption. The build caption says it adapts
##		  to any output speed.
##		- 20260924: closing the panes happens in a cut, which the encode drops
##		  along with its sound. The cursor turns to a bar and a phase fade in one
##		  step. The wheel caption names the minimap.
##		- 20260917: the WM session keeps every XDG folder inside its own HOME, so
##		  xfconfd no longer writes the demo theme over the desktop's, and ends
##		  with its bus and xfconfd; USER may be unset; the default binary follows
##		  CARGO_TARGET_DIR.
##		- 20260813: the run pins the app's frame rate to the capture rate
##		  (SILK_MAX_FPS) - the two were 60 and 50, so one frame in six was dropped
##		  and the picture hitched on a strict period. Gif back to 50fps on that
##		  footing; asset ceiling 12 MiB. The build scene walks the speed curve in
##		  five paced movements with a silence in the middle, so the wind-down is
##		  visible at all. Second pane split is horizontal - at a third of the width
##		  the prompt wrapped on resize and each pane eased the reprint in. Scene
##		  list mirrored in script.txt, which is the file to edit.
##		- 20260805: set_cfg resolves a dotted path from the indentation instead of
##		  matching the line literally, and stops the run when a key resolves to
##		  nothing - the config is nested by the time any scene changes a setting,
##		  so every live change had been a silent no-op. Wallpaper rotation off, or
##		  it adopts the reveal image at launch. Scrim values unpinned so the demo
##		  shows what ships. Cursor starts as the block and narrows to a bar.
##		- 20260729: a blank line closes the showcase output, so the sign-off block
##		  sits clear of the unicode row.
##		- 20260726: gif cut under 10 MiB - the `less` scene dropped, the wheel one
##		  direction only, the build scroll halved, the listing narrowed (no owner
##		  or group columns, bare time) and fewer typos. Alias scene gone (the flags
##		  live in the ls wrapper). Panes: two vertical splits, cursor stilled first,
##		  and the block-cursor switch moved to after them (back to pulsing, full
##		  block). Showcase bar sweeps the full hue circle, dark purple to dark red;
##		  katakana and emoji replace the dingbats; the sign-off sits between two
##		  bare prompts.
##		- 20260726: 8px black border; slate blue-gray decoration generated at
##		  record time; the cursor switches to a block mid-run through the control
##		  socket; two extra panes closed with `exit`; Settings and tabs scenes
##		  dropped; emoji and kanji in the closing showcase.
##		- 20260726: narration moved into a black band above the window (plain
##		  yellow, no box, same wobble pop); longer scene list (wheel scrollback,
##		  split panes, tabs) closing on the built-in wallpaper + a color/unicode
##		  showcase; screens cleared between scenes; the README gif is now the whole
##		  demo (the highlight cut is gone) and runs through gifsicle.
##		- 20260713: the faux window fills the view - only a 4px black border
##		  around it (was a 3%/5% dark margin); square-cornered dark decoration
##		  (Material-Black-Pistachio theme); the client is sized + the frame nudged
##		  so the outer decoration sits 4px inside each edge.
##		- 20260713: hold the final frame 3s then a 2s black screen at the end
##		  (tpad at encode; full-length outputs only, not the looping highlight gif).
##		- 20260713: per-key sound bank (mechvibes EG Oreo, one slice per
##		  physical key) replaces the per-row bank; chars map to their real key's
##		  sample, so variety is natural - dropped the pitch-shift/spectral-tilt/
##		  mid-click processing and the separate release sounds.
##		- 20260713: window size passed at launch (--pixel-width/height), never
##		  resized after - fixes the clipped video / band-at-top gif (VGL EGL
##		  latches the surface size at creation); both profiles start opaque on
##		  black (no bg image, image opacity 0.10); scene order alias-ls-build-
##		  settings-wp41-less-wp45-outro with two wallpaper scenes; synth desktop
##		  dropped.
##		- 20260712: GPU render via VirtualGL (real ~60fps, the actual judder
##		  fix - dropped the high-fps+tmix hack); one unified script for both
##		  profiles; gray-# outro via a prompt flag; solid-gray captions with a
##		  wobble pop, moved onto the title/menu chrome; Settings scene circles the
##		  scrim rows then Esc-cancels; focus-settle before typing (fixes a dropped
##		  first keystroke after the dialog).
##		- 20260712: Real window decoration; high-fps capture + motion-blur
##		  downsample (judder fix); dim vague dark desktop behind the glass; new
##		  scene order + mouse toggle/hold-arrow/gray-outro/wallpaper-clear;
##		  processed key bank (mid-click + variety) + soft wheel; top-right
##		  narration; video/gif split into their own output dirs.
##		- 20260712: Two recordings (1080p60 h265 + native 540p50 gif),
##		  see-through desktop via config+socket reload, Lato narration.
##		- 20260711: Created.
