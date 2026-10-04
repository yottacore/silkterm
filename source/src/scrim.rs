// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! Text readability scrim: a background-colored halo behind glyphs so text stays
//! legible over a light/busy background image or a near-transparent terminal. The
//! scene's text is rendered to a coverage texture, turned into a halo by one of
//! four functions, and composited UNDER the crisp text, colored per-pixel by a
//! `bgcolor` map so a glyph's halo takes ITS cell's bg color (a glyph on a
//! one-off colored cell isn't smeared with the global bg color).
//!
//! Two passes build the halo in `tex_a` (`tex_t` stays crisp for the border). Gaussian
//! (legacy, corners recede) is a separable sum-blur; the distance functions (dilate
//! / sdf / dt) are a separable, bounded Euclidean/Chebyshev distance transform so
//! corners stay full - pass a = per-column 1D distance, pass b = row combine. The
//! composite maps the blurred coverage OR the distance (through a falloff curve) to
//! the per-pixel bg color, plus a thin dilated outline of the crisp coverage.
//!
//! `tex_t` <- crisp TEXT coverage; `tex_cur` <- crisp CURSOR coverage (kept apart so
//! the cursor can join the halo and the outline independently, each by its own
//! flag; the first pass folds `tex_cur` in when `cursor_scrim`, the composite samples
//! `tex_t` / `tex_cur` to add the border when `cursor_outline`).

use crate::gfx::{RectInstance, RectRenderer};

