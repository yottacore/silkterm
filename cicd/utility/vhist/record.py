#!/usr/bin/env python3

##	Purpose:
##		Records what a full-screen program writes to its terminal while it is
##		scrolled through a numbered file, for the virtual history probe in
##		source/src/pane/vhist_probe.rs. Each read from the pty is kept as its own
##		chunk, so a replay can put a frame between any two of them.
##		Never point this at a coding-agent TUI: the keys it types would go to
##		that program as input.
##	Syntax:
##		record.py APP OUT [--cols N] [--rows N] [--lines N]
##		APP is less, vim, nano or htop. OUT gets the chunks, each a 4-byte
##		little-endian double for when it was read, in seconds from the start, a
##		4-byte little-endian length, and then the bytes.
##	Exit: 0 fine, 2 bad arguments.
##	History: At bottom of script.

##	Copyright (c) 2026 Bubbles
##	Licensed under The MIT License (MIT). Full text at:
##		https://mit-license.org/
##	SPDX-License-Identifier: MIT

import argparse
import fcntl
import os
import pty
import select
import shutil
import struct
import sys
import tempfile
import termios
import time
from pathlib import Path

CTRL = {c: chr(ord(c) - 96) for c in 'bdefuy'}

## (keys, count, seconds between). Each step scrolls a known way, so a replay
## can say which rows went past.
SCRIPTS = {
	'less': [
		('j', 40, 0.03), (' ', 4, 0.15), ('d', 3, 0.15), ('k', 30, 0.03),
		('b', 2, 0.15), ('u', 2, 0.15), ('j', 25, 0.03), ('G', 1, 0.3),
		('g', 1, 0.3), ('j', 10, 0.03), ('q', 1, 0.3),
	],
	'vim': [
		('j', 60, 0.03), (CTRL['e'], 20, 0.03), (CTRL['y'], 15, 0.03),
		(CTRL['f'], 3, 0.15), (CTRL['b'], 2, 0.15), (CTRL['d'], 3, 0.15),
		(CTRL['u'], 2, 0.15), ('j', 30, 0.03), ('G', 1, 0.3), ('gg', 1, 0.3),
		('j', 40, 0.03), (':q!\r', 1, 0.3),
	],
	'nano': [
		('\x1b[B', 60, 0.03), ('\x1b[A', 40, 0.03), ('\x1b[6~', 3, 0.15),
		('\x1b[5~', 2, 0.15), ('\x1b[B', 30, 0.03), ('\x18', 1, 0.3),
	],
	'htop': [
		('', 1, 3.0), ('\x1b[B', 60, 0.05), ('\x1b[A', 30, 0.05), ('', 1, 2.0),
		('q', 1, 0.3),
	],
}


def command_for(app: str, path: str) -> list[str]:
	if app == 'less':
		return ['less', '-S', path]
	if app == 'vim':
		vim = shutil.which('vim') or shutil.which('vim.tiny') or 'vi'
		return [vim, '-u', 'NONE', '-N', '-R', path]
	if app == 'nano':
		return ['nano', '--ignorercfiles', '-v', path]
	return ['htop', '-d', '5']


def main() -> int:
	ap = argparse.ArgumentParser()
	ap.add_argument('app', choices=sorted(SCRIPTS))
	ap.add_argument('out')
	ap.add_argument('--cols', type=int, default=100)
	ap.add_argument('--rows', type=int, default=30)
	ap.add_argument('--lines', type=int, default=600)
	args = ap.parse_args()

	work = tempfile.mkdtemp(prefix='vhist_')
	path = str(Path(work) / 'numbered.txt')
	with Path(path).open('w', encoding='ascii') as f:
		for n in range(1, args.lines + 1):
			f.write(f'L{n:05d} the quick brown fox jumps over the lazy dog\n')

	env = dict(os.environ, TERM='xterm-256color', LANG='C.UTF-8', LESS='', LESSHISTFILE='-',
		HOME=work, COLUMNS=str(args.cols), LINES=str(args.rows))
	pid, fd = pty.fork()
	if pid == 0:
		fcntl.ioctl(0, termios.TIOCSWINSZ, struct.pack('HHHH', args.rows, args.cols, 0, 0))
		os.chdir(work)
		os.execvpe(command_for(args.app, path)[0], command_for(args.app, path), env)

	keys: list[tuple[float, str]] = []
	at = time.monotonic() + 1.0
	for key, count, gap in SCRIPTS[args.app]:
		for _ in range(count):
			keys.append((at, key))
			at += gap
	chunks: list[tuple[float, bytes]] = []
	start = time.monotonic()
	deadline = at + 5.0
	while time.monotonic() < deadline:
		now = time.monotonic()
		while keys and keys[0][0] <= now:
			os.write(fd, keys.pop(0)[1].encode())
		wait = max(0.0, min(keys[0][0] - now, 0.05)) if keys else 0.05
		ready, _, _ = select.select([fd], [], [], wait)
		if not ready:
			continue
		try:
			data = os.read(fd, 65536)
		except OSError:
			break
		if not data:
			break
		chunks.append((time.monotonic() - start, data))
	try:
		os.kill(pid, 9)
	except ProcessLookupError:
		pass
	os.waitpid(pid, 0)
	shutil.rmtree(work)

	with Path(args.out).open('wb') as f:
		for at, c in chunks:
			f.write(struct.pack('<dI', at, len(c)))
			f.write(c)
	print(f'{args.app}: {len(chunks)} chunks, {sum(len(c) for _, c in chunks)} bytes -> {args.out}',
		file=sys.stderr)
	return 0


if __name__ == '__main__':
	sys.exit(main())

##	History:
##		- 20261013: Created.
