// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! Full-window background image. A textured quad drawn behind the terminal
//! content (over the pane background fill, under cells/text). Premultiplied so
//! it composites the same way as the rect pipeline and works with transparency.

use crate::config::Fit;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniform {
	resolution: [f32; 2],
	image_size: [f32; 2],
	amount: f32,
	fit: f32,         // 0 = stretch, 1 = zoom (cover)
	anchor: [f32; 2], // which part of the image survives a zoom crop; 0.5 = center
	// linear background color, and the alpha the whole layer is drawn at. Both
	// are only read on the perceptual path, which writes the background itself.
	bg: [f32; 4],
	perceptual: f32,
	_pad: [f32; 3],
}

// Wallpaper VRAM-content probe verdict (see `vram_check_poll`).
#[derive(Debug)]
pub enum WpProbe {
	Intact,
	Lost,
	MapFailed,
}

// The probe block edge. 64 keeps bytes_per_row (side*4 = 256) copy-aligned;
// images smaller than this in either dimension just skip the probe.
const PROBE_SIDE: u32 = 64;
const PROBE_BYTES: usize = (PROBE_SIDE * PROBE_SIDE * 4) as usize;

pub struct ImageRenderer {
	pipeline: wgpu::RenderPipeline,
	bind_group: wgpu::BindGroup,
	uniform: wgpu::Buffer,
	image_size: [f32; 2],
	opacity: f32,
	fit: f32,
	anchor: [f32; 2],
	// last resolution written to the uniform (skip the per-frame re-write)
	last: std::cell::Cell<(f32, f32, crate::visibility::Mix, [f32; 4])>,
	// VT-switch loss probe: this texture is a REAL casualty of a VRAM purge
	// (it is sampled every frame, so it lives hot in video memory - unlike a
	// synthetic sentinel, which the driver can keep restorable elsewhere). A
	// center block of the uploaded pixels is kept CPU-side and read back on the
	// probe tick; a mismatch means the purge hit us.
	texture: wgpu::Texture,
	probe_at: Option<(u32, u32)>, // block origin; None = image too small, probe disabled
	probe_ref: Vec<u8>,
	probe_buf: wgpu::Buffer,
	probe_inflight: Option<Arc<AtomicU8>>,
}

impl std::fmt::Debug for ImageRenderer {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("ImageRenderer").finish_non_exhaustive()
	}
}

