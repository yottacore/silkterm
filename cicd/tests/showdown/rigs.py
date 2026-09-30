#!/usr/bin/env python3

##	- Purpose:
##		Every row of the README's showdown table that was measured on the Linux rigs
##		has to be measurable again, by the entry point and by the rig that owns each
##		of its columns. This holds that list against the table. It also checks what
##		the GNOME Terminal, WezTerm, Tabby and Hyper entries rely on: finding the
##		terminal among the processes of the rig's throwaway account, billing a
##		bundle once for its own libraries, and taking a version from the command
##		that starts a terminal when the terminal itself will not give one. XTerm's
##		speed row came from X11, so the speed rig has to run it on an X server of
##		its own, and README note 9 has to say so.
##	- Test ID: ErE7yrA
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

import importlib.util
import os
import re
import shutil
import signal
import subprocess
import sys
import tempfile
from pathlib import Path

ME_DIR = Path(__file__).resolve().parent
REPO = ME_DIR.parents[2]
UTILITY = REPO / "utility"
INCLUDE = UTILITY / "include"

failures = 0
def check(what, ok, detail=""):
	global failures
	if ok:
		print(f"  ok   {what}")
	else:
		print(f"  FAIL {what}{': ' + detail if detail else ''}")
		failures += 1

def load(name, path):
	spec = importlib.util.spec_from_file_location(name, path)
	mod = importlib.util.module_from_spec(spec)
	spec.loader.exec_module(mod)
	return mod

scratch = Path(tempfile.mkdtemp(prefix="silk-showrig-"))

## Rows measured on Windows are taken from inside the terminal there, not by a rig.
WINDOWS_ROWS = {"conhost.exe", "Windows Terminal"}
us = load("update_showdown", UTILITY / "update-showdown.py")
sr = load("showdown_readme", INCLUDE / "showdown-readme.py")
mdtable = load("mdtable", INCLUDE / "mdtable.py")
readme = (REPO / "README.md").read_text(encoding="utf-8")
block = readme.split(sr.BEGIN, 1)[1].split(sr.END, 1)[0]
table = [mdtable.split_row(ln) for ln in block.splitlines() if ln.startswith("|")]
header = table[0]

def column(word):
	return next(i for i, cell in enumerate(header) if word in cell)

name_col, speed_col = column("Terminal"), column("1-byte")
size_cols = (column("File+"), column("Mem"))

def measured(cell):
	return re.fullmatch(r"[0-9.]+", cell.replace("*", "").strip()) is not None

rigs_for = {sr.norm(row): rigs for _, row, rigs in us.TERMS}
for cells in table[2:]:
	name = sr.norm(cells[name_col])
	shown = re.sub(r"<sup>.*?</sup>", "", cells[name_col]).strip()
	if measured(cells[speed_col]):
		check(f"{shown}'s speed figure can be taken again",
			rigs_for.get(name) in ("both", "speed"), str(rigs_for.get(name)))
	if any(measured(cells[i]) for i in size_cols) and shown not in WINDOWS_ROWS:
		check(f"{shown}'s size figures can be taken again",
			rigs_for.get(name) in ("both", "size"), str(rigs_for.get(name)))

## Each key the entry point hands a rig has a recipe there.
speed_rig = (INCLUDE / "termbench-run.bash").read_text(encoding="utf-8")
speed_arms = {key for arm in re.findall(r"^\t([a-z0-9|]+)\)$", speed_rig, re.M) for key in arm.split("|")}
size_keys = subprocess.run([str(INCLUDE / "sizebench-run.bash"), "--list"],
	capture_output=True, text=True, timeout=30).stdout.split()
for key, row, rigs in us.TERMS:
	if rigs in ("both", "speed"):
		check(f"the speed rig has a recipe for {key}", key in speed_arms)
	if rigs in ("both", "size"):
		check(f"the size rig has a recipe for {key}", key in size_keys)

## Through the compositor's Xwayland, xterm reads about a third below its X11 row.
x11_terms = re.search(r'^declare -r x11Terms="([^"]*)"$', speed_rig, re.M)
check("the speed rig runs xterm on an X server of its own",
	x11_terms is not None and "xterm" in x11_terms.group(1).split() and "Xvfb" in speed_rig)
note9 = next((ln for ln in readme.splitlines() if ln.startswith("<sub><sup>9</sup>")), "")
check("README note 9 says XTerm's row comes from a private X server",
	"XTerm draws only on X11, so its row comes from a private X server" in note9)

## Finding the terminal on the throwaway account. The stand-in terminal is a copy of
## bash, so it can start another copy of itself.
fake = scratch / "fake-term"
shutil.copy2("/bin/bash", fake)
home = scratch / "home"
home.mkdir()
spawned = []

def start(cmd, **env):
	proc = subprocess.Popen(["/bin/sh", "-c", cmd], env=dict(os.environ, FAKE=str(fake), **env),
		stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True, start_new_session=True)
	spawned.append(proc)
	return proc

def owned_root(launched, home_dir):
	got = subprocess.run(["bash", "-c", 'source "$1" && fOwnedRoot "$2" "$3" "$4"', "bash",
		str(INCLUDE / "bench-common.bash"), str(fake), str(home_dir), str(launched)],
		capture_output=True, text=True, timeout=60)
	return got.stdout.strip() if got.returncode == 0 else ""

def child_of(pid, exe):
	for _ in range(50):
		for kid in subprocess.run(["pgrep", "-P", str(pid)], capture_output=True, text=True).stdout.split():
			try:
				if os.path.realpath(f"/proc/{kid}/exe") == os.path.realpath(exe):
					return kid
			except OSError:
				pass
		subprocess.run(["sleep", "0.1"])
	return ""

