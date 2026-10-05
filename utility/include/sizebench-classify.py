#!/usr/bin/env python3

##	History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

"""Install size and resident memory of a running terminal, with the graphics stack split out.

Reads a whole process tree and answers two questions:

  File+deps  the executable plus every shared library it needs beyond a base OS
  Mem        unique resident footprint - private pages summed, shared mappings counted once

Both leave out the GPU driver, which is not the terminal's cost: it is shared with the
compositor and every other accelerated program, and it dwarfs everything else (libLLVM
alone is over 120 MiB on this box). A terminal that draws on the CPU pays none of it, so
counting it would make the table measure the driver instead of the terminal.

Two things here are easy to get wrong and were got wrong the first time round:

  * A static dependency list understates any program that loads libraries at runtime. ldd
    lists three for SilkTerm; the running process maps sixty-four. Always read a live
    process, and use the static list only to work out what pulled what in.

  * Classifying the driver by name misses what the driver pulls in - libstdc++ and libxml2
    exist in that map only because mesa wants them, and billing them to the terminal
    inflated a small one by a third. Classify by dependency CLOSURE from the graphics
    roots, and let the app closure win, since a library the app needs in its own right is
    the app's however many other things also happen to want it.

The accounting above the collectors is platform-neutral; only the reading of a live
process is not. Linux reads /proc, Windows asks the process API for its modules, its
mapped regions and which of their pages are resident and shared. The two are the same
measurement, but they are NOT comparable figures - see --help on that.
"""

import argparse
import json
import os
import re
import subprocess
import sys
from collections import defaultdict
from collections.abc import Callable, Iterable
from pathlib import Path
from typing import Any, Protocol

MIB = 1048576.0

#	The grid every row in the table is measured at. Memory scales with the surface, so a
#	figure taken at another size does not belong in the same column: the same SilkTerm
#	binary reads 38 MiB heavier at its default geometry than at this one.
GRID = (100, 30)

#	What measure() answers, as --json prints it.
Result = dict[str, Any]


#••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
#	Platform-neutral accounting
#••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••

class Collector(Protocol):
	"""What measure() reads from a live process."""

	name: str

	def mapped_files(self, pid: int) -> set[str]: ...
	def is_library(self, path: str) -> bool: ...
	def base_name(self, path: str) -> str: ...
	def find_library(self, name: str) -> str | None: ...
	def needed(self, path: str) -> list[str]: ...
	def is_gfx(self, path: str) -> bool: ...
	def is_base_os(self, path: str) -> bool: ...
	def regions(self, pid: int) -> list[tuple[str, int, int]]: ...
	def norm(self, path: str) -> str: ...


class Backend(Collector, Protocol):
	"""A collector that can also walk the process tree, which finding the terminal needs."""

	def exe_of(self, pid: int) -> str: ...
	def parent_of(self, pid: int) -> int: ...
	def children_of(self, pid: int) -> list[int]: ...


def closure(roots: Iterable[str], needed: Callable[[str], list[str]], resolve: Callable[[str], str | None]) -> set[str]:
	"""Everything reachable from roots through their dependency lists, as real paths."""
	seen: set[str] = set()
	stack = list(roots)
	while stack:
		path = stack.pop()
		if path in seen:
			continue
		seen.add(path)
		for name in needed(path):
			target = resolve(name)
			if target and target not in seen:
				stack.append(target)
	return seen


def measure(pids: list[int], exe_paths: list[str], be: Collector, verbose: bool = False, payload: str | None = None) -> Result:
	"""File+deps and Mem for one process tree, given a platform collector.

	With payload, the bundle's own libraries are already in its size, so only the ones
	it borrows from the system count as deps.
	"""
	# Every library any process in the tree has mapped.
	mapped: set[str] = set()
	for pid in pids:
		mapped |= be.mapped_files(pid)
	libs = {p for p in mapped if be.is_library(p)}

	by_name: dict[str, str] = {}
	for p in libs:
		by_name.setdefault(be.base_name(p), p)

	def resolve(name: str) -> str | None:
		"""Prefer the copy this process actually mapped, so a bundled lib wins over a system one."""
		hit = by_name.get(be.base_name(name))
		return hit if hit else be.find_library(name)

	# Injected rather than depended on. Windows machines routinely have libraries pushed
	# into every process - font hooks, security shims - and those belong to the machine,
	# not to the terminal: billing them makes a row that no other machine reproduces. The
	# test is self-calibrating rather than a name list: a library none of the tree's own
	# binaries import, which is nonetheless mapped into this tool's process as well, is
	# being injected into everything. Anything genuinely imported stays whoever else maps it.
	# Windows only for now, deliberately: every published row was measured on Linux, and
	# the rule there could only move one. Extending it needs a published row reproduced
	# first, which is the gate every change to this accounting goes through.
	injected: set[str] = set()
	if be.name == "windows":
		own_closure = closure([p for p in exe_paths if Path(p).exists()], be.needed, resolve)
		ambient = be.mapped_files(os.getpid())
		injected = {p for p in libs
		            if p in ambient and p not in own_closure and not be.is_base_os(p)}
		libs -= injected

	gfx_named = {p for p in libs if be.is_gfx(p)}
	driver_closure = closure(gfx_named, be.needed, resolve) | gfx_named

	# Anything that is neither named like a graphics library nor pulled in by one is
	# unambiguously the app's, and seeds the app's own closure. Order matters: libX11 is
	# in the driver closure too (mesa needs it), but the app needs it in its own right, so
	# the app closure wins. Graphics libraries are excluded from the seed set by name as
	# well as by closure, or a runtime-loaded back end nobody links against seeds it instead.
	app_roots = [p for p in exe_paths if Path(p).exists()]
	app_roots += [p for p in libs if p not in driver_closure]
	app_closure = closure(app_roots, be.needed, resolve)

	driver_libs = {p for p in driver_closure if p not in app_closure and be.is_library(p)}
	app_libs = {p for p in libs if p not in driver_libs and not be.is_base_os(p)}

	def disk(paths: Iterable[str]) -> int:
		total = 0
		for p in paths:
			try:
				total += Path(p).stat().st_size
			except OSError:
				pass
		return total

	exe_bytes = disk(exe_paths)
	inside = Path(payload).resolve() if payload else None
	deps_bytes = disk(p for p in app_libs - set(exe_paths)
	                  if not (inside and Path(p).resolve().is_relative_to(inside)))

	# Resident: private pages are per process, shared mappings are counted once across the
	# tree (per file, the largest any one process holds). Summing the whole resident set
	# instead would charge a multi-process terminal several times over for the same pages.
	priv: defaultdict[str, int] = defaultdict(int)
	shared_max: defaultdict[str, int] = defaultdict(int)
	for pid in pids:
		per_file: defaultdict[str, int] = defaultdict(int)
		for path, pv, sh in be.regions(pid):
			key = be.norm(path) if path else ""
			priv[key] += pv
			per_file[key] += sh
		for key, val in per_file.items():
			shared_max[key] = max(shared_max[key], val)

	app_mem = driver_mem = injected_mem = 0
	for key in set(priv) | set(shared_max):
		total = priv.get(key, 0) + shared_max.get(key, 0)
		if key in driver_libs:
			driver_mem += total
		elif key in injected:
			injected_mem += total
		else:
			app_mem += total

	result: Result = {
		"platform": be.name,
		"pids": sorted(pids),
		"exe_mib": exe_bytes / MIB,
		"deps_mib": deps_bytes / MIB,
		"file_deps_mib": (exe_bytes + deps_bytes) / MIB,
		"mem_mib": app_mem / MIB,
		"driver_mem_mib": driver_mem / MIB,
		"driver_disk_mib": disk(driver_libs) / MIB,
		"n_driver_libs": len(driver_libs),
		"n_app_libs": len(app_libs),
		"injected_mem_mib": injected_mem / MIB,
		"injected_disk_mib": disk(injected) / MIB,
		"n_injected_libs": len(injected),
	}
	if verbose:
		result["injected_libs"] = sorted(
			((Path(p).stat().st_size / MIB, p) for p in injected), reverse=True
		)
		result["app_libs"] = sorted(
			((Path(p).stat().st_size / MIB, p) for p in app_libs - set(exe_paths)), reverse=True
		)
		result["driver_libs"] = sorted(
			((Path(p).stat().st_size / MIB, p) for p in driver_libs), reverse=True
		)
	return result