pub const FMT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct BlurU {
	resolution: [f32; 2],
	dir: [f32; 2],
	sigma: f32,  // gaussian path: blur sigma in px
	ramp: f32,   // falloff curve: 0 sigmoid, 1 half-normal, 2 linear, 3 log, 4 exp
	cursor: f32, // 1 = fold the cursor coverage into the halo, 0 = leave it out
	radius: f32, // distance path: halo extent in px (also bounds the tap loop)
	metric: f32, // distance path: 0 = euclidean, 1 = chebyshev (square)
	_pad: [f32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct CompU {
	resolution: [f32; 2],
	intensity: f32, // coverage boost; the color comes from the bgcolor texture
	border_px: f32, // dilated outline radius around the crisp coverage (0 = none)
	cursor: f32,    // 1 = give the cursor an outline too, 0 = text only
	function: f32,  // 0 dilate, 1 sdf, 2 dt (distance paths), 3 gaussian (legacy blur)
	ramp: f32,      // falloff curve (distance path transfer)
	radius: f32,    // distance path: halo extent in px (normalizes the distance)
	strength: f32,  // doublings of the finished halo alpha, 0..5 (0 = as built)
	// 0 = outline only: the blur did not run, so the halo texture is stale.
	// (Also what rounds the struct up to the WGSL side's 8-byte alignment.)
	halo: f32,
}

pub struct Scrim {
	tex_t: wgpu::Texture, // crisp text coverage (kept for the border pass)
	tex_a: wgpu::Texture,
	tex_b: wgpu::Texture,
	view_t: wgpu::TextureView,
	view_a: wgpu::TextureView,
	view_b: wgpu::TextureView,
	// crisp cursor coverage, separate from the text so cursor_scrim (halo) and
	// cursor_outline (border) are independent toggles - folded into the halo by
	// the blur and into the border by the composite, each gated by its own flag.
	tex_cur: wgpu::Texture,
	view_cur: wgpu::TextureView,
	sampler: wgpu::Sampler,
	blur_pipe: wgpu::RenderPipeline,
	// distance-field paths (dilate/sdf/dt) reuse the blur bind groups + textures:
	// pass a = per-column 1D distance (tex_t->tex_b), pass b = row combine into the
	// final distance (tex_b->tex_a), metric per the selected function.
	dist_a_pipe: wgpu::RenderPipeline,
	dist_b_pipe: wgpu::RenderPipeline,
	blur_bgl: wgpu::BindGroupLayout,
	// one uniform PER direction: all queue.write_buffer calls are applied before
	// the command buffer runs, so a single shared buffer would give BOTH passes
	// the last-written dir (-> vertical blur twice, no horizontal). Two buffers fix it.
	blur_u_h: wgpu::Buffer,
	blur_u_v: wgpu::Buffer,
	blur_t2b: wgpu::BindGroup, // sample tex_t (uses blur_u_h), write tex_b
	blur_b2a: wgpu::BindGroup, // sample tex_b (uses blur_u_v), write tex_a
	comp_pipe: wgpu::RenderPipeline,
	comp_bgl: wgpu::BindGroupLayout,
	comp_u: wgpu::Buffer,
	comp_bind: wgpu::BindGroup, // sample tex_a (scrim alpha) + bgcolor (rgb) + tex_t (border)
	// per-pixel scrim color: cleared to the global bg, with per-cell bg rects drawn
	// over it, so a glyph's halo takes ITS cell's bg color (not always the global).
	bgcolor: wgpu::Texture,
	bgcolor_view: wgpu::TextureView,
	bg_rects: RectRenderer,
	// cursor quads drawn into tex_cur (its own coverage texture). Separate renderer:
	// bg_rects' instance buffer is uploaded for the bgcolor map in the SAME encoder,
	// and a second upload would clobber the first (queue writes all arrive before the
	// command buffer runs - same rule as the blur uniforms above).
	cursor_rects: RectRenderer,
	cursor_count: u32,
	// what is allocated, and what the surface actually is. With the scrim and the
	// outline both off nothing here draws, so the five full-screen textures are
	// allocated at one pixel instead - around 330 MB of VRAM at 3840x2160, and it
	// falls hardest on the machines the Low and Standard profiles exist for.
	w: u32,
	h: u32,
	surf_w: u32,
	surf_h: u32,
	enabled: bool,
}

impl std::fmt::Debug for Scrim {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("Scrim").finish_non_exhaustive()
	}
}

// The widest halo the distance passes can measure. They tap at most DIST_MAX
// pixels, and the composite divides by the extent it is given - so an extent past
// this made every pixel of every pane come out at full halo, a flat plate of
// background color. Both halves read the same number now.
pub const EXT_MAX: f32 = 40.0;

pub fn clamp_ext(ext: f32) -> f32 {
	ext.clamp(0.0, EXT_MAX)
}

// How big the texture set should be. One pixel when neither the scrim nor the
// outline draws: at full screen this is three Rgba16Float textures plus two
// more, which is hundreds of megabytes of VRAM for a feature doing nothing.
fn alloc_size(enabled: bool, surface: (u32, u32)) -> (u32, u32) {
	if enabled { surface } else { (1, 1) }
}

impl Scrim {
	pub fn new(device: &wgpu::Device, target: wgpu::TextureFormat, w: u32, h: u32) -> Self {
		let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
			label: Some("scrim shader"),
			source: wgpu::ShaderSource::Wgsl(WGSL.into()),
		});
		let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
			label: Some("scrim sampler"),
			mag_filter: wgpu::FilterMode::Linear,
			min_filter: wgpu::FilterMode::Linear,
			address_mode_u: wgpu::AddressMode::ClampToEdge,
			address_mode_v: wgpu::AddressMode::ClampToEdge,
			..Default::default()
		});
		// blur: uniform + sampled texture + sampler
		let blur_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("scrim blur bgl"),
			// binding 3 = the bgcolor map, whose alpha is an "own-bg" mask (see fs_blur);
			// binding 4 = the crisp cursor coverage (folded in when cursor_scrim)
			entries: &[
				ubuf_entry(0),
				tex_entry(1),
				samp_entry(2),
				tex_entry(3),
				tex_entry(4),
			],
		});
		let make_uniform_buf = |label| {
			device.create_buffer(&wgpu::BufferDescriptor {
				label: Some(label),
				size: std::mem::size_of::<BlurU>() as u64,
				usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
				mapped_at_creation: false,
			})
		};
		let blur_u_h = make_uniform_buf("scrim blur u h");
		let blur_u_v = make_uniform_buf("scrim blur u v");
		let blur_pipe = pipeline(device, &shader, "fs_blur", FMT, &blur_bgl, "scrim blur");
		let dist_a_pipe = pipeline(device, &shader, "fs_dist_a", FMT, &blur_bgl, "scrim dist a");
		let dist_b_pipe = pipeline(device, &shader, "fs_dist_b", FMT, &blur_bgl, "scrim dist b");

		let comp_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("scrim comp bgl"),
			entries: &[
				ubuf_entry(0),
				tex_entry(1),
				samp_entry(2),
				tex_entry(3),
				tex_entry(4),
				tex_entry(5),
			],
		});
		let comp_u = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("scrim comp u"),
			size: std::mem::size_of::<CompU>() as u64,
			usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});
		let comp_pipe = pipeline_blend(device, &shader, "fs_comp", target, &comp_bgl, "scrim comp");

		// nothing is drawn until something asks for it (see `set_enabled`)
		let (tex_t, tex_a, tex_b, view_t, view_a, view_b) = make_textures(device, 1, 1);
		let (tex_cur, view_cur) = cover_tex(device, 1, 1);
		let bgcolor = bgcolor_tex(device, 1, 1);
		let bgcolor_view = bgcolor.create_view(&Default::default());
		let bg_rects = RectRenderer::new(device, FMT);
		let cursor_rects = RectRenderer::new(device, FMT);
		let (blur_t2b, blur_b2a, comp_bind) = binds(
			device,
			&blur_bgl,
			&comp_bgl,
			&blur_u_h,
			&blur_u_v,
			&comp_u,
			&sampler,
			&view_t,
			&view_a,
			&view_b,
			&bgcolor_view,
			&view_cur,
		);

		Self {
			tex_t,
			tex_a,
			tex_b,
			view_t,
			view_a,
			view_b,
			tex_cur,
			view_cur,
			sampler,
			blur_pipe,
			dist_a_pipe,
			dist_b_pipe,
			blur_bgl,
			blur_u_h,
			blur_u_v,
			blur_t2b,
			blur_b2a,
			comp_pipe,
			comp_bgl,
			comp_u,
			comp_bind,
			bgcolor,
			bgcolor_view,
			bg_rects,
			cursor_rects,
			cursor_count: 0,
			w: 1,
			h: 1,
			surf_w: w,
			surf_h: h,
			enabled: false,
		}
	}

	// Answers whether anything was reallocated, which is the caller's cue that
	// this frame's prepared set is stale.
	pub fn set_enabled(&mut self, device: &wgpu::Device, on: bool) -> bool {
		if on == self.enabled {
			return false;
		}
		self.enabled = on;
		self.reallocate(device)
	}

	fn reallocate(&mut self, device: &wgpu::Device) -> bool {
		let (w, h) = alloc_size(self.enabled, (self.surf_w, self.surf_h));
		if w == 0 || h == 0 || (w == self.w && h == self.h) {
			return false;
		}
		self.rebuild(device, w, h);
		true
	}

	pub fn resize(&mut self, device: &wgpu::Device, w: u32, h: u32) {
		if w == 0 || h == 0 {
			return;
		}
		self.surf_w = w;
		self.surf_h = h;
		self.reallocate(device);
	}

	fn rebuild(&mut self, device: &wgpu::Device, w: u32, h: u32) {
		let (tex_t, tex_a, tex_b, view_t, view_a, view_b) = make_textures(device, w, h);
		self.tex_t = tex_t;
		self.tex_a = tex_a;
		self.tex_b = tex_b;
		self.view_t = view_t;
		self.view_a = view_a;
		self.view_b = view_b;
		let (tex_cur, view_cur) = cover_tex(device, w, h);
		self.tex_cur = tex_cur;
		self.view_cur = view_cur;
		self.bgcolor = bgcolor_tex(device, w, h);
		self.bgcolor_view = self.bgcolor.create_view(&Default::default());
		let (blur_t2b, blur_b2a, comp_bind) = binds(
			device,
			&self.blur_bgl,
			&self.comp_bgl,
			&self.blur_u_h,
			&self.blur_u_v,
			&self.comp_u,
			&self.sampler,
			&self.view_t,
			&self.view_a,
			&self.view_b,
			&self.bgcolor_view,
			&self.view_cur,
		);
		self.blur_t2b = blur_t2b;
		self.blur_b2a = blur_b2a;
		self.comp_bind = comp_bind;
		self.w = w;
		self.h = h;
	}

	// Build the per-pixel scrim-color map: clear to the global bg color, then draw
	// the per-cell bg rects (opaque) over it. A glyph's halo then takes its own
	// cell's bg color instead of always the global one. The alpha channel doubles
	// as an "own-bg" mask - cleared to 0, the opaque cell rects write 1, so the blur
	// can drop coverage from cells that already carry a solid bg (reverse video,
	// colored bg, selection): they have full contrast, so a halo there is only
	// artifact (nano's reverse header cast a jumping drop-shadow). See fs_blur.
	pub fn render_bgcolor(
		&mut self,
		device: &wgpu::Device,
		queue: &wgpu::Queue,
		encoder: &mut wgpu::CommandEncoder,
		cells: &[RectInstance],
		global_bg: [f32; 4],
	) {
		self.bg_rects
			.set_resolution(queue, self.w as f32, self.h as f32);
		self.bg_rects.upload(device, queue, cells);
		let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
			label: Some("scrim bgcolor"),
			color_attachments: &[Some(wgpu::RenderPassColorAttachment {
				view: &self.bgcolor_view,
				resolve_target: None,
				depth_slice: None,
				ops: wgpu::Operations {
					load: wgpu::LoadOp::Clear(wgpu::Color {
						r: global_bg[0] as f64,
						g: global_bg[1] as f64,
						b: global_bg[2] as f64,
						a: 0.0, // own-bg mask: 0 here, the cell rects write 1 (see fs_blur)
					}),
					store: wgpu::StoreOp::Store,
				},
			})],
			depth_stencil_attachment: None,
			timestamp_writes: None,
			occlusion_query_set: None,
			multiview_mask: None,
		});
		self.bg_rects.draw(&mut pass, 0..cells.len() as u32);
	}

	// The render target for the scene's text (tex_t). Clear it transparent and
	// render the prepared text into it before calling `blur`.
	pub fn text_view(&self) -> &wgpu::TextureView {
		&self.view_t
	}

	// The render target for the cursor coverage (tex_cur), separate from the text.
	// Clear it transparent and draw the cursor quads (`draw_cursors`) into it before
	// calling `blur`; the flags in `blur`/`composite` decide where it contributes.
	pub fn cursor_view(&self) -> &wgpu::TextureView {
		&self.view_cur
	}

	// Upload the cursor quads destined for tex_cur. Call before the cursor pass;
	// draw with `draw_cursors`.
	pub fn upload_cursors(
		&mut self,
		device: &wgpu::Device,
		queue: &wgpu::Queue,
		quads: &[RectInstance],
	) {
		self.cursor_rects
			.set_resolution(queue, self.w as f32, self.h as f32);
		self.cursor_rects.upload(device, queue, quads);
		self.cursor_count = quads.len() as u32;
	}

	// Draw the uploaded cursor quads into the current (tex_cur) pass.
	pub fn draw_cursors(&self, pass: &mut wgpu::RenderPass<'_>) {
		if self.cursor_count > 0 {
			self.cursor_rects.draw(pass, 0..self.cursor_count);
		}
	}

	// Two separable passes producing the scrim in tex_a; tex_t keeps the crisp
	// coverage for the border pass. `function` picks the path: gaussian (3) runs the
	// legacy sum-blur (H tex_t->tex_b, V tex_b->tex_a) shaped by `ramp`; the distance
	// paths (dilate 0 / sdf 1 / dt 2) run a separable Euclidean/Chebyshev distance
	// transform (pass a = per-column 1D distance, pass b = row combine) into tex_a,
	// bounded to `radius`. `sigma` = gaussian blur sigma; `radius` = distance extent.
	// `cursor` (0/1) folds the cursor coverage in - only in the first pass (the second
	// reads tex_b, which already carries it, so its flag stays 0).
	pub fn blur(
		&self,
		queue: &wgpu::Queue,
		encoder: &mut wgpu::CommandEncoder,
		sigma: f32,
		radius: f32,
		ramp: f32,
		cursor: f32,
		function: f32,
	) {
		let res = [self.w as f32, self.h as f32];
		let gaussian = function >= 2.5;
		let metric = if function < 0.5 { 1.0 } else { 0.0 }; // dilate = chebyshev, else euclid
		// write both uniforms up front (they target different buffers, so neither
		// overwrites the other when the queue applies them before the passes run)
		queue.write_buffer(
			&self.blur_u_h,
			0,
			bytemuck::bytes_of(&BlurU {
				resolution: res,
				dir: [1.0, 0.0],
				sigma,
				ramp,
				cursor,
				radius,
				metric,
				_pad: [0.0; 3],
			}),
		);
		queue.write_buffer(
			&self.blur_u_v,
			0,
			bytemuck::bytes_of(&BlurU {
				resolution: res,
				dir: [0.0, 1.0],
				sigma,
				ramp,
				cursor: 0.0,
				radius,
				metric,
				_pad: [0.0; 3],
			}),
		);
		let (pipe_a, pipe_b) = if gaussian {
			(&self.blur_pipe, &self.blur_pipe)
		} else {
			(&self.dist_a_pipe, &self.dist_b_pipe)
		};
		for (pipe, src_bind, dst) in [
			(pipe_a, &self.blur_t2b, &self.view_b),
			(pipe_b, &self.blur_b2a, &self.view_a),
		] {
			let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
				label: Some("scrim blur pass"),
				color_attachments: &[Some(wgpu::RenderPassColorAttachment {
					view: dst,
					resolve_target: None,
					depth_slice: None,
					ops: wgpu::Operations {
						load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
						store: wgpu::StoreOp::Store,
					},
				})],
				depth_stencil_attachment: None,
				timestamp_writes: None,
				occlusion_query_set: None,
				multiview_mask: None,
			});
			pass.set_pipeline(pipe);
			pass.set_bind_group(0, src_bind, &[]);
			pass.draw(0..3, 0..1);
		}
	}

	// Upload the composite uniform. Split from composite(): the draw runs once
	// per pane (scissored), but the args are frame-invariant, so the render loop
	// writes this once instead of staging an identical write per pane.
	pub fn write_comp_uniform(
		&self,
		queue: &wgpu::Queue,
		intensity: f32,
		border_px: f32,
		cursor: f32,
		function: f32,
		ramp: f32,
		radius: f32,
		strength: f32,
		halo: f32,
	) {
		queue.write_buffer(
			&self.comp_u,
			0,
			bytemuck::bytes_of(&CompU {
				resolution: [self.w as f32, self.h as f32],
				intensity,
				border_px,
				cursor,
				function,
				ramp,
				radius,
				strength,
				halo,
			}),
		);
	}

	// Draw the scrim into the current pass, under the text: blurred coverage from
	// tex_a, colored per-pixel by the bgcolor map, plus a `border_px` dilated
	// outline of the crisp coverage (tex_t, + tex_cur when `cursor` is 1).
	// write_comp_uniform must have run this frame.
	pub fn composite(&self, pass: &mut wgpu::RenderPass<'_>) {
		pass.set_pipeline(&self.comp_pipe);
		pass.set_bind_group(0, &self.comp_bind, &[]);
		pass.draw(0..3, 0..1);
	}
}

