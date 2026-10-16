// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! Which monitor a window is on, told apart well enough to keep a window size
//! for each one: its resolution, the scale the window is drawn at there, and
//! its physical size where the platform says so cheaply. The physical size
//! comes from the X server on X11, the monitor's EDID on Windows and
//! `CGDisplayScreenSize` on macOS. Wayland gives a program no way to ask, so
//! there it is resolution and scale.

use winit::window::Window;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MonitorId {
	pub width: u32,
	pub height: u32,
	pub scale_pct: u32,
	pub size_mm: Option<(u32, u32)>,
}

impl MonitorId {
	/// The monitor the window is on now, at the scale the window is drawn at.
	/// None when the platform cannot say, which on Wayland is the case until
	/// the window has been shown.
	pub fn of_window(window: &Window) -> Option<Self> {
		Self::find(window, false)
	}

	/// The monitor a window not shown yet will open on. It has no place of its
	/// own until the window manager maps it, and xfwm4 maps a window that asks
	/// for no position on the monitor under the pointer. winit guesses the
	/// window's scale the same way.
	pub fn of_new_window(window: &Window) -> Option<Self> {
		Self::find(window, true)
	}

	#[cfg_attr(not(target_os = "linux"), allow(unused_variables))]
	fn find(window: &Window, at_pointer: bool) -> Option<Self> {
		let scale = window.scale_factor();
		#[cfg(target_os = "linux")]
		if let Some(under) = x11_monitor_under(window, at_pointer) {
			let (px, mm) = under?;
			return Self::new(px.0, px.1, scale, mm.map(|mm| upright(mm, px)));
		}
		#[cfg(target_os = "linux")]
		if let Some(on) = wayland::outputs_on(window) {
			let laid: Vec<Laid> = on.iter().map(|o| o.laid).collect();
			let at = laid_drawn_for(&laid, scale, logical_size(window, scale))?;
			let (w, h) = on[at].mode;
			return Self::new(w, h, scale, None);
		}
		let monitor = drawn_on(window, scale)?;
		let px = monitor.size();
		let size_mm = physical_mm(&monitor).map(|mm| upright(mm, (px.width, px.height)));
		Self::new(px.width, px.height, scale, size_mm)
	}

	pub fn new(width: u32, height: u32, scale: f64, size_mm: Option<(u32, u32)>) -> Option<Self> {
		if width == 0 || height == 0 || !scale.is_finite() || scale <= 0.0 {
			return None;
		}
		Some(Self {
			width,
			height,
			scale_pct: (scale * 100.0).round() as u32,
			size_mm: size_mm.and_then(plausible),
		})
	}

	/// `2560x1440_125pct`, with `_597x336mm` after it when the size is known.
	/// It is the monitor's name in the config file, so it keeps to what SHCL
	/// allows in a bare name: letters, digits, `-` and `_`.
	pub fn key(&self) -> String {
		let key = format!("{}x{}_{}pct", self.width, self.height, self.scale_pct);
		match self.size_mm {
			Some((w, h)) => format!("{key}_{w}x{h}mm"),
			None => key,
		}
	}

	/// A `key` read back.
	pub fn from_key(key: &str) -> Option<Self> {
		let pair = |text: &str| {
			let (a, b) = text.split_once('x')?;
			Some((a.parse().ok()?, b.parse().ok()?))
		};
		let mut parts = key.split('_');
		let (width, height) = pair(parts.next()?)?;
		let scale_pct = parts.next()?.strip_suffix("pct")?.parse().ok()?;
		let size_mm = match parts.next() {
			Some(mm) => Some(pair(mm.strip_suffix("mm")?)?),
			None => None,
		};
		if parts.next().is_some() {
			return None;
		}
		Some(Self {
			width,
			height,
			scale_pct,
			size_mm,
		})
	}

	/// The same monitor, read again? An EDID gives the size twice, to the mm
	/// and in whole cm, and the X server hands over either one. Which one can
	/// change across a power save: b23's portrait monitor has read as both
	/// 401x641 and 400x640 mm.
	pub fn same_as(&self, other: &Self) -> bool {
		let near = |a: u32, b: u32| a.abs_diff(b) < MM_SLACK;
		self.width == other.width
			&& self.height == other.height
			&& self.scale_pct == other.scale_pct
			&& match (self.size_mm, other.size_mm) {
				(None, None) => true,
				(Some(a), Some(b)) => near(a.0, b.0) && near(a.1, b.1),
				_ => false,
			}
	}
}

// Under a cm either way, which is all the rounding to whole cm can move it.
const MM_SLACK: u32 = 10;

/// Do two monitor keys name the same monitor? See `MonitorId::same_as`.
pub fn same_monitor(a: &str, b: &str) -> bool {
	a == b
		|| MonitorId::from_key(a)
			.zip(MonitorId::from_key(b))
			.is_some_and(|(a, b)| a.same_as(&b))
}

/// Where every monitor is. The window's own resize can make the window manager
/// move it, but never changes these, so a move made while they changed is the
/// window manager's.
pub fn layout(window: &Window) -> Vec<(i64, i64, i64, i64)> {
	// winit's list on X11 misses an output that comes back without the screen
	// changing size
	#[cfg(target_os = "linux")]
	if let Some(screen) = X11Screen::of(window) {
		return screen.crtcs.iter().map(crtc_rect).collect();
	}
	window
		.available_monitors()
		.map(|monitor| {
			let (at, size) = (monitor.position(), monitor.size());
			(
				i64::from(at.x),
				i64::from(at.y),
				i64::from(size.width),
				i64::from(size.height),
			)
		})
		.collect()
}

/// Where in the layout each output a Wayland window is on sits, first entered
/// first. None off Wayland, or where winit's state can't be read.
pub fn outputs_under(window: &Window) -> Option<Vec<(i64, i64, i64, i64)>> {
	#[cfg(target_os = "linux")]
	return wayland::outputs_on(window).map(|on| {
		on.iter()
			.map(|o| {
				let (x, y, w, h) = o.laid.bounds;
				(x as i64, y as i64, w as i64, h as i64)
			})
			.collect()
	});
	#[cfg(not(target_os = "linux"))]
	{
		let _ = window;
		None
	}
}

fn logical_size(window: &Window, scale: f64) -> (f64, f64) {
	let px = window.inner_size();
	(f64::from(px.width) / scale, f64::from(px.height) / scale)
}

