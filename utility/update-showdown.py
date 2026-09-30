#!/usr/bin/env python3

##	Purpose:
##		Refresh the README "Terminal showdown" table. One entry point over everything
##		that feeds it, because the parts are easy to run inconsistently: they measure
##		different things, at different grid sizes, on different displays.
##
##		Two ways in:
##
##		THIS TERMINAL, any OS. Measures whatever terminal you are sitting in - speed,
##		then size and memory - and writes its row. Needs nothing but Python 3 and a
##		tty, so it is the only way to measure the terminals that exist solely on
##		Windows or macOS. This is what you get if you name no terminal. Size the window
##		to 100x30 first, or the size half will refuse; pass --label with the table's
##		row name, or it will measure but not write.
##
##		THE RIGS, Linux only. Bring up a display, launch a named terminal into it, fit
##		it to a fixed grid, measure, tear down. Two of them:
##
##		  speed  include/termbench-run.bash   160x42, headless sway on the real GPU
##		  size   include/sizebench-run.bash   100x30, private Xvfb
##
##		The grids differ deliberately and must not be unified: the speed figure wants a
##		realistic working grid, while memory scales with the surface, so the size rows
##		are taken small and identical. The same SilkTerm binary reads 38 MiB heavier at
##		its default geometry than at 100x30.
##
##		Nothing is published unless it is comparable. Re-measure a terminal already in
##		the table before adding a new one: the rig reproduced SilkTerm's speed within
##		0.6% and its memory within 1.3%, and a figure taken any other way does not
##		belong beside the existing rows. Figures from a second machine are a separate
##		question again - see include/showdown-readme.md on calibrating one.
##
##	Usage:
##		utility/update-showdown.py                      measure this terminal
##		utility/update-showdown.py --label 'MobaXterm'  ... and write that row
##		utility/update-showdown.py --speed-only         ... speed alone, no 100x30 needed
##		utility/update-showdown.py --any-size           ... size anyway, NOT comparable
##		utility/update-showdown.py --term alacritty     drive both rigs (Linux)
##		utility/update-showdown.py --all
##		utility/update-showdown.py --term kitty --size-only
##		utility/update-showdown.py --list
##
##	History:
##		20260730 Written, to drive both shootout rigs from one place.
##		20260730 Ported from shell so it runs on Windows too, and absorbed the
##		         measure-this-terminal path, which had no wrapper before.
##		20260928 Both rigs for GNOME Terminal, WezTerm and Tabby; size for Hyper.
##		20260929 Both rigs for XTerm; its speed runs on a private X server.

import argparse
import os
import re
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
INCLUDE = os.path.join(HERE, "include")
REPO = os.path.dirname(HERE)
README = os.path.join(REPO, "README.md")

#	key: README row name, then which rigs can drive it here. A terminal the size rig has
#	no recipe for still gets its speed row; the size columns are left as they were.
TERMS = [
	("silkterm",   "SilkTerm +candy", "both"),
	("silkplain",  "SilkTerm plain",  "both"),
	("alacritty",  "Alacritty",       "both"),
	("kitty",      "kitty",           "both"),
	("xfce4",      "XFCE4 Terminal",  "both"),
	("terminator", "Terminator",      "both"),
	("xterm",      "XTerm",           "both"),
	("gnome",      "GNOME Terminal",  "both"),
	("wezterm",    "WezTerm",         "both"),
	("tabby",      "Tabby",           "both"),
	#	Never answers the speed rig's barrier, so it has no speed figure to take.
	("hyper",      "Hyper",           "size"),
]

LETTERBOX = "-" * 78

#	The grids the two halves of the table are measured at. They differ on purpose - speed
#	wants a realistic working grid, memory has to be pinned small and identical - so one
#	window cannot serve both, and measuring at the wrong one quietly produces a figure that
#	does not belong in the column.
SPEED_GRID = (160, 42)
SIZE_GRID = (100, 30)


#•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
#	Output
#•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••

#	Matches the house style of cicd.bash and the rigs: a bracketed status line, repeat
#	blanks collapsed, so the blank-line rhythm does the visual grouping.
_last_blank = [False]


def echo_clean(text=""):
	if text:
		print(text)
		_last_blank[0] = False
	elif not _last_blank[0]:
		print()
		_last_blank[0] = True


def echo(text=""):
	echo_clean("[ %s ]" % text if text else "")


def section(text):
	echo_clean()
	echo_clean(LETTERBOX)
	echo(text)


def die(text):
	echo_clean()
	sys.stdout.flush()                             ## or the reason appears above its own output
	print("[ FAILED: %s ]" % text, file=sys.stderr)
	sys.exit(1)


#•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
#	Running the parts
#•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••