#[allow(clippy::type_complexity)]
fn make_textures(
	device: &wgpu::Device,
	w: u32,
	h: u32,
) -> (
	wgpu::Texture,
	wgpu::Texture,
	wgpu::Texture,
	wgpu::TextureView,
	wgpu::TextureView,
	wgpu::TextureView,
) {
	let desc = |label| wgpu::TextureDescriptor {
		label: Some(label),
		size: wgpu::Extent3d {
			width: w.max(1),
			height: h.max(1),
			depth_or_array_layers: 1,
		},
		mip_level_count: 1,
		sample_count: 1,
		dimension: wgpu::TextureDimension::D2,
		format: FMT,
		usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
		view_formats: &[],
	};
	let tex_t = device.create_texture(&desc("scrim tex t"));
	let tex_a = device.create_texture(&desc("scrim tex a"));
	let tex_b = device.create_texture(&desc("scrim tex b"));
	let view_t = tex_t.create_view(&Default::default());
	let view_a = tex_a.create_view(&Default::default());
	let view_b = tex_b.create_view(&Default::default());
	(tex_t, tex_a, tex_b, view_t, view_a, view_b)
}

// A single FMT coverage texture + its view (the cursor's crisp coverage).
fn cover_tex(device: &wgpu::Device, w: u32, h: u32) -> (wgpu::Texture, wgpu::TextureView) {
	let tex = device.create_texture(&wgpu::TextureDescriptor {
		label: Some("scrim tex cur"),
		size: wgpu::Extent3d {
			width: w.max(1),
			height: h.max(1),
			depth_or_array_layers: 1,
		},
		mip_level_count: 1,
		sample_count: 1,
		dimension: wgpu::TextureDimension::D2,
		format: FMT,
		usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
		view_formats: &[],
	});
	let view = tex.create_view(&Default::default());
	(tex, view)
}

