#!/usr/bin/env python3

##	- Purpose:
##		The demo recorder runs a window manager session of its own, and that
##		session once wrote its theme over the desktop's settings and outlived the
##		recording. This drives the session the recorder's way, with a stand-in for
##		the window manager, and checks where the recorder finds its run folder and
##		its binary. Nothing here needs a display.
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

if failures:
	print(f"{failures} failed")
	sys.exit(1)
print("all passed")

##	History:
##		- 20260917 JC: Created.
