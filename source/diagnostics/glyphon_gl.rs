// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! Isolation test (no window): does glyphon render text on a NATIVE wgpu GL device
//! (no glutin/external context, no surface)? Renders to an offscreen texture and
//! reads it back to PNG. If text shows -> the transparent-path bug is the external
//! context; if not -> glyphon+wgpu-GL is broken generally.
//! Run: DISPLAY=:0.0 cargo run --example `glyphon_gl`
#![allow(
	clippy::unwrap_used,
	clippy::expect_used,
	reason = "a dev-only probe, where a failure should stop it on the spot"
)]
use glyphon::{
	Attrs, Buffer, Cache, Color, Family, FontSystem, Metrics, Resolution, Shaping, SwashCache,
	TextArea, TextAtlas, TextBounds, TextRenderer, Viewport,
};
use winit::application::ApplicationHandler;
use winit::event_loop::{ActiveEventLoop, EventLoop};

const W: u32 = 640;
const H: u32 = 160;

#[derive(Default)]
struct App {
	done: bool,
}

impl ApplicationHandler for App {
	fn resumed(&mut self, event_loop: &ActiveEventLoop) {
		if self.done {
			return;
		}
		self.done = true;
		run(event_loop);
		event_loop.exit();
	}
	fn window_event(
		&mut self,
		_: &ActiveEventLoop,
		_: winit::window::WindowId,
		_: winit::event::WindowEvent,
	) {
	}
}

fn run(event_loop: &ActiveEventLoop) {
	let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
		backends: wgpu::Backends::GL,
		flags: wgpu::InstanceFlags::empty(),
		memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
		backend_options: wgpu::BackendOptions::default(),
		display: Some(Box::new(event_loop.owned_display_handle())),
	});
	let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
		power_preference: wgpu::PowerPreference::HighPerformance,
		compatible_surface: None,
		force_fallback_adapter: false,
	}))
	.expect("GL adapter");
	println!("adapter: {:?}", adapter.get_info());
	let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
		label: None,
		required_features: wgpu::Features::empty(),
		required_limits: adapter.limits(),
		..Default::default()
	}))
	.expect("device");

	let format = wgpu::TextureFormat::Rgba8UnormSrgb;
	let target = device.create_texture(&wgpu::TextureDescriptor {
		label: Some("target"),
		size: wgpu::Extent3d {
			width: W,
			height: H,
			depth_or_array_layers: 1,
		},
		mip_level_count: 1,
		sample_count: 1,
		dimension: wgpu::TextureDimension::D2,
		format,
		usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
		view_formats: &[],
	});
	let view = target.create_view(&Default::default());

	let mut font_system = FontSystem::new();
	let mut swash = SwashCache::new();
	let cache = Cache::new(&device);
	let mut atlas = TextAtlas::new(&device, &queue, &cache, format);
	let mut viewport = Viewport::new(&device, &cache);
	let mut renderer =
		TextRenderer::new(&mut atlas, &device, wgpu::MultisampleState::default(), None);
	let mut buffer = Buffer::new(&mut font_system, Metrics::new(30.0, 38.0));
	let mut attrs = Attrs::new();
	attrs.family = Family::SansSerif;
	buffer.set_text(
		&mut font_system,
		"GLYPHON native wgpu-GL 0123 ABC",
		&attrs,
		Shaping::Advanced,
		None,
	);
	buffer.shape_until_scroll(&mut font_system, false);

	viewport.update(
		&queue,
		Resolution {
			width: W,
			height: H,
		},
	);
	renderer
		.prepare(
			&device,
			&queue,
			&mut font_system,
			&mut atlas,
			&viewport,
			[TextArea {
				buffer: &buffer,
				left: 12.0,
				top: 12.0,
				scale: 1.0,
				bounds: TextBounds {
					left: 0,
					top: 0,
					right: W as i32,
					bottom: H as i32,
				},
				default_color: Color::rgb(230, 230, 230),
				custom_glyphs: &[],
			}],
			&mut swash,
		)
		.expect("prepare");

	let mut encoder = device.create_command_encoder(&Default::default());
	{
		let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
			label: None,
			color_attachments: &[Some(wgpu::RenderPassColorAttachment {
				view: &view,
				resolve_target: None,
				depth_slice: None,
				ops: wgpu::Operations {
					load: wgpu::LoadOp::Clear(wgpu::Color {
						r: 0.05,
						g: 0.05,
						b: 0.10,
						a: 1.0,
					}),
					store: wgpu::StoreOp::Store,
				},
			})],
			depth_stencil_attachment: None,
			timestamp_writes: None,
			occlusion_query_set: None,
			multiview_mask: None,
		});
		renderer
			.render(&atlas, &viewport, &mut pass)
			.expect("render");
	}

	// read back to PNG
	let unpadded = W * 4;
	let bytes_per_row =
		unpadded.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
	let readback = device.create_buffer(&wgpu::BufferDescriptor {
		label: Some("read"),
		size: (bytes_per_row * H) as u64,
		usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
		mapped_at_creation: false,
	});
	encoder.copy_texture_to_buffer(
		wgpu::TexelCopyTextureInfo {
			texture: &target,
			mip_level: 0,
			origin: wgpu::Origin3d::ZERO,
			aspect: wgpu::TextureAspect::All,
		},
		wgpu::TexelCopyBufferInfo {
			buffer: &readback,
			layout: wgpu::TexelCopyBufferLayout {
				offset: 0,
				bytes_per_row: Some(bytes_per_row),
				rows_per_image: Some(H),
			},
		},
		wgpu::Extent3d {
			width: W,
			height: H,
			depth_or_array_layers: 1,
		},
	);
	queue.submit(Some(encoder.finish()));
	readback.slice(..).map_async(wgpu::MapMode::Read, |_| {});
	let _ = device.poll(wgpu::PollType::wait_indefinitely());
	let data = readback.slice(..).get_mapped_range();
	let mut pixels = Vec::with_capacity((W * H * 4) as usize);
	let mut bright_count = 0u32;
	for row in 0..H {
		let row_start = (row * bytes_per_row) as usize;
		let line = &data[row_start..row_start + unpadded as usize];
		pixels.extend_from_slice(line);
		for px in line.chunks(4) {
			// count clearly-bright pixels (the ~230 text on a dark bg)
			if px[0] as u32 + px[1] as u32 + px[2] as u32 > 360 {
				bright_count += 1;
			}
		}
	}
	let _ = image::save_buffer(
		"/tmp/glyphon_gl.png",
		&pixels,
		W,
		H,
		image::ExtendedColorType::Rgba8,
	);
	println!(
		"bright (text) pixels: {bright_count}  -> {}",
		if bright_count > 50 {
			"TEXT RENDERED"
		} else {
			"NO TEXT"
		}
	);
}

fn main() {
	env_logger::init();
	let event_loop = EventLoop::new().unwrap();
	event_loop.run_app(&mut App::default()).unwrap();
}