fn bgcolor_tex(device: &wgpu::Device, w: u32, h: u32) -> wgpu::Texture {
	device.create_texture(&wgpu::TextureDescriptor {
		label: Some("scrim bgcolor"),
		size: wgpu::Extent3d {
			width: w.max(1),
			height: h.max(1),
			depth_or_array_layers: 1,
		},
		mip_level_count: 1,
		sample_count: 1,
		dimension: wgpu::TextureDimension::D2,
		format: FMT,
		usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
		view_formats: &[],
	})
}

#[allow(clippy::too_many_arguments)]
fn binds(
	device: &wgpu::Device,
	blur_bgl: &wgpu::BindGroupLayout,
	comp_bgl: &wgpu::BindGroupLayout,
	blur_u_h: &wgpu::Buffer,
	blur_u_v: &wgpu::Buffer,
	comp_u: &wgpu::Buffer,
	sampler: &wgpu::Sampler,
	view_t: &wgpu::TextureView,
	view_a: &wgpu::TextureView,
	view_b: &wgpu::TextureView,
	bgcolor_view: &wgpu::TextureView,
	view_cur: &wgpu::TextureView,
) -> (wgpu::BindGroup, wgpu::BindGroup, wgpu::BindGroup) {
	let mk_blur = |ubuf: &wgpu::Buffer, view| {
		device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("scrim blur bind"),
			layout: blur_bgl,
			entries: &[
				wgpu::BindGroupEntry {
					binding: 0,
					resource: ubuf.as_entire_binding(),
				},
				wgpu::BindGroupEntry {
					binding: 1,
					resource: wgpu::BindingResource::TextureView(view),
				},
				wgpu::BindGroupEntry {
					binding: 2,
					resource: wgpu::BindingResource::Sampler(sampler),
				},
				wgpu::BindGroupEntry {
					binding: 3,
					resource: wgpu::BindingResource::TextureView(bgcolor_view),
				},
				wgpu::BindGroupEntry {
					binding: 4,
					resource: wgpu::BindingResource::TextureView(view_cur),
				},
			],
		})
	};
	let comp = device.create_bind_group(&wgpu::BindGroupDescriptor {
		label: Some("scrim comp bind"),
		layout: comp_bgl,
		entries: &[
			wgpu::BindGroupEntry {
				binding: 0,
				resource: comp_u.as_entire_binding(),
			},
			wgpu::BindGroupEntry {
				binding: 1,
				resource: wgpu::BindingResource::TextureView(view_a),
			},
			wgpu::BindGroupEntry {
				binding: 2,
				resource: wgpu::BindingResource::Sampler(sampler),
			},
			wgpu::BindGroupEntry {
				binding: 3,
				resource: wgpu::BindingResource::TextureView(bgcolor_view),
			},
			wgpu::BindGroupEntry {
				binding: 4,
				resource: wgpu::BindingResource::TextureView(view_t),
			},
			wgpu::BindGroupEntry {
				binding: 5,
				resource: wgpu::BindingResource::TextureView(view_cur),
			},
		],
	});
	// t2b: H pass samples tex_t (horizontal uniform); b2a: V pass samples tex_b.
	(mk_blur(blur_u_h, view_t), mk_blur(blur_u_v, view_b), comp)
}

