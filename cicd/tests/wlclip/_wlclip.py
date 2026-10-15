#!/usr/bin/env python3

##	- Purpose:
##		Reads or sets a Wayland selection from outside any window, through
##		the wlr data control protocol, which sway has. The same job as
##		wl-paste and wl-copy, for a box without them. A piece of
##		wlclip/run.bash, not a test of its own. Speaks the Wayland wire
##		format itself, so it needs nothing past python3.
##	- Syntax:
##		_wlclip.py get clipboard|primary
##			Prints the text and exits.
##		_wlclip.py set clipboard|primary <text>
##			Prints "ready" once the compositor has it, then hands the text to
##			whoever asks, until another program takes the selection.
##	- Exit: 0 done, 1 no compositor or no data control there, 2 bad arguments,
##	  4 nothing is in that selection, 5 the owner never sent the text.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

import os
import select
import socket
import struct
import sys
from collections.abc import Callable
from pathlib import Path

DISPLAY, REGISTRY = 1, 2
TEXT_TYPES = ("text/plain;charset=utf-8", "UTF8_STRING", "text/plain")


class Wire:
	def __init__(self, path: str) -> None:
		self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
		self.sock.connect(path)
		self.buf = b""
		self.fds: list[int] = []
		self.next_id = REGISTRY + 1
		self.globals: dict[str, tuple[int, int]] = {}

	def new_id(self) -> int:
		got = self.next_id
		self.next_id += 1
		return got

	def send(self, obj: int, opcode: int, *args: int | str, fd: int | None = None) -> None:
		body = b""
		for arg in args:
			if isinstance(arg, str):
				raw = arg.encode() + b"\0"
				body += struct.pack("<I", len(raw)) + raw + b"\0" * (-len(raw) % 4)
			else:
				body += struct.pack("<I", arg & 0xFFFFFFFF)
		msg = struct.pack("<II", obj, (8 + len(body)) << 16 | opcode) + body
		if fd is None:
			self.sock.sendall(msg)
		else:
			socket.send_fds(self.sock, [msg], [fd])

	def event(self) -> tuple[int, int, bytes]:
		while True:
			if len(self.buf) >= 8:
				obj, word = struct.unpack_from("<II", self.buf)
				size = word >> 16
				if len(self.buf) >= size:
					body, self.buf = self.buf[8:size], self.buf[size:]
					return obj, word & 0xFFFF, body
			more, fds, _, _ = socket.recv_fds(self.sock, 65536, 16)
			if not more:
				raise ConnectionError("the compositor hung up")
			self.buf += more
			self.fds += fds

	## Every event up to now, through handle, once everything sent so far has
	## been handled.
	def roundtrip(self, handle: Callable[[int, int, bytes], None] | None = None) -> None:
		done = self.new_id()
		self.send(DISPLAY, 0, done)
		while True:
			obj, opcode, body = self.event()
			if obj == done:
				return
			if obj == DISPLAY and opcode == 0:
				_, code, size = struct.unpack_from("<III", body)
				raise ConnectionError(f"protocol error {code}: {body[12:12 + size - 1].decode(errors='replace')}")
			if obj == REGISTRY and opcode == 0:
				name, size = struct.unpack_from("<II", body)
				interface = body[8:8 + size - 1].decode()
				(version,) = struct.unpack_from("<I", body, 8 + size + (-size % 4))
				self.globals[interface] = (name, version)
			elif handle is not None:
				handle(obj, opcode, body)

	def bind(self, interface: str, version: int) -> int:
		name, _ = self.globals[interface]
		got = self.new_id()
		self.send(REGISTRY, 0, name, interface, version, got)
		return got


def text_arg(body: bytes) -> str:
	(size,) = struct.unpack_from("<I", body)
	return body[4:4 + size - 1].decode(errors="replace")


def connect() -> tuple[Wire, int, int]:
	path = Path(os.environ.get("XDG_RUNTIME_DIR", "")) / os.environ.get("WAYLAND_DISPLAY", "wayland-0")
	wire = Wire(str(path))
	wire.send(DISPLAY, 1, REGISTRY)
	wire.roundtrip()
	got = wire.globals.get("zwlr_data_control_manager_v1")
	if got is None or got[1] < 2:
		raise ConnectionError("no data control with primary selection in this compositor")
	seat = wire.bind("wl_seat", 1)
	manager = wire.bind("zwlr_data_control_manager_v1", 2)
	return wire, seat, manager


def get(which: str) -> int:
	wire, seat, manager = connect()
	device = wire.new_id()
	types: dict[int, list[str]] = {}
	current = {"clipboard": 0, "primary": 0}

	def handle(obj: int, opcode: int, body: bytes) -> None:
		if obj == device and opcode == 0:
			types[struct.unpack_from("<I", body)[0]] = []
		elif obj == device and opcode in (1, 3):
			current["clipboard" if opcode == 1 else "primary"] = struct.unpack_from("<I", body)[0]
		elif obj in types and opcode == 0:
			types[obj].append(text_arg(body))

	wire.send(manager, 1, device, seat)
	wire.roundtrip(handle)
	offer = current[which]
	mime = next((t for t in TEXT_TYPES if t in types.get(offer, [])), None)
	if not offer or mime is None:
		return 4
	read_end, write_end = os.pipe()
	wire.send(offer, 0, mime, fd=write_end)
	os.close(write_end)
	wire.roundtrip(handle)
	got = b""
	while select.select([read_end], [], [], 3)[0]:
		more = os.read(read_end, 65536)
		if not more:
			sys.stdout.write(got.decode(errors="replace"))
			return 0
		got += more
	return 5


def put(which: str, text: str) -> int:
	wire, seat, manager = connect()
	device = wire.new_id()
	source = wire.new_id()
	wire.send(manager, 1, device, seat)
	wire.send(manager, 0, source)
	for mime in TEXT_TYPES:
		wire.send(source, 0, mime)
	wire.send(device, 0 if which == "clipboard" else 2, source)
	wire.roundtrip()
	print("ready", flush=True)
	while True:
		obj, opcode, _ = wire.event()
		if obj == source and opcode == 0:
			fd = wire.fds.pop(0)
			try:
				os.write(fd, text.encode())
			finally:
				os.close(fd)
		elif obj == source and opcode == 1:
			return 0
		elif obj == DISPLAY and opcode == 0:
			raise ConnectionError("protocol error")


def main() -> int:
	args = sys.argv[1:]
	ok = (len(args) == 2 and args[0] == "get") or (len(args) == 3 and args[0] == "set")
	if not ok or args[1] not in ("clipboard", "primary"):
		print("syntax: _wlclip.py get clipboard|primary, or set clipboard|primary <text>", file=sys.stderr)
		return 2
	try:
		return get(args[1]) if args[0] == "get" else put(args[1], args[2])
	except (OSError, ConnectionError) as e:
		print(f"no selection: {e}", file=sys.stderr)
		return 1


if __name__ == "__main__":
	sys.exit(main())

##	History:
##		- 20261015 JC: Created.