impl ImageRenderer {
	pub fn new(
		device: &wgpu::Device,
		queue: &wgpu::Queue,
		format: wgpu::TextureFormat,
		rgba: &[u8],
		width: u32,
		height: u32,
		opacity: f32,
		fit: Fit,
		anchor: [f32; 2],
	) -> Self {
		let size = wgpu::Extent3d {
			width,
			height,
			depth_or_array_layers: 1,
		};
		let texture = device.create_texture(&wgpu::TextureDescriptor {
			label: Some("bg image"),
			size,
			mip_level_count: 1,
			sample_count: 1,
			dimension: wgpu::TextureDimension::D2,
			format: wgpu::TextureFormat::Rgba8UnormSrgb,
			usage: wgpu::TextureUsages::TEXTURE_BINDING
				| wgpu::TextureUsages::COPY_DST
				| wgpu::TextureUsages::COPY_SRC,
			view_formats: &[],
		});
		queue.write_texture(
			wgpu::TexelCopyTextureInfo {
				texture: &texture,
				mip_level: 0,
				origin: wgpu::Origin3d::ZERO,
				aspect: wgpu::TextureAspect::All,
			},
			rgba,
			wgpu::TexelCopyBufferLayout {
				offset: 0,
				bytes_per_row: Some(4 * width),
				rows_per_image: Some(height),
			},
			size,
		);
		let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
		let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
			label: Some("bg sampler"),
			mag_filter: wgpu::FilterMode::Linear,
			min_filter: wgpu::FilterMode::Linear,
			..Default::default()
		});

		let uniform = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("bg uniform"),
			size: std::mem::size_of::<Uniform>() as u64,
			usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});

		let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("bg bgl"),
			entries: &[
				wgpu::BindGroupLayoutEntry {
					binding: 0,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Buffer {
						ty: wgpu::BufferBindingType::Uniform,
						has_dynamic_offset: false,
						min_binding_size: None,
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 1,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
						view_dimension: wgpu::TextureViewDimension::D2,
						multisampled: false,
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 2,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
					count: None,
				},
			],
		});
		let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("bg bind"),
			layout: &bgl,
			entries: &[
				wgpu::BindGroupEntry {
					binding: 0,
					resource: uniform.as_entire_binding(),
				},
				wgpu::BindGroupEntry {
					binding: 1,
					resource: wgpu::BindingResource::TextureView(&view),
				},
				wgpu::BindGroupEntry {
					binding: 2,
					resource: wgpu::BindingResource::Sampler(&sampler),
				},
			],
		});

		let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
			label: Some("bg shader"),
			source: wgpu::ShaderSource::Wgsl(BG_WGSL.into()),
		});
		let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
			label: Some("bg layout"),
			bind_group_layouts: &[Some(&bgl)],
			immediate_size: 0,
		});
		let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
			label: Some("bg pipeline"),
			layout: Some(&layout),
			vertex: wgpu::VertexState {
				module: &shader,
				entry_point: Some("vs"),
				compilation_options: Default::default(),
				buffers: &[],
			},
			fragment: Some(wgpu::FragmentState {
				module: &shader,
				entry_point: Some("fs"),
				compilation_options: Default::default(),
				targets: &[Some(wgpu::ColorTargetState {
					format,
					blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
					write_mask: wgpu::ColorWrites::ALL,
				})],
			}),
			primitive: wgpu::PrimitiveState {
				topology: wgpu::PrimitiveTopology::TriangleStrip,
				..Default::default()
			},
			depth_stencil: None,
			multisample: wgpu::MultisampleState::default(),
			multiview_mask: None,
			cache: None,
		});

		// Reference block from the image center (corners are more likely to be a
		// flat color a zero-wipe could coincidentally match).
		let probe_at = (width >= PROBE_SIDE && height >= PROBE_SIDE)
			.then(|| ((width - PROBE_SIDE) / 2, (height - PROBE_SIDE) / 2));
		let probe_ref = probe_at.map_or_else(Vec::new, |(bx, by)| {
			let mut block = Vec::with_capacity(PROBE_BYTES);
			for row in 0..PROBE_SIDE {
				let start = (((by + row) * width + bx) * 4) as usize;
				block.extend_from_slice(&rgba[start..start + (PROBE_SIDE * 4) as usize]);
			}
			block
		});
		let probe_buf = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("bg probe read"),
			size: PROBE_BYTES as u64,
			usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
			mapped_at_creation: false,
		});

		Self {
			pipeline,
			bind_group,
			uniform,
			image_size: [width as f32, height as f32],
			opacity,
			fit: if fit == Fit::Zoom { 1.0 } else { 0.0 },
			anchor: [anchor[0].clamp(0.0, 1.0), anchor[1].clamp(0.0, 1.0)],
			texture,
			probe_at,
			probe_ref,
			probe_buf,
			probe_inflight: None,
			last: std::cell::Cell::new((
				0.0,
				0.0,
				crate::visibility::Mix {
					amount: -1.0,
					perceptual: false,
				},
				[0.0; 4],
			)),
		}
	}

	// What the slider (or the image's own tag) asked for, which is not always what
	// gets drawn - see `set_look`.
	pub fn opacity(&self) -> f32 {
		self.opacity
	}

	// Called per frame. fit/anchor are fixed at construction, so the uniform only
	// changes on a resize, or when the mode or the visibility moves.
	pub fn set_look(
		&self,
		queue: &wgpu::Queue,
		w: f32,
		h: f32,
		mix: crate::visibility::Mix,
		bg: [f32; 4],
	) {
		let now = (w, h, mix, bg);
		if self.last.get() == now {
			return;
		}
		self.last.set(now);
		let uniform_data = Uniform {
			resolution: [w, h],
			image_size: self.image_size,
			amount: mix.amount,
			fit: self.fit,
			anchor: self.anchor,
			bg,
			perceptual: if mix.perceptual { 1.0 } else { 0.0 },
			_pad: [0.0; 3],
		};
		queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&uniform_data));
	}

	pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
		pass.set_pipeline(&self.pipeline);
		pass.set_bind_group(0, &self.bind_group, &[]);
		pass.draw(0..4, 0..1);
	}

	// Start an async readback of the probe block. False when the probe is
	// disabled (tiny image) or one is already in flight.
	pub fn vram_check_start(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) -> bool {
		let Some((bx, by)) = self.probe_at else {
			return false;
		};
		if self.probe_inflight.is_some() {
			return false;
		}
		let mut enc = device.create_command_encoder(&Default::default());
		enc.copy_texture_to_buffer(
			wgpu::TexelCopyTextureInfo {
				texture: &self.texture,
				mip_level: 0,
				origin: wgpu::Origin3d { x: bx, y: by, z: 0 },
				aspect: wgpu::TextureAspect::All,
			},
			wgpu::TexelCopyBufferInfo {
				buffer: &self.probe_buf,
				layout: wgpu::TexelCopyBufferLayout {
					offset: 0,
					bytes_per_row: Some(PROBE_SIDE * 4),
					rows_per_image: Some(PROBE_SIDE),
				},
			},
			wgpu::Extent3d {
				width: PROBE_SIDE,
				height: PROBE_SIDE,
				depth_or_array_layers: 1,
			},
		);
		queue.submit(Some(enc.finish()));
		let flag = Arc::new(AtomicU8::new(0));
		let done = flag.clone();
		self.probe_buf
			.slice(..)
			.map_async(wgpu::MapMode::Read, move |r| {
				done.store(if r.is_ok() { 1 } else { 2 }, Ordering::Release);
			});
		self.probe_inflight = Some(flag);
		true
	}

	// Poll an in-flight probe. Lost is not reseeded here - on loss the caller
	// reloads the wallpaper wholesale (recover_gpu), replacing this instance.
	pub fn vram_check_poll(&mut self, device: &wgpu::Device) -> Option<WpProbe> {
		let flag = self.probe_inflight.as_ref()?.clone();
		if flag.load(Ordering::Acquire) == 0 {
			// non-blocking pump so the map callback can run
			let _ = device.poll(wgpu::PollType::Poll);
		}
		match flag.load(Ordering::Acquire) {
			0 => None,
			2 => {
				self.probe_inflight = None;
				Some(WpProbe::MapFailed)
			}
			_ => {
				self.probe_inflight = None;
				let intact = {
					let data = self.probe_buf.slice(..).get_mapped_range();
					data[..] == self.probe_ref[..]
				};
				self.probe_buf.unmap();
				Some(if intact {
					WpProbe::Intact
				} else {
					WpProbe::Lost
				})
			}
		}
	}

	// Diagnostic (SILK_VRAMLOSS): zero the probe block to fake a content loss.
	pub fn vram_clobber(&self, queue: &wgpu::Queue) {
		let Some((bx, by)) = self.probe_at else {
			return;
		};
		queue.write_texture(
			wgpu::TexelCopyTextureInfo {
				texture: &self.texture,
				mip_level: 0,
				origin: wgpu::Origin3d { x: bx, y: by, z: 0 },
				aspect: wgpu::TextureAspect::All,
			},
			&[0u8; PROBE_BYTES],
			wgpu::TexelCopyBufferLayout {
				offset: 0,
				bytes_per_row: Some(PROBE_SIDE * 4),
				rows_per_image: Some(PROBE_SIDE),
			},
			wgpu::Extent3d {
				width: PROBE_SIDE,
				height: PROBE_SIDE,
				depth_or_array_layers: 1,
			},
		);
	}
}

