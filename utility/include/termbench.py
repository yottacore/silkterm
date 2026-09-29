#!/usr/bin/env python3

##	Purpose:
##		Measure how fast a terminal emulator actually draws text. Runs on any
##		terminal, any OS, needs nothing but Python 3.8+ and a tty.
##
##		Five scenes, each a long stream of one UTF-8 width class, because that is
##		what separates a fast terminal from a slow one: 1-byte ASCII takes the
##		renderer's fast path, 2-byte stays in the primary font, 3-byte goes
##		double-width, and 4-byte emoji fall out of the mono font entirely into
##		fallback and color-glyph paths. A fifth scene mixes all four with color
##		and attribute changes. ASCII is tested four times as often as the wide
##		classes and 2-byte twice, so the overall score leans the way real output
##		does.
##
##		Two things make the numbers mean something:
##
##		SYNCHRONIZED TIMING. write() returns when the pty accepted the bytes, not
##		when the terminal drew them, so timing a bare write measures the kernel
##		buffer and reports nonsense (a terminal that reads greedily and renders
##		later looks infinitely fast). Every run therefore ends with a Primary DA
##		query and the clock stops on the reply, which a terminal can only send
##		after parsing everything queued before it. Caveat worth knowing: that is
##		a parser barrier. Terminals that render on another thread can still have
##		frames in flight when they answer. Reps run back to back with no gap, so
##		deferred work falls inside the next rep and comes out in the average, but
##		a single rep can flatter an asynchronous renderer.
##
##		ON WINDOWS THE BARRIER IS ANSWERED BY THE CONSOLE HOST, NOT THE TERMINAL.
##		ConPTY parses the child's output itself and replies on its own account:
##		measured, the reply is a DEC-level "ESC[?61;..." and the query never
##		reaches the terminal at all, so a terminal that would have answered
##		"ESC[?6c" never sees one. A Windows figure is therefore the whole chain
##		- console host plus terminal, coupled through the pipe between them -
##		and not the terminal on its own. It also puts a low ceiling on every
##		Windows row: a consumer that reads the bytes and throws them away
##		measures the same figure as a real terminal, to within a percent.
##
##		CELLS PER SECOND, not just MB/s. Megabytes reward ASCII for being narrow
##		and flatter emoji for being wide: at one point 2-byte text measured
##		faster than ASCII in MB/s while being slower per character. Cells per
##		second says which actually moved more screen, and the score uses it.
##
##		WHAT THIS IS, PRECISELY: throughput under flood - how fast a terminal
##		swallows output and keeps up. It is not a glyph rasterization rate. Only
##		a screenful of cells is ever visible, so most of the stream is parsed,
##		stored and scrolled past without being drawn, and terminals that render
##		on another thread overlap what drawing they do with the next read. That
##		is the honest shape of the question "why does it bog down when something
##		dumps a lot of text", which is what this exists to answer.
##
##		The payload is deterministic: identical bytes on every machine and every
##		run, from a fixed seed, so two terminals are always compared on exactly
##		the same work. It is generated into memory before the clock starts and
##		written in 1 MB blocks, so the harness is never the thing being measured
##		- the report proves that with a write-to-null ceiling.
##
##		Results append to a data file under the user's data directory and build a
##		history table: every terminal that has ever been benchmarked here, by name
##		and version, newest five builds each. Where the tool sits in a SilkTerm
##		checkout it also refreshes the speed columns of the shootout table in the
##		README, latest build per terminal only, leaving the size and memory
##		columns beside them untouched.
##
##		Deliberately not tested: combining marks, ZWJ emoji sequences, variation
##		selectors and RTL. All four make the cell count ambiguous, and an
##		ambiguous denominator is worse than a missing scene.
##
##	Usage:
##		Normally reached through utility/update-showdown.py, which runs this in the
##		terminal you are sitting in and then writes the row. Standalone:
##
##		utility/include/termbench.py                 full run, about two minutes
##		utility/include/termbench.py --quick         about thirty seconds
##		utility/include/termbench.py --history       print the table, measure nothing
##		utility/include/termbench.py --label 'name/build'
##
##	History:
##		20260728 Initial.
##		20260730 Moved under utility/include/, behind update-showdown.py.
##		20260928 WezTerm's and GNOME Terminal's versions come from their launchers.

import argparse
import array
import hashlib
import json
import math
import os
import platform
import re
import shutil
import statistics
import subprocess
import sys
import time
from datetime import datetime, timezone

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import mdtable  # noqa: E402

APP = "silkterm-bench"
PAYLOAD_VERSION = 1
DATA_FILE = "results.jsonl"

README_BEGIN = "<!-- termbench:begin -->"
README_END = "<!-- termbench:end -->"
# The table's rows were all taken this way, and a figure taken any other way
# does not belong beside them. update-showdown.py's SPEED_GRID is the same grid.
TABLE_GRID = "160x42"
TABLE_LINE_CELLS = 80

MB = 1024 * 1024

# Chunk handed to each write(). Big enough that per-call overhead vanishes,
# small enough to stay off the terminal's back with one giant blocking write.
WRITE_CHUNK = 1 << 20

# How long to wait for the device-attributes reply that ends a run. Generous:
# a terminal that buffers aggressively can still be parsing long after the
# last write returned.
SYNC_TIMEOUT = 180.0

# Palette of pre-rendered characters the payload is assembled from. Large
# enough that assembled lines never repeat, small enough to build instantly.
PALETTE_CHARS = 1 << 18


#•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
#	Alphabets
#
#	Each scene draws from one width class. Widths are declared here rather
#	than measured, so the cell count is exact without a wcwidth table.
#•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••

# (first, last) inclusive codepoint spans.
ASCII_SPANS = [(0x21, 0x7E)]