#••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
#	Linux: /proc
#••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••

# Roots of the graphics stack. Everything these pull in is the driver's unless the app
# needs it independently.
#
# The trailing lookahead has to admit '_', because the vendor back ends are named
# libEGL_nvidia / libGLX_mesa / libdrm_intel and are loaded at runtime rather than linked.
# A plain word boundary misses all of them, they then look like libraries nobody in the
# graphics stack needs, and seeding the app closure with one drags libLLVM in behind it -
# which is how the terminal ended up billed for 200 MiB of mesa.
LINUX_GFX_RE = re.compile(
	r"""^(?:
		  libGL | libGLX | libGLdispatch | libGLESv\d | libEGL | libglapi
		| libgallium | libvulkan | libVkLayer | libgbm | libdrm
		| libnvidia | libcuda | libnvcuvid
		| .*_dri | swrast | iris | crocus | radeonsi | nouveau | zink | virtio_gpu | lvp
		| libxcb-dri\d | libxcb-present | libxcb-sync | libxcb-glx | libxcb-dri
		| libxshmfence | libpciaccess | libsensors
	)(?=[._-]|$)""",
	re.X,
)

# Present on any base install, so not something a terminal makes you install.
LINUX_BASE_RE = re.compile(
	r"^(?:ld-linux.*|libc|libm|libdl|librt|libpthread|libgcc_s|libresolv|libnsl|libutil)\b"
)

LINUX_LIB_DIRS = ("/usr/lib/x86_64-linux-gnu", "/lib/x86_64-linux-gnu", "/usr/lib", "/lib")


class LinuxBackend:
	name = "linux"

	def __init__(self) -> None:
		self._needed: dict[str, list[str]] = {}

	def norm(self, path: str) -> str:
		try:
			return str(Path(path).resolve())
		except OSError:
			return path

	def base_name(self, path: str) -> str:
		"""Strip the version tail so libX11.so.6.4.0 and libX11.so.6 compare equal."""
		name = Path(path).name
		cut = name.find(".so")
		return name[:cut] if cut >= 0 else name

	def is_library(self, path: str) -> bool:
		return ".so" in Path(path).name

	def is_gfx(self, path: str) -> bool:
		return bool(LINUX_GFX_RE.match(self.base_name(path)))

	def is_base_os(self, path: str) -> bool:
		return bool(LINUX_BASE_RE.match(self.base_name(path)))

	def needed(self, path: str) -> list[str]:
		"""DT_NEEDED entries of one ELF file."""
		if path in self._needed:
			return self._needed[path]
		out: list[str] = []
		try:
			raw = subprocess.run(
				["objdump", "-p", path], capture_output=True, text=True, timeout=30, check=False
			).stdout
			out = re.findall(r"^\s*NEEDED\s+(\S+)", raw, re.M)
		except Exception:
			pass
		self._needed[path] = out
		return out

	def find_library(self, soname: str) -> str | None:
		for root in LINUX_LIB_DIRS:
			cand = Path(root) / soname
			if cand.exists():
				return self.norm(str(cand))
		return None

	def mapped_files(self, pid: int) -> set[str]:
		"""Distinct real paths this process has mapped from disk."""
		paths: set[str] = set()
		try:
			with Path(f"/proc/{pid}/maps").open() as fh:
				for line in fh:
					parts = line.split(None, 5)
					if len(parts) < 6:
						continue
					path = parts[5].strip()
					if path.startswith("/") and not path.startswith(("/dev/", "/memfd", "/SYSV")):
						paths.add(self.norm(path))
		except OSError:
			pass
		return paths

	def regions(self, pid: int) -> list[tuple[str, int, int]]:
		"""Per-mapping private and shared resident bytes, keyed by backing file."""
		rows: list[tuple[str, int, int]] = []
		path, priv, shared = "", 0, 0
		try:
			with Path(f"/proc/{pid}/smaps").open() as fh:
				for line in fh:
					if re.match(r"^[0-9a-f]+-[0-9a-f]+ ", line):
						rows.append((path, priv, shared))
						parts = line.split(None, 5)
						path = parts[5].strip() if len(parts) >= 6 else ""
						priv = shared = 0
					elif line.startswith("Private_"):
						priv += int(line.split()[1]) * 1024
					elif line.startswith("Shared_"):
						shared += int(line.split()[1]) * 1024
			rows.append((path, priv, shared))
		except OSError:
			return []
		return [(p if p.startswith("/") else "", pv, sh) for p, pv, sh in rows]

	def exe_of(self, pid: int) -> str:
		try:
			return self.norm(f"/proc/{pid}/exe")
		except OSError:
			return ""

	def parent_of(self, pid: int) -> int:
		try:
			with Path(f"/proc/{pid}/stat").open() as fh:
				data = fh.read()
			# comm can contain spaces and parens, so start after the last ')'.
			return int(data[data.rfind(")") + 2:].split()[1])
		except (OSError, ValueError, IndexError):
			return 0

	def children_of(self, pid: int) -> list[int]:
		try:
			with Path(f"/proc/{pid}/task/{pid}/children").open() as fh:
				return [int(x) for x in fh.read().split()]
		except (OSError, ValueError):
			pass
		#	That file needs a kernel option that is usually but not always on, so fall back
		#	to reading every process's parent.
		out = []
		for entry in Path("/proc").iterdir():
			if entry.name.isdigit() and self.parent_of(int(entry.name)) == pid:
				out.append(int(entry.name))
		return out