// The monitor whose scale the window is drawn at. On Wayland winit's current
// monitor is the first output the window entered and still touches, while the
// scale is the one of the output most of it is on. Dropped with a sliver left
// on another output, the two differ, and the key read one output's resolution
// at the other's scale (2026101409220514). Elsewhere they agree. On Wayland
// this is the fallback, for when winit's own state can't be read.
fn drawn_on(window: &Window, scale: f64) -> Option<winit::monitor::MonitorHandle> {
	let first = window.current_monitor()?;
	if at_scale(first.scale_factor(), scale) {
		return Some(first);
	}
	// winit lists each Wayland output twice
	let mut monitors: Vec<winit::monitor::MonitorHandle> = vec![first];
	for monitor in window.available_monitors() {
		if !monitors.contains(&monitor) {
			monitors.push(monitor);
		}
	}
	let placed: Vec<Placed> = monitors
		.iter()
		.map(|m| Placed {
			at: (i64::from(m.position().x), i64::from(m.position().y)),
			px: (i64::from(m.size().width), i64::from(m.size().height)),
			scale: m.scale_factor(),
		})
		.collect();
	let at = other_at_scale(0, &placed, scale, logical_size(window, scale))?;
	Some(monitors.swap_remove(at))
}

/// An output as the compositor lays it out: its place and size there, in the
/// layout's own units, and its pixels turned the way it shows them.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
#[derive(Clone, Copy, Debug)]
struct Laid {
	bounds: Bounds,
	px: (f64, f64),
}

#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
impl Laid {
	// Its size in the layout is its pixels over its scale, give or take the
	// compositor's rounding, so a 150% output and a 200% one are told apart
	// where winit reads both as 2.
	fn at_scale(&self, scale: f64) -> bool {
		let near = |px: f64, units: f64| (px / scale - units).abs() <= (units / 500.0).max(1.0);
		near(self.px.0, self.bounds.2) && near(self.px.1, self.bounds.3)
	}
}

// Of the outputs a Wayland window is on, first entered first, the one it is
// drawn for: the first when it is at the window's scale, as on the other
// path, else the only one at that scale, else the only one of those most of a
// window this size could be on while it touches the rest. Between two at
// 100% with a 200% output first, that's the one it's on, which the other path
// couldn't say.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
fn laid_drawn_for(on: &[Laid], scale: f64, size: (f64, f64)) -> Option<usize> {
	if on.first()?.at_scale(scale) {
		return Some(0);
	}
	let only = |found: Vec<usize>| (found.len() == 1).then(|| found[0]);
	let at: Vec<usize> = (0..on.len()).filter(|&i| on[i].at_scale(scale)).collect();
	if at.len() < 2 {
		return only(at);
	}
	let bounds: Vec<Bounds> = on.iter().map(|o| o.bounds).collect();
	let all: Vec<usize> = (0..on.len()).collect();
	only(
		at.into_iter()
			.filter(|&i| could_be_most_on(i, &all, &bounds, &bounds, size))
			.collect(),
	)
}

// Wayland gives an output's scale in whole numbers, so a 150% one reads as 2
// while a window on it is drawn at 1.5.
fn at_scale(monitor: f64, window: f64) -> bool {
	(monitor - window).abs() < 0.01 || (monitor.fract() == 0.0 && monitor == window.ceil())
}

type Bounds = (f64, f64, f64, f64);

/// A monitor as winit lists it: its place, its size in its own pixels, and
/// its scale.
#[derive(Clone, Copy, Debug)]
struct Placed {
	at: (i64, i64),
	px: (i64, i64),
	scale: f64,
}

impl Placed {
	// Where it is in the layout, at its largest and its smallest. winit gives
	// a Wayland output's place times its scale in whole numbers, and a 150%
	// output says 2, so its real scale is anything above 1 up to 2.
	fn largest(&self) -> Bounds {
		let k = self.scale.max(1.0);
		self.sized(if k.fract() == 0.0 && k > 1.0 {
			k - 1.0
		} else {
			k
		})
	}

	fn smallest(&self) -> Bounds {
		self.sized(self.scale.max(1.0))
	}

	fn sized(&self, by: f64) -> Bounds {
		let k = self.scale.max(1.0);
		(
			self.at.0 as f64 / k,
			self.at.1 as f64 / k,
			self.px.0 as f64 / by,
			self.px.1 as f64 / by,
		)
	}
}

// The monitor a window is drawn for when `first` is at another scale: of the
// ones at the window's scale, the only one most of a window this size could
// be on while it still touches `first`. None when that is not one, which
// keeps the window's size.
fn other_at_scale(
	first: usize,
	monitors: &[Placed],
	scale: f64,
	size: (f64, f64),
) -> Option<usize> {
	let only = |found: Vec<usize>| (found.len() == 1).then(|| found[0]);
	let at: Vec<usize> = (0..monitors.len())
		.filter(|&i| at_scale(monitors[i].scale, scale))
		.collect();
	if at.len() < 2 {
		return only(at);
	}
	let largest: Vec<Bounds> = monitors.iter().map(Placed::largest).collect();
	let smallest: Vec<Bounds> = monitors.iter().map(Placed::smallest).collect();
	only(
		at.into_iter()
			.filter(|&i| could_be_most_on(i, &[first], &largest, &smallest, size))
			.collect(),
	)
}

// Could most of a window this size be on monitor `on`, while it touches each
// of `touch`? Tried at places a step apart, with `on` and `touch` grown by
// half a step and the rest shrunk by as much, so a place between two tries is
// never missed. Where a monitor's size is a guess, it is taken at its largest
// where that helps `on`, and at its smallest where it doesn't.
fn could_be_most_on(
	on: usize,
	touch: &[usize],
	largest: &[Bounds],
	smallest: &[Bounds],
	size: (f64, f64),
) -> bool {
	let (w, h) = size;
	if w <= 0.0 || h <= 0.0 || touch.is_empty() {
		return false;
	}
	let first_at = largest[touch[0]];
	let step = (w.min(h) / 8.0)
		.max((first_at.2 + w) / 256.0)
		.max((first_at.3 + h) / 256.0)
		.max(4.0);
	let half = step / 2.0;
	let grown = |r: Bounds, by: f64| (r.0 - by, r.1 - by, r.2 + 2.0 * by, r.3 + 2.0 * by);
	let first_at = grown(first_at, half);
	let touched: Vec<Bounds> = touch.iter().map(|&i| grown(largest[i], half)).collect();
	let mine = grown(largest[on], half);
	let rest: Vec<Bounds> = (0..smallest.len())
		.filter(|&i| i != on)
		.map(|i| grown(smallest[i], -half))
		.collect();
	let area = |a: Bounds, b: Bounds| {
		let x = (a.0 + a.2).min(b.0 + b.2) - a.0.max(b.0);
		let y = (a.1 + a.3).min(b.1 + b.3) - a.1.max(b.1);
		x.max(0.0) * y.max(0.0)
	};
	let mut y = first_at.1 - h;
	while y <= first_at.1 + first_at.3 {
		let mut x = first_at.0 - w;
		while x <= first_at.0 + first_at.2 {
			let win = (x, y, w, h);
			let here = area(win, mine);
			if here > 0.0
				&& touched.iter().all(|&t| area(win, t) > 0.0)
				&& rest.iter().all(|&o| area(win, o) <= here)
			{
				return true;
			}
			x += step;
		}
		y += step;
	}
	false
}