fn ubuf_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
	wgpu::BindGroupLayoutEntry {
		binding,
		visibility: wgpu::ShaderStages::FRAGMENT,
		ty: wgpu::BindingType::Buffer {
			ty: wgpu::BufferBindingType::Uniform,
			has_dynamic_offset: false,
			min_binding_size: None,
		},
		count: None,
	}
}
fn tex_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
	wgpu::BindGroupLayoutEntry {
		binding,
		visibility: wgpu::ShaderStages::FRAGMENT,
		ty: wgpu::BindingType::Texture {
			sample_type: wgpu::TextureSampleType::Float { filterable: true },
			view_dimension: wgpu::TextureViewDimension::D2,
			multisampled: false,
		},
		count: None,
	}
}
fn samp_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
	wgpu::BindGroupLayoutEntry {
		binding,
		visibility: wgpu::ShaderStages::FRAGMENT,
		ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
		count: None,
	}
}

fn pipeline(
	device: &wgpu::Device,
	shader: &wgpu::ShaderModule,
	fs: &str,
	format: wgpu::TextureFormat,
	bgl: &wgpu::BindGroupLayout,
	label: &str,
) -> wgpu::RenderPipeline {
	make_pipeline(device, shader, fs, format, bgl, label, None)
}
fn pipeline_blend(
	device: &wgpu::Device,
	shader: &wgpu::ShaderModule,
	fs: &str,
	format: wgpu::TextureFormat,
	bgl: &wgpu::BindGroupLayout,
	label: &str,
) -> wgpu::RenderPipeline {
	make_pipeline(
		device,
		shader,
		fs,
		format,
		bgl,
		label,
		Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
	)
}

