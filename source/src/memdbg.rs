// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

// SILK_MEMDBG=1 prints what a window holds, on the graphics card and in its
// own heap, to stderr whenever it changes. The card figures are the
// allocator's own (Vulkan and DX12; the GL path keeps no report). The heap
// figures are counted from the structures that grow with use, so they are a
// floor: malloc's own overhead and everything else in the process are not in
// them. See the reducing resources design doc for what they were used for.

use std::collections::BTreeMap;
use std::fmt::Write as _;

const MIB: f64 = 1024.0 * 1024.0;

pub fn mib(bytes: usize) -> f64 {
	bytes as f64 / MIB
}

// One device's allocations, summed by label, largest first. Swapchain images
// belong to the window system, not the allocator, so they are never in here.
pub fn gpu_line(tag: &str, device: &wgpu::Device) -> String {
	let Some(report) = device.generate_allocator_report() else {
		return format!("{tag}: no allocator report on this backend");
	};
	let mut by_label: BTreeMap<&str, (u64, usize)> = BTreeMap::new();
	for alloc in &report.allocations {
		let entry = by_label.entry(alloc.name.as_str()).or_default();
		entry.0 += alloc.size;
		entry.1 += 1;
	}
	let mut parts: Vec<(&str, (u64, usize))> = by_label.into_iter().collect();
	parts.sort_by_key(|&(_, (size, _))| std::cmp::Reverse(size));
	let blocks: Vec<String> = report
		.blocks
		.iter()
		.map(|block| format!("{:.0}", block.size as f64 / MIB))
		.collect();
	let mut line = format!(
		"{tag}: {:.1} MiB in use, {:.1} MiB reserved in blocks of {};",
		report.total_allocated_bytes as f64 / MIB,
		report.total_reserved_bytes as f64 / MIB,
		blocks.join("+")
	);
	for (label, (size, count)) in parts {
		let label = if label.is_empty() { "unlabeled" } else { label };
		let _ = write!(line, " {label} {:.2}", size as f64 / MIB);
		if count > 1 {
			let _ = write!(line, " x{count}");
		}
		line.push(',');
	}
	line.pop();
	line
}

// Prints each line only when it differs from what was printed under its tag.
#[derive(Debug, Default)]
pub struct Printer {
	last: BTreeMap<String, String>,
}

impl Printer {
	pub fn say(&mut self, tag: &str, line: String) {
		if self.last.get(tag) == Some(&line) {
			return;
		}
		eprintln!("memdbg {line}");
		self.last.insert(tag.to_string(), line);
	}
}
