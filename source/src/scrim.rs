// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! Text readability scrim: a background-colored halo behind glyphs so text stays
//! legible over a light/busy background image or a near-transparent terminal. The
//! scene's text is rendered to a coverage texture, turned into a halo by one of
//! four functions, and composited UNDER the crisp text, colored per-pixel by a
//! `bgcolor` map so a glyph's halo takes ITS cell's bg color (a glyph on a
//! one-off colored cell isn't smeared with the global bg color).
//!
//! Two passes build the halo in `halo_a` (the text layer stays crisp for the border). Gaussian
//! (legacy, corners recede) is a separable sum-blur; the distance functions (dilate
//! / sdf / dt) are a separable, bounded Euclidean/Chebyshev distance transform so
//! corners stay full - pass a = per-column 1D distance, pass b = row combine. The
//! composite maps the blurred coverage OR the distance (through a falloff curve) to
//! the per-pixel bg color, plus a thin dilated outline of the crisp coverage.
//!
//! `text` <- crisp TEXT coverage; `cursor` <- crisp CURSOR coverage (kept apart so
//! the cursor can join the halo and the outline independently, each by its own
//! flag; the first pass folds the cursor in when `cursor_scrim`, the composite samples
//! both to add the border when `cursor_outline`).

use crate::config::Choice;
use crate::gfx::{RectInstance, RectRenderer};

/// Each layer stores only what is read back from it. Only alpha of the text
/// coverage is read, but glyphon writes the glyph's color too, so the text keeps
/// four 8-bit channels, the precision the glyph atlas has anyway. The cursor
/// quads are ours and draw white, so their coverage ends up in one red channel. The
/// blur layers hold blurred coverage or a distance in px, which bands in 8 bits
/// (see the reducing resources design doc). The color map holds opaque cell
/// colors that came from sRGB bytes, so it stores them sRGB encoded and gives
/// them back exactly. The encode is done here rather than by an sRGB format,
/// because the GL path never turns sRGB writes on: an sRGB target there stores
/// the linear value as is and still decodes it on read.
pub const TEXT_FMT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const CURSOR_FMT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;
const HALO_FMT: wgpu::TextureFormat = wgpu::TextureFormat::R16Float;
const BG_FMT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

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
	// 1 = the source is the text coverage (read .a), 0 = a blur layer (read .r)
	source: f32,
	_pad: [f32; 2],
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
	halo: f32,
	// 1 = redraw each alpha off `curve` (`visibility::HaloMatch`). 0 leaves it as
	// asked, which is all dark mode gets.
	matched: f32,
	// puts `curve` on the 16-byte boundary a WGSL uniform array sits on
	_pad: f32,
	// the HALO_NODES points
	curve: [f32; 12],
}

// The view keeps its texture alive.
struct Layer {
	view: wgpu::TextureView,
}

impl Layer {
	fn new(
		device: &wgpu::Device,
		label: &str,
		format: wgpu::TextureFormat,
		(w, h): (u32, u32),
	) -> Self {
		let tex = device.create_texture(&wgpu::TextureDescriptor {
			label: Some(label),
			size: wgpu::Extent3d {
				width: w.max(1),
				height: h.max(1),
				depth_or_array_layers: 1,
			},
			mip_level_count: 1,
			sample_count: 1,
			dimension: wgpu::TextureDimension::D2,
			format,
			usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
			view_formats: &[],
		});
		Self {
			view: tex.create_view(&Default::default()),
		}
	}
}

// The layers sized together: the crisp ones by whether anything draws, the two
// blur layers by whether the halo does.
struct Layers {
	text: Layer,   // crisp text coverage (kept for the border pass)
	halo_a: Layer, // the finished halo
	halo_b: Layer, // the first pass's output
	// crisp cursor coverage, separate from the text so cursor_scrim (halo) and
	// cursor_outline (border) are independent toggles - folded into the halo by
	// the blur and into the border by the composite, each gated by its own flag.
	cursor: Layer,
	// per-pixel scrim color: cleared to the global bg, with per-cell bg rects drawn
	// over it, so a glyph's halo takes ITS cell's bg color (not always the global).
	bgcolor: Layer,
}