#[allow(clippy::too_many_arguments)]
fn make_pipeline(
	device: &wgpu::Device,
	shader: &wgpu::ShaderModule,
	fs: &str,
	format: wgpu::TextureFormat,
	bgl: &wgpu::BindGroupLayout,
	label: &str,
	blend: Option<wgpu::BlendState>,
) -> wgpu::RenderPipeline {
	let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
		label: Some(label),
		bind_group_layouts: &[Some(bgl)],
		immediate_size: 0,
	});
	device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
		label: Some(label),
		layout: Some(&layout),
		vertex: wgpu::VertexState {
			module: shader,
			entry_point: Some("vs"),
			compilation_options: Default::default(),
			buffers: &[],
		},
		fragment: Some(wgpu::FragmentState {
			module: shader,
			entry_point: Some(fs),
			compilation_options: Default::default(),
			targets: &[Some(wgpu::ColorTargetState {
				format,
				blend,
				write_mask: wgpu::ColorWrites::ALL,
			})],
		}),
		primitive: wgpu::PrimitiveState::default(),
		depth_stencil: None,
		multisample: wgpu::MultisampleState::default(),
		multiview_mask: None,
		cache: None,
	})
}

const WGSL: &str = r"
struct VsOut { @builtin(position) clip: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex
fn vs(@builtin(vertex_index) i: u32) -> VsOut {
    var xy = array<vec2<f32>, 3>(vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0));
    let p = xy[i];
    var o: VsOut;
    o.clip = vec4<f32>(p, 0.0, 1.0);
    o.uv = vec2<f32>((p.x + 1.0) * 0.5, 1.0 - (p.y + 1.0) * 0.5);
    return o;
}

// scalar pads (NOT vec3, which would force 16-byte alignment and mismatch the
// 48-byte Rust struct)
struct BlurU { resolution: vec2<f32>, dir: vec2<f32>, sigma: f32, ramp: f32, cursor: f32, radius: f32, metric: f32, _p0: f32, _p1: f32, _p2: f32 };
@group(0) @binding(0) var<uniform> bu: BlurU;
@group(0) @binding(1) var tex: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;
@group(0) @binding(3) var bmask: texture_2d<f32>; // bgcolor map; .a = own-bg mask
@group(0) @binding(4) var bcur: texture_2d<f32>;  // crisp cursor coverage

const DIST_MAX: i32 = 40; // hard cap on the distance-transform tap window

// Shared falloff curve, used both as the gaussian blur kernel weight and as the
// distance-path transfer. `t` is normalized 0 (glyph, full) .. 1 (edge, zero).
// ramp: 0 sigmoid, 1 half-normal, 2 linear, 3 logarithmic, 4 exponential.
fn falloff(td: f32, ramp: f32) -> f32 {
    let t = clamp(td, 0.0, 1.0);
    if (ramp < 0.5) {                 // sigmoid (smoothstep on 1-t)
        let u = 1.0 - t;
        return u * u * (3.0 - 2.0 * u);
    } else if (ramp < 1.5) {          // half-normal
        // normalized to reach 0 at the edge, the way the exponential below is:
        // a bell alone still stands at ~1.1% there, and Strength multiplies that
        // floor into a solid wash over the whole pane. Costs ~1% of peak alpha.
        let e = exp(-4.5);
        return (exp(-4.5 * t * t) - e) / (1.0 - e);
    } else if (ramp < 2.5) {          // linear (tent)
        return 1.0 - t;
    } else if (ramp < 3.5) {          // logarithmic: drops fast, then slow
        return clamp(1.0 - log(1.0 + 1.7182818 * t), 0.0, 1.0);
    } else {                          // exponential: drops away hard
        let k = 6.0;
        return (exp(-k * t) - exp(-k)) / (1.0 - exp(-k));
    }
}

// separable gaussian [ugly] blur; fixed 25 taps scaled by sigma so any radius is
// ~3sigma-covered. Corners recede (a round kernel) - the distance paths fix that.
//
// Each tap is gated by `keep` = 1 - own-bg mask, so coverage sitting over a cell
// with its own solid bg (reverse video, colored bg, selection) contributes to no
// pixel's halo. Gating the SOURCE coverage (not the final pixel) is what stops a
// reverse-video header's glyphs from bleeding a halo below the bar - the artifact
// that read as a drop-shadow jumping with the app-scroll slide.
@fragment
fn fs_blur(in: VsOut) -> @location(0) vec4<f32> {
    let texel = 1.0 / bu.resolution;
    let s = max(bu.sigma, 0.0001);
    let spacing = max(1.0, s * 3.0 / 12.0);
    let ext = s * 3.0; // kernel extent; the falloff hits (near) zero here
    var sum = vec4<f32>(0.0);
    var wsum = 0.0;
    for (var i = -12; i <= 12; i = i + 1) {
        let off = f32(i) * spacing;
        let w = falloff(abs(off) / ext, bu.ramp);
        let uv = in.uv + bu.dir * (off * texel);
        let keep = 1.0 - textureSample(bmask, samp, uv).a;
        // fold the cursor coverage into the H pass (bu.cursor is 0 in the V pass)
        let cov = textureSample(tex, samp, uv) + bu.cursor * textureSample(bcur, samp, uv);
        sum += cov * (w * keep);
        wsum += w;
    }
    return sum / wsum;
}

