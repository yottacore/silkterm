#!/usr/bin/env python3

##	- Purpose:
##		A pointer for a compositor with no input devices, through the
##		wlr virtual pointer protocol, which sway has. A piece of monwl/run.bash,
##		not a test of its own. Speaks the Wayland wire format itself, so it
##		needs nothing past python3.
##	- Syntax: _vpointer.py <layout width> <layout height>
##		Reads commands on stdin, one a line, until EOF:
##			move <x> <y>   to a point in layout coordinates
##			down | up      the left button
##		Prints "ready" once the pointer exists, and "ok" after each command
##		once the compositor has it.
##	- Exit: 0 at EOF, 1 no compositor or no virtual pointer there, 2 bad arguments.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

import os
import socket
import struct
import sys
import time
from pathlib import Path

BTN_LEFT = 0x110
DISPLAY, REGISTRY = 1, 2


class Wire:
	def __init__(self, path: str) -> None:
		self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
		self.sock.connect(path)
		self.buf = b""
		self.next_id = REGISTRY + 1
		self.globals: dict[str, tuple[int, int]] = {}

	def new_id(self) -> int:
		got = self.next_id
		self.next_id += 1
		return got

	def send(self, obj: int, opcode: int, *args: int | str) -> None:
		body = b""
		for arg in args:
			if isinstance(arg, str):
				raw = arg.encode() + b"\0"
				body += struct.pack("<I", len(raw)) + raw + b"\0" * (-len(raw) % 4)
			else:
				body += struct.pack("<I", arg & 0xFFFFFFFF)
		self.sock.sendall(struct.pack("<II", obj, (8 + len(body)) << 16 | opcode) + body)

	def event(self) -> tuple[int, int, bytes]:
		while True:
			if len(self.buf) >= 8:
				obj, word = struct.unpack_from("<II", self.buf)
				size = word >> 16
				if len(self.buf) >= size:
					body, self.buf = self.buf[8:size], self.buf[size:]
					return obj, word & 0xFFFF, body
			more = self.sock.recv(65536)
			if not more:
				raise ConnectionError("the compositor hung up")
			self.buf += more

	## Everything sent so far has been handled once this returns.
	def roundtrip(self) -> None:
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

	def bind(self, interface: str, version: int) -> int:
		name, _ = self.globals[interface]
		got = self.new_id()
		self.send(REGISTRY, 0, name, interface, version, got)
		return got


def main() -> int:
	if len(sys.argv) != 3:
		print("syntax: _vpointer.py <layout width> <layout height>", file=sys.stderr)
		return 2
	width, height = int(sys.argv[1]), int(sys.argv[2])
	path = Path(os.environ.get("XDG_RUNTIME_DIR", "")) / os.environ.get("WAYLAND_DISPLAY", "wayland-0")
	try:
		wire = Wire(str(path))
		wire.send(DISPLAY, 1, REGISTRY)
		wire.roundtrip()
		if "zwlr_virtual_pointer_manager_v1" not in wire.globals:
			print("no virtual pointer in this compositor", file=sys.stderr)
			return 1
		seat = wire.bind("wl_seat", 1)
		manager = wire.bind("zwlr_virtual_pointer_manager_v1", 1)
		pointer = wire.new_id()
		wire.send(manager, 0, seat, pointer)
		wire.roundtrip()
	except (OSError, ConnectionError) as e:
		print(f"no pointer: {e}", file=sys.stderr)
		return 1
	print("ready", flush=True)
	for line in sys.stdin:
		words = line.split()
		if not words:
			continue
		now = int(time.monotonic() * 1000)
		if words[0] == "move" and len(words) == 3:
			wire.send(pointer, 1, now, int(words[1]), int(words[2]), width, height)
		elif words[0] in ("down", "up"):
			wire.send(pointer, 2, now, BTN_LEFT, 1 if words[0] == "down" else 0)
		else:
			print(f"unknown command: {line.strip()}", file=sys.stderr)
			continue
		wire.send(pointer, 4)  # frame
		wire.roundtrip()
		print("ok", flush=True)
	return 0


if __name__ == "__main__":
	sys.exit(main())

##	History:
##		- 20261014 JC: Created.