#••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
#	Windows: the process API
#••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••

# Vendor drivers, the runtime loaders in front of them, and the software back ends that
# bundled browsers ship. Matched on the stem, case-folded.
WIN_GFX_RE = re.compile(
	r"""^(?:
		  opengl32 | glu32 | vulkan-\d+ | dxgi | dxcore | dxil | d3d\d+.* | d3dcompiler.*
		| nvoglv\d+ | nvapi\d* | nvcuda | nvml | nvgpucomp.* | nvldumd.* | nvwgf2.*
		| ig\dicd\d+ | igvk\d+ | igd\w* | igc\w* | intelcl.*
		| amdxc\d+ | amdvlk\d+ | amdihk\d+ | atio\w* | atiadl\w* | aticfx\w*
		| libglesv\d | libegl | vk_swiftshader | swiftshader.* | vulkan_rt.*
		| lvp\w* | llvmpipe.* | mesa\w*
	)$""",
	re.X,
)


def _win_paths() -> tuple[str, ...]:
	root = os.environ.get("SystemRoot", r"C:\Windows")
	return tuple(os.path.normcase(str(Path(root) / d))
	             for d in ("system32", "syswow64", "winsxs"))


class WindowsBackend:
	"""Reads a live process through the process API.

	Deliberately the same shape as the Linux collector: module list in place of the mapped
	library set, mapped regions plus their resident pages in place of smaps, PE imports in
	place of DT_NEEDED. What differs is what counts as the base OS - on Linux that is only
	the C runtime, because a desktop library is something you installed, while on Windows
	everything under System32 ships with the machine. That difference is real, but it means
	the two platforms' File+deps figures answer slightly different questions.
	"""

	name = "windows"

	def __init__(self) -> None:
		import ctypes
		from ctypes import wintypes

		self.ct = ctypes
		self.wt = wintypes
		self._needed: dict[str, list[str]] = {}
		self._sys_dirs = _win_paths()
		#	WinDLL is only in ctypes on Windows, where this runs, so a check on Linux
		#	cannot see it.
		self._k32 = ctypes.WinDLL("kernel32", use_last_error=True)  # type: ignore[attr-defined]
		self._psapi = ctypes.WinDLL("psapi", use_last_error=True)  # type: ignore[attr-defined]
		self._page = 4096
		self._devmap: dict[str, str] | None = None
		self._handles: dict[int, int | None] = {}
		self._declare()

	#	--- API declarations -------------------------------------------------------------

	def _declare(self) -> None:
		#	The same modules as self.ct and self.wt, named here so the structs below
		#	have a real base class to check against.
		import ctypes as ct
		from ctypes import wintypes as wt

		k32, psapi = self._k32, self._psapi

		class MEMORY_BASIC_INFORMATION(ct.Structure):
			#	The 4 bytes after AllocationProtect and after Type are padding on 64-bit.
			#	Newer headers name the first of them PartitionId; same offset either way.
			if ct.sizeof(ct.c_void_p) == 8:
				_fields_ = [("BaseAddress", ct.c_void_p),
				            ("AllocationBase", ct.c_void_p),
				            ("AllocationProtect", wt.DWORD),
				            ("_pad1", wt.DWORD),
				            ("RegionSize", ct.c_size_t),
				            ("State", wt.DWORD),
				            ("Protect", wt.DWORD),
				            ("Type", wt.DWORD),
				            ("_pad2", wt.DWORD)]
			else:
				_fields_ = [("BaseAddress", ct.c_void_p),
				            ("AllocationBase", ct.c_void_p),
				            ("AllocationProtect", wt.DWORD),
				            ("RegionSize", ct.c_size_t),
				            ("State", wt.DWORD),
				            ("Protect", wt.DWORD),
				            ("Type", wt.DWORD)]

		class WS_EX_INFO(ct.Structure):
			#	VirtualAttributes is a bitfield; read it whole and pick bits out, which is
			#	steadier across compilers than declaring c_ulonglong bitfields in ctypes.
			_fields_ = [("VirtualAddress", ct.c_void_p),
			            ("VirtualAttributes", ct.c_size_t)]

		class PROCESSENTRY32W(ct.Structure):
			#	Fixed widths, not c_long: it is 4 bytes here and 8 on most of the world, and
			#	a struct whose layout the caller has to get exactly right is no place for a
			#	type whose size moves.
			_fields_ = [("dwSize", wt.DWORD),
			            ("cntUsage", wt.DWORD),
			            ("th32ProcessID", wt.DWORD),
			            ("th32DefaultHeapID", ct.c_size_t),
			            ("th32ModuleID", wt.DWORD),
			            ("cntThreads", wt.DWORD),
			            ("th32ParentProcessID", wt.DWORD),
			            ("pcPriClassBase", ct.c_int32),
			            ("dwFlags", wt.DWORD),
			            ("szExeFile", ct.c_wchar * 260)]

		class SYSTEM_INFO(ct.Structure):
			_fields_ = [("wProcessorArchitecture", wt.WORD),
			            ("wReserved", wt.WORD),
			            ("dwPageSize", wt.DWORD),
			            ("lpMinimumApplicationAddress", ct.c_void_p),
			            ("lpMaximumApplicationAddress", ct.c_void_p),
			            ("dwActiveProcessorMask", ct.c_size_t),   ## DWORD_PTR, an integer
			            ("dwNumberOfProcessors", wt.DWORD),
			            ("dwProcessorType", wt.DWORD),
			            ("dwAllocationGranularity", wt.DWORD),
			            ("wProcessorLevel", wt.WORD),
			            ("wProcessorRevision", wt.WORD)]

		self.MBI, self.WSEX, self.PE32, self.SYSINFO = (
			MEMORY_BASIC_INFORMATION, WS_EX_INFO, PROCESSENTRY32W, SYSTEM_INFO)

		#	These layouts are a contract with the OS, and getting one wrong does not fail -
		#	it reads neighboring bytes and reports confident nonsense. Check the documented
		#	sizes up front so a bad declaration says so instead.
		if ct.sizeof(ct.c_void_p) == 8:
			for cls, want in ((MEMORY_BASIC_INFORMATION, 48), (WS_EX_INFO, 16),
			                  (PROCESSENTRY32W, 568), (SYSTEM_INFO, 48)):
				if ct.sizeof(cls) != want:
					raise SystemExit(f"{cls.__name__} came out {ct.sizeof(cls)} bytes, expected {want} - the struct "
					                 "declaration does not match this Windows")

		k32.OpenProcess.argtypes = [wt.DWORD, wt.BOOL, wt.DWORD]
		k32.OpenProcess.restype = wt.HANDLE
		k32.CloseHandle.argtypes = [wt.HANDLE]
		k32.CloseHandle.restype = wt.BOOL
		k32.VirtualQueryEx.argtypes = [wt.HANDLE, ct.c_void_p,
		                               ct.POINTER(MEMORY_BASIC_INFORMATION), ct.c_size_t]
		k32.VirtualQueryEx.restype = ct.c_size_t
		k32.QueryFullProcessImageNameW.argtypes = [wt.HANDLE, wt.DWORD, wt.LPWSTR,
		                                           ct.POINTER(wt.DWORD)]
		k32.QueryFullProcessImageNameW.restype = wt.BOOL
		k32.QueryDosDeviceW.argtypes = [wt.LPCWSTR, wt.LPWSTR, wt.DWORD]
		k32.QueryDosDeviceW.restype = wt.DWORD
		k32.CreateToolhelp32Snapshot.argtypes = [wt.DWORD, wt.DWORD]
		k32.CreateToolhelp32Snapshot.restype = wt.HANDLE
		k32.Process32FirstW.argtypes = [wt.HANDLE, ct.POINTER(PROCESSENTRY32W)]
		k32.Process32FirstW.restype = wt.BOOL
		k32.Process32NextW.argtypes = [wt.HANDLE, ct.POINTER(PROCESSENTRY32W)]
		k32.Process32NextW.restype = wt.BOOL
		k32.GetSystemInfo.argtypes = [ct.POINTER(SYSTEM_INFO)]
		k32.GetSystemInfo.restype = None

		psapi.EnumProcessModulesEx.argtypes = [wt.HANDLE, ct.POINTER(wt.HMODULE), wt.DWORD,
		                                       ct.POINTER(wt.DWORD), wt.DWORD]
		psapi.EnumProcessModulesEx.restype = wt.BOOL
		psapi.GetModuleFileNameExW.argtypes = [wt.HANDLE, wt.HMODULE, wt.LPWSTR, wt.DWORD]
		psapi.GetModuleFileNameExW.restype = wt.DWORD
		psapi.GetMappedFileNameW.argtypes = [wt.HANDLE, ct.c_void_p, wt.LPWSTR, wt.DWORD]
		psapi.GetMappedFileNameW.restype = wt.DWORD
		psapi.QueryWorkingSetEx.argtypes = [wt.HANDLE, ct.c_void_p, wt.DWORD]
		psapi.QueryWorkingSetEx.restype = wt.BOOL

		info = SYSTEM_INFO()
		k32.GetSystemInfo(self.ct.byref(info))
		if info.dwPageSize:
			self._page = int(info.dwPageSize)

	#	--- handles ----------------------------------------------------------------------

	PROCESS_QUERY_INFORMATION = 0x0400
	PROCESS_VM_READ = 0x0010
	PROCESS_QUERY_LIMITED = 0x1000

	def _open(self, pid: int) -> int | None:
		"""A handle good enough to read maps, or a weaker one, or none."""
		if pid in self._handles:
			return self._handles[pid]
		handle = self._k32.OpenProcess(
			self.PROCESS_QUERY_INFORMATION | self.PROCESS_VM_READ, False, pid)
		if not handle:
			handle = self._k32.OpenProcess(self.PROCESS_QUERY_LIMITED, False, pid)
		self._handles[pid] = handle or None
		return self._handles[pid]

	def close(self) -> None:
		for handle in self._handles.values():
			if handle:
				self._k32.CloseHandle(handle)
		self._handles.clear()

	#	--- naming -----------------------------------------------------------------------

	#	abspath, since it also folds ".." away, which Path.absolute() does not.
	def norm(self, path: str) -> str:
		return os.path.normcase(os.path.abspath(path)) if path else ""  # noqa: PTH100

	def base_name(self, path: str) -> str:
		stem = Path(path).name
		if stem.lower().endswith(".dll"):
			stem = stem[:-4]
		return stem.lower()

	def is_library(self, path: str) -> bool:
		return path.lower().endswith(".dll")

	def is_gfx(self, path: str) -> bool:
		return bool(WIN_GFX_RE.match(self.base_name(path)))

	def is_base_os(self, path: str) -> bool:
		"""Ships with the machine, so installing a terminal does not bring it with it.

		Path-based rather than a name list: System32 holds well over two thousand DLLs and
		any list of them goes stale. This is where Windows and Linux genuinely differ -
		user32 and gdi32 are the OS, whereas their Linux counterparts are packages.
		"""
		low = self.norm(path)
		return any(low.startswith(d) for d in self._sys_dirs)

	def find_library(self, name: str) -> str | None:
		for root in self._sys_dirs:
			cand = Path(root) / name
			if cand.exists():
				return self.norm(str(cand))
		return None

	def needed(self, path: str) -> list[str]:
		if path not in self._needed:
			self._needed[path] = pe_imports(path)
		return self._needed[path]

	#	--- reading a process ------------------------------------------------------------

	def _device_map(self) -> dict[str, str]:
		"""\\Device\\HarddiskVolume3 -> C:, so mapped names can be opened and stat'd."""
		if self._devmap is not None:
			return self._devmap
		self._devmap = {}
		buf = self.ct.create_unicode_buffer(1024)
		for letter in "ABCDEFGHIJKLMNOPQRSTUVWXYZ":
			drive = f"{letter}:"
			if self._k32.QueryDosDeviceW(drive, buf, 1024):
				self._devmap[buf.value.lower()] = drive
		return self._devmap

	def _from_device_path(self, path: str) -> str:
		low = path.lower()
		for dev, drive in self._device_map().items():
			if low.startswith(dev + "\\"):
				return drive + path[len(dev):]
		return ""

	def mapped_files(self, pid: int) -> set[str]:
		"""Every module the process has loaded, however it was loaded.

		The module list is the right source rather than the import table, for the same
		reason the Linux side reads a live process: anything loaded at runtime - a vendor
		back end, a plugin - is invisible to the static list.
		"""
		handle = self._open(pid)
		if not handle:
			return set()
		ct, wt = self.ct, self.wt
		count = 1024
		for _ in range(4):
			arr = (wt.HMODULE * count)()
			need = wt.DWORD(0)
			if not self._psapi.EnumProcessModulesEx(
					handle, arr, ct.sizeof(arr), ct.byref(need), 0x03):     ## LIST_MODULES_ALL
				return set()
			got = need.value // ct.sizeof(wt.HMODULE)
			if got <= count:
				break
			count = got + 64
		out: set[str] = set()
		buf = ct.create_unicode_buffer(32768)
		for i in range(min(got, count)):
			if self._psapi.GetModuleFileNameExW(handle, arr[i], buf, 32768):
				out.add(self.norm(buf.value))
		return out

	def regions(self, pid: int) -> list[tuple[str, int, int]]:
		"""Per-region private and shared resident bytes, keyed by backing file.

		A region's pages are looked up in the working set: a page that is not valid is not
		resident and costs nothing, and one flagged shared is charged once across the tree
		rather than to each process holding it.
		"""
		handle = self._open(pid)
		if not handle:
			return []
		ct = self.ct
		rows: list[tuple[str, int, int]] = []
		mbi = self.MBI()
		addr = 0
		limit = (1 << 47) if ct.sizeof(ct.c_void_p) == 8 else (1 << 31)
		while addr < limit:
			if not self._k32.VirtualQueryEx(handle, ct.c_void_p(addr), ct.byref(mbi),
			                                ct.sizeof(mbi)):
				break
			size = int(mbi.RegionSize)
			if size <= 0:
				break
			if mbi.State == 0x1000:                                       ## MEM_COMMIT
				path = ""
				if mbi.Type in (0x1000000, 0x40000):                       ## IMAGE, MAPPED
					path = self._mapped_name(handle, addr)
				priv, shared = self._resident(handle, addr, size)
				if priv or shared:
					rows.append((path, priv, shared))
			addr += size
		return rows

	def _mapped_name(self, handle: int, addr: int) -> str:
		buf = self.ct.create_unicode_buffer(32768)
		if not self._psapi.GetMappedFileNameW(handle, self.ct.c_void_p(addr), buf, 32768):
			return ""
		return self.norm(self._from_device_path(buf.value))

	def _resident(self, handle: int, addr: int, size: int) -> tuple[int, int]:
		"""(private, shared) resident bytes in one region."""
		ct = self.ct
		page = self._page
		pages = size // page
		if pages <= 0:
			return 0, 0
		priv = shared = 0
		done = 0
		chunk = 4096
		while done < pages:
			take = min(chunk, pages - done)
			arr = (self.WSEX * take)()
			for i in range(take):
				arr[i].VirtualAddress = ct.c_void_p(addr + (done + i) * page)
			if not self._psapi.QueryWorkingSetEx(handle, ct.byref(arr), ct.sizeof(arr)):
				return priv, shared
			for i in range(take):
				attrs = int(arr[i].VirtualAttributes)
				if not attrs & 1:                                          ## not resident
					continue
				if (attrs >> 15) & 1:                                      ## shared
					shared += page
				else:
					priv += page
			done += take
		return priv, shared

	def exe_of(self, pid: int) -> str:
		handle = self._open(pid)
		if not handle:
			return ""
		size = self.wt.DWORD(32768)
		buf = self.ct.create_unicode_buffer(32768)
		if self._k32.QueryFullProcessImageNameW(handle, 0, buf, self.ct.byref(size)):
			return self.norm(buf.value)
		return ""

	def _snapshot(self) -> dict[int, int]:
		"""(pid -> parent) for every process, read once."""
		ct = self.ct
		out: dict[int, int] = {}
		snap = self._k32.CreateToolhelp32Snapshot(0x2, 0)                  ## SNAPPROCESS
		if not snap or snap == self.wt.HANDLE(-1).value:
			return out
		try:
			entry = self.PE32()
			entry.dwSize = ct.sizeof(self.PE32)
			ok = self._k32.Process32FirstW(snap, ct.byref(entry))
			while ok:
				out[int(entry.th32ProcessID)] = int(entry.th32ParentProcessID)
				ok = self._k32.Process32NextW(snap, ct.byref(entry))
		finally:
			self._k32.CloseHandle(snap)
		return out

	def parent_of(self, pid: int) -> int:
		return self._snapshot().get(pid, 0)

	def children_of(self, pid: int) -> list[int]:
		return [kid for kid, parent in self._snapshot().items() if parent == pid]

	def console_owner(self) -> int:
		"""Whichever process owns this console window, or 0.

		The ancestor walk cannot find a classic console on its own: conhost is attached to
		the console program as a CHILD, not a parent, so walking up from here goes straight
		past it to the shell and out to the desktop. Asking the console window who owns it
		is the only way round that, and it answers for the modern hosts too.
		"""
		ct, wt = self.ct, self.wt
		try:
			user32 = ct.WinDLL("user32", use_last_error=True)
			self._k32.GetConsoleWindow.restype = wt.HWND
			hwnd = self._k32.GetConsoleWindow()
			if not hwnd:
				return 0
			user32.GetWindowThreadProcessId.argtypes = [wt.HWND, ct.POINTER(wt.DWORD)]
			user32.GetWindowThreadProcessId.restype = wt.DWORD
			pid = wt.DWORD(0)
			user32.GetWindowThreadProcessId(hwnd, ct.byref(pid))
			return int(pid.value)
		except Exception:
			return 0