// A rotated monitor reports its pixels turned and its millimeters not.
fn upright(mm: (u32, u32), px: (u32, u32)) -> (u32, u32) {
	if (px.0 >= px.1) == (mm.0 >= mm.1) {
		mm
	} else {
		(mm.1, mm.0)
	}
}

// A projector, or a TV that gives only its aspect ratio, reports a size of
// zero or a few centimeters. That is no size at all.
fn plausible(mm: (u32, u32)) -> Option<(u32, u32)> {
	let fits = |side: u32| (40..=10_000).contains(&side);
	(fits(mm.0) && fits(mm.1)).then_some(mm)
}

// The physical size an EDID block gives: the first detailed timing's image
// size in mm, else the basic block's whole centimeters.
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn edid_mm(edid: &[u8]) -> Option<(u32, u32)> {
	const HEADER: [u8; 8] = [0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00];
	if edid.len() < 128 || edid[..8] != HEADER {
		return None;
	}
	let timing = &edid[54..72];
	if timing[0] != 0 || timing[1] != 0 {
		let w = u32::from(timing[12]) | (u32::from(timing[14] & 0xf0) << 4);
		let h = u32::from(timing[13]) | (u32::from(timing[14] & 0x0f) << 8);
		if let Some(mm) = plausible((w, h)) {
			return Some(mm);
		}
	}
	plausible((u32::from(edid[21]) * 10, u32::from(edid[22]) * 10))
}

// A monitor's mode, and its size in mm where the server knows it.
#[cfg(target_os = "linux")]
type PxMm = ((u32, u32), Option<(u32, u32)>);

#[cfg(target_os = "linux")]
struct X11Screen {
	conn: x11rb::rust_connection::RustConnection,
	root: u32,
	xid: u32,
	config_timestamp: u32,
	// the ones lit, with an output
	crtcs: Vec<x11rb::protocol::randr::GetCrtcInfoReply>,
}

#[cfg(target_os = "linux")]
impl X11Screen {
	// None when the window is not an X11 one, or the server will not say.
	fn of(window: &Window) -> Option<Self> {
		use raw_window_handle::{HasWindowHandle, RawWindowHandle};
		use x11rb::connection::Connection;
		use x11rb::protocol::randr::ConnectionExt as _;

		let xid = match window.window_handle().ok()?.as_raw() {
			RawWindowHandle::Xlib(h) => h.window as u32,
			RawWindowHandle::Xcb(h) => h.window.get(),
			_ => return None,
		};
		let (conn, screen) = x11rb::connect(None).ok()?;
		let root = conn.setup().roots.get(screen)?.root;
		let resources = conn
			.randr_get_screen_resources_current(root)
			.ok()?
			.reply()
			.ok()?;
		let crtcs = resources
			.crtcs
			.iter()
			.filter_map(|&crtc| {
				conn.randr_get_crtc_info(crtc, resources.config_timestamp)
					.ok()
					.and_then(|cookie| cookie.reply().ok())
			})
			.filter(|info| info.mode != 0 && !info.outputs.is_empty())
			.collect();
		Some(Self {
			conn,
			root,
			xid,
			config_timestamp: resources.config_timestamp,
			crtcs,
		})
	}
}

#[cfg(target_os = "linux")]
fn crtc_rect(info: &x11rb::protocol::randr::GetCrtcInfoReply) -> (i64, i64, i64, i64) {
	(
		i64::from(info.x),
		i64::from(info.y),
		i64::from(info.width),
		i64::from(info.height),
	)
}

// On X11 the server is asked which monitor the window overlaps most, or
// for a window not shown yet which one the pointer is on, and that
// monitor's mode and millimeters. winit keeps a list it may not have
// refreshed since the resolution changed. Some(None) is a window on no
// monitor at all.
#[cfg(target_os = "linux")]
fn x11_monitor_under(window: &Window, at_pointer: bool) -> Option<Option<PxMm>> {
	use x11rb::protocol::randr::ConnectionExt as _;
	use x11rb::protocol::xproto::ConnectionExt as _;

	let screen = X11Screen::of(window)?;
	let conn = &screen.conn;
	let size = conn.get_geometry(screen.xid).ok()?.reply().ok()?;
	let at = conn
		.translate_coordinates(screen.xid, screen.root, 0, 0)
		.ok()?
		.reply()
		.ok()?;
	let win = (
		i64::from(at.dst_x),
		i64::from(at.dst_y),
		i64::from(size.width),
		i64::from(size.height),
	);
	let pointer = at_pointer
		.then(|| conn.query_pointer(screen.root).ok()?.reply().ok())
		.flatten()
		.map(|reply| (i64::from(reply.root_x), i64::from(reply.root_y)));
	let rects: Vec<_> = screen.crtcs.iter().map(crtc_rect).collect();
	let Some(index) = monitor_under(win, pointer, &rects) else {
		return Some(None);
	};
	let crtc = &screen.crtcs[index];
	let mm = conn
		.randr_get_output_info(crtc.outputs[0], screen.config_timestamp)
		.ok()
		.and_then(|cookie| cookie.reply().ok())
		.map(|out| (out.mm_width, out.mm_height));
	Some(Some(((u32::from(crtc.width), u32::from(crtc.height)), mm)))
}

