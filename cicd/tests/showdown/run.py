#!/usr/bin/env python3

##	- Purpose:
##		The README's showdown table only holds figures measured the same way as the
##		rows beside them. A quick run, a scaled run and a run at another grid each
##		used to rewrite their row anyway. This drives both table writers with the
##		terminal and the measuring faked out, against a scratch README. It also
##		checks the rigs find the build where CARGO_TARGET_DIR puts it, and that
##		the payloads repeat and the score counts weighted cells.
##	- Test ID: EqBnA5g
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

import importlib.util
import io
import math
import os
import shutil
import subprocess
import sys
import tempfile
from contextlib import redirect_stdout
from pathlib import Path
from types import SimpleNamespace

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import _testdir  # noqa: E402
_testdir.use()

ME_DIR = Path(__file__).resolve().parent
REPO = ME_DIR.parents[2]
UTILITY = REPO / "utility"

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

scratch = Path(tempfile.mkdtemp(prefix="silk-showdown-"))
readme = scratch / "README.md"
shutil.copyfile(REPO / "README.md", readme)
original = readme.read_text(encoding="utf-8")

## The measuring core, untouched: two terminals are only comparable on the same
## bytes, and the score is cells per second weighted toward plain ASCII.
core = load("termbench_core", UTILITY / "include/termbench.py")
for scene in core.SCENES:
	first = core.build_payload(scene, 0.01, 160)
	check(f"the {scene.name} payload is the same bytes every time",
		first[0] and first == core.build_payload(scene, 0.01, 160))

def score_with(name, kcells):
	per = {s.name: {"kcells": 1.0, "mbs": 1.0} for s in core.SCENES}
	per[name]["kcells"] = kcells
	return core.score_of(per)

## Weights 4, 2, 1, 1, 1 sum to 9, so e^9 in one scene lifts the score to e^weight.
for name, weight in (("ascii", 4), ("latin", 2), ("cjk", 1), ("emoji", 1), ("mixed", 1)):
	got = score_with(name, math.exp(9))
	check(f"the {name} scene weighs {weight} in the score",
		math.isclose(got, math.exp(weight)), f"{got} against {math.exp(weight)}")
row = core.summarize([2.0, 2.0], 8_000_000, 1_000_000, 2_000_000)
check("a scene's rate is cells per second", math.isclose(row["kcells"], 1000.0), str(row["kcells"]))
per = {s.name: {"kcells": 500.0, "mbs": 1.0 + i} for i, s in enumerate(core.SCENES)}
check("and the score reads cells, not bytes", math.isclose(core.score_of(per), 500.0), str(core.score_of(per)))

## termbench.py, with everything that touches a terminal replaced.
tb = load("termbench", UTILITY / "include/termbench.py")
saved = []

class FakeConsole:
	def __enter__(self): return self
	def __exit__(self, *exc): return False
	def emit(self, text): pass

class Tty:
	@staticmethod
	def isatty(): return True

grid = [(160, 42)]
tb.Console = FakeConsole
tb.sys = SimpleNamespace(stdout=Tty, stdin=Tty, stderr=io.StringIO())
tb.time = SimpleNamespace(perf_counter=lambda: 0.0, sleep=lambda s: None)
tb.terminal_size = lambda: grid[0]
tb.build_payload = lambda scene, scale, cells: (b"x" * 1000, 1000, 1000, 10)
tb.harness_ceiling = lambda blob: 0.0
tb.run_scene = lambda console, scene, blob, reps, quiet: ([0.001, 0.001], True)
tb.save = lambda records: saved.extend(records) or "memory"
tb.load = lambda: list(saved)
tb.readme_path = lambda: str(readme)

def bench(*args):
	with redirect_stdout(io.StringIO()) as out:
		tb.main(["--scene", "ascii", "--label", "XTerm/999-rc1+20260917", *args])
	return out.getvalue()

mdtable = load("mdtable", UTILITY / "include/mdtable.py")