#••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
#	PE imports, for the Windows dependency closure
#••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••

def _u16(buf: bytes, off: int) -> int:
	return int.from_bytes(buf[off:off + 2], "little")


def _u32(buf: bytes, off: int) -> int:
	return int.from_bytes(buf[off:off + 4], "little")


def _cstr(buf: bytes, off: int) -> str:
	end = buf.find(b"\0", off)
	return buf[off:end if end >= 0 else len(buf)].decode("latin-1")


def pe_imports(path: str) -> list[str]:
	"""DLL names one PE file imports, both the ordinary and the delay-loaded tables.

	Pure parsing rather than a tool call, so it works from either platform - which is what
	lets the closure logic be checked on the machine that has no Windows on it.

	A managed assembly returns nothing, because the runtime resolves its references instead
	of the loader. That costs the closure some edges, so a .NET library only ever ends in
	the app's column - which is the safe direction: it can leave a driver-side library
	billed to the terminal, never the reverse.
	"""
	try:
		buf = Path(path).read_bytes()
	except OSError:
		return []
	if len(buf) < 0x40 or buf[:2] != b"MZ":
		return []
	pe = _u32(buf, 0x3C)
	if pe <= 0 or pe + 24 > len(buf) or buf[pe:pe + 4] != b"PE\0\0":
		return []

	opt_size = _u16(buf, pe + 20)
	opt = pe + 24
	if opt + 2 > len(buf):
		return []
	magic = _u16(buf, opt)
	if magic == 0x20B:
		dirs = opt + 112
	elif magic == 0x10B:
		dirs = opt + 96
	else:
		return []

	# Section table, for turning a virtual address back into a file offset.
	nsec = _u16(buf, pe + 6)
	sec = opt + opt_size
	sections: list[tuple[int, int, int, int]] = []
	for i in range(nsec):
		base = sec + i * 40
		if base + 40 > len(buf):
			break
		sections.append((_u32(buf, base + 12), _u32(buf, base + 8),
		                 _u32(buf, base + 16), _u32(buf, base + 20)))

	def to_offset(rva: int) -> int:
		for va, vsize, rawsize, rawptr in sections:
			span = max(vsize, rawsize)
			if va <= rva < va + span:
				off = rva - va + rawptr
				return off if off < len(buf) else -1
		return -1

	def dir_entry(index: int) -> tuple[int, int]:
		base = dirs + index * 8
		if base + 8 > len(buf):
			return 0, 0
		return _u32(buf, base), _u32(buf, base + 4)

	out: list[str] = []

	# Ordinary imports: descriptors of 20 bytes, name at +12, terminated by a zero entry.
	rva, _ = dir_entry(1)
	off = to_offset(rva) if rva else -1
	if off >= 0:
		while off + 20 <= len(buf):
			name_rva = _u32(buf, off + 12)
			if not name_rva and not _u32(buf, off) and not _u32(buf, off + 16):
				break
			noff = to_offset(name_rva)
			if noff >= 0:
				out.append(_cstr(buf, noff))
			off += 20

	# Delay-loaded imports: 32 bytes, name at +4. When bit 0 of the attributes is clear the
	# fields are addresses rather than offsets, an old form no current linker emits; skip it
	# rather than mis-resolving into the middle of the file.
	rva, _ = dir_entry(13)
	off = to_offset(rva) if rva else -1
	if off >= 0:
		while off + 32 <= len(buf):
			attrs = _u32(buf, off)
			name_rva = _u32(buf, off + 4)
			if not name_rva:
				break
			if attrs & 1:
				noff = to_offset(name_rva)
				if noff >= 0:
					out.append(_cstr(buf, noff))
			off += 32

	#	First spelling of each name, any case.
	firsts: dict[str, str] = {}
	for n in out:
		if n:
			firsts.setdefault(n.lower(), n)
	return list(firsts.values())