// The outputs a Wayland window is on and where each is in the layout, read
// from winit's own toolkit state on its own connection. winit hands out only
// the first output and each one's scale rounded up to a whole number, so 125%,
// 150% and 200% all read 2 (2026101411554909). The toolkit keeps the rest, as
// user data on winit's surface and outputs, which is where winit's own
// `current_monitor` reads the first from. None when the window isn't a
// Wayland one or that state can't be read, as when winit moves to another
// toolkit release; then the rounded numbers are all there is.
#[cfg(target_os = "linux")]
mod wayland {
	use super::Laid;
	use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};
	use smithay_client_toolkit::compositor::SurfaceData;
	use smithay_client_toolkit::output::OutputData;
	use wayland_backend::client::{Backend, ObjectId};
	use wayland_client::Proxy;
	use wayland_client::protocol::wl_output::{Transform, WlOutput};
	use wayland_client::protocol::wl_surface::WlSurface;
	use winit::window::Window;

	#[derive(Debug)]
	pub struct Out {
		/// Its mode, as winit gives it, unturned.
		pub mode: (u32, u32),
		pub laid: Laid,
	}

	pub fn outputs_on(window: &Window) -> Option<Vec<Out>> {
		let RawWindowHandle::Wayland(surface) = window.window_handle().ok()?.as_raw() else {
			return None;
		};
		let RawDisplayHandle::Wayland(display) = window.display_handle().ok()?.as_raw() else {
			return None;
		};
		// A guest on winit's connection, which it doesn't close. Made for each
		// read, since kept it could outlive the connection.
		// SAFETY: winit's display, live while its window is.
		let backend = unsafe { Backend::from_foreign_display(display.display.as_ptr().cast()) };
		// SAFETY: winit's surface, live while its window is borrowed here. The
		// interface is checked by name, and a surface no Rust code made reads
		// as dead.
		let id =
			unsafe { ObjectId::from_ptr(WlSurface::interface(), surface.surface.as_ptr().cast()) }
				.ok()?;
		let data = backend.get_data(id).ok()?;
		let on = data.data_as_any().downcast_ref::<SurfaceData>()?;
		on.outputs().map(|output| laid(&output)).collect()
	}

	fn laid(output: &WlOutput) -> Option<Out> {
		output.data::<OutputData>()?.with_output_info(|info| {
			let (w, h) = info.modes.iter().find(|m| m.current)?.dimensions;
			let (x, y) = info.logical_position?;
			let (lw, lh) = info.logical_size?;
			let turned = matches!(
				info.transform,
				Transform::_90 | Transform::_270 | Transform::Flipped90 | Transform::Flipped270
			);
			let px = if turned { (h, w) } else { (w, h) };
			Some(Out {
				mode: (u32::try_from(w).ok()?, u32::try_from(h).ok()?),
				laid: Laid {
					bounds: (f64::from(x), f64::from(y), f64::from(lw), f64::from(lh)),
					px: (f64::from(px.0), f64::from(px.1)),
				},
			})
		})
	}
}

// Which of the monitors a window is on: the one under the pointer when one
// is given, else the first. Without a pointer it is the one the window
// overlaps most, and none when it overlaps none, as when the monitor it was
// on has just gone dark. Taking the first then gave a window left in the
// gap the size of a monitor it was never on.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
fn monitor_under(
	window: (i64, i64, i64, i64),
	pointer: Option<(i64, i64)>,
	monitors: &[(i64, i64, i64, i64)],
) -> Option<usize> {
	let probe = pointer.map_or(window, |(x, y)| (x, y, 1, 1));
	let mut best: Option<(usize, i64)> = None;
	for (index, &monitor) in monitors.iter().enumerate() {
		let shared = overlap(probe, monitor);
		if best.is_none_or(|(_, most)| shared > most) {
			best = Some((index, shared));
		}
	}
	best.filter(|&(_, shared)| shared > 0 || pointer.is_some())
		.map(|(index, _)| index)
}

// Shared area of two (x, y, width, height) rectangles.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
fn overlap(a: (i64, i64, i64, i64), b: (i64, i64, i64, i64)) -> i64 {
	let w = (a.0 + a.2).min(b.0 + b.2) - a.0.max(b.0);
	let h = (a.1 + a.3).min(b.1 + b.3) - a.1.max(b.1);
	w.max(0) * h.max(0)
}

#[cfg(windows)]
fn physical_mm(monitor: &winit::monitor::MonitorHandle) -> Option<(u32, u32)> {
	use windows_sys::Win32::Graphics::Gdi::{DISPLAY_DEVICEW, EnumDisplayDevicesW};
	use windows_sys::Win32::System::Registry::{
		HKEY_LOCAL_MACHINE, RRF_RT_REG_BINARY, RegGetValueW,
	};
	use windows_sys::Win32::UI::WindowsAndMessaging::EDD_GET_DEVICE_INTERFACE_NAME;
	use winit::platform::windows::MonitorHandleExtWindows;

	let wide = |s: &str| {
		s.encode_utf16()
			.chain(std::iter::once(0))
			.collect::<Vec<u16>>()
	};
	let adapter = wide(&monitor.native_id());
	// SAFETY: DISPLAY_DEVICEW is plain data; zero is a valid starting value.
	let mut device: DISPLAY_DEVICEW = unsafe { std::mem::zeroed() };
	device.cb = std::mem::size_of::<DISPLAY_DEVICEW>() as u32;
	// SAFETY: both pointers are live for the call and `cb` is set.
	let found = unsafe {
		EnumDisplayDevicesW(
			adapter.as_ptr(),
			0,
			&raw mut device,
			EDD_GET_DEVICE_INTERFACE_NAME,
		)
	};
	if found == 0 {
		return None;
	}
	let len = device
		.DeviceID
		.iter()
		.position(|&c| c == 0)
		.unwrap_or(device.DeviceID.len());
	let subkey = edid_key(&String::from_utf16_lossy(&device.DeviceID[..len]))?;
	let (subkey, value) = (wide(&subkey), wide("EDID"));
	// Asked twice, since an EDID with extension blocks is longer than the
	// first block, which is all that is read from it.
	let read = |data: *mut u8, size: &mut u32| {
		// SAFETY: `data` is null or has room for `size` bytes, and both strings end in 0.
		unsafe {
			RegGetValueW(
				HKEY_LOCAL_MACHINE,
				subkey.as_ptr(),
				value.as_ptr(),
				RRF_RT_REG_BINARY,
				std::ptr::null_mut(),
				data.cast(),
				size,
			)
		}
	};
	let mut size = 0u32;
	if read(std::ptr::null_mut(), &mut size) != 0 || size == 0 {
		return None;
	}
	let mut edid = vec![0u8; size as usize];
	if read(edid.as_mut_ptr(), &mut size) != 0 {
		return None;
	}
	edid.truncate(size as usize);
	edid_mm(&edid)
}