def xterm_cells():
	return next((c for c in map(mdtable.split_row, readme.read_text(encoding="utf-8").splitlines())
	             if len(c) > 1 and c[1] == "XTerm"), [])

def xterm_row():
	return " | ".join(xterm_cells())

## Every line of the table has a leading pipe and no trailing one, the columns
## line up, and each column's alignment is spelled out.
def table_is_tidy():
	text = readme.read_text(encoding="utf-8")
	block = text.split(tb.README_BEGIN, 1)[1].split(tb.README_END, 1)[0]
	lines = [ln for ln in block.splitlines() if ln.startswith("|")]
	pipes = {tuple(i for i, ch in enumerate(ln) if ch == "|") for ln in lines}
	rule = mdtable.split_row(lines[1])
	return (len(lines) > 2 and not any(ln.rstrip().endswith("|") for ln in lines)
	        and len(pipes) == 1 and all(c.startswith(":") or c.endswith(":") for c in rule))

before = xterm_row()
out = bench("--quick")
check("a quick run leaves the table alone", xterm_row() == before, xterm_row())
check("and says why", "not touched: a quick run" in out, out)
out = bench("--scale", "0.5")
check("a scaled run leaves the table alone", xterm_row() == before, xterm_row())
grid[0] = (100, 30)
out = bench()
check("a run at another grid leaves the table alone", xterm_row() == before, xterm_row())
check("and names the grid", "grid 100x30" in out, out)
grid[0] = (160, 42)
bench("--history", "--quick")
check("a quick history refresh leaves the table alone", xterm_row() == before, xterm_row())
bench()
check("a full run at the table's grid still writes its row", "999-rc1" in xterm_cells(), xterm_row())
check("and its version keeps the prerelease tag, not the stamp", "999-rc1" in xterm_cells(), xterm_row())
check("and the table it writes lines up, with no trailing pipes", table_is_tidy())

## showdown-readme.py writes the size cells into the same table the same way.
sr = load("showdown_readme", UTILITY / "include/showdown-readme.py")
text, err = sr.update(readme, "XTerm", 123456.7, 8.9)
readme.write_text(text or "", encoding="utf-8")
check("the size writer updates its row", not err and "123456.7" in xterm_cells() and "8.9" in xterm_cells(), err or xterm_row())
check("and lays the table out again for the wider cell", table_is_tidy())

## The writer puts back a table it has nothing new for exactly as it was.
block = original.split(tb.README_BEGIN, 1)[1].split(tb.README_END, 1)[0]
again = tb.readme_table(block, [])
check("an unchanged table is rewritten byte for byte",
	again == tb.README_BEGIN + block + tb.README_END)

## update-showdown.py: a quick or --any-size run passes nothing on to be written.
us = load("update_showdown", UTILITY / "update-showdown.py")
calls = []
us.run_plain = lambda cmd: calls.append(cmd) or True
us.run_capturing = lambda cmd: calls.append(cmd) or ["RESULT filedeps=1.0 mem=2.0"]
us.terminal_grid = lambda: us.SPEED_GRID

def showdown(*args):
	calls.clear()
	with redirect_stdout(io.StringIO()):
		us.main(list(args))
	return calls

def termbench_cmd(cmds):
	return next((c for c in cmds if c[1].endswith("termbench.py")), [])

def wrote_size(cmds):
	return any(c[1].endswith("showdown-readme.py") for c in cmds)

cmd = termbench_cmd(showdown("--speed-only", "--quick", "--label", "XTerm"))
check("a quick run here tells termbench not to write", "--no-readme" in cmd, str(cmd))
cmd = termbench_cmd(showdown("--speed-only", "--any-size", "--label", "XTerm"))
check("an --any-size speed run here tells termbench not to write", "--no-readme" in cmd, str(cmd))
cmd = termbench_cmd(showdown("--speed-only", "--label", "XTerm"))
check("a full speed run here may write", cmd and "--no-readme" not in cmd, str(cmd))
check("an --any-size size run writes no size figure",
	not wrote_size(showdown("--size-only", "--any-size", "--label", "XTerm")))
