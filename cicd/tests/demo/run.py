#!/usr/bin/env python3

##	- Purpose:
##		The demo recorder runs a window manager session of its own, and that
##		session once wrote its theme over the desktop's settings and outlived the
##		recording. This drives the session the recorder's way, with a stand-in for
##		the window manager, and checks where the recorder finds its run folder and
##		its binary. Also the environment the app gets, and the settings changes
##		made mid-scene. Nothing here needs a display.
##	- Test ID: EqBe4cC
##	- History: At bottom of file.

##	Copyright (c) 2026 Bubbles
##	SPDX-License-Identifier: MIT

import importlib.util
import os
import pwd
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from types import SimpleNamespace

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import _testdir  # noqa: E402
_testdir.use()

ME_DIR = Path(__file__).resolve().parent
RECORDER = ME_DIR.parents[1] / "utility/demo-video/demo-video.py"

failures = 0
def check(what, ok, detail=""):
	global failures
	if ok:
		print(f"  ok   {what}")
	else:
		print(f"  FAIL {what}{': ' + detail if detail else ''}")
		failures += 1

def load():
	spec = importlib.util.spec_from_file_location("demo_video", RECORDER)
	mod = importlib.util.module_from_spec(spec)
	spec.loader.exec_module(mod)
	return mod

def make_rec(mod):
	return mod.Rec(SimpleNamespace(display=":197", keep_work=False), mod.PROFILES["gif"])

def with_env(changes, fn):
	saved = dict(os.environ)
	try:
		for k, v in changes.items():
			if v is None:
				os.environ.pop(k, None)
			else:
				os.environ[k] = v
		return fn()
	finally:
		os.environ.clear()
		os.environ.update(saved)

try:
	demo = load()
except ImportError as e:
	print(f"  skip the recorder cannot load here ({e})")
	_testdir.end(0)
	sys.exit(0)

def done(rec):
	shutil.rmtree(rec.work, ignore_errors=True)

## Run folder: the one gui-headless.bash uses, with USER set or not.
me = pwd.getpwuid(os.getuid()).pw_name
rec = with_env({"USER": None}, lambda: make_rec(demo))
check("USER unset finds the run folder by account name",
	rec.auth == f"/tmp/cicd-gui-headless-{me}/Xauthority-197", rec.auth)
done(rec)
rec = with_env({"USER": "someone"}, lambda: make_rec(demo))
check("USER set is still what names it", "/tmp/cicd-gui-headless-someone/" in rec.auth, rec.auth)
done(rec)

## Binary: under CARGO_TARGET_DIR when there is no SILK_BIN, relative to the repo.
cases = [
	("no target dir", None, demo.REPO / "target/release/silkterm"),
	("an absolute target dir", "/elsewhere/tgt", Path("/elsewhere/tgt/release/silkterm")),
	("a relative target dir", "build/t", demo.REPO / "build/t/release/silkterm"),
]
for what, tdir, want in cases:
	rec = with_env({"SILK_BIN": None, "CARGO_TARGET_DIR": tdir}, lambda: make_rec(demo))
	check(f"binary with {what}", Path(rec.bin) == want, rec.bin)
	done(rec)
rec = with_env({"SILK_BIN": "/opt/x/silkterm", "CARGO_TARGET_DIR": "/elsewhere"}, lambda: make_rec(demo))
check("SILK_BIN still wins", rec.bin == "/opt/x/silkterm", rec.bin)
done(rec)

## Everything launched belongs on the private X display. winit prefers Wayland
## whenever it sees one, so DISPLAY alone left the window on the real desktop.
wayland = {"WAYLAND_DISPLAY": "wayland-0", "XDG_SESSION_TYPE": "wayland"}
rec = with_env(wayland, lambda: make_rec(demo))
for what, env_of in (("what the recorder runs", rec.env), ("the app", rec.app_env)):
	e = with_env(wayland, env_of)
	check(f"{what} has no Wayland session",
		"WAYLAND_DISPLAY" not in e and "XDG_SESSION_TYPE" not in e,
		f"{e.get('WAYLAND_DISPLAY')} {e.get('XDG_SESSION_TYPE')}")
	check(f"{what} is on the private display", e.get("DISPLAY") == ":197", str(e.get("DISPLAY")))
done(rec)

## The app paints at the rate the capture samples at, in every profile.
for name, profile in demo.PROFILES.items():
	rec = demo.Rec(SimpleNamespace(display=":197", keep_work=False), profile)
	fps = rec.app_env().get("SILK_MAX_FPS")
	check(f"the {name} profile pins the app to its capture rate",
		fps == str(profile["cap_fps"]), f"{fps} against {profile['cap_fps']}")
	done(rec)

## gui-headless.bash and the profiler stage start windows on a private display too.
def launch_env(script):
	env = dict(os.environ, **wayland)
	got = subprocess.run(["bash", "-c", script], env=env, capture_output=True, text=True, timeout=30)
	return dict(ln.split("=", 1) for ln in got.stdout.splitlines() if "=" in ln)