// `\\?\DISPLAY#DEL40F4#5&1a2b&0&UID4352#{e6f07b5f-...}` names the monitor's
// key under Enum, where Windows keeps the EDID it read.
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn edid_key(interface: &str) -> Option<String> {
	let mut parts = interface.strip_prefix(r"\\?\")?.split('#');
	let (class, model, instance) = (parts.next()?, parts.next()?, parts.next()?);
	if class.is_empty() || model.is_empty() || instance.is_empty() {
		return None;
	}
	Some(format!(
		r"SYSTEM\CurrentControlSet\Enum\{class}\{model}\{instance}\Device Parameters"
	))
}

#[cfg(target_os = "macos")]
fn physical_mm(monitor: &winit::monitor::MonitorHandle) -> Option<(u32, u32)> {
	use winit::platform::macos::MonitorHandleExtMacOS;

	#[repr(C)]
	struct CgSize {
		width: f64,
		height: f64,
	}
	#[link(name = "CoreGraphics", kind = "framework")]
	unsafe extern "C" {
		fn CGDisplayScreenSize(display: u32) -> CgSize;
	}
	// SAFETY: takes a display id by value; an unknown one answers zero.
	let size = unsafe { CGDisplayScreenSize(monitor.native_id()) };
	if !(size.width.is_finite() && size.height.is_finite()) {
		return None;
	}
	Some((size.width.round() as u32, size.height.round() as u32))
}

// Wayland tells a program nothing of a monitor's size.
#[cfg(not(any(windows, target_os = "macos")))]
fn physical_mm(_monitor: &winit::monitor::MonitorHandle) -> Option<(u32, u32)> {
	None
}

/// Is a mouse button down anywhere on the screen? A window being dragged by
/// its title bar sees no button events of its own, and a pause in the drag
/// looks the same as the drop. None where nobody can say: Wayland shows a
/// window no buttons but its own, and Xwayland can't see one held on the
/// compositor's title bar.
#[cfg(target_os = "linux")]
pub fn button_held(window: &Window) -> Option<bool> {
	use raw_window_handle::{HasWindowHandle, RawWindowHandle};
	use x11rb::connection::Connection;
	use x11rb::protocol::xproto::{ConnectionExt as _, KeyButMask};

	let x11 = window.window_handle().is_ok_and(|handle| {
		matches!(
			handle.as_raw(),
			RawWindowHandle::Xlib(_) | RawWindowHandle::Xcb(_)
		)
	});
	if !x11 {
		return None;
	}
	let (conn, screen) = x11rb::connect(None).ok()?;
	let xwayland = conn
		.query_extension(b"XWAYLAND")
		.ok()
		.and_then(|cookie| cookie.reply().ok())
		.is_some_and(|reply| reply.present);
	if xwayland {
		return None;
	}
	let root = conn.setup().roots[screen].root;
	let reply = conn.query_pointer(root).ok()?.reply().ok()?;
	Some(
		reply
			.mask
			.intersects(KeyButMask::BUTTON1 | KeyButMask::BUTTON2 | KeyButMask::BUTTON3),
	)
}

#[cfg(windows)]
#[allow(clippy::unnecessary_wraps)] // Linux answers None
pub fn button_held(_window: &Window) -> Option<bool> {
	use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
		GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON,
	};
	// SAFETY: a plain query; the high bit means down now.
	Some(
		[VK_LBUTTON, VK_RBUTTON]
			.into_iter()
			.any(|key| unsafe { GetAsyncKeyState(i32::from(key)) } < 0),
	)
}

#[cfg(target_os = "macos")]
#[allow(clippy::unnecessary_wraps)] // Linux answers None
pub fn button_held(_window: &Window) -> Option<bool> {
	Some(objc2_app_kit::NSEvent::pressedMouseButtons() != 0)
}

#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
#[allow(clippy::unnecessary_wraps)] // Linux answers None
pub fn button_held(_window: &Window) -> Option<bool> {
	Some(false)
}

/// Is the window fullscreen? winit on X11 knows only of a fullscreen the
/// program asked for itself, so one the window manager set from its own key
/// or menu had its size saved as the size to open at (2026101413075422).
pub fn fullscreen(window: &Window) -> bool {
	window.fullscreen().is_some() || wm_fullscreen(window)
}

#[cfg(target_os = "linux")]
fn wm_fullscreen(window: &Window) -> bool {
	use raw_window_handle::{HasWindowHandle, RawWindowHandle};
	use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _};

	let xid = match window.window_handle().map(|handle| handle.as_raw()) {
		Ok(RawWindowHandle::Xlib(h)) => h.window as u32,
		Ok(RawWindowHandle::Xcb(h)) => h.window.get(),
		_ => return false,
	};
	let Ok((conn, _)) = x11rb::connect(None) else {
		return false;
	};
	let atom = |name: &[u8]| {
		conn.intern_atom(true, name)
			.ok()
			.and_then(|cookie| cookie.reply().ok())
			.map(|reply| reply.atom)
	};
	let (Some(state), Some(full)) = (atom(b"_NET_WM_STATE"), atom(b"_NET_WM_STATE_FULLSCREEN"))
	else {
		return false;
	};
	conn.get_property(false, xid, state, AtomEnum::ATOM, 0, 64)
		.ok()
		.and_then(|cookie| cookie.reply().ok())
		.is_some_and(|reply| {
			reply
				.value32()
				.is_some_and(|mut atoms| atoms.any(|a| a == full))
		})
}

#[cfg(not(target_os = "linux"))]
fn wm_fullscreen(_window: &Window) -> bool {
	false
}

#[cfg(test)]
mod tests {
	use super::*;

	// Test ID: EreYcuM
	#[test]
	fn a_monitor_key_is_a_bare_shcl_name_and_reads_plainly() {
		let known = MonitorId::new(2560, 1440, 1.25, Some((597, 336))).unwrap();
		assert_eq!(known.key(), "2560x1440_125pct_597x336mm");
		let unknown = MonitorId::new(1920, 1080, 1.0, None).unwrap();
		assert_eq!(unknown.key(), "1920x1080_100pct");
		// a key the file takes as a name, and reads back as one
		for key in [known.key(), unknown.key()] {
			let doc = shcl::Document::parse(&format!(
				"window:\n\tmonitors:\n\t\t{key}:\n\t\t\tcolumns: 90\n"
			));
			assert_eq!(doc.children("window.monitors"), vec![key.clone()]);
			assert_eq!(
				doc.get_int(&format!("window.monitors.{key}.columns")).ok(),
				Some(90)
			);
		}
	}

	// Test ID: EreYcuN
	#[test]
	fn a_size_that_is_no_size_is_left_out_and_a_turned_one_is_turned_back() {
		assert_eq!(
			MonitorId::new(1920, 1080, 1.0, Some((0, 0)))
				.unwrap()
				.size_mm,
			None
		);
		assert_eq!(
			MonitorId::new(1920, 1080, 1.0, Some((16, 9)))
				.unwrap()
				.size_mm,
			None
		);
		assert_eq!(upright((597, 336), (1440, 2560)), (336, 597));
		assert_eq!(upright((597, 336), (2560, 1440)), (597, 336));
		assert!(MonitorId::new(0, 1080, 1.0, None).is_none());
		assert!(MonitorId::new(1920, 1080, 0.0, None).is_none());
		assert!(MonitorId::new(1920, 1080, f64::NAN, None).is_none());
	}

	// Test ID: EreYcuO
	#[test]
	fn the_edid_size_comes_from_the_timing_and_falls_back_to_centimeters() {
		let mut edid = [0u8; 128];
		edid[..8].copy_from_slice(&[0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00]);
		edid[21] = 60;
		edid[22] = 34;
		assert_eq!(
			edid_mm(&edid),
			Some((600, 340)),
			"no timing: whole centimeters"
		);
		// 597 x 336 mm: low bytes 0x55 0x50, high nibbles 0x2 and 0x1
		edid[54] = 0x02;
		edid[55] = 0x3a;
		edid[66] = 0x55;
		edid[67] = 0x50;
		edid[68] = 0x21;
		assert_eq!(edid_mm(&edid), Some((597, 336)));
		edid[0] = 1;
		assert_eq!(edid_mm(&edid), None, "not an EDID");
		assert_eq!(edid_mm(&edid[..100]), None, "too short");
	}