us.terminal_grid = lambda: us.SIZE_GRID
check("a size run at the size grid still writes its figure",
	wrote_size(showdown("--size-only", "--label", "XTerm")))

## The rigs and the wine launcher find the build under CARGO_TARGET_DIR. Nothing
## here may start a display: sway, Xvfb and xdpyinfo are stubs that fail.
target = scratch / "elsewhere"
(target / "release").mkdir(parents=True)
stubs = scratch / "stubs"
stubs.mkdir()
for name in ("sway", "Xvfb", "xdpyinfo"):
	(stubs / name).write_text("#!/bin/sh\nexit 1\n")
	(stubs / name).chmod(0o755)
env = dict(os.environ, CARGO_TARGET_DIR=str(target), PATH=f"{stubs}:{os.environ['PATH']}")
env.pop("DISPLAY", None)
env.pop("WAYLAND_DISPLAY", None)

def bash(script, *args):
	return subprocess.run(["bash", "-c", script, "bash", *args], env=env,
		capture_output=True, text=True, timeout=60)

size_rig = UTILITY / "include/sizebench-run.bash"
got = bash('source "$1" && fMain --term silkterm --settle 0', str(size_rig))
check("the size rig refuses when the build is not under CARGO_TARGET_DIR",
	got.returncode != 0 and str(target / "release/silkterm") in got.stderr, got.stderr)
check("and stops before it starts anything", "Rig" not in got.stdout, got.stdout)
fake = target / "release/silkterm"
fake.write_text("#!/bin/sh\n")
fake.chmod(0o755)
got = bash('source "$1" && fTermBinary silkterm', str(size_rig))
check("the size rig finds the build under CARGO_TARGET_DIR", got.stdout == str(fake), got.stdout)
fake.unlink()

got = bash('"$1" --term silkterm', str(UTILITY / "include/termbench-run.bash"))
check("the speed rig looks under CARGO_TARGET_DIR",
	f"no build at {fake}" in got.stderr, got.stderr)

wine = (UTILITY / "run-windows-build-via-wine.bash").read_text(encoding="utf-8")
lookup = wine.split("declare -r  mingwCc=", 1)[1].split("\n", 1)[1].split("\n)\n", 1)[0] + "\n)\n"
got = bash('repoRoot=/repo; appId=silkterm\n' + lookup + 'printf "%s" "${exeCandidates[0]}"')
check("the wine launcher looks under CARGO_TARGET_DIR",
	got.stdout == f"{target}/x86_64-pc-windows-gnu/release/silkterm.exe", got.stdout + got.stderr)
env["CARGO_TARGET_DIR"] = "tgt"
got = bash('repoRoot=/repo; appId=silkterm\n' + lookup + 'printf "%s" "${exeCandidates[0]}"')
check("and takes a relative one from the repository",
	got.stdout == "/repo/tgt/x86_64-pc-windows-gnu/release/silkterm.exe", got.stdout + got.stderr)