impl Layers {
	fn new(device: &wgpu::Device, crisp: (u32, u32), halo: (u32, u32)) -> Self {
		Self {
			text: Layer::new(device, "scrim text", TEXT_FMT, crisp),
			halo_a: Layer::new(device, "scrim halo a", HALO_FMT, halo),
			halo_b: Layer::new(device, "scrim halo b", HALO_FMT, halo),
			cursor: Layer::new(device, "scrim cursor", CURSOR_FMT, crisp),
			bgcolor: Layer::new(device, "scrim bgcolor", BG_FMT, crisp),
		}
	}
}

pub struct Scrim {
	layers: Layers,
	sampler: wgpu::Sampler,
	blur_pipe: wgpu::RenderPipeline,
	// distance-field paths (dilate/sdf/dt) reuse the blur bind groups + textures:
	// pass a = per-column 1D distance (text->halo_b), pass b = row combine into the
	// final distance (halo_b->halo_a), metric per the selected function.
	dist_a_pipe: wgpu::RenderPipeline,
	dist_b_pipe: wgpu::RenderPipeline,
	blur_bgl: wgpu::BindGroupLayout,
	// one uniform PER direction: all queue.write_buffer calls are applied before
	// the command buffer runs, so a single shared buffer would give BOTH passes
	// the last-written dir (-> vertical blur twice, no horizontal). Two buffers fix it.
	blur_u_h: wgpu::Buffer,
	blur_u_v: wgpu::Buffer,
	blur_t2b: wgpu::BindGroup, // sample text (uses blur_u_h), write halo_b
	blur_b2a: wgpu::BindGroup, // sample halo_b (uses blur_u_v), write halo_a
	comp_pipe: wgpu::RenderPipeline,
	comp_bgl: wgpu::BindGroupLayout,
	comp_u: wgpu::Buffer,
	comp_bind: wgpu::BindGroup, // sample halo_a + bgcolor (rgb) + text (border)
	bg_rects: RectRenderer,
	// cursor quads drawn into the cursor layer. Separate renderer: bg_rects'
	// instance buffer is uploaded for the bgcolor map in the SAME encoder, and a
	// second upload would clobber the first (queue writes all arrive before the
	// command buffer runs - same rule as the blur uniforms above).
	cursor_rects: RectRenderer,
	cursor_count: u32,
	white_cursors: Vec<RectInstance>,
	encoded_cells: Vec<RectInstance>,
	// what is allocated, and what the surface actually is. With the scrim and the
	// outline both off nothing here draws, so the layers are allocated at one
	// pixel instead - and it falls hardest on the machines the Low and Standard
	// profiles exist for. With the halo off, the two blur layers are too.
	w: u32,
	h: u32,
	halo_size: (u32, u32),
	surf_w: u32,
	surf_h: u32,
	use_: Use,
}

impl std::fmt::Debug for Scrim {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("Scrim").finish_non_exhaustive()
	}
}

/// What draws this frame, which decides what is worth allocating.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Use {
	Nothing,
	OutlineOnly,
	Halo,
}

impl Use {
	pub fn of(halo: bool, outline: bool) -> Self {
		match (halo, outline) {
			(true, _) => Self::Halo,
			(false, true) => Self::OutlineOnly,
			(false, false) => Self::Nothing,
		}
	}
}

/// The halo's falloff curve, `text.scrim.ramp`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ramp {
	Exp,
	HalfNormal,
	Log,
	Sigmoid,
	Linear,
}

impl Choice for Ramp {
	const ALL: &'static [Self] = &[
		Self::Exp,
		Self::HalfNormal,
		Self::Log,
		Self::Sigmoid,
		Self::Linear,
	];

	fn key(self) -> &'static str {
		match self {
			Self::Exp => "exp",
			Self::HalfNormal => "half_normal",
			Self::Log => "log",
			Self::Sigmoid => "sigmoid",
			Self::Linear => "linear",
		}
	}

	// the older spellings still parse: "s" was renamed to "sigmoid" (which is
	// what a smoothstep is), and the falloff's "gaussian" to "half_normal" so
	// it stops reading like the gaussian BLUR the function list also offers.
	fn parse(text: &str) -> Option<Self> {
		let text = text.trim();
		if text.eq_ignore_ascii_case("s") {
			Some(Self::Sigmoid)
		} else if text.eq_ignore_ascii_case("gaussian") {
			Some(Self::HalfNormal)
		} else {
			Self::ALL
				.iter()
				.copied()
				.find(|ramp| ramp.key().eq_ignore_ascii_case(text))
		}
	}
}