	// Test ID: EreYcuV
	#[test]
	fn the_monitor_a_window_overlaps_most_wins() {
		let left = (0, 0, 1920, 1080);
		let right = (1920, 0, 2560, 1440);
		assert_eq!(overlap((1800, 100, 400, 300), left), 120 * 300);
		assert_eq!(overlap((1800, 100, 400, 300), right), 280 * 300);
		assert_eq!(overlap((5000, 0, 10, 10), right), 0, "apart");
		assert_eq!(
			overlap((-50, -50, 100, 100), left),
			50 * 50,
			"partly off screen"
		);
	}

	// Test ID: ErkYUyK
	#[test]
	fn a_window_not_shown_yet_opens_on_the_monitor_under_the_pointer() {
		let monitors = [(0, 0, 1920, 1080), (1920, 0, 1280, 1024)];
		// a hidden window sits at the origin whichever monitor it will open on
		let hidden = (0, 0, 860, 578);
		assert_eq!(monitor_under(hidden, Some((2500, 500)), &monitors), Some(1));
		assert_eq!(monitor_under(hidden, Some((1919, 500)), &monitors), Some(0));
		assert_eq!(monitor_under(hidden, Some((1920, 0)), &monitors), Some(1));
		assert_eq!(
			monitor_under(hidden, None, &monitors),
			Some(0),
			"no pointer"
		);
		assert_eq!(
			monitor_under(hidden, Some((2500, 1050)), &monitors),
			Some(0),
			"pointer off every monitor"
		);
		assert_eq!(
			monitor_under((1800, 100, 400, 300), None, &monitors),
			Some(1),
			"a shown window goes by overlap"
		);
		assert_eq!(monitor_under(hidden, Some((2500, 500)), &[]), None);
	}

	// A Wayland window dropped mostly on a 200% output with a sliver left on a
	// 100% one is drawn at 2, while winit's current monitor is still the 100%
	// one it entered first. The key read 1920x1080_200pct, which no monitor is.
	// Test ID: EshSMlf
	#[test]
	fn a_window_astride_two_outputs_is_named_for_the_one_it_is_drawn_for() {
		let placed = |at, px, scale| Placed { at, px, scale };
		// winit's numbers for monwl's layout: the place is times the scale
		let h1 = placed((0, 0), (1920, 1080), 1.0);
		let h2 = placed((3840, 0), (2560, 1600), 2.0);
		let h3 = placed((0, 1080), (1280, 1024), 1.0);
		let all = [h1, h2, h3];
		let window = (1180.0, 496.0);
		assert_eq!(
			other_at_scale(0, &all, 2.0, window),
			Some(1),
			"the sliver drop"
		);
		// Back the other way, either 100% output could be it, since a 200%
		// output may be a 150% one and reach further.
		assert_eq!(other_at_scale(1, &all, 1.0, window), None);
		// a 150% output says 2, and its window is drawn at 1.5
		let frac = placed((3840, 0), (2560, 1440), 2.0);
		assert_eq!(other_at_scale(0, &[h1, frac], 1.5, window), Some(1));
		assert_eq!(
			other_at_scale(0, &all, 3.0, window),
			None,
			"none at that scale"
		);
		// Two at 200% and the first at 100% between them: only the one the
		// window can reach. The far one is past another 100% output.
		let left = placed((0, 0), (2560, 1600), 2.0);
		let mid = placed((1280, 0), (1920, 1080), 1.0);
		let gap = placed((3200, 0), (1920, 1080), 1.0);
		let far = placed((10240, 0), (2560, 1600), 2.0);
		let row = [mid, left, gap, far];
		assert_eq!(other_at_scale(0, &row, 2.0, (600.0, 400.0)), Some(1));
		assert_eq!(
			other_at_scale(0, &row, 2.0, (4000.0, 400.0)),
			None,
			"wide enough to reach both"
		);

		assert!(at_scale(1.0, 1.0));
		assert!(at_scale(2.0, 1.5), "Wayland's whole number");
		assert!(at_scale(1.25, 1.25), "Windows and macOS say it as it is");
		assert!(!at_scale(2.0, 1.0));
		assert!(!at_scale(1.0, 2.0));
		assert!(!at_scale(1.0, 1.5));
		assert!(!at_scale(1.5, 2.0));
	}

	// winit reads a 125%, a 150% and a 200% output all as 2, so a window
	// astride two of them was named for one at the other's scale, a monitor
	// that isn't there (2026101411554909). The outputs it is on, with their
	// sizes in the layout, tell them apart.
	// Test ID: EsoIP9H
	#[test]
	fn a_wayland_window_astride_outputs_that_round_alike_is_named_for_its_own() {
		let laid = |at: (f64, f64), px: (f64, f64), scale: f64| Laid {
			bounds: (at.0, at.1, (px.0 / scale).round(), (px.1 / scale).round()),
			px,
		};
		// monwl's layout
		let h1 = laid((0.0, 0.0), (1920.0, 1080.0), 1.0);
		let h2 = laid((1920.0, 0.0), (2560.0, 1600.0), 2.0);
		let h4 = laid((3200.0, 0.0), (1680.0, 1050.0), 1.0);
		let h5 = laid((1920.0, 800.0), (1920.0, 1200.0), 1.5);
		let window = (560.0, 700.0);
		assert!(h5.at_scale(1.5) && !h5.at_scale(2.0) && !h5.at_scale(1.25));
		assert!(h2.at_scale(2.0) && !h2.at_scale(1.5));
		assert_eq!(
			laid_drawn_for(&[h2, h5], 1.5, window),
			Some(1),
			"150% after 200%"
		);
		assert_eq!(
			laid_drawn_for(&[h5, h2], 2.0, window),
			Some(1),
			"200% after 150%"
		);
		assert_eq!(laid_drawn_for(&[h5, h2], 1.5, window), Some(0), "first");
		let q = laid((0.0, 0.0), (2560.0, 1600.0), 1.25);
		let r = laid((2048.0, 0.0), (2400.0, 1350.0), 1.5);
		assert_eq!(
			laid_drawn_for(&[q, r], 1.5, window),
			Some(1),
			"125% then 150%"
		);
		assert_eq!(
			laid_drawn_for(&[r, q], 1.25, window),
			Some(1),
			"150% then 125%"
		);
		// The 200% one first, with 100% outputs either side. Only the one the
		// window is on counts, different than before when every 100% output in
		// the layout did.
		assert_eq!(laid_drawn_for(&[h2, h4], 1.0, window), Some(1));
		// Wide enough to be on both, and either could have most of it.
		let wide = (2600.0, 400.0);
		assert_eq!(laid_drawn_for(&[h2, h1, h4], 1.0, wide), None);
		assert_eq!(
			laid_drawn_for(&[h2], 1.0, window),
			None,
			"none at that scale"
		);
		assert_eq!(laid_drawn_for(&[], 1.0, window), None, "on no output");
		// turned 90 degrees: its pixels are turned with it
		let turned = Laid {
			bounds: (0.0, 0.0, 720.0, 1280.0),
			px: (1080.0, 1920.0),
		};
		assert!(turned.at_scale(1.5));
	}