## Both rigs start SilkTerm on settings of their own, never the measuring account's.
## The terminal here is a stand-in that does what SilkTerm does at launch: it rewrites
## the settings file it loads and adds a PowerShell profile beside it. The speed rig
## also gets a compositor that only makes its two sockets.
account = scratch / "account"
sentinel = account / ".config/silkterm/config.shcl"
sentinel.parent.mkdir(parents=True)
sentinel.write_text("performance:\n\tautomatic: true\n\tprofile: low\n")
record = scratch / "record"
## Beside scratch rather than in it: the sockets bound here have to fit in 107
## bytes, and the temp dir may already be a long path.
runtime = Path(tempfile.mkdtemp(prefix="rt-"))
work = scratch / "work"
work.mkdir()
fakes = scratch / "fakes"
fakes.mkdir()
(fakes / "sway").write_text(f"""#!/usr/bin/env python3
import os, socket, time
run = {str(runtime)!r}
for name in (f"sway-ipc.{{os.getuid()}}.{{os.getpid()}}.sock", f"wayland-{{os.getpid()}}"):
	sock = socket.socket(socket.AF_UNIX)
	sock.bind(os.path.join(run, name))
	sock.listen(1)
	globals()[name] = sock
time.sleep(120)
""")
(fakes / "swaymsg").write_text("#!/bin/sh\nexit 0\n")
(fakes / "xdpyinfo").write_text("#!/bin/sh\nexit 0\n")
fake.write_text(f"""#!/bin/sh
{{ env; printf 'ARGS'; printf ' [%s]' "$@"; echo; }} > {record}
loaded="${{XDG_CONFIG_HOME:-$HOME/.config}}/silkterm/config.shcl"
while [ $# -gt 0 ]; do [ "$1" = --config ] && loaded="$2"; shift; done
mkdir -p "$(dirname "$loaded")" "${{XDG_CONFIG_HOME:-$HOME/.config}}/powershell"
echo "## written at launch" >> "$loaded"
echo "# block" >> "${{XDG_CONFIG_HOME:-$HOME/.config}}/powershell/Microsoft.PowerShell_profile.ps1"
if [ -n "${{GO_FILE:-}}" ]; then
	while [ ! -f "$GO_FILE" ]; do echo "42 160" > "$SIZE_FILE"; sleep 0.2; done
	echo "sync DA1" > "$OUT_FILE"
	echo "exit=0" > "$OUT_FILE.done"
	exit 0
fi
exec sleep 30
""")
for stub in (fakes / "sway", fakes / "swaymsg", fakes / "xdpyinfo", fake):
	stub.chmod(0o755)
env = dict(os.environ, CARGO_TARGET_DIR=str(target), PATH=f"{fakes}:{os.environ['PATH']}",
	HOME=str(account), XDG_CONFIG_HOME=str(account / ".config"), XDG_RUNTIME_DIR=str(runtime),
	TMPDIR=str(work), DBUS_SESSION_BUS_ADDRESS="unix:path=/nonexistent/the-accounts-own-bus")
for name in ("DISPLAY", "WAYLAND_DISPLAY", "XDG_DATA_HOME", "XDG_STATE_HOME", "XDG_CACHE_HOME"):
	env.pop(name, None)
sentinel_was = sentinel.read_bytes()

def launched():
	got = {}
	for line in record.read_text().splitlines():
		if line.startswith("ARGS"):
			got["ARGS"] = line
		elif "=" in line:
			key, value = line.split("=", 1)
			got[key] = value
	return got

def config_arg(got):
	args = got.get("ARGS", "")
	return args.split("[--config] [", 1)[1].split("]", 1)[0] if "[--config] [" in args else ""

if shutil.which("dbus-run-session") is None:
	print("  skip the speed rig's launch: no dbus-run-session here")