impl Ramp {
	// the number `falloff` in the WGSL below tests
	fn code(self) -> f32 {
		match self {
			Self::Sigmoid => 0.0,
			Self::HalfNormal => 1.0,
			Self::Linear => 2.0,
			Self::Log => 3.0,
			Self::Exp => 4.0,
		}
	}
}

/// How the halo is built from the glyphs, `text.scrim.function`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Function {
	Sdf,
	Dt,
	Dilate,
	Gaussian,
}

impl Choice for Function {
	const ALL: &'static [Self] = &[Self::Sdf, Self::Dt, Self::Dilate, Self::Gaussian];

	fn key(self) -> &'static str {
		match self {
			Self::Sdf => "sdf",
			Self::Dt => "dt",
			Self::Dilate => "dilate",
			Self::Gaussian => "gaussian",
		}
	}
}

impl Function {
	// the number `fs_comp` in the WGSL below tests
	fn code(self) -> f32 {
		match self {
			Self::Dilate => 0.0,
			Self::Sdf => 1.0,
			Self::Dt => 2.0,
			Self::Gaussian => 3.0,
		}
	}
}

/// The widest halo the distance passes can measure. They tap at most `DIST_MAX`
/// pixels, and the composite divides by the extent it is given - so an extent past
/// this made every pixel of every pane come out at full halo, a flat plate of
/// background color. Both halves read the same number now.
pub const EXT_MAX: f32 = 40.0;

pub fn clamp_ext(ext: f32) -> f32 {
	ext.clamp(0.0, EXT_MAX)
}