#••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
#	Finding the terminal to measure
#••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••

def make_backend() -> Backend:
	if sys.platform.startswith("win"):
		return WindowsBackend()
	if sys.platform.startswith("linux"):
		return LinuxBackend()
	raise SystemExit(f"no collector for {sys.platform} - only Linux and Windows are supported")


# Anything that can sit between this script and the terminal window.
SHELLISH = {"bash", "sh", "dash", "zsh", "fish", "ksh", "csh", "tcsh", "python", "python3",
            "pwsh", "powershell", "cmd", "conhost", "openconsole", "login", "su", "sudo",
            "env", "screen", "script", "py", "winpty", "sizebench-classify.py"}

# Console hosts, which draw a window but are not the thing you installed.
CONSOLE_HOSTS = {"conhost", "openconsole"}

#	Anything that could be running this tool rather than being measured by it.
INTERPRETERS = {"python", "python3", "pythonw", "py"}


def stem_of(path: str) -> str:
	name = Path(path).name.lower()
	return name[:-4] if name.endswith(".exe") else name


def tree_of(be: Backend, root: int) -> list[int]:
	"""A pid and everything under it."""
	out, queue = [root], [root]
	while queue:
		pid = queue.pop(0)
		for kid in be.children_of(pid):
			if kid not in out:
				out.append(kid)
				queue.append(kid)
	return out