// Distance transform, pass a: per-column 1D distance to the nearest coverage seed
// within `radius`, scanning vertically. A seed is coverage (text + folded cursor)
// that is NOT on an own-bg cell (same gating as fs_blur). Stored in .r; saturates
// at radius (beyond it the halo is zero anyway, so no bigger value is needed).
@fragment
fn fs_dist_a(in: VsOut) -> @location(0) vec4<f32> {
    let texel = 1.0 / bu.resolution;
    let R = clamp(bu.radius, 1.0, f32(DIST_MAX));
    let RI = min(i32(ceil(R)), DIST_MAX);
    var best = R;
    for (var k = -RI; k <= RI; k = k + 1) {
        let uv = in.uv + vec2<f32>(0.0, f32(k)) * texel;
        let keep = 1.0 - textureSample(bmask, samp, uv).a;
        let cov = textureSample(tex, samp, uv).a + bu.cursor * textureSample(bcur, samp, uv).a;
        let seed = step(0.5, cov * keep);
        best = min(best, mix(R, f32(abs(k)), seed));
    }
    return vec4<f32>(best, 0.0, 0.0, 1.0);
}

// Distance transform, pass b: combine the per-column distances horizontally into
// the final 2D distance. `metric` picks euclidean (round, corners stay full) or
// chebyshev (square growth - the dilation look). Bounded to `radius`.
@fragment
fn fs_dist_b(in: VsOut) -> @location(0) vec4<f32> {
    let texel = 1.0 / bu.resolution;
    let R = clamp(bu.radius, 1.0, f32(DIST_MAX));
    let RI = min(i32(ceil(R)), DIST_MAX);
    var best = R;
    for (var k = -RI; k <= RI; k = k + 1) {
        let uv = in.uv + vec2<f32>(f32(k), 0.0) * texel;
        let dv = textureSample(tex, samp, uv).r; // per-column vertical distance
        let kx = f32(abs(k));
        var d = sqrt(dv * dv + kx * kx);         // euclidean
        if (bu.metric >= 0.5) {
            d = max(dv, kx);                     // chebyshev (square)
        }
        best = min(best, d);
    }
    return vec4<f32>(best, 0.0, 0.0, 1.0);
}

struct CompU { resolution: vec2<f32>, intensity: f32, border_px: f32, cursor: f32, function: f32, ramp: f32, radius: f32, strength: f32, halo: f32 };
@group(0) @binding(0) var<uniform> cu: CompU;
@group(0) @binding(1) var gtex: texture_2d<f32>;   // scrim: blurred coverage (.a) or distance (.r)
@group(0) @binding(2) var gsamp: sampler;
@group(0) @binding(3) var bgtex: texture_2d<f32>;  // per-pixel scrim color
@group(0) @binding(4) var ttex: texture_2d<f32>;   // crisp glyph coverage
@group(0) @binding(5) var ccur: texture_2d<f32>;   // crisp cursor coverage

// color the scrim coverage per-pixel by the local bg color; premultiplied.
// border: dilate the crisp coverage by border_px (8 taps; linear sampling keeps
// it antialiased) and take the union with the scrim - a solid bg-colored plate
// hugging each glyph. The crisp text draws over its interior, so what remains
// visible is the thin outline around the letterforms. Each border tap is gated by
// the own-bg mask (bgtex.a) too, matching fs_blur, so an own-bg glyph casts no
// outline (the blurred halo is already masked at its source). The cursor coverage
// joins the outline source only when cu.cursor is 1.
fn border_tap(uv: vec2<f32>) -> f32 {
    // the cursor's own coverage is only wanted when the cursor outline is on
    var cov = textureSample(ttex, gsamp, uv).a;
    if (cu.cursor > 0.5) {
        cov = max(cov, textureSample(ccur, gsamp, uv).a);
    }
    return cov * (1.0 - textureSample(bgtex, gsamp, uv).a);
}
@fragment
fn fs_comp(in: VsOut) -> @location(0) vec4<f32> {
    var ga = 0.0;
    if (cu.function >= 2.5) {
        // gaussian [ugly]: blurred coverage alpha, boosted by intensity
        ga = clamp(textureSample(gtex, gsamp, in.uv).a * cu.intensity, 0.0, 1.0);
    } else {
        // distance paths: gtex.r is the distance to the nearest coverage; run it
        // through the falloff curve. intensity/10 is the peak alpha (softness).
        let t = textureSample(gtex, gsamp, in.uv).r / max(cu.radius, 0.001);
        var w = falloff(t, cu.ramp);
        if (cu.function >= 1.5) {
            w = smoothstep(0.12, 0.5, w); // dt: harden the soft glow into a solid plate
        }
        ga = clamp(w * (cu.intensity / 10.0), 0.0, 1.0);
    }
    // Strength: double the halo alpha cu.strength times over (0 = leave it as
    // built). Clamping after the multiply is what makes it read as bolder rather
    // than merely brighter - the core saturates first and the solid part grows
    // outward along the falloff, so the plate thickens instead of the edge moving.
    // (No double quotes anywhere in here - the whole shader is one raw literal.)
    ga = clamp(ga * exp2(cu.strength), 0.0, 1.0) * cu.halo;
    let rgb = textureSample(bgtex, gsamp, in.uv).rgb;
    let texel = 1.0 / cu.resolution;
    // Eight taps, three samples each. They were run on every pixel of every frame
    // and multiplied by zero at the end; cu.border_px is uniform, so skipping
    // them is a uniform branch.
    var border = 0.0;
    if (cu.border_px > 0.001) {
        let r = max(cu.border_px, 0.0001);
        let dg = r * 0.7071; // diagonal taps at the same radius -> round outline
        var m = 0.0;
        m = max(m, border_tap(in.uv + vec2<f32>( r, 0.0) * texel));
        m = max(m, border_tap(in.uv + vec2<f32>(-r, 0.0) * texel));
        m = max(m, border_tap(in.uv + vec2<f32>(0.0,  r) * texel));
        m = max(m, border_tap(in.uv + vec2<f32>(0.0, -r) * texel));
        m = max(m, border_tap(in.uv + vec2<f32>( dg,  dg) * texel));
        m = max(m, border_tap(in.uv + vec2<f32>( dg, -dg) * texel));
        m = max(m, border_tap(in.uv + vec2<f32>(-dg,  dg) * texel));
        m = max(m, border_tap(in.uv + vec2<f32>(-dg, -dg) * texel));
        border = clamp(m, 0.0, 1.0);
    }
    let a = max(ga, border);
    return vec4<f32>(rgb * a, a);
}
";