LATIN_SPANS = [
	(0x00C0, 0x00D6), (0x00D8, 0x00F6), (0x00F8, 0x00FF),  # Latin-1 letters
	(0x0100, 0x017F),                                       # Latin Extended-A
	(0x0391, 0x03A9), (0x03B1, 0x03C9),                     # Greek
	(0x0410, 0x044F),                                       # Cyrillic
]

# A realistic working set rather than the whole of Unified Han: real CJK text
# reuses a few thousand characters, and a glyph cache should get some hits.
CJK_SPANS = [
	(0x4E00, 0x5FFF),                   # common Han
	(0x3041, 0x3096),                   # hiragana
	(0x30A1, 0x30FA),                   # katakana
	(0xAC00, 0xACFF),                   # hangul syllables
]

# Single-codepoint, default-emoji-presentation, all covered by Noto Color
# Emoji and its peers. No sequences, so every one is exactly two cells.
EMOJI_SPANS = [
	(0x1F300, 0x1F320), (0x1F330, 0x1F393), (0x1F3A0, 0x1F3CA),
	(0x1F400, 0x1F43E), (0x1F440, 0x1F4FC), (0x1F600, 0x1F64F),
	(0x1F680, 0x1F6C5), (0x1F910, 0x1F93A), (0x1F950, 0x1F96B),
	(0x1F980, 0x1F991),
]

# Narrow 3-byte symbols. Only used by the mixed scene, where an exact
# per-character cell count is kept anyway.
SYMBOL_SPANS = [
	(0x2500, 0x257F),                   # box drawing
	(0x2190, 0x21FF),                   # arrows
	(0x2200, 0x22FF),                   # math
]


def _expand(spans):
	out = []
	for lo, hi in spans:
		out.extend(chr(c) for c in range(lo, hi + 1))
	return out


def _alphabet(spans, sep, sep_share):
	"""Characters plus a separator repeated until it is sep_share of the whole."""
	base = _expand(spans)
	if not sep:
		return base
	pad = max(1, int(len(base) * sep_share / (1.0 - sep_share)))
	return base + [sep] * pad


# name -> (alphabet, bytes-per-char, cells-per-char)
CLASSES = {
	"ascii": (_alphabet(ASCII_SPANS, " ", 0.17), 1, 1),
	"latin": (_alphabet(LATIN_SPANS, "\u00a0", 0.17), 2, 1),   # NBSP keeps it 2-byte
	"cjk":   (_alphabet(CJK_SPANS, "\u3000", 0.10), 3, 2),     # ideographic space
	"emoji": (_alphabet(EMOJI_SPANS, None, 0), 4, 2),          # no 4-byte space exists
	"symbol": (_alphabet(SYMBOL_SPANS, None, 0), 3, 1),
}


#•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
#	Scenes
#•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••

class Scene:
	def __init__(self, name, label, weight, megabytes):
		self.name = name
		self.label = label
		self.weight = weight
		self.megabytes = megabytes


# Sized individually, not by one shared scale, so that a single rep of each
# costs about the same wall time: emoji run roughly twenty times slower per
# byte than ASCII, so equal payloads would spend the whole run on emoji. The
# figures come from measuring xfce4-terminal and put a full run near two
# minutes there; --scale moves all of them together.
SCENES = [
	Scene("ascii", "1-byte", 4, 100),
	Scene("latin", "2-byte", 2, 85),
	Scene("cjk",   "3-byte", 1, 50),
	Scene("emoji", "4-byte", 1, 50),
	Scene("mixed", "mixed",  1, 44),
]

# Runs per scene are this times the scene's weight, so ASCII is measured four
# times as often as the wide classes. Both modes use the same payloads, so the
# rates are directly comparable and only the confidence differs.
REPS_FULL = 12
REPS_QUICK = 3

SCENE_BY_NAME = {s.name: s for s in SCENES}


#•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
#	Payload generation
#
#	Deterministic and fast. shake_128 gives a reproducible byte stream on every
#	platform; everything downstream is C-level slicing and joining, so a 24 MB
#	payload builds in well under a second and none of it is inside the clock.
#•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••

def _rand(seed, nbytes):
	return hashlib.shake_128(seed.encode("utf-8")).digest(nbytes)


def _u16(seed, count):
	"""count deterministic 16-bit values, byte order normalized."""
	a = array.array("H")
	a.frombytes(_rand(seed, count * 2))
	if sys.byteorder != "little":
		a.byteswap()
	return a


def _palette(seed, alphabet, size):
	n = len(alphabet)
	idx = _u16(seed + ":pal", size)
	return "".join([alphabet[i % n] for i in idx])