def find_terminal(be: Backend) -> tuple[list[int], str]:
	"""(pids, exe) for the terminal drawing this session, or ([], '').

	Three shapes to cope with. A terminal that spawns the shell is an ancestor, so walking
	up the tree finds it. A classic Windows console is the other way round - the host hangs
	off the console program as a child - so walking up goes straight past it to the desktop,
	and the console window has to be asked who owns it instead. A modern host sits between
	the two: owned by a window process which is the real terminal, with the shell below it.

	Whatever is found, this process and anything it started are dropped again. The measuring
	tool is inside the tree it is measuring, and billing a terminal for the interpreter that
	happens to be reading it would add tens of MiB that no other row carries.
	"""
	pids: list[int] = []
	exe = ""

	owner = be.console_owner() if hasattr(be, "console_owner") else 0
	program = owner
	if owner and stem_of(be.exe_of(owner)) not in CONSOLE_HOSTS:
		#	Windows 11 answers this query with the console PROGRAM, not the host, so the
		#	host has to be found beside it: a child where the system attached one, a parent
		#	where it was launched explicitly. Only adjacent - walking further up leaves the
		#	session entirely and ends up on whatever started it.
		for near in [be.parent_of(owner)] + be.children_of(owner):
			if near > 1 and stem_of(be.exe_of(near)) in CONSOLE_HOSTS:
				owner = near
				break

	if owner:
		exe = be.exe_of(owner)
		if stem_of(exe) in CONSOLE_HOSTS:
			parent = be.parent_of(owner)
			above = be.exe_of(parent) if parent > 1 else ""
			if above and stem_of(above) not in SHELLISH:
				#	A window process owns the host, so that is the terminal.
				pids, exe = tree_of(be, parent), above
			elif program and program != owner and program in tree_of(be, owner):
				#	The host was launched explicitly and the console program runs below
				#	it, so the pair is the host's own tree.
				pids = tree_of(be, owner)
			else:
				#	Classic console: the host draws and the program it is attached to runs
				#	in it, which is the same pair measured elsewhere as terminal plus shell.
				pids = tree_of(be, parent) if parent > 1 else tree_of(be, owner)
		else:
			#	No host beside it, so nothing came between: the terminal spawned this
			#	program itself and sits above it. Walk up past the shells to the window
			#	process, which is what actually draws - stopping at the program would
			#	measure the shell and call it the terminal.
			up = be.parent_of(owner)
			for _ in range(8):
				if up <= 1:
					break
				got = be.exe_of(up)
				if got and stem_of(got) not in SHELLISH:
					pids, exe = tree_of(be, up), got
					break
				up = be.parent_of(up)
			if not pids:
				pids = tree_of(be, owner)

	if not pids:
		pid = os.getpid()
		for _ in range(12):
			pid = be.parent_of(pid)
			if pid <= 1:
				break
			got = be.exe_of(pid)
			if not got:
				continue
			if stem_of(got) in SHELLISH:
				continue
			pids, exe = tree_of(be, pid), got
			break

	mine = set(tree_of(be, os.getpid()))
	#	The wrapper runs this script, so the interpreter above is part of the measuring
	#	tool too and has to go the same way its child does - billing a terminal for it adds
	#	about 19 MiB that no rig-measured row carries. Only interpreters are dropped, so the
	#	walk stops at the shell, which the terminal is entitled to.
	runner = be.parent_of(os.getpid())
	while runner > 1 and stem_of(be.exe_of(runner)) in INTERPRETERS:
		mine.add(runner)
		runner = be.parent_of(runner)
	return [p for p in pids if p not in mine], exe