	// The outputs a Wayland window is on are read from winit's toolkit, which
	// works only while it is the toolkit release named here.
	// Test ID: EsoIPE8
	#[cfg(target_os = "linux")]
	#[test]
	fn winit_links_the_wayland_crates_monitor_reads_its_state_with() {
		let lock = std::fs::read_to_string(
			std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../Cargo.lock"),
		)
		.unwrap();
		let deps = |package: &str| -> Vec<String> {
			let block = lock
				.split("[[package]]")
				.find(|b| b.contains(&format!("\nname = \"{package}\"\n")))
				.unwrap_or_else(|| panic!("no {package} in Cargo.lock"));
			block
				.lines()
				.map(|l| l.trim().trim_matches(|c| c == '"' || c == ','))
				.filter(|l| {
					[
						"smithay-client-toolkit",
						"wayland-backend",
						"wayland-client",
					]
					.iter()
					.any(|name| l == name || l.starts_with(&format!("{name} ")))
				})
				.map(str::to_owned)
				.collect()
		};
		let (ours, winit) = (deps("silkterm"), deps("winit"));
		assert_eq!(ours.len(), 3, "{ours:?}");
		assert_eq!(ours, winit);
	}

	// Test ID: EsDZDBp
	#[test]
	fn a_monitor_read_again_to_the_cm_is_the_same_monitor() {
		for key in [
			"1600x2560_100pct_401x641mm",
			"2560x1440_125pct_597x336mm",
			"1920x1080_100pct",
		] {
			assert_eq!(MonitorId::from_key(key).unwrap().key(), key);
		}
		for odd in [
			"",
			"1920x1080",
			"1920x1080_100",
			"1920x1080_100pct_x",
			"a_b_c_d",
		] {
			assert!(MonitorId::from_key(odd).is_none(), "{odd}");
		}
		// b23's portrait monitor, before and after a power save
		let portrait = "1600x2560_100pct_401x641mm";
		assert!(same_monitor(portrait, "1600x2560_100pct_400x640mm"));
		assert!(!same_monitor(portrait, "1600x2560_100pct_411x641mm"));
		assert!(!same_monitor(portrait, "1600x2560_125pct_401x641mm"));
		assert!(!same_monitor(portrait, "2560x1600_100pct_641x401mm"));
		assert!(!same_monitor(portrait, "1600x2560_100pct"), "size unknown");
		assert!(same_monitor("1920x1080_100pct", "1920x1080_100pct"));
		assert!(!same_monitor("1920x1080_100pct", "1920x1200_100pct"));
		assert!(same_monitor("odd", "odd"));
		assert!(!same_monitor("odd", portrait));
	}

	// The monitor a window was on went dark and the window was left where it
	// was. Counting it on the first monitor gave it that one's size.
	// Test ID: EsDZDFu
	#[test]
	fn a_shown_window_on_no_monitor_is_on_none() {
		// b23 with the portrait monitor at the top left gone dark
		let landscape = [(1600, 554, 3440, 1440)];
		let window = (100, 100, 1300, 900);
		assert_eq!(monitor_under(window, None, &landscape), None);
		assert_eq!(
			monitor_under((1500, 600, 300, 300), None, &landscape),
			Some(0),
			"partly on it"
		);
		// a window not shown yet still opens somewhere
		assert_eq!(
			monitor_under(window, Some((50, 50)), &landscape),
			Some(0),
			"pointer off every monitor"
		);
	}

	// Test ID: EreYcuP
	#[test]
	fn the_monitor_interface_name_points_at_its_edid_key() {
		assert_eq!(
			edid_key(
				r"\\?\DISPLAY#DEL40F4#5&1a2b3c&0&UID4352#{e6f07b5f-ee97-4a90-b076-33f57bf4eaa7}"
			)
			.as_deref(),
			Some(
				r"SYSTEM\CurrentControlSet\Enum\DISPLAY\DEL40F4\5&1a2b3c&0&UID4352\Device Parameters"
			)
		);
		assert_eq!(edid_key(r"DISPLAY#DEL40F4#x#{g}"), None);
		assert_eq!(edid_key(r"\\?\DISPLAY##x#{g}"), None);
	}

	mod fuzz {
		use super::super::{Laid, Placed, at_scale, laid_drawn_for, other_at_scale, overlap};
		use crate::fuzz::{self, Rng};

		type Rect = (i64, i64, i64, i64);

		const SIZES: [(i64, i64); 5] = [
			(1920, 1080),
			(1280, 800),
			(1600, 900),
			(2560, 1440),
			(1080, 1920),
		];
		const SCALES: [f64; 5] = [1.0, 1.25, 1.5, 2.0, 3.0];

		// A Wayland layout as the compositor has it: outputs edge to edge in
		// its own units, each at a scale that may not be whole.
		fn layout(rng: &mut Rng) -> Vec<(Rect, f64)> {
			let mut outs: Vec<(Rect, f64)> = vec![((0, 0, 1920, 1080), *rng.pick(&SCALES))];
			for _ in 0..=rng.below(4) {
				let (w, h) = *rng.pick(&SIZES);
				let scale = *rng.pick(&SCALES);
				for _ in 0..50 {
					let a = outs[rng.below(outs.len())].0;
					let at = match rng.below(4) {
						0 => (a.0 + a.2, a.1 + slide(rng, a.3, h)),
						1 => (a.0 - w, a.1 + slide(rng, a.3, h)),
						2 => (a.0 + slide(rng, a.2, w), a.1 + a.3),
						_ => (a.0 + slide(rng, a.2, w), a.1 - h),
					};
					let r = (at.0, at.1, w, h);
					if outs.iter().all(|&(o, _)| overlap(o, r) == 0) {
						outs.push((r, scale));
						break;
					}
				}
			}
			outs
		}

		// Where along a side of `len` something `size` long goes, sharing 100 of it.
		fn slide(rng: &mut Rng, len: i64, size: i64) -> i64 {
			let low = 100 - size;
			low + rng.below((len - 100 - low + 1) as usize) as i64
		}