#[cfg(test)]
mod tests {
	use super::{EXT_MAX, WGSL, alloc_size, clamp_ext};

	// The exponential arm's exponent, mirrored so the curve can be checked
	// without a GPU. The shader owns the number and the test below holds the
	// two against each other.
	const EXP_K: f32 = 6.0;

	fn exp_falloff(t: f32) -> f32 {
		let t = t.clamp(0.0, 1.0);
		let e = (-EXP_K).exp();
		(((-EXP_K * t).exp()) - e) / (1.0 - e)
	}

	// Bytes the set costs: three Rgba16Float (8 per pixel), the coverage texture
	// and the bgcolor map (4 each).
	fn bytes((w, h): (u32, u32)) -> u64 {
		u64::from(w) * u64::from(h) * (8 * 3 + 4 * 2)
	}

	// The scrim used to build its five full-screen textures whether or not it drew
	// anything, and it falls hardest on the machines the Low and Standard profiles
	// exist for.
	// The outline's eight taps are three texture samples each, on every pixel of
	// every frame, and the result was multiplied by zero when the outline was off.
	// Test ID: EpHbxSC
	#[test]
	fn the_outline_taps_only_run_when_there_is_an_outline() {
		let comp = WGSL
			.split("fn fs_comp")
			.nth(1)
			.expect("the composite shader");
		let guard = comp.find("if (cu.border_px > 0.001)").expect("no guard");
		let first_tap = comp.find("border_tap(").expect("no taps");
		assert!(guard < first_tap, "the taps run before the guard");
		// and the cursor coverage is only sampled when the cursor outline is on
		let tap = WGSL
			.split("fn border_tap")
			.nth(1)
			.expect("the tap function");
		assert!(
			tap.find("if (cu.cursor > 0.5)")
				.is_some_and(|at| at < tap.find("textureSample(ccur").expect("the cursor sample")),
			"the cursor texture is sampled with the cursor outline off"
		);
	}

	// Past the tap window the distance saturates while the composite kept dividing
	// by the extent it was given, so every pixel came out at full halo - a flat
	// plate of background color over every pane. The slider cannot reach it; the
	// config file can.
	// Test ID: EpHXu9Y
	#[test]
	fn a_halo_wider_than_the_taps_is_held_to_them() {
		// the shader owns the tap window; this is the other half of that number
		assert!(
			WGSL.contains(&format!("const DIST_MAX: i32 = {};", EXT_MAX as i32)),
			"EXT_MAX and the shader's tap window have drifted apart"
		);
		// the shipped default, doubled: unchanged
		assert!((clamp_ext(5.0 * 2.0) - 10.0).abs() < f32::EPSILON);
		// the slider's own ceiling, doubled: exactly the tap window
		assert!((clamp_ext(20.0 * 2.0) - EXT_MAX).abs() < f32::EPSILON);
		// and what the config file allows past it
		assert!((clamp_ext(50.0 * 2.0) - EXT_MAX).abs() < f32::EPSILON);
	}

	// The exponential curve is the one for a halo that hugs the glyph, so it has
	// to be well under a straight line rather than near it. It still has to
	// reach zero at the edge: the distance paths saturate there, so whatever it
	// returns at 1 is the alpha of every pixel in the pane.
	// Test ID: EqN5SfI
	#[test]
	fn the_exponential_falloff_drops_away_hard() {
		assert!(
			WGSL.contains(&format!("let k = {EXP_K:.1};")),
			"the exponent and the shader's own have drifted apart"
		);
		assert!((exp_falloff(0.0) - 1.0).abs() < 1e-6);
		assert_eq!(exp_falloff(1.0), 0.0);
		// a quarter of the way out it is already under a quarter, and halfway
		// out there is almost nothing left
		assert!(exp_falloff(0.25) < 0.25, "{}", exp_falloff(0.25));
		assert!(exp_falloff(0.5) < 0.1, "{}", exp_falloff(0.5));
		let mut prev = f32::INFINITY;
		for i in 0..=20 {
			let v = exp_falloff(i as f32 / 20.0);
			assert!(v < prev, "not falling at {i}");
			prev = v;
		}
	}

	// Test ID: EpHXO9g
	#[test]
	fn nothing_drawing_costs_no_memory() {
		let uhd = (3840, 2160);
		assert_eq!(alloc_size(true, uhd), uhd);
		assert!(bytes(alloc_size(true, uhd)) > 200 << 20, "the real cost");
		assert!(
			bytes(alloc_size(false, uhd)) < 1 << 10,
			"switched off it should cost nothing"
		);
	}
}