// How big the crisp layers and the blur layers should be. One pixel for what
// does not draw: at full screen each layer is many megabytes of VRAM.
fn alloc_size(what: Use, surface: (u32, u32)) -> ((u32, u32), (u32, u32)) {
	match what {
		Use::Nothing => ((1, 1), (1, 1)),
		Use::OutlineOnly => (surface, (1, 1)),
		Use::Halo => (surface, surface),
	}
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
		let blur_pipe = pipeline(
			device,
			&shader,
			"fs_blur",
			HALO_FMT,
			&blur_bgl,
			"scrim blur",
		);
		let dist_a_pipe = pipeline(
			device,
			&shader,
			"fs_dist_a",
			HALO_FMT,
			&blur_bgl,
			"scrim dist a",
		);
		let dist_b_pipe = pipeline(
			device,
			&shader,
			"fs_dist_b",
			HALO_FMT,
			&blur_bgl,
			"scrim dist b",
		);

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

		// nothing is drawn until something asks for it (see `set_use`)
		let layers = Layers::new(device, (1, 1), (1, 1));
		let bg_rects = RectRenderer::new(device, BG_FMT);
		let cursor_rects = RectRenderer::new(device, CURSOR_FMT);
		let (blur_t2b, blur_b2a, comp_bind) = binds(
			device, &blur_bgl, &comp_bgl, &blur_u_h, &blur_u_v, &comp_u, &sampler, &layers,
		);

		Self {
			layers,
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
			bg_rects,
			cursor_rects,
			cursor_count: 0,
			white_cursors: Vec::new(),
			encoded_cells: Vec::new(),
			w: 1,
			h: 1,
			halo_size: (1, 1),
			surf_w: w,
			surf_h: h,
			use_: Use::Nothing,
		}
	}

	/// Answers whether anything was reallocated, which is the caller's cue that
	/// this frame's prepared set is stale.
	pub fn set_use(&mut self, device: &wgpu::Device, what: Use) -> bool {
		if what == self.use_ {
			return false;
		}
		self.use_ = what;
		self.reallocate(device)
	}

	fn reallocate(&mut self, device: &wgpu::Device) -> bool {
		let (crisp, halo) = alloc_size(self.use_, (self.surf_w, self.surf_h));
		if crisp.0 == 0 || crisp.1 == 0 || (crisp == (self.w, self.h) && halo == self.halo_size) {
			return false;
		}
		self.rebuild(device, crisp, halo);
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

	fn rebuild(&mut self, device: &wgpu::Device, crisp: (u32, u32), halo: (u32, u32)) {
		self.layers = Layers::new(device, crisp, halo);
		let (blur_t2b, blur_b2a, comp_bind) = binds(
			device,
			&self.blur_bgl,
			&self.comp_bgl,
			&self.blur_u_h,
			&self.blur_u_v,
			&self.comp_u,
			&self.sampler,
			&self.layers,
		);
		self.blur_t2b = blur_t2b;
		self.blur_b2a = blur_b2a;
		self.comp_bind = comp_bind;
		(self.w, self.h) = crisp;
		self.halo_size = halo;
	}

	/// Build the per-pixel scrim-color map: clear to the global bg color, then draw
	/// the per-cell bg rects (opaque) over it. A glyph's halo then takes its own
	/// cell's bg color instead of always the global one. The alpha channel doubles
	/// as an "own-bg" mask - cleared to 0, the opaque cell rects write 1, so the blur
	/// can drop coverage from cells that already carry a solid bg (reverse video,
	/// colored bg, selection): they have full contrast, so a halo there is only
	/// artifact (nano's reverse header cast a jumping drop-shadow). See `fs_blur`.
	pub fn render_bgcolor(
		&mut self,
		device: &wgpu::Device,
		queue: &wgpu::Queue,
		encoder: &mut wgpu::CommandEncoder,
		cells: &[RectInstance],
		global_bg: [f32; 4],
	) {
		// Cell rects are opaque (alpha 1), so writing the encoded color is exact.
		let encode = |c: [f32; 4]| {
			let e = crate::config::from_linear;
			[e(c[0]), e(c[1]), e(c[2]), c[3]]
		};
		self.encoded_cells.clear();
		self.encoded_cells
			.extend(cells.iter().map(|cell| RectInstance {
				color: encode(cell.color),
				..*cell
			}));
		let clear = encode(global_bg);
		self.bg_rects
			.set_resolution(queue, self.w as f32, self.h as f32);
		self.bg_rects.upload(device, queue, &self.encoded_cells);
		let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
			label: Some("scrim bgcolor"),
			color_attachments: &[Some(wgpu::RenderPassColorAttachment {
				view: &self.layers.bgcolor.view,
				resolve_target: None,
				depth_slice: None,
				ops: wgpu::Operations {
					load: wgpu::LoadOp::Clear(wgpu::Color {
						r: clear[0] as f64,
						g: clear[1] as f64,
						b: clear[2] as f64,
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

	/// The render target for the scene's text. Clear it transparent and render the
	/// prepared text into it before calling `blur`.
	pub fn text_view(&self) -> &wgpu::TextureView {
		&self.layers.text.view
	}

	/// The render target for the cursor coverage, separate from the text. Clear it
	/// transparent and draw the cursor quads (`draw_cursors`) into it before
	/// calling `blur`; the flags in `blur`/`composite` decide where it contributes.
	pub fn cursor_view(&self) -> &wgpu::TextureView {
		&self.layers.cursor.view
	}

	/// Upload the cursor quads destined for the cursor layer. Call before the
	/// cursor pass; draw with `draw_cursors`.
	pub fn upload_cursors(
		&mut self,
		device: &wgpu::Device,
		queue: &wgpu::Queue,
		quads: &[RectInstance],
	) {
		// The layer is one red channel, and a quad writes its color times its
		// coverage. White makes that the coverage alone, fades included.
		self.white_cursors.clear();
		self.white_cursors
			.extend(quads.iter().map(|quad| RectInstance {
				color: [1.0, 1.0, 1.0, quad.color[3]],
				..*quad
			}));
		self.cursor_rects
			.set_resolution(queue, self.w as f32, self.h as f32);
		self.cursor_rects.upload(device, queue, &self.white_cursors);
		self.cursor_count = quads.len() as u32;
	}

	/// Draw the uploaded cursor quads into the current (cursor layer) pass.
	pub fn draw_cursors(&self, pass: &mut wgpu::RenderPass<'_>) {
		if self.cursor_count > 0 {
			self.cursor_rects.draw(pass, 0..self.cursor_count);
		}
	}

	/// Two separable passes producing the scrim in `halo_a`; the text layer keeps
	/// the crisp coverage for the border pass. `function` picks the path: gaussian
	/// runs the legacy sum-blur (H text->halo_b, V halo_b->halo_a) shaped by
	/// `ramp`; the distance paths (dilate / sdf / dt) run a separable
	/// Euclidean/Chebyshev distance transform (pass a = per-column 1D distance,
	/// pass b = row combine) into `halo_a`, bounded to `radius`. `sigma` = gaussian
	/// blur sigma; `radius` = distance extent. `cursor` (0/1) folds the cursor
	/// coverage in - only in the first pass (the second reads `halo_b`, which
	/// already carries it, so its flag stays 0).
	pub fn blur(
		&self,
		queue: &wgpu::Queue,
		encoder: &mut wgpu::CommandEncoder,
		sigma: f32,
		radius: f32,
		ramp: Ramp,
		cursor: f32,
		function: Function,
	) {
		let res = [self.halo_size.0 as f32, self.halo_size.1 as f32];
		// dilate is chebyshev, the other distance paths euclidean
		let (gaussian, metric) = match function {
			Function::Gaussian => (true, 0.0),
			Function::Dilate => (false, 1.0),
			Function::Sdf | Function::Dt => (false, 0.0),
		};
		let ramp = ramp.code();
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
				source: 1.0,
				_pad: [0.0; 2],
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
				source: 0.0,
				_pad: [0.0; 2],
			}),
		);
		let (pipe_a, pipe_b) = if gaussian {
			(&self.blur_pipe, &self.blur_pipe)
		} else {
			(&self.dist_a_pipe, &self.dist_b_pipe)
		};
		for (pipe, src_bind, dst) in [
			(pipe_a, &self.blur_t2b, &self.layers.halo_b.view),
			(pipe_b, &self.blur_b2a, &self.layers.halo_a.view),
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

	/// Upload the composite uniform. Split from `composite()`: the draw runs once
	/// per pane (scissored), but the args are frame-invariant, so the render loop
	/// writes this once instead of staging an identical write per pane.
	pub fn write_comp_uniform(
		&self,
		queue: &wgpu::Queue,
		intensity: f32,
		border_px: f32,
		cursor: f32,
		function: Function,
		ramp: Ramp,
		radius: f32,
		strength: f32,
		halo: f32,
		matched: Option<crate::visibility::HaloMatch>,
	) {
		let mut curve = [0.0f32; 12];
		if let Some(m) = matched {
			curve[..m.curve.len()].copy_from_slice(&m.curve);
		}
		queue.write_buffer(
			&self.comp_u,
			0,
			bytemuck::bytes_of(&CompU {
				resolution: [self.w as f32, self.h as f32],
				intensity,
				border_px,
				cursor,
				function: function.code(),
				ramp: ramp.code(),
				radius,
				strength,
				halo,
				matched: if matched.is_some() { 1.0 } else { 0.0 },
				_pad: 0.0,
				curve,
			}),
		);
	}

	/// Draw the scrim into the current pass, under the text: the halo from `halo_a`,
	/// colored per-pixel by the bgcolor map, plus a `border_px` dilated outline of
	/// the crisp coverage (text, + cursor when `cursor` is 1).
	/// `write_comp_uniform` must have run this frame.
	pub fn composite(&self, pass: &mut wgpu::RenderPass<'_>) {
		pass.set_pipeline(&self.comp_pipe);
		pass.set_bind_group(0, &self.comp_bind, &[]);
		pass.draw(0..3, 0..1);
	}
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
	layers: &Layers,
) -> (wgpu::BindGroup, wgpu::BindGroup, wgpu::BindGroup) {
	let bgcolor_view = &layers.bgcolor.view;
	let view_cur = &layers.cursor.view;
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
				resource: wgpu::BindingResource::TextureView(&layers.halo_a.view),
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
				resource: wgpu::BindingResource::TextureView(&layers.text.view),
			},
			wgpu::BindGroupEntry {
				binding: 5,
				resource: wgpu::BindingResource::TextureView(view_cur),
			},
		],
	});
	// t2b: H pass samples the text (horizontal uniform); b2a: V pass samples halo_b.
	(
		mk_blur(blur_u_h, &layers.text.view),
		mk_blur(blur_u_v, &layers.halo_b.view),
		comp,
	)
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
struct BlurU { resolution: vec2<f32>, dir: vec2<f32>, sigma: f32, ramp: f32, cursor: f32, radius: f32, metric: f32, source: f32, _p1: f32, _p2: f32 };
@group(0) @binding(0) var<uniform> bu: BlurU;
@group(0) @binding(1) var tex: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;
@group(0) @binding(3) var bmask: texture_2d<f32>; // bgcolor map; .a = own-bg mask
@group(0) @binding(4) var bcur: texture_2d<f32>;  // crisp cursor coverage (.r)

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
    var sum = 0.0;
    var wsum = 0.0;
    for (var i = -12; i <= 12; i = i + 1) {
        let off = f32(i) * spacing;
        let w = falloff(abs(off) / ext, bu.ramp);
        let uv = in.uv + bu.dir * (off * texel);
        let keep = 1.0 - textureSample(bmask, samp, uv).a;
        // the H pass reads the text's alpha, the V pass the blur layer's red
        let src = textureSample(tex, samp, uv);
        // fold the cursor coverage into the H pass (bu.cursor is 0 in the V pass)
        let cov = mix(src.r, src.a, bu.source) + bu.cursor * textureSample(bcur, samp, uv).r;
        sum += cov * (w * keep);
        wsum += w;
    }
    return vec4<f32>(sum / wsum, 0.0, 0.0, 1.0);
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
        let cov = textureSample(tex, samp, uv).a + bu.cursor * textureSample(bcur, samp, uv).r;
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

struct CompU { resolution: vec2<f32>, intensity: f32, border_px: f32, cursor: f32, function: f32, ramp: f32, radius: f32, strength: f32, halo: f32, matched: f32, _pad: f32, curve: array<vec4<f32>, 3> };
@group(0) @binding(0) var<uniform> cu: CompU;
@group(0) @binding(1) var gtex: texture_2d<f32>;   // scrim: blurred coverage or distance (.r)
@group(0) @binding(2) var gsamp: sampler;
@group(0) @binding(3) var bgtex: texture_2d<f32>;  // per-pixel scrim color, sRGB encoded
@group(0) @binding(4) var ttex: texture_2d<f32>;   // crisp glyph coverage
@group(0) @binding(5) var ccur: texture_2d<f32>;   // crisp cursor coverage (.r)

fn srgb_decode(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3<f32>(2.4)), c / 12.92, c <= vec3<f32>(0.04045));
}

// Light mode's halo alpha, read off the twelve points visibility::halo_match
// solved for this picture, joined by straight lines. They are spaced by how far
// dark mode's halo moves a picture rather than by alpha, which is where the
// curve bends. The Rust side reads them the same way in HaloMatch::alpha.
fn curve_at(i: u32) -> f32 {
    return cu.curve[i / 4u][i % 4u];
}
fn matched_alpha(a: f32) -> f32 {
    let x = (1.0 - pow(1.0 - clamp(a, 0.0, 1.0), 1.0 / 2.4)) * 11.0;
    let i = min(u32(x), 10u);
    return mix(curve_at(i), curve_at(i + 1u), x - f32(i));
}

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
        cov = max(cov, textureSample(ccur, gsamp, uv).r);
    }
    return cov * (1.0 - textureSample(bgtex, gsamp, uv).a);
}
@fragment
fn fs_comp(in: VsOut) -> @location(0) vec4<f32> {
    var ga = 0.0;
    if (cu.function >= 2.5) {
        // gaussian [ugly]: blurred coverage alpha, boosted by intensity
        ga = clamp(textureSample(gtex, gsamp, in.uv).r * cu.intensity, 0.0, 1.0);
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
    let rgb = srgb_decode(textureSample(bgtex, gsamp, in.uv).rgb);
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
    var a = max(ga, border);
    if (cu.matched > 0.5) {
        a = matched_alpha(a);
    }
    return vec4<f32>(rgb * a, a);
}
";

#[cfg(test)]
mod tests {
	use super::{
		BG_FMT, CURSOR_FMT, EXT_MAX, Function, HALO_FMT, Ramp, TEXT_FMT, Use, WGSL, alloc_size,
		clamp_ext,
	};
	use crate::config::Choice;

	// The shader tells the curves apart by `ramp < N`, and each branch names its
	// curve in a comment. Every curve's number has to land in its own branch.
	// Test ID: Erstarj
	#[test]
	fn each_ramp_code_reaches_the_shader_branch_for_that_curve() {
		let body = WGSL
			.split("fn falloff(")
			.nth(1)
			.and_then(|rest| rest.split("\n}\n").next())
			.expect("the falloff function");
		let named = |note: &str| {
			note.split_whitespace()
				.next()
				.unwrap_or("")
				.trim_end_matches(':')
				.to_string()
		};
		let mut ladder: Vec<(f32, String)> = Vec::new();
		for line in body.lines() {
			let Some((code, note)) = line.split_once("//") else {
				continue;
			};
			if let Some(rest) = code.split("ramp < ").nth(1) {
				let bound = rest
					.trim_end_matches(|c: char| c == ')' || c == '{' || c.is_whitespace())
					.parse::<f32>()
					.expect("a number");
				ladder.push((bound, named(note)));
			} else if code.contains("} else {") {
				ladder.push((f32::INFINITY, named(note)));
			}
		}
		assert_eq!(ladder.len(), Ramp::ALL.len(), "{ladder:?}");
		for &ramp in Ramp::ALL {
			let want = match ramp {
				Ramp::Exp => "exponential",
				Ramp::HalfNormal => "half-normal",
				Ramp::Log => "logarithmic",
				Ramp::Sigmoid => "sigmoid",
				Ramp::Linear => "linear",
			};
			let (_, got) = ladder
				.iter()
				.find(|(bound, _)| ramp.code() < *bound)
				.expect("a branch");
			assert_eq!(got, want, "{ramp:?}");
		}
	}

	// The composite takes the legacy blur at `function >= 2.5` and hardens dt's
	// glow at `function >= 1.5`, in that order.
	// Test ID: Erstaxm
	#[test]
	fn each_function_code_takes_its_own_path_in_the_composite() {
		let gates: Vec<f32> = WGSL
			.split("cu.function >= ")
			.skip(1)
			.map(|rest| {
				rest.split(')')
					.next()
					.and_then(|n| n.trim().parse().ok())
					.expect("a number")
			})
			.collect();
		let [blur, harden] = gates[..] else {
			panic!("two gates, got {gates:?}");
		};
		for &function in Function::ALL {
			let code = function.code();
			assert_eq!(code >= blur, function == Function::Gaussian, "{function:?}");
			if function != Function::Gaussian {
				assert_eq!(code >= harden, function == Function::Dt, "{function:?}");
			}
		}
		// the shader cannot tell two paths with one number apart
		let codes: Vec<f32> = Function::ALL.iter().map(|f| f.code()).collect();
		assert!(
			codes
				.iter()
				.all(|c| codes.iter().filter(|o| *o == c).count() == 1)
		);
	}

	// The exponential arm's exponent, mirrored so the curve can be checked
	// without a GPU. The shader owns the number and the test below holds the
	// two against each other.
	const EXP_K: f32 = 6.0;

	fn exp_falloff(t: f32) -> f32 {
		let t = t.clamp(0.0, 1.0);
		let e = (-EXP_K).exp();
		(((-EXP_K * t).exp()) - e) / (1.0 - e)
	}

	fn texel(format: wgpu::TextureFormat) -> u64 {
		u64::from(format.block_copy_size(None).expect("a plain color format"))
	}

	fn pixels((w, h): (u32, u32)) -> u64 {
		u64::from(w) * u64::from(h)
	}

	// Bytes the set costs, read off the formats themselves.
	fn bytes(what: Use, surface: (u32, u32)) -> u64 {
		let (crisp, halo) = alloc_size(what, surface);
		pixels(crisp) * (texel(TEXT_FMT) + texel(CURSOR_FMT) + texel(BG_FMT))
			+ pixels(halo) * 2 * texel(HALO_FMT)
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

	// A shader edit that does not compile only shows up at pipeline creation, in
	// a window. This catches it here, with no GPU.
	// Test ID: Ers4srl
	#[test]
	fn the_scrim_shader_compiles() {
		use wgpu::naga;
		let module = naga::front::wgsl::parse_str(WGSL).expect("the scrim shader parses");
		naga::valid::Validator::new(
			naga::valid::ValidationFlags::all(),
			naga::valid::Capabilities::empty(),
		)
		.validate(&module)
		.expect("the scrim shader validates");
	}

	// Dark mode is the reference and must draw what it always has, so the light
	// mode match runs only behind its own flag, and the flag is only set when
	// visibility.rs hands over a match.
	// Test ID: Ers4tCw
	#[test]
	fn only_a_matched_halo_is_redrawn() {
		let comp = WGSL
			.split("fn fs_comp")
			.nth(1)
			.expect("the composite shader");
		let guard = comp.find("if (cu.matched > 0.5)").expect("no guard");
		let call = comp.find("matched_alpha(").expect("no call");
		assert!(guard < call, "the match runs unguarded");
		assert_eq!(comp.matches("matched_alpha(").count(), 1);
		// and the Rust side of the uniform still lines up with the WGSL one
		assert_eq!(std::mem::size_of::<super::CompU>(), 96);
		assert_eq!(std::mem::offset_of!(super::CompU, curve), 48);
		assert_eq!(crate::visibility::HALO_NODES, 12);
		assert!(
			WGSL.contains(&format!("* {}.0;", crate::visibility::HALO_NODES - 1)),
			"the shader's span count and HALO_NODES have drifted apart"
		);
	}

	// Test ID: EpHXO9g
	#[test]
	fn nothing_drawing_costs_no_memory() {
		let uhd = (3840, 2160);
		assert_eq!(alloc_size(Use::Halo, uhd), (uhd, uhd));
		// was: assert!(bytes(..) > 200 << 20, "the real cost") - the layers were
		// made smaller (2026100418225502), so the full set at 4K is about 100 MiB
		assert!(bytes(Use::Halo, uhd) > 100 << 20, "the real cost");
		assert!(
			bytes(Use::Nothing, uhd) < 1 << 10,
			"switched off it should cost nothing"
		);
		// the outline alone never runs the blur, so its two layers stay a pixel
		assert_eq!(alloc_size(Use::OutlineOnly, uhd), (uhd, (1, 1)));
	}

	// Five Rgba16Float layers were 40 bytes a pixel, 150 MiB of a 268 MiB window
	// at 2560x1440. Each layer keeps only the channels and precision read back.
	// Test ID: ErnU09H
	#[test]
	fn the_scrim_costs_at_most_13_bytes_a_pixel() {
		let qhd = (2560, 1440);
		let per_pixel = |what| bytes(what, qhd) as f64 / pixels(qhd) as f64;
		assert!(
			per_pixel(Use::Halo) <= 13.0,
			"{} bytes a pixel",
			per_pixel(Use::Halo)
		);
		assert!(
			per_pixel(Use::OutlineOnly) <= 9.01,
			"{} bytes a pixel with only the outline",
			per_pixel(Use::OutlineOnly)
		);
	}

	// The color map is drawn from cell colors that started as sRGB bytes, and
	// it stores them encoded in 8 bits, so it hands back every one of them
	// exactly. Stored linear in 8 bits, the dark end would collapse.
	// Test ID: ErnU0UJ
	#[test]
	fn the_color_map_keeps_every_byte_color_exactly() {
		assert_eq!(texel(BG_FMT), 4);
		assert!(
			!BG_FMT.is_srgb(),
			"the GL path writes an sRGB target unencoded"
		);
		for byte in 0..=255u8 {
			let stored =
				(crate::config::from_linear(crate::config::to_linear(byte)) * 255.0).round();
			assert_eq!(stored as u8, byte);
		}
		assert!(
			WGSL.contains("srgb_decode(textureSample(bgtex"),
			"the composite reads it back encoded"
		);
	}
}
