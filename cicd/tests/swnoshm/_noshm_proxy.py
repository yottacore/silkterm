#!/usr/bin/env python3

##	- Purpose:
##		An X server with no MIT-SHM shared pixmaps, made by standing in front
##		of a real one that has them. Everything goes through as sent, file
##		descriptors too, except that MIT-SHM QueryVersion answers no shared
##		pixmaps and every ShmCreatePixmap fails BadImplementation, as on a
##		server whose driver cannot make them (NVIDIA's). A piece of
##		swnoshm/run.bash, not a test of its own.
##	- Syntax: _noshm_proxy.py <real display, :N> <display number to answer as>
##	- Exit: runs until killed; 2 bad arguments, no python-xlib, or the real
##	  server has no MIT-SHM.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

import array
import os
import signal
import socket
import struct
import sys
import threading
from pathlib import Path

try:
	from Xlib import display
except ImportError:
	sys.exit(2)

BAD_IMPLEMENTATION = 17
SHM_QUERY_VERSION = 0
SHM_CREATE_PIXMAP = 5
MAX_FDS = 16


def recv_some(sock: socket.socket) -> tuple[bytes, list[int]]:
	fds = array.array("i")
	data, anc, _flags, _addr = sock.recvmsg(1 << 16, socket.CMSG_LEN(MAX_FDS * fds.itemsize))
	for level, kind, payload in anc:
		if level == socket.SOL_SOCKET and kind == socket.SCM_RIGHTS:
			fds.frombytes(payload[: len(payload) - (len(payload) % fds.itemsize)])
	return data, list(fds)


## The descriptors go with the first piece, which is never later than the
## request that names them.
def send_all(sock: socket.socket, data: bytes, fds: list[int]) -> None:
	first = True
	while data or (first and fds):
		anc = [(socket.SOL_SOCKET, socket.SCM_RIGHTS, array.array("i", fds))] if first and fds else []
		sent = sock.sendmsg([data], anc)
		if first:
			for fd in fds:
				os.close(fd)
		first = False
		data = data[sent:]


class Session:
	def __init__(self, client: socket.socket, server: socket.socket, shm_opcode: int) -> None:
		self.client = client
		self.server = server
		self.shm = shm_opcode
		self.lock = threading.Lock()
		## low 16 bits of the sequence numbers to answer differently
		self.versions: set[int] = set()
		self.pixmaps: set[int] = set()

	def upstream(self) -> None:
		buf = b""
		fds: list[int] = []
		setup_done = False
		seq = 0
		try:
			while True:
				data, got = recv_some(self.client)
				if not data and not got:
					break
				buf += data
				fds += got
				out = bytearray()
				while True:
					if not setup_done:
						if len(buf) < 12:
							break
						name_len, data_len = struct.unpack_from("<HH", buf, 6)
						size = 12 + (name_len + 3) // 4 * 4 + (data_len + 3) // 4 * 4
						if len(buf) < size:
							break
						out += buf[:size]
						buf = buf[size:]
						setup_done = True
						continue
					if len(buf) < 4:
						break
					units = struct.unpack_from("<H", buf, 2)[0]
					if units == 0:  # BIG-REQUESTS
						if len(buf) < 8:
							break
						units = struct.unpack_from("<I", buf, 4)[0]
					size = units * 4
					if len(buf) < size:
						break
					req = bytearray(buf[:size])
					buf = buf[size:]
					seq = (seq + 1) & 0xFFFF
					if req[0] == self.shm and req[1] in (SHM_QUERY_VERSION, SHM_CREATE_PIXMAP):
						with self.lock:
							(self.versions if req[1] == SHM_QUERY_VERSION else self.pixmaps).add(seq)
						if req[1] == SHM_CREATE_PIXMAP:
							## A minor the server does not know gets an error at this
							## sequence, so the numbering stays as the client counts it.
							req[1] = 0xFF
					out += req
				if out or fds:
					send_all(self.server, bytes(out), fds)
					fds = []
		except OSError:
			pass
		finally:
			self.close()

	def downstream(self) -> None:
		buf = b""
		fds: list[int] = []
		setup_done = False
		try:
			while True:
				data, got = recv_some(self.server)
				if not data and not got:
					break
				buf += data
				fds += got
				out = bytearray()
				while True:
					if not setup_done:
						if len(buf) < 8:
							break
						size = 8 + struct.unpack_from("<H", buf, 6)[0] * 4
						if len(buf) < size:
							break
						out += buf[:size]
						buf = buf[size:]
						setup_done = True
						continue
					if len(buf) < 32:
						break
					kind = buf[0] & 0x7F
					size = 32
					if kind in (1, 35):  # a reply, or a generic event
						size += struct.unpack_from("<I", buf, 4)[0] * 4
					if len(buf) < size:
						break
					msg = bytearray(buf[:size])
					buf = buf[size:]
					seq = struct.unpack_from("<H", msg, 2)[0]
					with self.lock:
						if kind == 1 and seq in self.versions:
							self.versions.discard(seq)
							msg[1] = 0  # shared_pixmaps
							msg[16] = 0  # pixmap_format, none without them
						elif kind == 0 and seq in self.pixmaps:
							self.pixmaps.discard(seq)
							msg[1] = BAD_IMPLEMENTATION
							struct.pack_into("<H", msg, 8, SHM_CREATE_PIXMAP)
					out += msg
				if out or fds:
					send_all(self.client, bytes(out), fds)
					fds = []
		except OSError:
			pass
		finally:
			self.close()

	def close(self) -> None:
		for sock in (self.client, self.server):
			try:
				sock.shutdown(socket.SHUT_RDWR)
			except OSError:
				pass


def main() -> int:
	if len(sys.argv) != 3 or not sys.argv[1].startswith(":") or not sys.argv[2].isdigit():
		return 2
	real = f"/tmp/.X11-unix/X{sys.argv[1][1:].split('.')[0]}"
	ext = display.Display(sys.argv[1]).query_extension("MIT-SHM")
	if ext is None:
		return 2
	path = Path(f"/tmp/.X11-unix/X{sys.argv[2]}")
	listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
	listener.bind(str(path))
	signal.signal(signal.SIGTERM, lambda *_: sys.exit(0))
	try:
		listener.listen(16)
		while True:
			client, _ = listener.accept()
			server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
			server.connect(real)
			session = Session(client, server, ext.major_opcode)
			threading.Thread(target=session.upstream, daemon=True).start()
			threading.Thread(target=session.downstream, daemon=True).start()
	finally:
		path.unlink()


if __name__ == "__main__":
	sys.exit(main())

##	History:
##		- 20261006 JC: Created.