def console_grid() -> tuple[int, int] | None:
	"""(columns, rows) straight from the console, on Windows only.

	Asking the standard streams cannot work here. The wrapper reads this script through a
	pipe with stderr folded into it, and stdin is an input handle, which no grid query
	answers - so all three fail and the run is refused however the window is sized. CONOUT$
	is the screen buffer itself and answers whatever the streams have been redirected to.
	Python's own get_terminal_size cannot be used on it: on Windows it resolves a file
	descriptor back to one of the three standard handles, so any other one is rejected.
	"""
	import ctypes
	from ctypes import wintypes

	class ScreenBuffer(ctypes.Structure):
		_fields_ = [
			("dwSize", wintypes._COORD),
			("dwCursorPosition", wintypes._COORD),
			("wAttributes", wintypes.WORD),
			("srWindow", wintypes.SMALL_RECT),
			("dwMaximumWindowSize", wintypes._COORD),
		]

	kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)  # type: ignore[attr-defined]
	handle = kernel32.CreateFileW("CONOUT$", 0xC000_0000, 3, None, 3, 0, None)
	if handle == -1:
		return None
	try:
		info = ScreenBuffer()
		if not kernel32.GetConsoleScreenBufferInfo(wintypes.HANDLE(handle), ctypes.byref(info)):
			return None
		#	The visible window, not dwSize: the buffer stays tall for scrollback.
		box = info.srWindow
		return (box.Right - box.Left + 1, box.Bottom - box.Top + 1)
	finally:
		kernel32.CloseHandle(wintypes.HANDLE(handle))


def terminal_grid() -> tuple[int, int] | None:
	"""(columns, rows) from whichever standard stream is still a terminal.

	All three are tried because the wrapper reads this script's output through a pipe, and
	asking only stdout would then report no terminal at all and refuse every run.
	"""
	if os.name == "nt":
		try:
			got = console_grid()
		except Exception:
			got = None
		if got:
			return got
	for stream in (sys.__stdout__, sys.__stderr__, sys.__stdin__):
		if stream is None:
			continue
		try:
			size = os.get_terminal_size(stream.fileno())
			return (size.columns, size.lines)
		except (OSError, ValueError, AttributeError):
			continue
	return None


def check_grid(strict: bool) -> bool:
	"""Refuse a measurement at the wrong window size, which is the trap that voids one."""
	got = terminal_grid()
	if got == GRID:
		return True
	shown = f"{got[0]}x{got[1]}" if got else "unknown"
	msg = (f"this terminal is {shown}, and the table's rows are all measured at {GRID[0]}x{GRID[1]} - memory "
	       "scales with the surface, so a figure taken at another size is not comparable")
	if strict:
		print(f"REFUSED: {msg}", file=sys.stderr)
		return False
	print(f"WARNING: {msg}", file=sys.stderr)
	return True


#••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
#	Self-check
#••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••

def selftest(be: Backend) -> int:
	"""Measure this process, where the answer can be checked against something else.

	There is no published row on Windows to reproduce, which is how a new rig is normally
	validated, so this stands in for it: for a single process the unique footprint is just
	its resident set, and the two are arrived at by completely different routes. They will
	not match exactly - the resident set moves while it is being read - but a real error in
	the page walk shows up as a wrong order of magnitude, not a few percent.
	"""
	pid = os.getpid()
	rows = be.regions(pid)
	total = sum(pv + sh for _, pv, sh in rows) / MIB
	print(f"  regions        {len(rows)}")
	print(f"  resident       {total:8.2f} MiB  (private {sum(pv for _, pv, _ in rows) / MIB:.2f}, "
	      f"shared {sum(sh for _, _, sh in rows) / MIB:.2f})")

	other = reference_resident(be, pid)
	if other is None:
		print("  no second opinion available on this platform")
		return 0
	print(f"  reported       {other:8.2f} MiB  by the platform's own counter")
	if total <= 0 or other <= 0:
		print("  FAIL: one of the two read as zero")
		return 1
	drift = abs(total - other) / other
	print(f"  agreement      {100 * (1 - drift):8.1f}%")
	if drift > 0.25:
		print("  FAIL: the two disagree by more than a quarter, so the page walk is wrong")
		return 1
	print("  OK")

	mods = be.mapped_files(pid)
	print(f"  modules        {len(mods)}")
	if not mods:
		print("  FAIL: no modules found for this process")
		return 1
	return 0