const BG_WGSL: &str = r"
struct Uniform {
    resolution: vec2<f32>,
    image_size: vec2<f32>,
    amount: f32,
    fit: f32,
    anchor: vec2<f32>,
    bg: vec4<f32>,
    perceptual: f32,
};
@group(0) @binding(0) var<uniform> u: Uniform;
@group(0) @binding(1) var tex: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;

@vertex
fn vs(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    let corner = vec2<f32>(f32(vi & 1u), f32((vi >> 1u) & 1u));
    return vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0);
}

// The one sRGB encode runs on premultiplied values, so a see-through fill is
// written as the color that encodes to la times its own encoding (gfx::see_through).
fn enc(c: vec3<f32>) -> vec3<f32> {
    let cl = max(c, vec3<f32>(0.0));
    return select(1.055 * pow(cl, vec3<f32>(1.0 / 2.4)) - 0.055, cl * 12.92, cl <= vec3<f32>(0.0031308));
}
fn dec(c: vec3<f32>) -> vec3<f32> {
    let cl = max(c, vec3<f32>(0.0));
    return select(pow((cl + 0.055) / 1.055, vec3<f32>(2.4)), cl / 12.92, cl <= vec3<f32>(0.04045));
}

@fragment
fn fs(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let p = frag.xy; // framebuffer pixels (y-down)
    var uv: vec2<f32>;
    if (u.fit < 0.5) {
        uv = p / u.resolution; // stretch
    } else {
        // zoom / cover: fill while preserving aspect, crop about the anchor
        // (0 keeps the left/top edge, 1 the right/bottom, 0.5 centers)
        let scale = max(u.resolution.x / u.image_size.x, u.resolution.y / u.image_size.y);
        let disp = u.image_size * scale;
        uv = (p + (disp - u.resolution) * u.anchor) / disp;
    }
    let c = textureSample(tex, samp, uv);
    if (u.perceptual < 0.5) {
        let a = c.a * u.amount;
        return vec4<f32>(c.rgb * a, a); // premultiplied
    }
    // Light mode. The eye reads a power curve, not linear light, so the same
    // linear alpha that shows a picture over black shows almost nothing over
    // white - mix in the curve instead. That needs the background color, which a
    // hardware blend cannot supply, so this writes the pane fill itself and the
    // caller clips it to the pane.
    // (No double quotes anywhere in here - the whole shader is one raw literal.)
    let filled = c.rgb + u.bg.rgb * (1.0 - c.a); // the picture's own holes
    let e = vec3<f32>(1.0 / 2.4);
    let lo = pow(max(u.bg.rgb, vec3<f32>(0.0)), e);
    let hi = pow(max(filled, vec3<f32>(0.0)), e);
    let mixed = pow(mix(lo, hi, vec3<f32>(u.amount)), vec3<f32>(2.4));
    let la = u.bg.a;
    if (la < 1.0) {
        return vec4<f32>(dec(enc(mixed) * la), la);
    }
    return vec4<f32>(mixed * la, la); // premultiplied
}
";