try:
	## A copy of the same program that is somebody else's: not launched, no rig HOME.
	start('"$FAKE" -c "sleep 60; :" >/dev/null & wait')
	## Launched, with the program's own HOME, and a copy of itself as its child. Electron
	## looks like this: its environment is written over, so only the tree says it is ours.
	launched = start('"$FAKE" -c \'"$FAKE" -c "sleep 60; :" & wait\' >/dev/null & wait')
	top = child_of(launched.pid, fake)
	## Started by a session bus that forks twice, so it is nobody's child, but it has the
	## account's HOME. GNOME Terminal's server looks like this.
	escaped = start('"$FAKE" -c "sleep 60; :" >/dev/null & echo $!', HOME=str(home))
	escaped_pid = escaped.stdout.readline().strip()
	unrelated = start("sleep 60")

	check("the topmost copy in the launched tree is the terminal",
		top != "" and owned_root(launched.pid, home) == top, f"{top} against {owned_root(launched.pid, home)}")
	check("one outside the tree with the account's HOME is found",
		escaped_pid != "" and owned_root(unrelated.pid, home) == escaped_pid, f"{escaped_pid} against {owned_root(unrelated.pid, home)}")
	check("a copy with neither is never taken", owned_root(unrelated.pid, scratch / "elsewhere") == "")
finally:
	for proc in spawned:
		try:
			os.killpg(proc.pid, signal.SIGKILL)
		except OSError:
			pass

## A bundle's own libraries are already in its unpacked size, so File+deps adds only
## what it borrows from the system. Counting them twice put WezTerm 3 MiB and Tabby
## 7 MiB above their published rows.
classify = load("sizebench_classify", INCLUDE / "sizebench-classify.py")
bundle = scratch / "bundle"
(bundle / "usr/bin").mkdir(parents=True)
(scratch / "system").mkdir()
term_exe, own_lib, borrowed_lib = bundle / "usr/bin/term", bundle / "libown.so.1", scratch / "system/libborrowed.so.2"
for path, mib in ((term_exe, 3), (own_lib, 1), (borrowed_lib, 2)):
	path.write_bytes(b"\0" * (mib << 20))

class Collector:
	name = "linux"
	def mapped_files(self, pid): return {str(term_exe), str(own_lib), str(borrowed_lib)}
	def is_library(self, path): return ".so" in os.path.basename(path)
	def base_name(self, path): return os.path.basename(path)
	def find_library(self, name): return None
	def needed(self, path): return []
	def is_gfx(self, path): return False
	def is_base_os(self, path): return False
	def regions(self, pid): return []
	def norm(self, path): return path

try:
	bundled = classify.measure([1], [str(term_exe)], Collector(), payload=str(bundle))
	check("a bundle adds only the libraries it borrows", bundled["deps_mib"] == 2.0, str(bundled["deps_mib"]))
except TypeError as err:
	check("a bundle adds only the libraries it borrows", False, str(err))
packaged = classify.measure([1], [str(term_exe)], Collector())
check("a packaged terminal still adds all of them", packaged["deps_mib"] == 3.0, str(packaged["deps_mib"]))

got = subprocess.run(["bash", "-c", 'source "$1" && fAppDir "$2"', "bash",
	str(INCLUDE / "sizebench-run.bash"), str(term_exe)], capture_output=True, text=True, timeout=30)
check("a binary outside an AppImage has no bundle root", got.returncode != 0 and got.stdout == "", got.stdout)
(bundle / "AppRun").write_text("#!/bin/sh\n")
got = subprocess.run(["bash", "-c", 'source "$1" && fAppDir "$2"', "bash",
	str(INCLUDE / "sizebench-run.bash"), str(term_exe)], capture_output=True, text=True, timeout=30)
check("an extracted AppImage is billed from the folder holding AppRun", got.stdout == str(bundle), got.stdout)

## A terminal whose working binary has no version of its own takes one from the command
## that starts it: beside it for wezterm-gui, on PATH for gnome-terminal-server.
tb = load("termbench", INCLUDE / "termbench.py")
libexec, bindir = scratch / "libexec", scratch / "bin"
libexec.mkdir()
bindir.mkdir()
scripts = {
	bundle / "usr/bin/wez-gui": "echo 'wez-gui someone forgot to call assign_version_info'",
	bundle / "usr/bin/wez": "echo 'wez 20240203-110809-5046fc22'",
	libexec / "gnt-server": "exit 1",
	bindir / "gnt": "echo '# GNT Terminal 3.58.1 using VTE 0.80.1 +BIDI'",
}
for path, line in scripts.items():
	path.write_text(f"#!/bin/sh\n{line}\n")
	path.chmod(0o755)
os.environ["PATH"] = f"{bindir}:{os.environ['PATH']}"
tb._silkterm_exe = lambda: ""

class NoConsole:
	ok = False

for exe, name, version in ((bundle / "usr/bin/wez-gui", "wez", "20240203"),
                           (libexec / "gnt-server", "GNT Terminal", "3.58.1")):
	tb._ancestor_terminal = lambda exe=exe: str(exe)
	got_name, build, _ = tb.identify(NoConsole())
	check(f"{exe.name} reports {name} {version}",
		got_name == name and build.split("+", 1)[0] == version, f"{got_name} {build}")

shutil.rmtree(scratch, ignore_errors=True)
if failures:
	print(f"{failures} failed")
	sys.exit(1)
print("all passed")

##	History:
##		- 20260928 JC: Created.
##		- 20260929 JC: XTerm's speed row, on an X server of its own.