def reference_resident(be: Backend, pid: int) -> float | None:
	"""Resident set as the platform itself reports it, in MiB."""
	if be.name == "linux":
		try:
			with Path(f"/proc/{pid}/statm").open() as fh:
				return int(fh.read().split()[1]) * 4096 / MIB
		except (OSError, ValueError, IndexError):
			return None
	if not isinstance(be, WindowsBackend):
		return None
	try:
		import ctypes
		from ctypes import wintypes

		class COUNTERS(ctypes.Structure):
			_fields_ = [("cb", wintypes.DWORD),
			            ("PageFaultCount", wintypes.DWORD),
			            ("PeakWorkingSetSize", ctypes.c_size_t),
			            ("WorkingSetSize", ctypes.c_size_t),
			            ("QuotaPeakPagedPoolUsage", ctypes.c_size_t),
			            ("QuotaPagedPoolUsage", ctypes.c_size_t),
			            ("QuotaPeakNonPagedPoolUsage", ctypes.c_size_t),
			            ("QuotaNonPagedPoolUsage", ctypes.c_size_t),
			            ("PagefileUsage", ctypes.c_size_t),
			            ("PeakPagefileUsage", ctypes.c_size_t)]

		psapi = ctypes.WinDLL("psapi", use_last_error=True)  # type: ignore[attr-defined]
		info = COUNTERS()
		info.cb = ctypes.sizeof(COUNTERS)
		handle = be._open(pid)
		if not handle or not psapi.GetProcessMemoryInfo(handle, ctypes.byref(info), info.cb):
			return None
		return float(info.WorkingSetSize) / MIB
	except Exception:
		return None


#••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
#	Entry
#••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••

def main() -> int:
	ap = argparse.ArgumentParser(description=__doc__.splitlines()[0] if __doc__ else None)
	ap.add_argument("pids", nargs="*", type=int, help="every pid in the terminal's tree")
	ap.add_argument("--here", action="store_true",
	                help="measure the terminal this is running inside")
	ap.add_argument("--any-size", action="store_true",
	                help="with --here, measure even at the wrong grid (not comparable)")
	ap.add_argument("--exe", action="append", default=[], help="executable, repeatable")
	ap.add_argument("--payload", help="extracted bundle dir, counted instead of the executable")
	ap.add_argument("--verbose", action="store_true", help="itemize both library sets")
	ap.add_argument("--json", action="store_true")
	ap.add_argument("--summary", action="store_true",
	                help="also print one RESULT line, for the wrapper to read")
	ap.add_argument("--selftest", action="store_true",
	                help="check the collector against this process and stop")
	ap.add_argument("--imports", metavar="FILE",
	                help="list what one PE file imports and stop")
	args = ap.parse_args()

	if args.imports:
		for name in pe_imports(args.imports):
			print(name)
		return 0

	be = make_backend()

	if args.selftest:
		return selftest(be)

	pids = list(args.pids)
	exes = [be.norm(e) for e in args.exe]

	if args.here:
		if pids:
			print("--here finds the terminal itself, so it takes no pids", file=sys.stderr)
			return 2
		if not check_grid(strict=not args.any_size):
			return 2
		pids, exe = find_terminal(be)
		if not pids:
			print("could not work out which terminal is drawing this session",
			      file=sys.stderr)
			return 2
		if not exes:
			exes = [exe] if exe else []
		#	Say what was picked: a wrong guess here is the difference between measuring the
		#	terminal and measuring the desktop, and it is not visible in the numbers.
		print(f"  found          {exe or '(unnamed)'}")
	elif not pids:
		print("give the terminal's pids, or --here to find them", file=sys.stderr)
		return 2

	if not exes:
		for pid in pids:
			got = be.exe_of(pid)
			if got:
				exes.append(got)
		exes = list(dict.fromkeys(exes))[:1]

	res = measure(pids, exes, be, verbose=args.verbose, payload=args.payload)

	# A self-contained bundle has no meaningful "executable" - what you install is the
	# whole extracted payload, plus the system libraries it still borrows.
	if args.payload:
		total = 0
		for root, _, files in os.walk(args.payload):
			for f in files:
				try:
					total += (Path(root) / f).lstat().st_size
				except OSError:
					pass
		res["exe_mib"] = total / MIB
		res["file_deps_mib"] = res["exe_mib"] + res["deps_mib"]

	if args.json:
		print(json.dumps(res, indent=2))
		return 0

	print(f"  processes      {len(res['pids'])}  {res['pids']}")
	print(f"  file size      {res['exe_mib']:8.1f} MiB")
	print(f"  own deps       {res['deps_mib']:8.1f} MiB  ({res['n_app_libs']} libraries)")
	print(f"  File+deps      {res['file_deps_mib']:8.1f} MiB")
	print(f"  Mem            {res['mem_mib']:8.1f} MiB")
	print(f"  driver (excl)  {res['driver_mem_mib']:8.1f} MiB resident, "
	      f"{res['driver_disk_mib']:.1f} MiB on disk, {res['n_driver_libs']} libraries")
	#	Said out loud rather than dropped quietly: a silent exclusion reads as a wrong number.
	if res.get("n_injected_libs"):
		print(f"  injected (excl){res['injected_mem_mib']:8.1f} MiB resident, "
		      f"{res['injected_disk_mib']:.1f} MiB on disk, {res['n_injected_libs']} libraries")
	if args.verbose:
		print("\n  billed to the terminal:")
		for size, path in res["app_libs"]:
			print(f"    {size:7.2f}  {path}")
		print("\n  billed to the driver:")
		for size, path in res["driver_libs"][:20]:
			print(f"    {size:7.2f}  {path}")
		if res.get("injected_libs"):
			print("\n  injected into every process, so billed to nobody:")
			for size, path in res["injected_libs"]:
				print(f"    {size:7.2f}  {path}")
	if args.summary:
		print(f"RESULT file={res['exe_mib']:.1f} filedeps={res['file_deps_mib']:.1f} "
		      f"mem={res['mem_mib']:.1f} driver={res['driver_mem_mib']:.1f}")
	return 0


if __name__ == "__main__":
	sys.exit(main())

##	History:
##		- 20260730 JC: Created.
##		- 20261005: Type hints, pathlib, f-strings.