def run_plain(cmd):
	"""Run a child on this terminal's own stdio, and say whether it worked.

	The throughput tool stops its clock on the terminal's reply, so its stdout has to
	stay on the tty. Capturing it would time a pipe instead.
	"""
	try:
		return subprocess.call(cmd) == 0
	except OSError as err:
		echo("WARNING: could not run %s: %s" % (os.path.basename(cmd[0]), err))
		return False


def run_capturing(cmd):
	"""Run a child, echoing its output as it arrives, and return the lines.

	The rig's own output is the record of what was measured, so it stays on screen; the
	summary line is picked out of the same stream afterwards.
	"""
	try:
		proc = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
		                        text=True, bufsize=1)
	except OSError as err:
		echo("WARNING: could not run %s: %s" % (os.path.basename(cmd[0]), err))
		return []
	lines = []
	for line in proc.stdout:
		sys.stdout.write(line)
		sys.stdout.flush()
		lines.append(line.rstrip("\n"))
	proc.wait()
	_last_blank[0] = False
	return lines


def terminal_grid():
	"""(columns, rows) of this window, or None."""
	for stream in (sys.__stdout__, sys.__stderr__, sys.__stdin__):
		try:
			size = os.get_terminal_size(stream.fileno())
			return (size.columns, size.lines)
		except (OSError, ValueError, AttributeError):
			continue
	return None


def speed_grid_ok(any_size):
	"""The throughput tool measures whatever window it is given, so check here.

	The rig fits every terminal to the same grid before measuring; on this path there is no
	rig to do it, and a run at another size records a figure that looks fine and is not
	comparable with the rows beside it.
	"""
	got = terminal_grid()
	if got == SPEED_GRID:
		return True
	shown = "%dx%d" % got if got else "unknown"
	msg = ("speed rows are measured at %dx%d and this window is %s"
	       % (SPEED_GRID[0], SPEED_GRID[1], shown))
	if any_size:
		echo("WARNING: %s - the figure will not be comparable" % msg)
		return True
	echo("SKIPPED: %s - resize and run again" % msg)
	return False


def measure_here(reps, quick, label, write_readme, publish):
	"""Measure the terminal this is running inside."""
	cmd = [sys.executable, os.path.join(INCLUDE, "termbench.py")]
	if quick:
		cmd.append("--quick")
	else:
		cmd += ["--reps", str(reps)]
	if label:
		cmd += ["--label", label]
	if not write_readme:
		#	Same meaning as on the rig path: measure and print, record nothing anywhere.
		cmd.append("--no-save")
	elif not publish:
		#	Kept in the tool's own history, where a quick run has its own table.
		cmd.append("--no-readme")
	return run_plain(cmd)


def read_result(lines):
	"""The rig's RESULT line as a dict of floats, or None."""
	line = next((l for l in lines if l.startswith("RESULT ")), "")
	if not line:
		return None
	got = dict(re.findall(r"(\w+)=([0-9.]+)", line))
	if "filedeps" not in got or "mem" not in got:
		return None
	return {k: float(v) for k, v in got.items()}


def write_size_row(row, file_deps, mem):
	wrote = run_plain([sys.executable, os.path.join(INCLUDE, "showdown-readme.py"),
	                   "--readme", README, "--terminal", row,
	                   "--file-deps", "%.1f" % file_deps, "--mem", "%.1f" % mem])
	if not wrote:
		echo("WARNING: could not write the %s row" % row)
	return wrote


def size_here(label, publish, any_size):
	"""Size and memory of the terminal this is running inside.

	The row has to be named to be written. Guessing it from the executable would quietly
	put a figure in the wrong row on the terminals that share a family name, and a wrong
	row is worse than a missing one.
	"""
	cmd = [sys.executable, os.path.join(INCLUDE, "sizebench-classify.py"),
	       "--here", "--summary"]
	if any_size:
		cmd.append("--any-size")
	got = read_result(run_capturing(cmd))
	if not got:
		echo("WARNING: no size measurement came back")
		return False
	if not publish:
		return True
	if not label:
		echo("NOTE: pass --label with the table's row name to write these into the README")
		return True
	return write_size_row(label, got["filedeps"], got["mem"])


def measure_speed(key, row, reps, write_readme):
	cmd = [os.path.join(INCLUDE, "termbench-run.bash"),
	       "--term", key, "--reps", str(reps), "--label", row]
	if not write_readme:
		cmd.append("--no-save")
	if not run_plain(cmd):
		echo("WARNING: speed run failed for %s" % key)


def measure_size(key, row, write_readme):
	"""File+deps and Mem in MiB, or None if the rig could not say."""
	got = read_result(run_capturing(
		[os.path.join(INCLUDE, "sizebench-run.bash"), "--term", key]))
	if not got:
		echo("WARNING: no usable result from the size rig for %s" % key)
		return None
	if write_readme:
		write_size_row(row, got["filedeps"], got["mem"])
	return got["filedeps"], got["mem"]