else:
	speed = str(UTILITY / "include/termbench-run.bash")
	for key, marker in (("silkterm", "profile: custom"), ("silkplain", "text.scrim.enabled: false")):
		record.unlink(missing_ok=True)
		got = bash('"$1" --term "$2" --no-save --reps 1 --keep', speed, key)
		## --keep leaves the stand-in compositor up as well; end that one by its pid
		kept_rig = got.stdout.split("rig: sway pid ", 1)[1].split(",", 1)[0] if "rig: sway pid " in got.stdout else ""
		if kept_rig.isdigit():
			os.kill(int(kept_rig), 15)
		check(f"the speed rig runs its {key} row through", got.returncode == 0, got.stdout[-400:] + got.stderr[-400:])
		seen = launched() if record.exists() else {}
		home = seen.get("HOME", "")
		check(f"{key} gets a home the rig made", home.startswith(str(work)) and home != str(account), home)
		check(f"{key} gets settings and data folders under it",
			all(seen.get(name, "").startswith(home + "/") for name in
				("XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_STATE_HOME", "XDG_CACHE_HOME")), str(seen))
		check(f"{key} gets a session bus of its own",
			seen.get("DBUS_SESSION_BUS_ADDRESS", "") not in ("", env["DBUS_SESSION_BUS_ADDRESS"]),
			seen.get("DBUS_SESSION_BUS_ADDRESS", ""))
		loaded = config_arg(seen)
		check(f"{key} is handed the rig's own settings file",
			loaded.startswith(str(work)) and marker in Path(loaded).read_text(), loaded)
		check(f"{key} prints the profile in force",
			"profile in force: automatic=false profile=custom" in got.stdout, got.stdout[-300:])
	check("the account's settings file is as it was", sentinel.read_bytes() == sentinel_was)
	check("and nothing was added beside it", not (account / ".config/powershell").exists())
	## --keep leaves the work folder for the checks above. Without it the rig removes it.
	for left in work.iterdir():
		shutil.rmtree(left, ignore_errors=True)
	bash('"$1" --term silkterm --no-save --reps 1', speed)
	check("the speed rig removes what it made", not any(work.iterdir()), str(list(work.iterdir())))

## The size rig's +candy row: shipped settings with the profile pinned, and the rig
## says which profile ran. A row whose profile moved is refused.
record.unlink(missing_ok=True)
got = bash('source "$1" && fMain --term silkterm --settle 1 --keep', str(size_rig))
seen = launched() if record.exists() else {}
candy = Path(seen.get("XDG_CONFIG_HOME", "/nonexistent")) / "silkterm/config.shcl"
check("the size rig's +candy row starts on the rig's settings",
	candy.is_file() and "profile: custom" in candy.read_text() and str(account) not in str(candy), str(candy))
check("and prints the profile in force",
	"profile in force: automatic=false profile=custom" in got.stdout, got.stdout[-300:] + got.stderr[-300:])
check("the account's settings file is still as it was", sentinel.read_bytes() == sentinel_was)
got = bash('source "$1"; printf "performance:\n\tautomatic: true\n\tprofile: low\n" > "$2"; fRequireCandyProfile "$2"',
	str(UTILITY / "include/bench-common.bash"), str(scratch / "stepped.shcl"))
check("a +candy row whose profile stepped down is refused",
	got.returncode != 0 and "automatic=true profile=low" in got.stderr, got.stdout + got.stderr)
fake.unlink()
kept = candy.parents[2] if candy.is_file() else None
check("the size rig keeps its folder under the temp dir it was given",
	kept is not None and kept.parent == work, str(kept))
if kept and kept.name.startswith("sizebench.") and kept.parent == work:
	shutil.rmtree(kept, ignore_errors=True)

## README note 9 names the rig behind each group of columns. Each has to be the
## rig its script really is.
note9 = next((ln for ln in original.splitlines() if ln.startswith("<sub><sup>9</sup>")), "")
speed_rig = (UTILITY / "include/termbench-run.bash").read_text(encoding="utf-8")
size_rig_text = size_rig.read_text(encoding="utf-8")
check("note 9 says the speed rig is a headless Wayland compositor",
	"headless Wayland compositor" in note9 and "WLR_BACKENDS=headless" in speed_rig)
check("note 9 says the size rig is an X server drawing in software at its grid",
	"private X server drawing in software, at a 100x30 grid" in note9
	and "Xvfb" in size_rig_text and 'grid="100x30"' in size_rig_text)

shutil.rmtree(scratch, ignore_errors=True)
shutil.rmtree(runtime, ignore_errors=True)
if failures:
	print(f"{failures} failed")
	sys.exit(1)
print("all passed")

##	History:
##		- 20260917 JC: Created.
##		- 20260918 JC: Both rigs start SilkTerm on settings of their own.
##		- 20260926 JC: Payloads, scene weights and cell rates.
##		- 20260930 JC: The size rig keeps its folder under the temp dir it was given. A
##		  shorter runtime folder.