def _chars(seed, alphabet, count):
	"""A deterministic run of `count` characters drawn from `alphabet`."""
	size = min(PALETTE_CHARS, max(count, 1024))
	pal = _palette(seed, alphabet, size)
	if count <= len(pal):
		return pal[:count]
	# Assemble from varying-length slices so no line ever repeats another.
	cuts = _u16(seed + ":cut", (count // 200) + 64)
	parts, have, k, span = [], 0, 0, len(pal)
	while have < count:
		take = 192 + (cuts[k % len(cuts)] % 128)
		start = cuts[(k + 1) % len(cuts)] * 4 % max(1, span - take)
		piece = pal[start:start + take]
		parts.append(piece)
		have += len(piece)
		k += 2
	return "".join(parts)[:count]


def _wrap(text, per_line):
	"""Break into fixed-width lines. CRLF because raw mode gives no ONLCR."""
	lines = [text[i:i + per_line] for i in range(0, len(text), per_line)]
	return ("\r\n".join(lines) + "\r\n").encode("utf-8")


def _build_uniform(cls, target_bytes, line_cells):
	alphabet, wbytes, wcells = CLASSES[cls]
	per_line = max(1, line_cells // wcells)
	nchars = max(per_line, target_bytes // wbytes)
	nchars -= nchars % per_line
	text = _chars("silkterm-bench-v%d-%s" % (PAYLOAD_VERSION, cls), alphabet, nchars)
	blob = _wrap(text, per_line)
	nlines = nchars // per_line
	return blob, nchars, nchars * wcells, nlines


# Foreground colors cycled through the mixed scene. Chosen to include the
# 16-color, 256-color and 24-bit paths, since terminals treat them
# differently, plus bold and reverse for the attribute path.
MIXED_SGR = [
	"\x1b[0m", "\x1b[1m", "\x1b[31m", "\x1b[1;32m", "\x1b[38;5;208m",
	"\x1b[38;5;45m", "\x1b[38;2;255;128;0m", "\x1b[38;2;80;200;255m",
	"\x1b[7m", "\x1b[0;36m",
]


# Share of each line's cells given to each width class. Roughly the shape of
# real polyglot output, and fixed shares are what guarantee every class
# actually appears - an earlier draft ordered segments by hand and the emoji
# never fitted, so the mixed scene quietly tested no 4-byte characters at all.
MIXED_BUDGET = [
	("ascii", 0.50), ("latin", 0.15), ("symbol", 0.08),
	("cjk", 0.19), ("emoji", 0.08),
]


def _build_mixed(target_bytes, line_cells):
	"""
	Every line: an SGR change, then a segment of each width class sized from the
	budget above, in a rotating order, then a reset. Cells are accumulated as the
	line is assembled because segment widths differ.
	"""
	pools = {}
	for cls, _ in MIXED_BUDGET:
		alphabet, _, _ = CLASSES[cls]
		pools[cls] = _chars("silkterm-bench-v%d-mixed-%s" % (PAYLOAD_VERSION, cls),
		                    alphabet, PALETTE_CHARS)

	# Leave room for the separators the segments are joined with.
	budget = max(1, int(line_cells * 0.93))
	plan = []
	for cls, share in MIXED_BUDGET:
		_, _, wcells = CLASSES[cls]
		take = max(1, int(round(budget * share)) // wcells)
		plan.append((cls, take, wcells))

	jitter = _u16("silkterm-bench-v%d-mixed-mix" % PAYLOAD_VERSION, 1 << 16)
	out, cells, chars, nlines, total, j = [], 0, 0, 0, 0, 0

	while total < target_bytes:
		line, used = [], 0
		line.append(MIXED_SGR[jitter[j % len(jitter)] % len(MIXED_SGR)])
		j += 1
		spin = jitter[j % len(jitter)] % len(plan)
		j += 1
		for step in range(len(plan)):
			cls, take, wcells = plan[(spin + step) % len(plan)]
			pool = pools[cls]
			start = (jitter[j % len(jitter)] * 7) % max(1, len(pool) - take)
			j += 1
			line.append(pool[start:start + take])
			used += take * wcells
			chars += take
			if step < len(plan) - 1:
				line.append(" ")
				used += 1
				chars += 1
		line.append("\x1b[0m")
		blob = ("".join(line) + "\r\n").encode("utf-8")
		out.append(blob)
		total += len(blob)
		cells += used
		nlines += 1

	return b"".join(out), chars, cells, nlines


def build_payload(scene, scale, line_cells):
	target = int(scene.megabytes * MB * scale)
	target = max(target, 256 * 1024)
	if scene.name == "mixed":
		return _build_mixed(target, line_cells)
	return _build_uniform(scene.name, target, line_cells)


#•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
#	Console: raw mode, blast, and the synchronization barrier
#•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••

class Console:
	"""Raw tty on POSIX, raw console on Windows, restored on the way out."""

	def __init__(self):
		self.ok = False
		self._posix = None
		self._win = None

	def __enter__(self):
		try:
			if os.name == "posix":
				self._enter_posix()
			elif os.name == "nt":
				self._enter_windows()
			self.ok = True
		except Exception:
			self.ok = False
		return self

	def __exit__(self, *exc):
		try:
			if self._posix:
				import termios
				fd, saved = self._posix
				termios.tcsetattr(fd, termios.TCSADRAIN, saved)
			elif self._win:
				import ctypes
				k = ctypes.windll.kernel32
				hin, hout, min_, mout = self._win
				k.SetConsoleMode(hin, min_)
				k.SetConsoleMode(hout, mout)
		except Exception:
			pass
		return False

	def _enter_posix(self):
		import termios
		import tty
		fd = sys.stdin.fileno()
		saved = termios.tcgetattr(fd)
		self._posix = (fd, saved)
		tty.setraw(fd, termios.TCSANOW)

	def _enter_windows(self):
		import ctypes
		import msvcrt
		k = ctypes.windll.kernel32
		hin = k.GetStdHandle(-10)
		hout = k.GetStdHandle(-11)
		min_ = ctypes.c_uint()
		mout = ctypes.c_uint()
		k.GetConsoleMode(hin, ctypes.byref(min_))
		k.GetConsoleMode(hout, ctypes.byref(mout))
		self._win = (hin, hout, min_.value, mout.value)
		# Off: line input, echo, processed input. On: raw VT in and out.
		k.SetConsoleMode(hin, (min_.value & ~0x0002 & ~0x0004 & ~0x0001) | 0x0200)
		k.SetConsoleMode(hout, mout.value | 0x0004)
		msvcrt.setmode(sys.stdout.fileno(), os.O_BINARY)

	# -- output ---------------------------------------------------------------

	@staticmethod
	def write(blob):
		fd = sys.stdout.fileno()
		mv = memoryview(blob)
		off, n = 0, len(blob)
		while off < n:
			off += os.write(fd, mv[off:off + WRITE_CHUNK])

	@staticmethod
	def emit(text):
		Console.write(text.encode("utf-8"))

	# -- input ----------------------------------------------------------------

	def drain(self):
		"""Throw away anything already pending so a stale reply can't fool us."""
		deadline = time.monotonic() + 0.08
		while time.monotonic() < deadline:
			if not self._read(0.01):
				break

	def _read(self, timeout):
		if os.name == "posix":
			import select
			r, _, _ = select.select([sys.stdin.fileno()], [], [], timeout)
			if not r:
				return b""
			try:
				return os.read(sys.stdin.fileno(), 4096)
			except OSError:
				return b""
		import msvcrt
		deadline = time.monotonic() + timeout
		got = b""
		while time.monotonic() < deadline:
			if msvcrt.kbhit():
				got += msvcrt.getch()
				while msvcrt.kbhit():
					got += msvcrt.getch()
				return got
			time.sleep(0.001)
		return got

	def query(self, request, terminator, timeout, drain=True):
		"""Send a query, collect the reply up to `terminator`. b'' on timeout."""
		if drain:
			self.drain()
		self.emit(request)
		got = b""
		deadline = time.monotonic() + timeout
		while time.monotonic() < deadline:
			chunk = self._read(min(0.25, max(0.01, deadline - time.monotonic())))
			if chunk:
				got += chunk
				if terminator in got:
					return got
		return b""

	def sync(self, timeout=SYNC_TIMEOUT, drain=True):
		"""
		Primary DA. The terminal cannot answer until it has parsed everything
		queued ahead of it, which is what turns a write into a measurement.
		Pass drain=False inside a timed section: draining first costs a poll
		timeout, which on a short scene is several percent of the measurement.
		"""
		return bool(self.query("\x1b[c", b"c", timeout, drain))


def terminal_size():
	try:
		size = shutil.get_terminal_size()
		return size.columns, size.lines
	except Exception:
		return 0, 0


#•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
#	Which terminal is this
#•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••

SYSTEM_PREFIXES = ("/usr/", "/bin/", "/sbin/", "/opt/", "/snap/",
                   "c:\\program files", "c:\\windows")

VERSION_RE = re.compile(r"(\d+\.\d+(?:\.\d+)?(?:[-+.\w]*)?)")
VERSION_PAREN_RE = re.compile(r"\((\d+)\)")
# WezTerm's release names: date, time and commit, of which the table shows the date.
VERSION_DATE_RE = re.compile(r"\b(20\d{6})-\d{6}-[0-9a-f]{6,}\b")
STAMP_RE = re.compile(r"(\d{8}-\d{6})")


def _exe_of(pid):
	try:
		if sys.platform.startswith("linux"):
			return os.readlink("/proc/%d/exe" % pid)
		out = subprocess.run(["ps", "-o", "comm=", "-p", str(pid)],
		                     capture_output=True, text=True, timeout=3)
		return out.stdout.strip()
	except Exception:
		return ""


def _ppid_of(pid):
	try:
		if sys.platform.startswith("linux"):
			with open("/proc/%d/stat" % pid, "r") as fh:
				data = fh.read()
			# comm can contain spaces and parens, so start after the last ')'.
			return int(data[data.rfind(")") + 2:].split()[1])
		out = subprocess.run(["ps", "-o", "ppid=", "-p", str(pid)],
		                     capture_output=True, text=True, timeout=3)
		return int(out.stdout.strip())
	except Exception:
		return 0


SHELLISH = {"bash", "sh", "dash", "zsh", "fish", "ksh", "csh", "tcsh", "python",
            "python3", "pwsh", "powershell", "cmd", "conhost", "login", "su",
            "sudo", "env", "termbench.py", "screen", "script"}


def _ancestor_terminal():
	"""Walk up the process tree until something that is not a shell shows up."""
	pid = os.getpid()
	for _ in range(12):
		pid = _ppid_of(pid)
		if pid <= 1:
			return ""
		exe = _exe_of(pid)
		if not exe:
			continue
		base = os.path.basename(exe).lower()
		if base.endswith(".exe"):
			base = base[:-4]
		if base in SHELLISH:
			continue
		return exe
	return ""


def _run_version(exe, flag):
	try:
		out = subprocess.run([exe, flag], capture_output=True, text=True,
		                     timeout=5, stdin=subprocess.DEVNULL)
		line = (out.stdout or out.stderr).strip().splitlines()
		return line[0].strip() if line else ""
	except Exception:
		return ""


def _clean_name(text):
	"""Trim a version banner's leading words down to just the product name."""
	name = text.strip().strip("#:-()\t ")
	name = re.sub(r"[\s:(-]*\bversion\b[\s:(-]*$", "", name, flags=re.I)
	return name.strip("#:-()\t ")[:40]


def _probe_version(exe, base):
	"""
	(name, version) from the program itself. Only output carrying a real version
	token is believed: xterm answers --version with 'bad command line option' and
	terminator with a complaint about DISPLAY, and taking either at face value
	would put an error message in the results table as the terminal's name.
	"""
	for flag in ("--version", "-version"):
		raw = _run_version(exe, flag)
		if not raw:
			continue
		got = VERSION_RE.search(raw)
		if got:
			return (_clean_name(raw.split(got.group(1))[0]) or base), got.group(1)
		# Versions with no dot at all, as in xterm's "XTerm(407)".
		got = VERSION_PAREN_RE.search(raw)
		if got:
			return (_clean_name(raw.split("(")[0]) or base), got.group(1)
		got = VERSION_DATE_RE.search(raw)
		if got:
			return (_clean_name(raw.split(got.group(0))[0]) or base), got.group(1)
	return base, ""


def _stamp_for(exe):
	got = STAMP_RE.search(os.path.basename(exe))
	if got:
		return got.group(1)
	try:
		return datetime.fromtimestamp(os.path.getmtime(exe)).strftime("%Y%m%d-%H%M%S")
	except Exception:
		return ""


def _is_system(exe):
	low = exe.lower().replace("\\", "/")
	return any(low.startswith(p.replace("\\", "/")) for p in SYSTEM_PREFIXES)


def _silkterm_exe():
	"""SilkTerm exports its control socket, and the socket carries its pid."""
	sock = os.environ.get("SILKTERM_SOCKET", "")
	got = re.search(r"silkterm-ctl-(\d+)\.sock", sock)
	if not got:
		return ""
	return _exe_of(int(got.group(1)))


def identify(console):
	"""Best available (name, build, exe). Falls back rather than guessing wrong."""
	exe = _silkterm_exe() or _ancestor_terminal()

	name, version = "", ""
	if exe:
		base = os.path.basename(exe)
		if base.endswith(".exe"):
			base = base[:-4]
		# "xfce4-terminal 1.2.0 (Xfce 4.20)" -> name from the leading words.
		name, version = _probe_version(exe, base)
		# wezterm-gui answers --version with a placeholder and gnome-terminal-server not
		# at all. The command that starts each one knows.
		for suffix in ("-gui", "-server"):
			if version or not base.endswith(suffix):
				continue
			launcher = base[:-len(suffix)]
			beside = os.path.join(os.path.dirname(exe), launcher)
			found = beside if os.path.isfile(beside) else shutil.which(launcher)
			if found:
				name, version = _probe_version(found, launcher)

	if not name:
		# XTVERSION: not everything answers, but when it does it is definitive.
		reply = console.query("\x1b[>0q", b"\x1b\\", 0.35) if console.ok else b""
		got = re.search(rb"\x1bP>\|([^\x1b]+)", reply)
		if got:
			text = got.group(1).decode("utf-8", "replace").strip()
			ver = VERSION_RE.search(text)
			version = ver.group(1) if ver else ""
			name = (text.split(version)[0].strip() if version else text) or "unknown"

	if not name:
		for key, label in (("TERM_PROGRAM", None), ("KONSOLE_VERSION", "Konsole"),
		                   ("WT_SESSION", "Windows Terminal"), ("VTE_VERSION", "VTE")):
			if os.environ.get(key):
				name = label or os.environ[key]
				version = version or os.environ.get("TERM_PROGRAM_VERSION", "") \
					or (os.environ.get(key) if label else "")
				break

	if not name:
		name = os.environ.get("TERM", "unknown")

	# A development build reports the same version string every time it is
	# rebuilt, so those get a build stamp; packaged terminals do not need one.
	build = version or "unknown"
	if exe and not _is_system(exe):
		stamp = _stamp_for(exe)
		if stamp:
			build = "%s+%s" % (build, stamp)

	return name.strip(), build.strip(), exe


#•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
#	Storage
#•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••

def data_dir():
	if os.name == "nt":
		root = os.environ.get("LOCALAPPDATA") or os.path.expanduser("~")
	elif sys.platform == "darwin":
		root = os.path.expanduser("~/Library/Application Support")
	else:
		root = os.environ.get("XDG_DATA_HOME") or os.path.expanduser("~/.local/share")
	path = os.path.join(root, APP)
	os.makedirs(path, exist_ok=True)
	return path


def save(records):
	path = os.path.join(data_dir(), DATA_FILE)
	with open(path, "a", encoding="utf-8") as fh:
		for rec in records:
			fh.write(json.dumps(rec, sort_keys=True) + "\n")
	return path


def load():
	path = os.path.join(data_dir(), DATA_FILE)
	out = []
	try:
		with open(path, "r", encoding="utf-8") as fh:
			for line in fh:
				line = line.strip()
				if line:
					try:
						out.append(json.loads(line))
					except ValueError:
						pass
	except FileNotFoundError:
		pass
	return out


#•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
#	Measurement
#•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••

def harness_ceiling(blob):
	"""What the harness alone can push. If a terminal nears this, distrust it."""
	best = 0.0
	with open(os.devnull, "wb", buffering=0) as null:
		fd = null.fileno()
		mv = memoryview(blob)
		for _ in range(2):
			start = time.perf_counter()
			off, n = 0, len(blob)
			while off < n:
				off += os.write(fd, mv[off:off + WRITE_CHUNK])
			took = time.perf_counter() - start
			if took > 0:
				best = max(best, len(blob) / took)
	return best


def run_scene(console, scene, blob, reps, quiet):
	times, synced_all = [], True

	# One short warmup, not counted. The first sight of a glyph costs far more
	# than the next thousand - an unwarmed emoji scene measured 0.82 MB/s then
	# 11.40 on the very next rep, which is a cache being filled, not throughput.
	console.emit("\x1b[0m\x1b[H\x1b[2J\x1b[3J")
	console.write(blob[:max(1, len(blob) // 5)])
	console.sync()

	for _ in range(reps):
		# Known state: cursor home, screen and scrollback clear, so every rep
		# starts the same way and the scrollback regime cannot drift.
		console.emit("\x1b[0m\x1b[H\x1b[2J\x1b[3J")
		console.sync(5.0)
		console.drain()

		start = time.perf_counter()
		console.write(blob)
		synced = console.sync(drain=False)
		took = time.perf_counter() - start

		synced_all = synced_all and synced
		times.append(took)
	return times, synced_all


def summarize(times, nbytes, chars, cells):
	mean = statistics.fmean(times)
	sd = statistics.stdev(times) if len(times) > 1 else 0.0
	return {
		"runs": len(times),
		"secs_mean": mean,
		"secs_sd": sd,
		"mbs": (nbytes / mean) / 1e6,
		"mbs_min": (nbytes / max(times)) / 1e6,
		"mbs_max": (nbytes / min(times)) / 1e6,
		"mbs_sd": (nbytes / 1e6) * sd / (mean * mean) if mean else 0.0,
		"kchars": (chars / mean) / 1e3,
		"kcells": (cells / mean) / 1e3,
		"cv": (sd / mean * 100.0) if mean else 0.0,
	}


def score_of(per_scene):
	"""Weighted geometric mean of cells/sec: no one scene can dominate."""
	num, den = 0.0, 0.0
	for name, row in per_scene.items():
		scene = SCENE_BY_NAME.get(name)
		if not scene or row.get("kcells", 0) <= 0:
			continue
		num += scene.weight * math.log(row["kcells"])
		den += scene.weight
	return math.exp(num / den) if den else 0.0


#•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
#	Reporting
#•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••

def gfs_pick(items, keep=5, recent=3):
	"""
	Newest few always, then spread the remaining slots back through history so
	an old baseline stays visible instead of scrolling off.
	"""
	if len(items) <= keep:
		return items
	head, tail = items[:recent], items[recent:]
	slots = keep - recent
	if slots <= 1:
		return head + [tail[-1]]
	picks = sorted({round(i * (len(tail) - 1) / (slots - 1)) for i in range(slots)})
	return head + [tail[i] for i in picks]


def aggregate(records, mode, line_cells):
	"""Group comparable records into one row per terminal build."""
	rows = {}
	skipped = 0
	for rec in records:
		if rec.get("payload_version") != PAYLOAD_VERSION or rec.get("mode") != mode \
				or rec.get("line_cells") != line_cells:
			skipped += 1
			continue
		key = (rec["terminal"], rec["build"])
		row = rows.setdefault(key, {"terminal": rec["terminal"], "build": rec["build"],
		                            "scenes": {}, "runs": 0, "when": "", "grid": ""})
		row["scenes"].setdefault(rec["scene"], []).append(rec)
		row["runs"] += rec.get("runs", 1)
		if rec.get("when", "") > row["when"]:
			row["when"] = rec.get("when", "")
			row["grid"] = rec.get("grid", "")

	out = []
	for row in rows.values():
		per = {}
		for name, recs in row["scenes"].items():
			recs = sorted(recs, key=lambda r: r.get("when", ""))[-1:]  # newest wins
			per[name] = recs[0]
		row["per"] = per
		row["score"] = score_of(per)
		# Without the device-attributes barrier a scene only timed how fast the
		# terminal accepted bytes, which is not the same measurement as the rest.
		row["synced"] = all(r.get("synced") for r in per.values())
		out.append(row)
	return out, skipped


def unpublishable(mode, scale, grid, line_cells):
	"""Why a run at these settings may not reach the README, or "" if it may."""
	if mode != "full":
		return "a %s run" % mode
	if scale != 1.0:
		return "scale %g" % scale
	if grid != TABLE_GRID:
		return "grid %s, the table's is %s" % (grid or "unknown", TABLE_GRID)
	if line_cells != TABLE_LINE_CELLS:
		return "%d-cell lines, the table's are %d" % (line_cells, TABLE_LINE_CELLS)
	return ""


def readme_rows(records):
	"""aggregate() over only the records the table may show."""
	fit = [r for r in records
	       if not unpublishable(r.get("mode"), r.get("scale", 1.0), r.get("grid", ""),
	                            r.get("line_cells"))]
	return aggregate(fit, "full", TABLE_LINE_CELLS)[0]


def history_table(rows):
	by_term = {}
	for row in rows:
		by_term.setdefault(row["terminal"], []).append(row)

	lines = []
	head = "%-22s %-30s %-9s" % ("terminal", "build", "grid")
	head += "".join("%8s" % s.label for s in SCENES) + "%8s%6s" % ("score", "runs")
	lines.append(head)
	lines.append("-" * len(head))

	ordered = sorted(by_term.items(),
	                 key=lambda kv: max(r["score"] for r in kv[1]), reverse=True)
	elided = 0
	for _, group in ordered:
		group = sorted(group, key=lambda r: r["when"], reverse=True)
		keep = gfs_pick(group)
		elided += len(group) - len(keep)
		for row in keep:
			line = "%-22.22s %-30.30s %-9.9s" % (row["terminal"], row["build"], row["grid"])
			for scene in SCENES:
				rec = row["per"].get(scene.name)
				line += "%8.2f" % rec["mbs"] if rec else "%8s" % "-"
			line += "%8.1f%6d" % (row["score"] / 1000.0, row["runs"])
			lines.append(line)
	if elided:
		lines.append("(%d older build(s) not shown)" % elided)
	return "\n".join(lines)


def report(term, build, grid, mode, scale, per, synced, ceiling, elapsed):
	out = []
	out.append("SilkTerm terminal throughput benchmark    %s"
	           % datetime.now().strftime("%Y-%m-%d %H:%M:%S"))
	out.append("")
	out.append("terminal   %s %s" % (term, build))
	out.append("grid       %s     mode %s (scale %.2f)     sync %s"
	           % (grid, mode, scale, "DA1" if synced else "NONE - times are unreliable"))
	out.append("")

	head = "%-8s %-7s%6s%12s%10s%10s%8s%16s" % (
		"scene", "width", "runs", "MB/s", "Kchar/s", "Kcell/s", "CV%", "MB/s min-max")
	out.append(head)
	out.append("-" * len(head))
	for scene in SCENES:
		row = per.get(scene.name)
		if not row:
			continue
		out.append("%-8s %-7s%6d%8.2f+-%-4.2f%10.0f%10.0f%8.1f%8.2f -%7.2f" % (
			scene.name, scene.label, row["runs"], row["mbs"], row["mbs_sd"],
			row["kchars"], row["kcells"], row["cv"], row["mbs_min"], row["mbs_max"]))
	out.append("-" * len(head))
	out.append("score (weighted geometric mean, million cells/s)   %.1f"
	           % (score_of(per) / 1000.0))
	peak = max(r["mbs"] for r in per.values()) * 1e6
	ratio = (ceiling / peak) if peak else 0.0
	out.append("harness ceiling %.1f GB/s to a sink - %.0fx the terminal's best, "
	           "so the tool is not what was measured" % (ceiling / 1e9, ratio))
	out.append("total %.1fs" % elapsed)
	return "\n".join(out)


#•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
#	README
#•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••

def readme_path():
	"""The checkout's README, if this is running from inside one.

	Walks up rather than assuming a depth: the tool has lived at utility/ and now at
	utility/include/, and a fixed one-level guess silently stops refreshing the table
	the moment it moves - no error, just a column that quietly goes stale.
	"""
	here = os.path.dirname(os.path.abspath(__file__))
	for _ in range(4):
		here = os.path.dirname(here)
		if not here:
			break
		guess = os.path.join(here, "README.md")
		if os.path.isfile(guess):
			return guess
	return ""


def _plain(cell):
	"""A table cell reduced to comparable text: no markup, no footnote marks."""
	text = re.sub(r"<sup>.*?</sup>", "", cell)
	# The highlighted row is colored through GitHub's math renderer, with or
	# without a weight around the name, so unwrap the weight first.
	text = re.sub(r"\\text(?:bf|it)\{([^{}]*)\}", r"\1", text)
	text = re.sub(r"\$\\textcolor\{[^{}]*\}\{([^{}]*)\}\$", r"\1", text)
	return text.replace("**", "").replace("\\", "").strip()


def _key(name):
	"""Terminal names differ between the tool and the table (xfce4-terminal vs
	XFCE4 Terminal), so match on letters and digits alone."""
	return re.sub(r"[^a-z0-9]", "", name.lower())


# A run inside SilkTerm as shipped belongs on the row for SilkTerm as shipped;
# the stripped-down build is a different row and has to name itself with --label.
ROW_ALIASES = {"silkterm": "silktermcandy"}


def _row_key(name):
	key = _key(name)
	return ROW_ALIASES.get(key, key)


def _head_key(cell):
	"""A header reduced to its name alone: no markup, no footnote, no unit."""
	return _plain(cell).split(" (")[0].strip().lower()


def _short_version(build):
	"""
	The Ver column is narrow and shares its width with everything else, so it
	drops the build stamp, which matters only when comparing one dev build
	against another - the tool's own history table is for that. The prerelease
	tag stays, since without it the cell names a release that may not exist.
	"""
	return build.split("+", 1)[0] or "-"


def _split_table(block):
	"""(header, alignment, data rows) of the first markdown table in the block."""
	lines = [ln.strip() for ln in block.splitlines() if ln.strip().startswith("|")]
	if len(lines) < 2:
		return None
	grid = [mdtable.split_row(ln) for ln in lines]
	return grid[0], grid[1], grid[2:]


def _score_of(cells, at):
	try:
		return float(_plain(cells[at]))
	except (ValueError, IndexError):
		return None


def readme_table(existing, rows):
	"""
	Refresh the speed columns of the table already in the README, leaving every
	other column - platform, sizes, memory - exactly as written there. Newest
	build per terminal only; the history stays in the tool.

	Scored rows are re-sorted into the slots scored rows already occupy, so a
	row with no speed figure (a variant of another entry, say) keeps its place
	instead of being shuffled off by rows it cannot be compared with.
	"""
	parsed = _split_table(existing)
	if not parsed:
		return ""
	head, align, data = parsed
	col = {}
	for i, cell in enumerate(head):
		col[_head_key(cell)] = i

	# Headers get reworded to keep the table inside a readable width, so the
	# score column is found by its tail rather than one exact spelling.
	name_at, ver_at = col.get("terminal"), col.get("ver", col.get("version"))
	at = next((i for k, i in col.items() if k.endswith("speed score")), None)
	if name_at is None or ver_at is None or at is None:
		return ""
	scene_col = {s.name: col[s.label.lower()]
	             for s in SCENES if s.label.lower() in col}

	# A terminal that never answered the barrier stays out of the table: its
	# times are an upper bound, not the throughput every other row reports.
	newest = {}
	for row in sorted(rows, key=lambda r: r["when"]):
		if row.get("synced", True):
			newest[_row_key(row["terminal"])] = row

	seen = {_row_key(_plain(cells[name_at])): cells for cells in data}
	fresh = []
	for key, row in newest.items():
		cells = seen.get(key)
		if cells is None:
			cells = ["-"] * len(head)
			cells[name_at] = row["terminal"]
			fresh.append(cells)
			seen[key] = cells
		cells[ver_at] = _short_version(row["build"])
		for scene, place in scene_col.items():
			rec = row["per"].get(scene)
			cells[place] = "%.1f" % rec["mbs"] if rec else "-"
		cells[at] = "**%.1f**" % (row["score"] / 1000.0)

	# A terminal measured for the first time joins the scored block rather than
	# sitting under the unscored tail, where the ranking below could not reach it.
	scored = [i for i, cells in enumerate(data) if _score_of(cells, at) is not None]
	cut = max(scored) + 1 if scored else len(data)
	data[cut:cut] = fresh

	slots = [i for i, cells in enumerate(data) if _score_of(cells, at) is not None]
	ranked = sorted((data[i] for i in slots), key=lambda c: _score_of(c, at), reverse=True)
	for i, cells in zip(slots, ranked):
		data[i] = cells

	out = [README_BEGIN, ""]
	out.extend(mdtable.render(head, align, data))
	out.append("")
	out.append(README_END)
	return "\n".join(out)


def update_readme(path, rows):
	try:
		with open(path, "r", encoding="utf-8") as fh:
			text = fh.read()
	except OSError:
		return ""
	if README_BEGIN not in text or README_END not in text:
		return ""
	head, rest = text.split(README_BEGIN, 1)
	existing, tail = rest.split(README_END, 1)
	table = readme_table(existing, rows)
	if not table:
		return ""
	with open(path, "w", encoding="utf-8") as fh:
		fh.write(head + table + tail)
	return path


#•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
#	Entry point
#•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••

def parse_args(argv):
	ap = argparse.ArgumentParser(
		description="Measure terminal drawing throughput by UTF-8 width class.")
	ap.add_argument("--quick", action="store_true",
	                help="fewer runs per scene, about 30s; recorded separately")
	ap.add_argument("--scale", type=float, default=1.0,
	                help="multiply every payload size; try 0.5 on a slow terminal")
	ap.add_argument("--reps", type=int, default=None,
	                help="override runs per scene (default: 12x the scene weight)")
	ap.add_argument("--scene", action="append", default=None,
	                choices=[s.name for s in SCENES], help="run only these scenes")
	ap.add_argument("--line-cells", type=int, default=80,
	                help="logical line width in cells (default 80)")
	ap.add_argument("--label", default="",
	                help="name this run's row, as 'name' or 'name/build'; a bare "
	                     "name keeps the detected build")
	ap.add_argument("--history", action="store_true", help="print the table and exit")
	ap.add_argument("--out", default="",
	                help="also write the report to this file (stdout is the tty)")
	ap.add_argument("--no-save", action="store_true", help="do not record the result")
	ap.add_argument("--no-readme", action="store_true", help="do not touch README.md")
	ap.add_argument("--json", action="store_true", help="emit this run as JSON too")
	return ap.parse_args(argv)


def main(argv):
	args = parse_args(argv)
	mode = "quick" if args.quick else "full"
	scale = args.scale

	if args.history:
		rows, skipped = aggregate(load(), mode, args.line_cells)
		if not rows:
			print("no %s results recorded yet (%d other record(s) on file)"
			      % (mode, skipped))
			return 0
		print(history_table(rows))
		readme = readme_path()
		table_rows = readme_rows(load())
		if not args.no_readme and readme and table_rows and update_readme(readme, table_rows):
			print("\nREADME.md results table updated")
		return 0

	if not sys.stdout.isatty() or not sys.stdin.isatty():
		print("termbench needs a real terminal on both stdin and stdout "
		      "(it times the terminal's reply, which a pipe cannot give).",
		      file=sys.stderr)
		return 2

	scenes = [s for s in SCENES if not args.scene or s.name in args.scene]
	cols, lines = terminal_size()
	if cols and args.line_cells > cols:
		print("note: lines are %d cells but the terminal is %d wide, so every line "
		      "wraps. Results stay comparable only against runs at the same width."
		      % (args.line_cells, cols), file=sys.stderr)
		time.sleep(1.5)

	when = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
	started = time.perf_counter()
	per, records, synced_all, ceiling = {}, [], True, 0.0

	with Console() as console:
		if args.label:
			# Two configurations of one program look identical to detection, so
			# a name on its own only renames the row and keeps the real build.
			name, _, tag = args.label.partition("/")
			term = name.strip()
			if tag.strip():
				build, exe = tag.strip(), ""
			else:
				_, build, exe = identify(console)
		else:
			term, build, exe = identify(console)

		grid = "%dx%d" % (cols, lines)

		for scene in scenes:
			reps = args.reps or scene.weight * (REPS_QUICK if args.quick else REPS_FULL)

			console.emit("\x1b[0m\x1b[H\x1b[2J\x1b[3J")
			console.emit("building %s payload...\r\n" % scene.name)
			blob, chars, cells, nlines = build_payload(scene, scale, args.line_cells)
			ceiling = max(ceiling, harness_ceiling(blob))

			times, synced = run_scene(console, scene, blob, reps, False)
			synced_all = synced_all and synced

			row = summarize(times, len(blob), chars, cells)
			per[scene.name] = row
			records.append({
				"when": when, "terminal": term, "build": build, "exe": exe,
				"scene": scene.name, "mode": mode, "scale": scale,
				"line_cells": args.line_cells, "payload_version": PAYLOAD_VERSION,
				"grid": grid, "bytes": len(blob), "chars": chars, "cells": cells,
				"lines": nlines, "synced": synced, "times": times,
				"os": "%s %s" % (platform.system(), platform.release()),
				"host": platform.node(), **row,
			})
			del blob

		console.emit("\x1b[0m\x1b[H\x1b[2J\x1b[3J")

	elapsed = time.perf_counter() - started
	out = [report(term, build, grid, mode, scale, per, synced_all, ceiling, elapsed)]

	if not args.no_save:
		out.append("\nrecorded to %s" % save(records))

	rows, skipped = aggregate(load(), mode, args.line_cells)
	if rows:
		out.append("")
		out.append(history_table(rows))
		if skipped:
			out.append("(%d record(s) from another mode, width or payload version "
			           "not comparable here)" % skipped)

	if not args.no_readme and not args.no_save:
		why = unpublishable(mode, scale, grid, args.line_cells)
		readme = readme_path()
		if why:
			out.append("\nREADME.md not touched: %s" % why)
		elif readme and rows and update_readme(readme, readme_rows(load())):
			out.append("\nREADME.md results table updated")

	if not synced_all:
		out.append("\nWARNING: this terminal never answered the device-attributes "
		           "query, so the times measure how fast it accepted bytes, not how "
		           "fast it drew them. Treat the numbers as an upper bound.")

	text = "\n".join(out)
	print(text)
	if args.json:
		print()
		print(json.dumps(records, indent=1, sort_keys=True))
	if args.out:
		try:
			with open(args.out, "w", encoding="utf-8") as fh:
				fh.write(text + "\n")
				if args.json:
					fh.write(json.dumps(records, indent=1, sort_keys=True) + "\n")
		except OSError as err:
			print("could not write %s: %s" % (args.out, err), file=sys.stderr)
	return 0


if __name__ == "__main__":
	try:
		sys.exit(main(sys.argv[1:]))
	except KeyboardInterrupt:
		sys.stdout.write("\x1b[0m\r\n")
		sys.exit(130)