headless = (RECORDER.parents[1] / "gui-headless.bash").read_text(encoding="utf-8")
on_x = next((ln for ln in headless.splitlines() if ln.startswith("onX(){")), "")
e = launch_env(f'display=:197\n{on_x}\nonX env') if on_x else {}
check("gui-headless.bash's onX drops the Wayland session",
	e and "WAYLAND_DISPLAY" not in e and "XDG_SESSION_TYPE" not in e and e.get("DISPLAY") == ":197",
	on_x or "no onX line")

## The profiler's launch is one command continued over several lines; run it with
## a stand-in app that prints what it was handed.
pipeline = (RECORDER.parents[2] / "cicd.bash").read_text(encoding="utf-8").splitlines()
at = next((i for i, ln in enumerate(pipeline) if '"${PROFILE_BIN}" --shell' in ln), None)
launch = []
if at is not None:
	start = at
	while start > 0 and pipeline[start - 1].rstrip().endswith("\\"):
		start -= 1
	launch = pipeline[start:at + 1]
stand_in = Path(tempfile.mkdtemp(prefix="silk-demo-profiler-")) / "app"
stand_in.write_text("#!/bin/sh\nexec env\n")
stand_in.chmod(0o755)
e = launch_env(f"PROFILE_BIN={stand_in}; hdisp=:197\n" + "\n".join(launch)) if launch else {}
shutil.rmtree(stand_in.parent, ignore_errors=True)
check("the profiler stage drops the Wayland session",
	e and "WAYLAND_DISPLAY" not in e and "XDG_SESSION_TYPE" not in e and e.get("DISPLAY") == ":197",
	"\n".join(launch) or "no profiler launch found")

## Settings changes mid-scene: the app rewrites the file into nested blocks the
## first time it saves, so a key is found by its full path, and one that matches
## no line stops the run.
rec = make_rec(demo)
cfg = rec.home / ".config/silkterm/config.shcl"
cfg.parent.mkdir(parents=True)
cfg.write_text("cursor:\n\tsize:\n\t\twidth: 100\n\t\theight: 100\nwindow:\n\tsize:\n\t\twidth: 100\n")
sent = []
real_ctl = demo.ctl
demo.ctl = lambda r, line: sent.append(line) or True
try:
	demo.set_cfg(rec, {"cursor.size.width": 3})
	err = ""
except RuntimeError as ex:
	err = str(ex)
lines = cfg.read_text().split("\n")
check("a nested setting is found by its path",
	not err and lines[2] == "\t\twidth: 3" and lines[3] == "\t\theight: 100", err or repr(lines))
check("and only that one", lines[6] == "\t\twidth: 100", repr(lines))
check("and the app is told to reload", sent == ["reload"], str(sent))
try:
	demo.set_cfg(rec, {"cursor.size.depth": 3})
	missed = False
except RuntimeError:
	missed = True
check("a setting with no line stops the run", missed)
cfg.write_text("wallpaper:\n\tfallback_builtin: false\n")
demo.set_cfg(rec, {"wallpaper.fallback_builtin": True})
check("a switch is written the way shcl reads one",
	cfg.read_text().split("\n")[1] == "\tfallback_builtin: true", repr(cfg.read_text()))
demo.ctl = real_ctl
demo.write_config(rec.home, demo.PROFILES["gif"])
check("the recording's config turns wallpaper rotation off",
	"wallpaper.rotate.enabled: false" in cfg.read_text().splitlines())
check("and starts with no wallpaper, so the reveal changes something",
	"wallpaper.fallback_builtin: false" in cfg.read_text().splitlines())
done(rec)

## The window manager's session: its settings stay in the work folder, and
## nothing it started is left once it is stopped.
if not (shutil.which("dbus-run-session") and shutil.which("xfconf-query")):
	print("  skip window manager session (no dbus-run-session or xfconf-query)")
else:
	sentinel = Path(tempfile.mkdtemp(prefix="silk-demo-sentinel-"))
	xdg = {v: str(sentinel / v) for v in
		("XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME", "XDG_STATE_HOME")}
	for d in xdg.values():
		Path(d).mkdir()
	rec = with_env(xdg, lambda: make_rec(demo))
	def session():
		rec.start_wm("SilkDemo", wm="sleep 67")
		pgid = rec.wm.pid
		deadline = time.time() + 10
		chan = rec.wmhome / ".config/xfce4/xfconf/xfce-perchannel-xml/xfwm4.xml"
		while time.time() < deadline and not chan.exists():
			time.sleep(0.1)
		rec.stop_wm()
		return pgid, chan
	pgid, chan = with_env(xdg, session)
	left = [str(p.relative_to(sentinel)) for p in sentinel.rglob("*") if p.is_file()]
	check("the session writes nothing under the caller's XDG folders", not left, ", ".join(left))
	check("its settings are in its own HOME", chan.exists(), str(chan))
	ps = subprocess.run(["ps", "-eo", "pgid=,comm="], capture_output=True, text=True).stdout
	alive = [line.split(None, 1)[1] for line in ps.splitlines() if line.split()[0] == str(pgid)]
	check("nothing the session started is still running", not alive, ", ".join(alive))
	done(rec)
	shutil.rmtree(sentinel, ignore_errors=True)

_testdir.end(1 if failures else 0)
if failures:
	print(f"{failures} failed")
	sys.exit(1)
print("all passed")

##	History:
##		- 20260917 JC: Created.
##		- 20260926 JC: Wayland session, frame rate pin, settings changes.
