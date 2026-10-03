// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! Which monitor a window is on, told apart well enough to keep a window size
//! for each one: its resolution, the scale the OS set for it, and its physical
//! size where the platform says so cheaply. The physical size comes from the X
//! server on X11, the monitor's EDID on Windows and `CGDisplayScreenSize` on
//! macOS.
//! Wayland gives a program no way to ask, so there it is resolution and scale.

use winit::window::Window;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MonitorId {
	pub width: u32,
	pub height: u32,
	pub scale_pct: u32,
	pub size_mm: Option<(u32, u32)>,
}

impl MonitorId {
	// The monitor the window is on now. None when the platform cannot say,
	// which on Wayland is the case until the window has been shown.
	pub fn of_window(window: &Window) -> Option<Self> {
		let monitor = window.current_monitor()?;
		let px = monitor.size();
		let size_mm = physical_mm(window, &monitor).map(|mm| upright(mm, (px.width, px.height)));
		Self::new(px.width, px.height, monitor.scale_factor(), size_mm)
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

#[cfg(target_os = "linux")]
fn physical_mm(window: &Window, monitor: &winit::monitor::MonitorHandle) -> Option<(u32, u32)> {
	use raw_window_handle::{HasWindowHandle, RawWindowHandle};
	use winit::platform::x11::MonitorHandleExtX11;
	use x11rb::protocol::randr::ConnectionExt as _;

	// On Wayland the id is a Wayland one, and DISPLAY is Xwayland's.
	if !matches!(
		window.window_handle().ok()?.as_raw(),
		RawWindowHandle::Xlib(_) | RawWindowHandle::Xcb(_)
	) {
		return None;
	}
	// winit's X11 monitor id is the RandR CRTC; its first output is the panel.
	let crtc = monitor.native_id();
	let (conn, _) = x11rb::connect(None).ok()?;
	let info = conn.randr_get_crtc_info(crtc, 0).ok()?.reply().ok()?;
	let output = *info.outputs.first()?;
	let out = conn.randr_get_output_info(output, 0).ok()?.reply().ok()?;
	Some((out.mm_width, out.mm_height))
}

#[cfg(windows)]
fn physical_mm(_window: &Window, monitor: &winit::monitor::MonitorHandle) -> Option<(u32, u32)> {
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
	let mut edid = [0u8; 256];
	let mut size = edid.len() as u32;
	// SAFETY: the buffer and its size go together, and both strings end in 0.
	let status = unsafe {
		RegGetValueW(
			HKEY_LOCAL_MACHINE,
			subkey.as_ptr(),
			value.as_ptr(),
			RRF_RT_REG_BINARY,
			std::ptr::null_mut(),
			edid.as_mut_ptr().cast(),
			&raw mut size,
		)
	};
	// ERROR_MORE_DATA still filled nothing, and the first block is all we read.
	if status != 0 {
		return None;
	}
	edid_mm(&edid[..size as usize])
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
fn physical_mm(_window: &Window, monitor: &winit::monitor::MonitorHandle) -> Option<(u32, u32)> {
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

#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
fn physical_mm(_window: &Window, _monitor: &winit::monitor::MonitorHandle) -> Option<(u32, u32)> {
	None
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