#•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
#	Entry
#•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••

def header_text():
	"""The comment header, which is also the help text.

	Read by prefix rather than searched for a first line: a pattern matching that line
	also matches itself, which is how the shell version printed its own source twice.
	"""
	out = []
	with open(os.path.abspath(__file__)) as fh:
		next(fh)                                   ## the interpreter line
		for line in fh:
			if line.startswith("##"):
				out.append(line[2:].rstrip())
			elif out:
				break
	return "\n".join(out).strip("\n")


def show_list():
	echo_clean("  key          README row                rigs")
	for key, row, rigs in TERMS:
		echo_clean("  %-12s %-25s %s" % (key, row, rigs))


def main(argv):
	ap = argparse.ArgumentParser(add_help=False)
	ap.add_argument("--term", action="append", default=[], metavar="KEY")
	ap.add_argument("--all", action="store_true")
	ap.add_argument("--here", action="store_true")
	ap.add_argument("--speed-only", action="store_true")
	ap.add_argument("--size-only", action="store_true")
	ap.add_argument("--reps", type=int, default=6, metavar="N")
	ap.add_argument("--quick", action="store_true")
	ap.add_argument("--any-size", action="store_true")
	ap.add_argument("--label", default="", metavar="NAME")
	ap.add_argument("--no-readme", action="store_true")
	ap.add_argument("--list", action="store_true")
	ap.add_argument("-h", "--help", action="store_true")
	args = ap.parse_args(argv)

	if args.help:
		print(header_text())
		return 0
	if args.list:
		show_list()
		return 0

	keys = [k for k, _, _ in TERMS] if args.all else list(args.term)
	if args.here and keys:
		die("--here measures the terminal you are in, so it takes no --term")

	write_readme = not args.no_readme

	#	Naming no terminal means the measurements that need no rig, which are also the only
	#	ones available off Linux.
	if not keys:
		do_speed = not args.size_only
		do_size = not args.speed_only

		#	One window cannot be both grids, so asking for both means doing whichever this
		#	window is already set up for rather than silently taking one of them wrong.
		if do_speed and do_size and not args.any_size:
			got = terminal_grid()
			if got == SPEED_GRID:
				do_size = False
			elif got == SIZE_GRID:
				do_speed = False
			else:
				die("this window is %s. Speed is measured at %dx%d and size and memory at "
				    "%dx%d, so set it to one of those and run again"
				    % ("%dx%d" % got if got else "not a terminal",
				       SPEED_GRID[0], SPEED_GRID[1], SIZE_GRID[0], SIZE_GRID[1]))

		#	A quick run and an --any-size one are for looking, and the table only takes
		#	figures measured the way the rows beside them were.
		publish = write_readme and not args.quick and not args.any_size
		ok = True
		if do_speed:
			if args.quick:
				echo("NOTE: a quick run is aggregated separately and never reaches the table")
			section("Speed: this terminal")
			ok = speed_grid_ok(args.any_size) and measure_here(
				args.reps, args.quick, args.label, write_readme, publish)
		if do_size:
			section("Size and memory: this terminal")
			ok = size_here(args.label, publish, args.any_size) and ok
		echo_clean()
		if publish and ok:
			echo("README updated - check the diff before committing")
		elif write_readme and not publish:
			echo("README not touched - a %s run is not comparable with the table"
			     % ("quick" if args.quick else "--any-size"))
		else:
			echo("nothing written")
		echo_clean()
		return 0 if ok else 1

	if not sys.platform.startswith("linux"):
		die("the rigs need a Linux display; name no terminal to measure this one")

	known = {key: (row, rigs) for key, row, rigs in TERMS}
	for key in keys:
		if key not in known:
			show_list()
			die("unknown key '%s'" % key)

	measured = []
	for key in keys:
		row, rigs = known[key]
		if not args.size_only and rigs in ("both", "speed"):
			section("Speed: %s" % row)
			measure_speed(key, row, args.reps, write_readme)
		if not args.speed_only and rigs in ("both", "size"):
			section("Size and memory: %s" % row)
			got = measure_size(key, row, write_readme)
			if got:
				measured.append((row,) + got)

	if measured:
		section("Size and memory measured")
		echo_clean("  %-25s %10s %10s" % ("Terminal", "File+deps", "Mem"))
		for row, file_deps, mem in measured:
			echo_clean("  %-25s %10.1f %10.1f" % (row, file_deps, mem))

	echo_clean()
	echo("README updated - check the diff before committing" if write_readme
	     else "nothing written (--no-readme)")
	echo_clean()
	return 0


if __name__ == "__main__":
	sys.exit(main(sys.argv[1:]))