		// winit's numbers for an output: its place times its scale rounded up,
		// its pixels, and that whole number as its scale.
		fn seen(rect: Rect, scale: f64) -> Placed {
			let whole = scale.ceil();
			Placed {
				at: (rect.0 * whole as i64, rect.1 * whole as i64),
				px: (
					(rect.2 as f64 * scale).round() as i64,
					(rect.3 as f64 * scale).round() as i64,
				),
				scale: whole,
			}
		}

		#[derive(Debug)]
		struct Case {
			outs: Vec<(Rect, f64)>,
			size: (i64, i64),
			areas: Vec<i64>,
			touched: Vec<usize>,
			first: usize,
			primary: usize,
			named: Option<usize>,
		}

		// A window somewhere on the layout. The compositor draws it at the
		// scale of the output most of it is on; winit names the first output
		// it entered, any of the ones it is on.
		fn case(seed: u64) -> Case {
			let mut rng = Rng::new(seed);
			let outs = layout(&mut rng);
			let on = outs[rng.below(outs.len())].0;
			let (w, h) = (300 + rng.below(2200) as i64, 200 + rng.below(1300) as i64);
			let point = (
				on.0 + rng.below(on.2 as usize) as i64,
				on.1 + rng.below(on.3 as usize) as i64,
			);
			let window = (
				point.0 - rng.below(w as usize) as i64,
				point.1 - rng.below(h as usize) as i64,
				w,
				h,
			);
			let areas: Vec<i64> = outs.iter().map(|&(o, _)| overlap(window, o)).collect();
			let touched: Vec<usize> = (0..outs.len()).filter(|&i| areas[i] > 0).collect();
			let primary = touched
				.iter()
				.copied()
				.max_by_key(|&i| (areas[i], std::cmp::Reverse(i)))
				.unwrap();
			let first = touched[rng.below(touched.len())];
			let scale = outs[primary].1;
			let all: Vec<Placed> = outs.iter().map(|&(r, s)| seen(r, s)).collect();
			let named = if at_scale(all[first].scale, scale) {
				Some(first)
			} else {
				other_at_scale(first, &all, scale, (w as f64, h as f64))
			};
			Case {
				outs,
				size: (w, h),
				areas,
				touched,
				first,
				primary,
				named,
			}
		}

		fn check(seed: u64) {
			let c = case(seed);
			let scale = c.outs[c.primary].1;
			let whole = |i: usize| c.outs[i].1.ceil();
			if let Some(i) = c.named {
				assert!(at_scale(whole(i), scale), "another scale: {c:?}");
				assert!(c.touched.contains(&i), "one it isn't on: {c:?}");
			}
			let alike = (0..c.outs.len())
				.filter(|&i| at_scale(whole(i), scale))
				.count();
			if !at_scale(whole(c.first), scale) && alike == 1 {
				assert_eq!(c.named, Some(c.primary), "the only one at its scale: {c:?}");
			}
			if !at_scale(whole(c.first), scale) {
				assert!(
					c.named.is_none_or(|i| i == c.primary),
					"another than the one most of it is on: {c:?}"
				);
			}
		}

		// A window astride outputs at other scales is named for the output most
		// of it is on, which its scale is from, or for none. Never another one.
		// Test ID: EshXuye
		#[test]
		fn a_wayland_window_is_named_for_the_output_its_scale_is_from() {
			fuzz::soak("wlpick", check);
		}

		// The same window as winit's toolkit has it: the outputs it is on, in
		// the order it entered them, each with its size in the layout.
		fn laid_case(seed: u64) -> (Case, Vec<usize>) {
			let mut c = case(seed);
			let mut rng = Rng::new(seed ^ 0x5eed);
			let mut order: Vec<usize> = c
				.touched
				.iter()
				.copied()
				.filter(|&i| i != c.first)
				.collect();
			for i in (1..order.len()).rev() {
				order.swap(i, rng.below(i + 1));
			}
			order.insert(0, c.first);
			let on: Vec<Laid> = order
				.iter()
				.map(|&i| {
					let ((x, y, w, h), scale) = c.outs[i];
					Laid {
						bounds: (x as f64, y as f64, w as f64, h as f64),
						px: ((w as f64 * scale).round(), (h as f64 * scale).round()),
					}
				})
				.collect();
			let scale = c.outs[c.primary].1;
			let (w, h) = c.size;
			c.named = laid_drawn_for(&on, scale, (w as f64, h as f64)).map(|at| order[at]);
			(c, order)
		}

		fn laid_check(seed: u64) {
			let (c, _) = laid_case(seed);
			let scale = c.outs[c.primary].1;
			let most = c.touched.iter().map(|&i| c.areas[i]).max().unwrap();
			let alike: Vec<usize> = c
				.touched
				.iter()
				.copied()
				.filter(|&i| c.outs[i].1 == scale)
				.collect();
			if c.outs[c.first].1 == scale {
				assert_eq!(c.named, Some(c.first), "the first, at its scale: {c:?}");
				return;
			}
			if let Some(i) = c.named {
				assert!(
					c.areas[i] == most,
					"another than one most of it is on: {c:?}"
				);
			}
			if alike.len() == 1 {
				assert_eq!(c.named, Some(c.primary), "the only one at its scale: {c:?}");
			}
		}

		// Read from the outputs the window is on, a window astride any two is
		// named for the one most of it is on, or for none when more than one
		// at its scale could be. Never another.
		// Test ID: EsoIPIT
		#[test]
		fn a_wayland_window_on_outputs_whose_sizes_are_known_is_named_for_its_own() {
			fuzz::soak("wllaid", laid_check);
		}

		// Test ID: EsoIPMq
		#[test]
		fn the_laid_generator_reaches_every_branch() {
			let (mut first, mut found, mut unsure, mut rounded) = (0, 0, 0, 0);
			for seed in 0..600 {
				let (c, _) = laid_case(seed);
				let scale = c.outs[c.primary].1;
				let whole = |i: usize| c.outs[i].1.ceil();
				if c.outs[c.first].1 != scale && whole(c.first) == scale.ceil() {
					rounded += 1;
				}
				match c.named {
					Some(i) if i == c.first => first += 1,
					Some(_) => found += 1,
					None => unsure += 1,
				}
			}
			assert!(
				first > 0 && found > 0 && unsure > 0 && rounded > 0,
				"{first} {found} {unsure} {rounded}"
			);
		}

		// The generator still reaches each way of naming one.
		// Test ID: EshXv10
		#[test]
		fn the_layout_generator_reaches_every_branch() {
			let (mut first, mut found, mut unsure) = (0, 0, 0);
			for seed in 0..400 {
				let c = case(seed);
				match c.named {
					Some(i) if i == c.first => first += 1,
					Some(_) => found += 1,
					None => unsure += 1,
				}
			}
			assert!(
				first > 0 && found > 0 && unsure > 0,
				"{first} {found} {unsure}"
			);
		}
	}
}
