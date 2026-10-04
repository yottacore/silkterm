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
	// The monitor the window is on now, at the scale the window is drawn at.
	// None when the platform cannot say, which on Wayland is the case until
	// the window has been shown.
	pub fn of_window(window: &Window) -> Option<Self> {
		Self::find(window, false)
	}

	// The monitor a window not shown yet will open on. It has no place of its
	// own until the window manager maps it, and xfwm4 maps a window that asks
	// for no position on the monitor under the pointer. winit guesses the
	// window's scale the same way.
	pub fn of_new_window(window: &Window) -> Option<Self> {
		Self::find(window, true)
	}

	#[cfg_attr(not(target_os = "linux"), allow(unused_variables))]
	fn find(window: &Window, at_pointer: bool) -> Option<Self> {
		let scale = window.scale_factor();
		#[cfg(target_os = "linux")]
		if let Some((px, mm)) = x11_monitor_under(window, at_pointer) {
			return Self::new(px.0, px.1, scale, mm.map(|mm| upright(mm, px)));
		}
		let monitor = window.current_monitor()?;
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

	// `2560x1440_125pct`, with `_597x336mm` after it when the size is known.
	// It is the monitor's name in the config file, so it keeps to what SHCL
	// allows in a bare name: letters, digits, `-` and `_`.
	pub fn key(&self) -> String {
		let key = format!("{}x{}_{}pct", self.width, self.height, self.scale_pct);
		match self.size_mm {
			Some((w, h)) => format!("{key}_{w}x{h}mm"),
			None => key,
		}
	}
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

// On X11 the server is asked which monitor the window overlaps most, or
// for a window not shown yet which one the pointer is on, and that
// monitor's mode and millimeters. winit keeps a list it may not have
// refreshed since the resolution changed.
#[cfg(target_os = "linux")]
fn x11_monitor_under(
	window: &Window,
	at_pointer: bool,
) -> Option<((u32, u32), Option<(u32, u32)>)> {
	use raw_window_handle::{HasWindowHandle, RawWindowHandle};
	use x11rb::connection::Connection;
	use x11rb::protocol::randr::ConnectionExt as _;
	use x11rb::protocol::xproto::ConnectionExt as _;

	let xid = match window.window_handle().ok()?.as_raw() {
		RawWindowHandle::Xlib(h) => h.window as u32,
		RawWindowHandle::Xcb(h) => h.window.get(),
		_ => return None,
	};
	let (conn, screen) = x11rb::connect(None).ok()?;
	let root = conn.setup().roots.get(screen)?.root;
	let size = conn.get_geometry(xid).ok()?.reply().ok()?;
	let at = conn
		.translate_coordinates(xid, root, 0, 0)
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
		.then(|| conn.query_pointer(root).ok()?.reply().ok())
		.flatten()
		.map(|reply| (i64::from(reply.root_x), i64::from(reply.root_y)));
	let resources = conn
		.randr_get_screen_resources_current(root)
		.ok()?
		.reply()
		.ok()?;
	let crtcs: Vec<_> = resources
		.crtcs
		.iter()
		.filter_map(|&crtc| {
			conn.randr_get_crtc_info(crtc, resources.config_timestamp)
				.ok()
				.and_then(|cookie| cookie.reply().ok())
		})
		.filter(|info| info.mode != 0 && !info.outputs.is_empty())
		.collect();
	let rects: Vec<_> = crtcs
		.iter()
		.map(|info| {
			(
				i64::from(info.x),
				i64::from(info.y),
				i64::from(info.width),
				i64::from(info.height),
			)
		})
		.collect();
	let crtc = &crtcs[monitor_under(win, pointer, &rects)?];
	let mm = conn
		.randr_get_output_info(crtc.outputs[0], resources.config_timestamp)
		.ok()
		.and_then(|cookie| cookie.reply().ok())
		.map(|out| (out.mm_width, out.mm_height));
	Some(((u32::from(crtc.width), u32::from(crtc.height)), mm))
}

// Which of the monitors a window is on: the one under the pointer when one
// is given, else the one the window overlaps most, else the first.
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
	best.map(|(index, _)| index)
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

// Is a mouse button down anywhere on the screen? A window being dragged by
// its title bar sees no button events of its own, and a pause in the drag
// looks the same as the drop.
#[cfg(target_os = "linux")]
pub fn button_held(window: &Window) -> bool {
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
		return false;
	}
	let Ok((conn, screen)) = x11rb::connect(None) else {
		return false;
	};
	let root = conn.setup().roots[screen].root;
	conn.query_pointer(root)
		.ok()
		.and_then(|cookie| cookie.reply().ok())
		.is_some_and(|reply| {
			reply
				.mask
				.intersects(KeyButMask::BUTTON1 | KeyButMask::BUTTON2 | KeyButMask::BUTTON3)
		})
}

#[cfg(windows)]
pub fn button_held(_window: &Window) -> bool {
	use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
		GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON,
	};
	// SAFETY: a plain query; the high bit means down now.
	[VK_LBUTTON, VK_RBUTTON]
		.into_iter()
		.any(|key| unsafe { GetAsyncKeyState(i32::from(key)) } < 0)
}

#[cfg(target_os = "macos")]
pub fn button_held(_window: &Window) -> bool {
	objc2_app_kit::NSEvent::pressedMouseButtons() != 0
}

#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
pub fn button_held(_window: &Window) -> bool {
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
}
