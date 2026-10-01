// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

use std::num::NonZeroU32;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use glutin::config::GlConfig;
use glutin::context::{ContextApi, ContextAttributesBuilder, PossiblyCurrentContext, Version};
use glutin::display::GetGlDisplay;
use glutin::prelude::*;
use glutin::surface::{Surface as GlWindowSurface, SurfaceAttributesBuilder, WindowSurface};
use glutin_winit::DisplayBuilder;
use raw_window_handle::HasWindowHandle;
#[cfg(not(target_os = "macos"))]
use wgpu::hal::api::Gles;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowAttributes};

// COLOR PIPELINE CONTRACT (breaking it reproduces the "everything too dark /
// SELECTION_BG invisible" bug class): every fragment shader in this app writes
// LINEAR light (rect srgb_f32, glyphon Accurate, bg-image, scrim), and exactly
// ONE sRGB encode happens per frame, owned by this module - on the native path
// the sRGB surface format encodes on write; on the GL path the blit's lin2srgb
// does it into the non-sRGB fbo 0, so the offscreen MUST stay a non-sRGB,
// high-precision format (Rgba16Float; an sRGB view would decode in the blit's
// sample and cancel the encode, an 8-bit linear one bands dark gradients).
// New render features must not add their own encode.
//
// That one encode runs on premultiplied values, which is exact at alpha 1 and
// wrong anywhere else: sRGB(a * c) is brighter than a * sRGB(c), and the
// compositor blends what it gets as if it were the second. A light background
// at 80% came out as 91% of itself, so it covered most of what the desktop
// behind it should have shown, while black stayed black. The only translucent
// thing drawn is the pane fill, so rather than move the encode, the fill is
// given the color whose encoded, premultiplied value is right.
pub fn see_through(color: [f32; 4]) -> [f32; 4] {
	let a = color[3];
	if a >= 1.0 || a <= 0.0 {
		return color;
	}
	let k = |c: f32| crate::config::to_linear_f32(a * crate::config::from_linear(c)) / a;
	[k(color[0]), k(color[1]), k(color[2]), a]
}

// How a frame reaches the screen. `Native` is the normal wgpu surface (Vulkan/
// Metal/DX/Wayland - supports premultiplied alpha where the platform does).
// `Gl` runs wgpu on a glutin-created GL context so X11 can do per-pixel alpha:
// the wgpu surface there can't bind the window's ARGB visual, glutin can. We
// render to the GL default framebuffer (fbo 0) and present via swap_buffers.
enum Backend {
	Native(wgpu::Surface<'static>),
	// The GL default framebuffer (fbo 0) is Y-flipped vs wgpu's top-left origin,
	// which flips our quads and clips glyphon's bounds-limited text out entirely.
	// So the scene renders to `offscreen` (normal orientation, exactly like the
	// native path), then `blit` flips it into the default framebuffer `fb`.
	Gl {
		ctx: PossiblyCurrentContext,
		surface: GlWindowSurface<WindowSurface>,
		// the framebuffer config the window was made with, which a later context
		// on the same window has to match (see `Rebirth`)
		config: glutin::config::Config,
		fb: wgpu::Texture,
		// views of fb/offscreen, rebuilt on resize only (both textures are
		// persistent, so creating fresh views per frame was waste)
		fb_view: wgpu::TextureView,
		offscreen: wgpu::Texture,
		offscreen_view: wgpu::TextureView,
		blit: Blit,
	},
}

// Fullscreen flip-blit of the offscreen texture into the GL default framebuffer.
struct Blit {
	pipeline: wgpu::RenderPipeline,
	sampler: wgpu::Sampler,
	layout: wgpu::BindGroupLayout,
	bind: wgpu::BindGroup,
}

impl Blit {
	fn new(device: &wgpu::Device, format: wgpu::TextureFormat, src: &wgpu::TextureView) -> Self {
		let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
			label: Some("blit shader"),
			source: wgpu::ShaderSource::Wgsl(BLIT_WGSL.into()),
		});
		let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("blit bgl"),
			entries: &[
				wgpu::BindGroupLayoutEntry {
					binding: 0,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
						view_dimension: wgpu::TextureViewDimension::D2,
						multisampled: false,
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 1,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
					count: None,
				},
			],
		});
		let sampler = device.create_sampler(&wgpu::SamplerDescriptor::default());
		let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
			label: Some("blit layout"),
			bind_group_layouts: &[Some(&layout)],
			immediate_size: 0,
		});
		let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
			label: Some("blit pipeline"),
			layout: Some(&pl),
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
					blend: None, // straight copy; offscreen already holds premultiplied rgba
					write_mask: wgpu::ColorWrites::ALL,
				})],
			}),
			primitive: wgpu::PrimitiveState::default(),
			depth_stencil: None,
			multisample: wgpu::MultisampleState::default(),
			multiview_mask: None,
			cache: None,
		});
		let bind = Self::bind(device, &layout, &sampler, src);
		Self {
			pipeline,
			sampler,
			layout,
			bind,
		}
	}

	fn bind(
		device: &wgpu::Device,
		layout: &wgpu::BindGroupLayout,
		sampler: &wgpu::Sampler,
		src: &wgpu::TextureView,
	) -> wgpu::BindGroup {
		device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("blit bind"),
			layout,
			entries: &[
				wgpu::BindGroupEntry {
					binding: 0,
					resource: wgpu::BindingResource::TextureView(src),
				},
				wgpu::BindGroupEntry {
					binding: 1,
					resource: wgpu::BindingResource::Sampler(sampler),
				},
			],
		})
	}

	fn rebind(&mut self, device: &wgpu::Device, src: &wgpu::TextureView) {
		self.bind = Self::bind(device, &self.layout, &self.sampler, src);
	}
}

// Why begin_frame had nothing to draw into. Metal answers Occluded for a window
// that is hidden, minimized or fully covered, and draws nothing until it shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoFrame {
	Occluded,
	Other,
}

// A frame in flight, returned by `begin_frame` and consumed by `end_frame`.
pub enum Frame {
	Native(wgpu::SurfaceTexture),
	Gl,
}

// A VT switch (Ctrl+Alt+F1 and back) or suspend/resume can silently trash the
// CONTENTS of GPU textures on the GL path: the context survives, so per-frame
// procedural draws (rects, cursor) still work, but everything sampled from a
// once-uploaded texture - the glyph atlases (all text) and the wallpaper -
// reads garbage, which is the "window goes mostly black" bug. No event reports
// this, so known-pattern sentinel textures are probed on a slow tick; a
// mismatched readback means the uploads are gone and the app rebuilds them
// (State::recover_gpu). Native-surface backends already get Lost/Outdated from
// the swapchain and are not affected, so the sentinel exists only on GL.
//
// TWO witnesses, because the NVIDIA driver restores what it holds a sysmem
// backing for and purges the rest (NV_robustness_video_memory_purge: resources
// exclusively in video memory "will be lost"; the driver "attempts to hide"
// the purge for the ones it can restore). A round-1 single 64px copy-usage
// sentinel survived a real VT switch that still wiped the atlas, so:
// - `up_tex`: CPU-uploaded + TEXTURE_BINDING, sized like a glyph atlas -
//   catches drivers that purge sampled uploads.
// - `fbo_tex`: seeded only by a GPU-side copy, never from the CPU, so no
//   driver can re-materialize its contents from a sysmem copy - catches the
//   documented purge of vidmem-exclusive (rendered/FBO-class) resources.
//   Seeded by copy, not a render-pass clear: a clear could be tracked as
//   metadata and re-applied on restore, which would false-negative.
const SENTINEL_PX: u32 = 256; // atlas-sized, so it shares the real textures' VRAM pool
const SENTINEL_ROW: u32 = SENTINEL_PX * 4; // Rgba8Unorm; multiple of 256 (COPY_BYTES_PER_ROW_ALIGNMENT), so no pad rows
const SENTINEL_BYTES: usize = (SENTINEL_ROW * SENTINEL_PX) as usize;

// Odd multiplier = a byte permutation tiled over the texture; neither zeroed
// nor noise VRAM plausibly reproduces 16KB of it.
fn sentinel_pattern() -> Vec<u8> {
	(0..SENTINEL_BYTES)
		.map(|i| (i as u8).wrapping_mul(151).wrapping_add(43))
		.collect()
}

// One probe's verdict (see `vram_check_poll`).
pub enum VramProbe {
	Intact,
	// which witness lost its pattern (true = gone)
	Lost { uploaded: bool, rendered: bool },
	// readback map failed - inconclusive, will retry
	MapFailed,
}

struct Sentinel {
	up_tex: wgpu::Texture,
	fbo_tex: wgpu::Texture,
	buf: wgpu::Buffer, // both witnesses read back into one buffer (up at 0, fbo at SENTINEL_BYTES)
	// probe in flight; the map_async callback stores 1 = mapped ok, 2 = failed
	inflight: Option<Arc<AtomicU8>>,
}

impl Sentinel {
	fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
		let mk_tex = |label: &str, usage: wgpu::TextureUsages| {
			device.create_texture(&wgpu::TextureDescriptor {
				label: Some(label),
				size: wgpu::Extent3d {
					width: SENTINEL_PX,
					height: SENTINEL_PX,
					depth_or_array_layers: 1,
				},
				mip_level_count: 1,
				sample_count: 1,
				dimension: wgpu::TextureDimension::D2,
				format: wgpu::TextureFormat::Rgba8Unorm,
				usage,
				view_formats: &[],
			})
		};
		let up_tex = mk_tex(
			"vram sentinel uploaded",
			wgpu::TextureUsages::TEXTURE_BINDING
				| wgpu::TextureUsages::COPY_DST
				| wgpu::TextureUsages::COPY_SRC,
		);
		let fbo_tex = mk_tex(
			"vram sentinel rendered",
			wgpu::TextureUsages::RENDER_ATTACHMENT
				| wgpu::TextureUsages::TEXTURE_BINDING
				| wgpu::TextureUsages::COPY_DST
				| wgpu::TextureUsages::COPY_SRC,
		);
		let buf = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("vram sentinel read"),
			size: (SENTINEL_BYTES * 2) as u64,
			usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
			mapped_at_creation: false,
		});
		let sentinel = Self {
			up_tex,
			fbo_tex,
			buf,
			inflight: None,
		};
		sentinel.seed(device, queue, &sentinel_pattern());
		sentinel
	}

	// Upload `data` into up_tex, then GPU-copy it into fbo_tex (write_texture is
	// ordered before subsequently submitted command buffers, so the copy sees it).
	fn seed(&self, device: &wgpu::Device, queue: &wgpu::Queue, data: &[u8]) {
		queue.write_texture(
			wgpu::TexelCopyTextureInfo {
				texture: &self.up_tex,
				mip_level: 0,
				origin: wgpu::Origin3d::ZERO,
				aspect: wgpu::TextureAspect::All,
			},
			data,
			wgpu::TexelCopyBufferLayout {
				offset: 0,
				bytes_per_row: Some(SENTINEL_ROW),
				rows_per_image: Some(SENTINEL_PX),
			},
			wgpu::Extent3d {
				width: SENTINEL_PX,
				height: SENTINEL_PX,
				depth_or_array_layers: 1,
			},
		);
		let mut enc = device.create_command_encoder(&Default::default());
		enc.copy_texture_to_texture(
			wgpu::TexelCopyTextureInfo {
				texture: &self.up_tex,
				mip_level: 0,
				origin: wgpu::Origin3d::ZERO,
				aspect: wgpu::TextureAspect::All,
			},
			wgpu::TexelCopyTextureInfo {
				texture: &self.fbo_tex,
				mip_level: 0,
				origin: wgpu::Origin3d::ZERO,
				aspect: wgpu::TextureAspect::All,
			},
			wgpu::Extent3d {
				width: SENTINEL_PX,
				height: SENTINEL_PX,
				depth_or_array_layers: 1,
			},
		);
		queue.submit(Some(enc.finish()));
	}
}

pub struct Gfx {
	// Kept across a release (see `Rebirth`): the device and everything on it
	// go, the instance is what they are rebuilt from.
	instance: wgpu::Instance,
	pub device: wgpu::Device,
	pub queue: wgpu::Queue,
	pub config: wgpu::SurfaceConfiguration,
	pub format: wgpu::TextureFormat,
	pub transparent: bool, // surface can show the desktop through (compositor present)
	pub adapter_info: wgpu::AdapterInfo,
	backend: Backend,
	sentinel: Option<Sentinel>, // GL path only: VT-switch texture-content-loss probe
	_window: Arc<Window>,
}

// What is kept of a released `Gfx`, enough to build the device again on the
// same window. The instance is kept rather than made afresh because on the GL
// path its teardown terminates an EGL display that the glutin context may
// share, and because the adapter enumeration it holds is the slow part of a
// cold start on the others.
pub enum Rebirth {
	Native(wgpu::Instance),
	Gl(wgpu::Instance, glutin::config::Config),
}

impl Gfx {
	pub fn new(window: Arc<Window>) -> anyhow::Result<Self> {
		Self::with_backends(window, wgpu::Backends::all())
	}

	// Let the device and everything on it go. Every other wgpu object made on
	// this device must be gone already, or the device outlives this call: the
	// handles are refcounted, and the last one standing is what frees it.
	// Ordered by hand, since the GL objects are deleted through a context that
	// has to be current while it happens and glutin destroys one without
	// unbinding it first.
	pub fn release(self) -> Rebirth {
		let Self {
			instance,
			device,
			queue,
			backend,
			sentinel,
			..
		} = self;
		drop(sentinel);
		match backend {
			Backend::Native(surface) => {
				drop(surface);
				let _ = device.poll(wgpu::PollType::wait_indefinitely());
				drop(queue);
				drop(device);
				Rebirth::Native(instance)
			}
			Backend::Gl {
				ctx,
				surface,
				config,
				fb,
				fb_view,
				offscreen,
				offscreen_view,
				blit,
			} => {
				drop((blit, offscreen_view, offscreen, fb_view, fb));
				let _ = device.poll(wgpu::PollType::wait_indefinitely());
				drop(queue);
				drop(device);
				// Unbound before either is destroyed. GLX only defers destroying a
				// current drawable, so the window kept its old GLX surface, and
				// NVIDIA refused the rebuild a second one (BadDrawable, then
				// GLXBadWindow when the half-made one was dropped).
				let ctx = ctx.make_not_current();
				drop(surface);
				drop(ctx);
				Rebirth::Gl(instance, config)
			}
		}
	}

	// The device again, on the window it was released from. A kept instance that
	// can no longer serve the window (a driver that went away in the meantime)
	// falls back to a cold start.
	pub fn rebuild(rebirth: &Rebirth, window: &Arc<Window>) -> anyhow::Result<Self> {
		match rebirth {
			Rebirth::Native(instance) => Self::on(instance.clone(), window.clone(), false)
				.or_else(|_| Self::new(window.clone())),
			Rebirth::Gl(instance, config) => {
				Self::gl_on(instance.clone(), window.clone(), config.clone(), false)
			}
		}
	}

	// Windows per-pixel transparency. A swapchain made straight from the HWND
	// only ever composites opaque, whatever the window asked for, so the setting
	// used to change nothing there. DX12 can instead present through a
	// DirectComposition visual, which does carry premultiplied alpha - and it is
	// the only backend with that option, so it has to be the one picked. Falls
	// back to the ordinary path (opaque) when DX12 cannot serve this window.
	#[cfg(windows)]
	pub fn new_composited(window: Arc<Window>) -> anyhow::Result<Self> {
		let dx12 = wgpu::Dx12BackendOptions {
			presentation_system: wgpu::Dx12SwapchainKind::DxgiFromVisual,
			..Default::default()
		};
		let options = wgpu::BackendOptions {
			dx12,
			..Default::default()
		};
		Self::build(window.clone(), wgpu::Backends::DX12, options).or_else(|e| {
			eprintln!(
				"{}: composited DX12 surface unavailable ({e}); using native surface (no transparency)",
				crate::config::APP_NAME
			);
			Self::new(window)
		})
	}

	// Native wgpu path with a chosen backend set. Pop-out dialog windows pass
	// `Backends::PRIMARY` (Vulkan/Metal/DX12, NO GL): initializing wgpu's GL
	// backend while the main window holds a glutin GL/EGL context panics in
	// wgpu-hal's EGL teardown (`unmake_current().unwrap()`), so dialogs must avoid
	// touching EGL entirely.
	pub fn with_backends(window: Arc<Window>, backends: wgpu::Backends) -> anyhow::Result<Self> {
		Self::build(window, backends, wgpu::BackendOptions::default())
	}

	fn build(
		window: Arc<Window>,
		backends: wgpu::Backends,
		backend_options: wgpu::BackendOptions,
	) -> anyhow::Result<Self> {
		let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
			backends,
			flags: wgpu::InstanceFlags::default(),
			memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
			backend_options,
			display: None,
		});
		Self::on(instance, window, true)
	}

	// Surface, adapter and device on an instance that already exists: the cold
	// start above and a rebuild after a release (`log` is the difference, since
	// the adapter was reported the first time).
	fn on(instance: wgpu::Instance, window: Arc<Window>, log: bool) -> anyhow::Result<Self> {
		let surface = instance.create_surface(window.clone())?;

		// Prefer a real GPU; if none can be acquired, retry forcing a software
		// (CPU) adapter so the app still runs without hardware acceleration.
		let pick = |fallback| {
			pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
				power_preference: wgpu::PowerPreference::HighPerformance,
				compatible_surface: Some(&surface),
				force_fallback_adapter: fallback,
			}))
		};
		let adapter = pick(false).or_else(|_| pick(true))?;
		let adapter_info = adapter.get_info();

		let (device, queue) = pollster::block_on(request_device(&adapter))?;
		let (config, format, transparent) = surface_config(&surface, &adapter, &window)
			.ok_or_else(|| anyhow::anyhow!("adapter cannot present to this window"))?;
		if log {
			log_renderer(&adapter_info, transparent);
		}
		surface.configure(&device, &config);

		Ok(Self {
			instance,
			device,
			queue,
			config,
			format,
			transparent,
			adapter_info,
			backend: Backend::Native(surface),
			sentinel: None,
			_window: window,
		})
	}

	// Same native path, but on a context that was built ahead of time (see
	// `DialogGpu`). Only the surface is created here, which is sub-millisecond -
	// the instance/adapter/device that dominate `with_backends` are already paid
	// for. `None` means this warm context cannot serve this window, so the caller
	// must fall back to a cold `with_backends`.
	pub fn with_dialog_gpu(window: Arc<Window>, gpu: &DialogGpu) -> Option<Self> {
		// The warm instance was built with no display connection, so it may not be
		// able to make a surface for this window at all. That is the same answer as
		// an adapter that cannot present here: fall back, rather than failing the
		// open and leaving the dialog unreachable for the life of the process.
		let surface = gpu.instance.create_surface(window.clone()).ok()?;
		// The warm adapter was picked with no surface to check against (no window
		// existed yet), so a multi-GPU box could hand back one that can't draw
		// here. `surface_config` reports that as None.
		let (config, format, transparent) = surface_config(&surface, &gpu.adapter, &window)?;
		surface.configure(&gpu.device, &config);

		Some(Self {
			instance: gpu.instance.clone(),
			device: gpu.device.clone(),
			queue: gpu.queue.clone(),
			config,
			format,
			transparent,
			adapter_info: gpu.adapter_info.clone(),
			backend: Backend::Native(surface),
			sentinel: None,
			_window: window,
		})
	}

	// X11-only per-pixel transparency: glutin creates the window with a 32-bit
	// ARGB visual + transparent GL context, and wgpu runs on it via hal external
	// interop (PoCs on branch spike/x11-transparency). Returns the window it created.
	pub fn new_gl_transparent(
		el: &ActiveEventLoop,
		attrs: WindowAttributes,
	) -> anyhow::Result<(Self, Arc<Window>)> {
		#[cfg(target_os = "linux")]
		quiet_glx_errors();
		// No transparency requirement in the template: the picker closure must
		// return a Config (can't say "none fit"), and a panic there would abort
		// past resumed()'s native-backend fallback (panic=abort in release).
		// So match broadly, prefer transparent+deepest-alpha, validate after.
		let template = glutin::config::ConfigTemplateBuilder::new();
		let (window, config) = DisplayBuilder::new()
			.with_window_attributes(Some(attrs))
			.build(el, template, |cfgs| {
				cfgs.reduce(|best, cand| {
					let (best_transparent, cand_transparent) = (
						best.supports_transparency().unwrap_or(false),
						cand.supports_transparency().unwrap_or(false),
					);
					if (cand_transparent, cand.alpha_size()) > (best_transparent, best.alpha_size())
					{
						cand
					} else {
						best
					}
				})
				// unreachable unless GL reports zero framebuffer configs at all
				.expect("GL reported no framebuffer configs")
			})
			.map_err(|e| anyhow::anyhow!("glutin display build: {e}"))?;
		if !config.supports_transparency().unwrap_or(false) || config.alpha_size() < 8 {
			return Err(anyhow::anyhow!(
				"no transparency-capable GL config (no ARGB visual?)"
			));
		}
		let window = Arc::new(window.ok_or_else(|| anyhow::anyhow!("glutin made no window"))?);
		// empty flags: no indirect-validation (needs compute the GL 3.3 context
		// lacks; we never use indirect draws).
		let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
			backends: wgpu::Backends::GL,
			flags: wgpu::InstanceFlags::empty(),
			memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
			backend_options: wgpu::BackendOptions::default(),
			display: None,
		});
		let gfx = Self::gl_on(instance, window.clone(), config, true)?;
		Ok((gfx, window))
	}

	// The GL context, its surface and the wgpu device over them, on a window
	// glutin made. Shared by the cold start above and a rebuild after a release:
	// the window and its ARGB visual outlive the context, so a new one is built
	// from the same config.
	fn gl_on(
		instance: wgpu::Instance,
		window: Arc<Window>,
		config: glutin::config::Config,
		log: bool,
	) -> anyhow::Result<Self> {
		let raw = window.window_handle()?.as_raw();
		let gl_display = config.display();

		// Request a high GL version. NVIDIA/Linux honors the *exact* version asked
		// (gfx-rs/wgpu#8676), and many wgpu GL bugs - including rendering into a 2D
		// texture view, which is how glyphon draws its atlas - only disappear on
		// GL >=4.2 (gfx-rs/wgpu#8675). A 3.3/4.1 context renders no glyphon text.
		// Try 4.6 down so non-NVIDIA drivers still get a context.
		let ctx = {
			let mut picked = None;
			for (maj, min) in [(4u8, 6u8), (4, 3), (4, 2), (4, 1), (3, 3)] {
				let attrs = ContextAttributesBuilder::new()
					.with_context_api(ContextApi::OpenGl(Some(Version::new(maj, min))))
					.build(Some(raw));
				if let Ok(ctx) = unsafe { gl_display.create_context(&config, &attrs) } {
					picked = Some(ctx);
					break;
				}
			}
			picked.ok_or_else(|| anyhow::anyhow!("no GL context could be created"))?
		};
		let size = window.inner_size();
		let surface = unsafe {
			gl_display.create_window_surface(
				&config,
				&SurfaceAttributesBuilder::<WindowSurface>::new().build(
					raw,
					NonZeroU32::new(size.width.max(1)).unwrap(),
					NonZeroU32::new(size.height.max(1)).unwrap(),
				),
			)?
		};
		let ctx = ctx.make_current(&surface)?;
		// Frame pacing on this path is swap_buffers blocking on vblank; the driver
		// default isn't guaranteed (__GL_SYNC_TO_VBLANK=0, PRIME setups), and without
		// it every scroll animation becomes an unthrottled busy-render loop.
		// SILK_MAX_FPS (app.rs) paces the loop itself, and then vblank must NOT also
		// have a say: a swap that blocks to the next refresh puts every frame back on
		// the display's grid, which is the grid the pinned rate exists to leave.
		let interval = if std::env::var_os("SILK_MAX_FPS").is_some() {
			glutin::surface::SwapInterval::DontWait
		} else {
			glutin::surface::SwapInterval::Wait(NonZeroU32::MIN)
		};
		let _ = surface.set_swap_interval(&ctx, interval);

		let adapter = gl_adapter(&instance, &gl_display)?;
		let adapter_info = adapter.get_info();
		if log {
			log_renderer(&adapter_info, true);
		}
		let (device, queue) =
			pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
				label: Some("silkterm gl device"),
				required_features: wgpu::Features::empty(),
				required_limits: adapter.limits(),
				..Default::default()
			}))?;

		// The GL offscreen is linear-light, so it must NOT be sRGB (an sRGB-declared
		// offscreen makes the blit's textureSample DECODE, cancelling its lin2srgb).
		// It must also be HIGH-PRECISION: an 8-bit *linear* offscreen starves dark
		// gradients of codes -> pronounced banding (esp. a blurred background image).
		// Rgba16Float gives a linear intermediate with no banding; the blit then
		// does the single linear->sRGB encode (+ dither) into the 8-bit fbo 0.
		let format = wgpu::TextureFormat::Rgba16Float;
		let surface_cfg = wgpu::SurfaceConfiguration {
			usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
			format,
			width: size.width.max(1),
			height: size.height.max(1),
			present_mode: wgpu::PresentMode::AutoVsync,
			alpha_mode: wgpu::CompositeAlphaMode::PreMultiplied,
			view_formats: vec![],
			desired_maximum_frame_latency: 2,
		};
		let fb = default_fb(&device, FB_FORMAT, surface_cfg.width, surface_cfg.height);
		let fb_view = fb.create_view(&Default::default());
		let offscreen = offscreen_tex(&device, format, surface_cfg.width, surface_cfg.height);
		let offscreen_view = offscreen.create_view(&Default::default());
		let blit = Blit::new(&device, FB_FORMAT, &offscreen_view);

		let sentinel = Some(Sentinel::new(&device, &queue));
		Ok(Self {
			instance,
			device,
			queue,
			config: surface_cfg,
			format,
			transparent: true,
			adapter_info,
			backend: Backend::Gl {
				ctx,
				surface,
				config,
				fb,
				fb_view,
				offscreen,
				offscreen_view,
				blit,
			},
			sentinel,
			_window: window,
		})
	}

	// Acquire the frame's render target, or say why there is none this time.
	pub fn begin_frame(&mut self) -> Result<Frame, NoFrame> {
		match &self.backend {
			Backend::Native(surface) => {
				use wgpu::CurrentSurfaceTexture::*;
				match surface.get_current_texture() {
					Success(surface_tex) | Suboptimal(surface_tex) => {
						Ok(Frame::Native(surface_tex))
					}
					Outdated | Lost => {
						surface.configure(&self.device, &self.config);
						Err(NoFrame::Other)
					}
					Occluded => Err(NoFrame::Occluded),
					_ => Err(NoFrame::Other),
				}
			}
			Backend::Gl { .. } => Ok(Frame::Gl),
		}
	}

	pub fn frame_view(&self, frame: &Frame) -> wgpu::TextureView {
		match (frame, &self.backend) {
			(Frame::Native(surface_tex), _) => surface_tex
				.texture
				.create_view(&wgpu::TextureViewDescriptor::default()),
			// the scene renders to the offscreen texture (normal orientation)
			(Frame::Gl, Backend::Gl { offscreen_view, .. }) => offscreen_view.clone(),
			_ => unreachable!("frame/backend mismatch"),
		}
	}

	pub fn end_frame(&self, frame: Frame) {
		match (frame, &self.backend) {
			(Frame::Native(surface_tex), _) => surface_tex.present(),
			(
				Frame::Gl,
				Backend::Gl {
					ctx,
					surface,
					fb_view,
					blit,
					..
				},
			) => {
				// flip-blit the offscreen scene into the GL default framebuffer
				let mut enc = self
					.device
					.create_command_encoder(&wgpu::CommandEncoderDescriptor {
						label: Some("blit"),
					});
				{
					let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
						label: Some("blit pass"),
						color_attachments: &[Some(wgpu::RenderPassColorAttachment {
							view: fb_view,
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
					pass.set_pipeline(&blit.pipeline);
					pass.set_bind_group(0, &blit.bind, &[]);
					pass.draw(0..3, 0..1);
				}
				self.queue.submit(Some(enc.finish()));
				let _ = surface.swap_buffers(ctx);
			}
			_ => {}
		}
	}

	pub fn resize(&mut self, w: u32, h: u32) {
		if w == 0 || h == 0 {
			return;
		}
		self.config.width = w;
		self.config.height = h;
		match &mut self.backend {
			Backend::Native(surface) => surface.configure(&self.device, &self.config),
			Backend::Gl {
				surface,
				ctx,
				fb,
				fb_view,
				offscreen,
				offscreen_view,
				blit,
				config: _,
			} => {
				surface.resize(
					ctx,
					NonZeroU32::new(w).unwrap(),
					NonZeroU32::new(h).unwrap(),
				);
				*fb = default_fb(&self.device, FB_FORMAT, w, h);
				*fb_view = fb.create_view(&wgpu::TextureViewDescriptor::default());
				*offscreen = offscreen_tex(&self.device, self.format, w, h);
				*offscreen_view = offscreen.create_view(&wgpu::TextureViewDescriptor::default());
				blit.rebind(&self.device, offscreen_view);
			}
		}
	}
}

// VRAM-content probe (see the Sentinel comment above). All no-ops on the
// native backend, where sentinel is None.
impl Gfx {
	pub fn is_gl(&self) -> bool {
		matches!(self.backend, Backend::Gl { .. })
	}

	// Start an async sentinel readback (both witnesses into one buffer). False
	// when there's no sentinel (native path) or a probe is already in flight.
	pub fn vram_check_start(&mut self) -> bool {
		let Some(sent) = &mut self.sentinel else {
			return false;
		};
		if sent.inflight.is_some() {
			return false;
		}
		let mut enc = self.device.create_command_encoder(&Default::default());
		for (tex, offset) in [(&sent.up_tex, 0u64), (&sent.fbo_tex, SENTINEL_BYTES as u64)] {
			enc.copy_texture_to_buffer(
				wgpu::TexelCopyTextureInfo {
					texture: tex,
					mip_level: 0,
					origin: wgpu::Origin3d::ZERO,
					aspect: wgpu::TextureAspect::All,
				},
				wgpu::TexelCopyBufferInfo {
					buffer: &sent.buf,
					layout: wgpu::TexelCopyBufferLayout {
						offset,
						bytes_per_row: Some(SENTINEL_ROW),
						rows_per_image: Some(SENTINEL_PX),
					},
				},
				wgpu::Extent3d {
					width: SENTINEL_PX,
					height: SENTINEL_PX,
					depth_or_array_layers: 1,
				},
			);
		}
		self.queue.submit(Some(enc.finish()));
		let flag = Arc::new(AtomicU8::new(0));
		let done = flag.clone();
		sent.buf.slice(..).map_async(wgpu::MapMode::Read, move |r| {
			done.store(if r.is_ok() { 1 } else { 2 }, Ordering::Release);
		});
		sent.inflight = Some(flag);
		true
	}

	// Poll an in-flight probe. Some(Lost{..}) = a witness pattern is gone (the
	// sentinels are reseeded before returning so the caller only rebuilds the
	// rest). None = still pending / no probe.
	pub fn vram_check_poll(&mut self) -> Option<VramProbe> {
		let sent = self.sentinel.as_mut()?;
		let flag = sent.inflight.as_ref()?.clone();
		if flag.load(Ordering::Acquire) == 0 {
			// non-blocking pump so the map callback can run
			let _ = self.device.poll(wgpu::PollType::Poll);
		}
		match flag.load(Ordering::Acquire) {
			0 => None,
			2 => {
				sent.inflight = None;
				Some(VramProbe::MapFailed)
			}
			_ => {
				sent.inflight = None;
				let (up_ok, fbo_ok) = {
					let data = sent.buf.slice(..).get_mapped_range();
					let pattern = sentinel_pattern();
					(
						data[..SENTINEL_BYTES] == pattern[..],
						data[SENTINEL_BYTES..] == pattern[..],
					)
				};
				sent.buf.unmap();
				if up_ok && fbo_ok {
					Some(VramProbe::Intact)
				} else {
					sent.seed(&self.device, &self.queue, &sentinel_pattern());
					Some(VramProbe::Lost {
						uploaded: !up_ok,
						rendered: !fbo_ok,
					})
				}
			}
		}
	}

	// Diagnostic (SILK_VRAMLOSS): zero both sentinels to fake a content loss,
	// so the detect->rebuild path can be exercised without a real VT switch.
	pub fn vram_clobber(&self) {
		if let Some(sent) = &self.sentinel {
			sent.seed(&self.device, &self.queue, &vec![0u8; SENTINEL_BYTES]);
		}
	}
}

impl Gfx {
	// Diagnostic: read the GL offscreen texture back and save it as a PNG. Bypasses
	// the compositor/X-pixmap quirks that make screenshotting GL windows unreliable.
	pub fn dump_offscreen(&self, path: &str) {
		let Backend::Gl { offscreen, .. } = &self.backend else {
			return;
		};
		let (w, h) = (self.config.width, self.config.height);
		let unpadded = w * 8; // Rgba16Float = 8 bytes/texel
		let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
		let row_stride = unpadded.div_ceil(align) * align;
		let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("dump"),
			size: (row_stride * h) as u64,
			usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
			mapped_at_creation: false,
		});
		let mut enc = self.device.create_command_encoder(&Default::default());
		enc.copy_texture_to_buffer(
			wgpu::TexelCopyTextureInfo {
				texture: offscreen,
				mip_level: 0,
				origin: wgpu::Origin3d::ZERO,
				aspect: wgpu::TextureAspect::All,
			},
			wgpu::TexelCopyBufferInfo {
				buffer: &buf,
				layout: wgpu::TexelCopyBufferLayout {
					offset: 0,
					bytes_per_row: Some(row_stride),
					rows_per_image: Some(h),
				},
			},
			wgpu::Extent3d {
				width: w,
				height: h,
				depth_or_array_layers: 1,
			},
		);
		self.queue.submit(Some(enc.finish()));
		buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
		let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
		let data = buf.slice(..).get_mapped_range();
		// offscreen is linear Rgba16Float; decode f16 -> linear -> sRGB -> 8-bit so the
		// PNG matches what the blit produces on screen.
		let mut pixels = Vec::with_capacity((w * h * 4) as usize);
		for row in 0..h {
			let row_start = (row * row_stride) as usize;
			for texel in data[row_start..row_start + unpadded as usize].chunks_exact(8) {
				let ch =
					|i: usize| f16_to_f32(u16::from_le_bytes([texel[i * 2], texel[i * 2 + 1]]));
				let to_srgb = crate::config::from_linear_u8;
				pixels.extend_from_slice(&[
					to_srgb(ch(0)),
					to_srgb(ch(1)),
					to_srgb(ch(2)),
					(ch(3).clamp(0.0, 1.0) * 255.0 + 0.5) as u8,
				]);
			}
		}
		let _ = image::save_buffer(path, &pixels, w, h, image::ExtendedColorType::Rgba8);
	}
}

// Minimal half-float decode for the offscreen dump (no `half` dep).
fn f16_to_f32(bits: u16) -> f32 {
	let sign = (bits >> 15) & 1;
	let exp = (bits >> 10) & 0x1f;
	let mant = bits & 0x3ff;
	let magnitude = if exp == 0 {
		(mant as f32) * 2f32.powi(-24)
	} else if exp == 0x1f {
		f32::MAX
	} else {
		(1.0 + mant as f32 / 1024.0) * 2f32.powi(exp as i32 - 15)
	};
	if sign == 1 { -magnitude } else { magnitude }
}

fn request_device(
	adapter: &wgpu::Adapter,
) -> impl std::future::Future<Output = Result<(wgpu::Device, wgpu::Queue), wgpu::RequestDeviceError>>
{
	adapter.request_device(&wgpu::DeviceDescriptor {
		label: Some("silkterm device"),
		required_features: wgpu::Features::empty(),
		required_limits: adapter.limits(),
		..Default::default()
	})
}

// Surface format + alpha mode + configuration, shared by the cold and prewarmed
// native paths so the two can't drift. `None` means this adapter cannot present
// to this surface at all (no formats), which is only reachable on the prewarmed
// path - see `Gfx::with_dialog_gpu`.
fn surface_config(
	surface: &wgpu::Surface<'static>,
	adapter: &wgpu::Adapter,
	window: &Window,
) -> Option<(wgpu::SurfaceConfiguration, wgpu::TextureFormat, bool)> {
	let size = window.inner_size();
	let caps = surface.get_capabilities(adapter);
	if caps.formats.is_empty() {
		return None;
	}
	let format = caps
		.formats
		.iter()
		.copied()
		.find(wgpu::TextureFormat::is_srgb)
		.unwrap_or(caps.formats[0]);

	let (alpha_mode, transparent) = pick_alpha_mode(&caps.alpha_modes, adapter.get_info().backend);

	let config = wgpu::SurfaceConfiguration {
		usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
		format,
		width: size.width.max(1),
		height: size.height.max(1),
		present_mode: wgpu::PresentMode::AutoVsync,
		alpha_mode,
		view_formats: vec![],
		// Windows: one queued frame, not two. With Fifo + 2-frame DXGI latency
		// the CPU races ahead then blocks, so the wall-clock dt between frames
		// alternates short/long and the scroll ease steps unevenly (judder).
		// One frame paces present to the display -> steady dt -> smooth. The GL
		// path and other platforms already pace evenly, so leave them.
		desired_maximum_frame_latency: if cfg!(windows) { 1 } else { 2 },
	};
	Some((config, format, transparent))
}

// The compositing mode to ask for, and whether it lets a translucent background
// show the desktop. Premultiplied wherever it is offered. If only Opaque is
// available (no compositor), stay opaque and transparency is silently ignored.
//
// Metal never offers PreMultiplied, only Opaque and PostMultiplied, and the
// first one offered is Opaque, so a Mac window stayed opaque. PostMultiplied
// there only means the CAMetalLayer is not marked opaque. Core Animation still
// reads the layer as premultiplied, which is what the shaders write, so it is
// the same thing under another name. Elsewhere PostMultiplied would multiply
// a second time, so it is taken on Metal only.
fn pick_alpha_mode(
	offered: &[wgpu::CompositeAlphaMode],
	backend: wgpu::Backend,
) -> (wgpu::CompositeAlphaMode, bool) {
	use wgpu::CompositeAlphaMode as Mode;
	if offered.contains(&Mode::PreMultiplied) {
		return (Mode::PreMultiplied, true);
	}
	if backend == wgpu::Backend::Metal && offered.contains(&Mode::PostMultiplied) {
		return (Mode::PostMultiplied, true);
	}
	(offered.first().copied().unwrap_or(Mode::Opaque), false)
}

// A wgpu instance/adapter/device kept for the life of the process and shared by
// every pop-out dialog.
//
// Dialogs cannot borrow the terminal's context: on X11 that one is a glutin
// GL/EGL context, and a second GL instance panics in wgpu-hal's EGL teardown. So
// each dialog used to build a whole PRIMARY context of its own, on the click, and
// again on every reopen since nothing was retained. Building the instance,
// adapter and device is most of the time it takes to open a dialog. Warming it
// once on a worker thread moves that off the click, and keeping it moves it off
// every later open too.
#[derive(Clone, Debug)]
pub struct DialogGpu {
	instance: wgpu::Instance,
	adapter: wgpu::Adapter,
	device: wgpu::Device,
	queue: wgpu::Queue,
	adapter_info: wgpu::AdapterInfo,
}

// The instance and adapter of a `DialogGpu` whose device has been let go: what
// a rebuild starts from, since the device is the memory and the instance is
// the part a driver may not fully give back (file descriptors stayed open on
// NVIDIA's after every instance destroyed).
#[derive(Clone, Debug)]
pub struct DialogSeed {
	instance: wgpu::Instance,
	adapter: wgpu::Adapter,
	adapter_info: wgpu::AdapterInfo,
}

impl DialogGpu {
	// Runs off the winit thread, so there is no window to check the adapter
	// against - `Gfx::with_dialog_gpu` does that later against the real surface.
	// Not logged: the terminal already reported the GPU, and this picks the same
	// one on any single-adapter box.
	pub fn build() -> anyhow::Result<Self> {
		let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
			backends: wgpu::Backends::PRIMARY,
			flags: wgpu::InstanceFlags::default(),
			memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
			backend_options: wgpu::BackendOptions::default(),
			display: None,
		});
		let pick = |fallback| {
			pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
				power_preference: wgpu::PowerPreference::HighPerformance,
				compatible_surface: None,
				force_fallback_adapter: fallback,
			}))
		};
		let adapter = pick(false).or_else(|_| pick(true))?;
		let adapter_info = adapter.get_info();
		Self::on(DialogSeed {
			instance,
			adapter,
			adapter_info,
		})
	}

	// A device on an instance and adapter that already exist.
	fn on(seed: DialogSeed) -> anyhow::Result<Self> {
		let (device, queue) = pollster::block_on(request_device(&seed.adapter))?;
		Ok(Self {
			instance: seed.instance,
			adapter: seed.adapter,
			device,
			queue,
			adapter_info: seed.adapter_info,
		})
	}

	fn seed(&self) -> DialogSeed {
		DialogSeed {
			instance: self.instance.clone(),
			adapter: self.adapter.clone(),
			adapter_info: self.adapter_info.clone(),
		}
	}
}

// The warm-up worker and the context it produces. `Failed` is a state of its own
// rather than an empty `Ready`: a box with no usable adapter fails every time, so
// without it `start` would spawn another worker on the next event-loop pass and
// keep paying a full adapter probe (and printing) for the life of the process.
// Generic over the context only so the state machine can be exercised without a
// GPU; the one real instantiation is DialogGpu.
#[derive(Debug)]
enum Warm<T> {
	Idle,
	Building(std::thread::JoinHandle<Option<T>>),
	Ready(T),
	Failed,
}

impl<T> Warm<T> {
	// Join the worker if there is one, and hand back a state either way. Written
	// as a by-value transition so the caller cannot take the state and forget to
	// put it back - doing that dropped an already-built context and left every
	// later dialog on the cold path.
	fn settled(self) -> Self {
		match self {
			// a panicked worker reads as a failure, same as a returned None
			Self::Building(job) => job.join().ok().flatten().map_or(Self::Failed, Self::Ready),
			other => other,
		}
	}
}

#[derive(Debug)]
pub struct GpuWarm {
	state: Warm<DialogGpu>,
	// what the last context was built on, once its device has been let go
	seed: Option<DialogSeed>,
}

impl GpuWarm {
	pub const fn idle() -> Self {
		Self {
			state: Warm::Idle,
			seed: None,
		}
	}

	// Start warming. Called once the terminal is actually on screen, so the
	// device build happens in dead time rather than competing with startup.
	// Repeat calls are no-ops. After a release the device is asked of the
	// adapter that was kept, and a cold build is the fallback when that adapter
	// no longer answers.
	pub fn start(&mut self) {
		if !matches!(self.state, Warm::Idle) {
			return;
		}
		let seed = self.seed.take();
		self.state = Warm::Building(std::thread::spawn(move || {
			let built = match seed {
				Some(seed) => DialogGpu::on(seed).or_else(|_| DialogGpu::build()),
				None => DialogGpu::build(),
			};
			match built {
				Ok(gpu) => Some(gpu),
				// A dialog can still be opened without this - it just pays the old
				// cost - so a failure here is a note, not an error.
				Err(e) => {
					eprintln!(
						"{}: dialog GPU warm-up failed ({e}); dialogs will open more slowly",
						crate::config::APP_NAME
					);
					None
				}
			}
		}));
	}

	// Let the device go, keeping the instance and adapter it was built on for
	// the next `start`. The idle release calls this beside the terminal's own.
	pub fn release(&mut self) {
		self.state = std::mem::replace(&mut self.state, Warm::Failed).settled();
		if let Warm::Ready(gpu) = &self.state {
			self.seed = Some(gpu.seed());
		}
		self.state = Warm::Idle;
	}

	// The warm context, waiting on the worker if it is still going. That wait can
	// never cost more than building one here would have, since the work is
	// already under way - and normally it finished seconds ago.
	pub fn get(&mut self) -> Option<DialogGpu> {
		self.state = std::mem::replace(&mut self.state, Warm::Failed).settled();
		match &self.state {
			Warm::Ready(gpu) => Some(gpu.clone()),
			_ => None,
		}
	}
}

// How the About text names an adapter's device type. Shared by the dialog and
// `--about`, so a bug report reads the same either way.
pub const fn acceleration(device_type: wgpu::DeviceType) -> &'static str {
	match device_type {
		wgpu::DeviceType::Cpu => "Software (CPU)",
		wgpu::DeviceType::IntegratedGpu => "Hardware (integrated GPU)",
		wgpu::DeviceType::DiscreteGpu => "Hardware (discrete GPU)",
		wgpu::DeviceType::VirtualGpu => "Hardware (virtual GPU)",
		wgpu::DeviceType::Other => "Unknown",
	}
}

// An adapter for a test to describe itself with, since wgpu gives it no default.
#[cfg(test)]
pub fn test_adapter(name: &str, device_type: wgpu::DeviceType) -> wgpu::AdapterInfo {
	wgpu::AdapterInfo {
		name: name.to_string(),
		vendor: 0,
		device: 0,
		device_type,
		device_pci_bus_id: String::new(),
		driver: String::new(),
		driver_info: String::new(),
		backend: wgpu::Backend::Vulkan,
		subgroup_min_size: 4,
		subgroup_max_size: 128,
		transient_saves_memory: false,
	}
}

// Adapter details for `--about`, with no window and no device. Only the adapter
// is asked for: request_device is the expensive half (measured ~161ms against
// ~6ms), and nothing here draws. PRIMARY matches what the About dialog runs on,
// so the two report the same GPU. None on a box with no usable adapter - the
// rest of the About text is still worth printing.
pub fn probe_adapter_info() -> Option<wgpu::AdapterInfo> {
	let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
		backends: wgpu::Backends::PRIMARY,
		flags: wgpu::InstanceFlags::default(),
		memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
		backend_options: wgpu::BackendOptions::default(),
		display: None,
	});
	let pick = |fallback| {
		pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
			power_preference: wgpu::PowerPreference::HighPerformance,
			compatible_surface: None,
			force_fallback_adapter: fallback,
		}))
	};
	let adapter = pick(false).or_else(|_| pick(true)).ok()?;
	Some(adapter.get_info())
}

// `transparent` is whether the surface can carry alpha at all - the first thing
// to look at when the transparency setting appears to do nothing.
fn log_renderer(info: &wgpu::AdapterInfo, transparent: bool) {
	eprintln!(
		"{}: renderer = {} [{:?} / {:?}] alpha = {}",
		crate::config::APP_NAME,
		info.name,
		info.backend,
		info.device_type,
		if transparent {
			"premultiplied"
		} else {
			"opaque"
		},
	);
}

// Keep the GL path's X errors away from winit. winit holds on to the last X
// error no hook claimed, and its IME calls on a focus change `expect` to find
// none - so a GLX error left over from an earlier call, a failed device rebuild
// say, killed the window the next time it gained or lost focus. glutin has its
// own hook and still sees every error, since winit asks all of them.
#[cfg(target_os = "linux")]
fn quiet_glx_errors() {
	use std::sync::Once;
	use std::sync::atomic::AtomicU32;
	static ONCE: Once = Once::new();
	static LOGGED: AtomicU32 = AtomicU32::new(0);
	ONCE.call_once(|| {
		let Some(glx) = GlxCodes::query() else {
			return;
		};
		winit::platform::x11::register_xlib_error_hook(Box::new(move |_display, event| {
			// SAFETY: winit hands every hook the XErrorEvent it was called with
			let event = unsafe { &*(event as *const x11_dl::xlib::XErrorEvent) };
			if !glx.claims(event.request_code, event.error_code) {
				return false;
			}
			// winit logs only what nobody claimed, so say it here. A driver that
			// fails every frame would otherwise fill the log.
			if LOGGED.fetch_add(1, Ordering::Relaxed) < 20 {
				eprintln!(
					"{}: X error {} from GL request {}.{}, not fatal",
					crate::config::APP_NAME,
					event.error_code,
					event.request_code,
					event.minor_code
				);
			}
			true
		}));
	});
}

// Which X errors belong to GL: any raised by a GLX request or by NVIDIA's
// private NV-GLX one, and any in GLX's own error range. An opcode of 0 means
// the extension is not there.
#[cfg(target_os = "linux")]
#[derive(Clone, Copy)]
struct GlxCodes {
	requests: [u8; 2],
	first_error: u8,
}

#[cfg(target_os = "linux")]
impl GlxCodes {
	// GLXBadContext through GLXBadProfileARB
	const ERRORS: u16 = 14;

	// Asked on a connection of its own. Opcodes are the server's, so they are
	// the same on winit's.
	fn query() -> Option<Self> {
		use x11rb::protocol::xproto::ConnectionExt as _;
		let (conn, _) = x11rb::connect(None).ok()?;
		let ext = |name: &[u8]| {
			conn.query_extension(name)
				.ok()?
				.reply()
				.ok()
				.filter(|r| r.present)
		};
		let glx = ext(b"GLX")?;
		let nv = ext(b"NV-GLX").map_or(0, |r| r.major_opcode);
		Some(GlxCodes {
			requests: [glx.major_opcode, nv],
			first_error: glx.first_error,
		})
	}

	fn claims(self, request: u8, error: u8) -> bool {
		let first = u16::from(self.first_error);
		(request != 0 && self.requests.contains(&request))
			|| (first != 0 && (first..first + Self::ERRORS).contains(&u16::from(error)))
	}
}

// Offscreen scene target for the GL path: rendered top-left like the native
// surface, then flip-blitted into the default framebuffer.
fn offscreen_tex(
	device: &wgpu::Device,
	format: wgpu::TextureFormat,
	w: u32,
	h: u32,
) -> wgpu::Texture {
	device.create_texture(&wgpu::TextureDescriptor {
		label: Some("offscreen"),
		size: wgpu::Extent3d {
			width: w.max(1),
			height: h.max(1),
			depth_or_array_layers: 1,
		},
		mip_level_count: 1,
		sample_count: 1,
		dimension: wgpu::TextureDimension::D2,
		format,
		usage: wgpu::TextureUsages::RENDER_ATTACHMENT
			| wgpu::TextureUsages::TEXTURE_BINDING
			| wgpu::TextureUsages::COPY_SRC, // for the dump_offscreen diagnostic
		view_formats: &[],
	})
}

// The GL default framebuffer (fbo 0) is treated as plain (non-sRGB) RGBA: it isn't
// sRGB-capable, so the blit shader sRGB-encodes explicitly and writes raw here.
const FB_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

// wrap glutin's GL context as a wgpu adapter (hal external interop)
#[cfg(not(target_os = "macos"))]
fn gl_adapter(
	instance: &wgpu::Instance,
	gl_display: &glutin::display::Display,
) -> anyhow::Result<wgpu::Adapter> {
	let exposed = unsafe {
		wgpu::hal::gles::Adapter::new_external(
			|name| {
				std::ffi::CString::new(name).map_or(std::ptr::null(), |cstr| {
					gl_display.get_proc_address(&cstr).cast()
				})
			},
			wgpu::GlBackendOptions::default(),
		)
	}
	.ok_or_else(|| anyhow::anyhow!("wgpu GL external adapter init failed"))?;
	Ok(unsafe { instance.create_adapter_from_hal::<Gles>(exposed) })
}

// wgpu builds no GL backend on macOS (Metal only), and the GL path is X11-only
// anyway, so there it just reports itself unavailable and resumed() falls back
// to the native surface.
#[cfg(target_os = "macos")]
fn gl_adapter(
	_instance: &wgpu::Instance,
	_gl_display: &glutin::display::Display,
) -> anyhow::Result<wgpu::Adapter> {
	anyhow::bail!("no wgpu GL backend on macOS")
}

// A wgpu texture aliasing the GL default framebuffer (fbo 0 = glutin's window).
#[cfg(not(target_os = "macos"))]
fn default_fb(device: &wgpu::Device, format: wgpu::TextureFormat, w: u32, h: u32) -> wgpu::Texture {
	// Safety: aliasing the GL default framebuffer is sound only while every GL
	// call stays on the winit main thread - rendering here is single-threaded
	// by construction; don't move GL work onto helper threads.
	let hal = wgpu::hal::gles::Texture::default_framebuffer(format);
	unsafe {
		device.create_texture_from_hal::<Gles>(
			hal,
			&wgpu::TextureDescriptor {
				label: Some("default fb"),
				size: wgpu::Extent3d {
					width: w.max(1),
					height: h.max(1),
					depth_or_array_layers: 1,
				},
				mip_level_count: 1,
				sample_count: 1,
				dimension: wgpu::TextureDimension::D2,
				format,
				usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
				view_formats: &[],
			},
		)
	}
}

// Only Backend::Gl calls this, and gl_adapter() above never lets one be built on macOS.
#[cfg(target_os = "macos")]
fn default_fb(_: &wgpu::Device, _: wgpu::TextureFormat, _: u32, _: u32) -> wgpu::Texture {
	unreachable!("no GL backend on macOS")
}

#[repr(C)]
#[derive(Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct RectInstance {
	pub pos: [f32; 2],
	pub size: [f32; 2],
	pub color: [f32; 4],
	// params.x = mode (0 solid quad, 1 close-"X" mark, 2 rounded quad,
	// 3 triangle - a submenu arrow, or a move-this-row arrow,
	// 4 the color picker's saturation/brightness square, 5 its hue strip),
	// params.y = stroke px for the X, corner radius for the rounded quad,
	// quarter-turns clockwise for the triangle (0 right, 1 down, 2 left, 3 up).
	// The X and the arrows are drawn in the fragment shader, so each centers
	// exactly in its quad (a font glyph never did - baseline metrics vary, and
	// there is no arrow every interface font carries).
	//
	// Mode 4 reads `color` as sRGB rather than linear, unlike every other mode:
	// it is the hue the square mixes toward, and mixing toward white in linear
	// light gives a gradient nobody would recognise as a color picker. Modes 4
	// and 5 encode the result themselves, so the value that arrives is the one
	// the box is showing. Neither may use params.y - it is a length, and
	// `quads_px` scales it.
	pub params: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniform {
	resolution: [f32; 2],
	_pad: [f32; 2],
}

// flat colored quads: backgrounds, cursor, dividers, focus ring
pub struct RectRenderer {
	pipeline: wgpu::RenderPipeline,
	instances: wgpu::Buffer,
	capacity: u64,
	uniform: wgpu::Buffer,
	bind_group: wgpu::BindGroup,
	// last resolution written to the uniform (skip the per-frame re-write)
	last_res: std::cell::Cell<(f32, f32)>,
}

impl RectRenderer {
	pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
		let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
			label: Some("rect shader"),
			source: wgpu::ShaderSource::Wgsl(RECT_WGSL.into()),
		});

		let uniform = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("rect uniform"),
			size: std::mem::size_of::<Uniform>() as u64,
			usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});

		let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("rect bgl"),
			entries: &[wgpu::BindGroupLayoutEntry {
				binding: 0,
				visibility: wgpu::ShaderStages::VERTEX,
				ty: wgpu::BindingType::Buffer {
					ty: wgpu::BufferBindingType::Uniform,
					has_dynamic_offset: false,
					min_binding_size: None,
				},
				count: None,
			}],
		});

		let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("rect bg"),
			layout: &bgl,
			entries: &[wgpu::BindGroupEntry {
				binding: 0,
				resource: uniform.as_entire_binding(),
			}],
		});

		let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
			label: Some("rect layout"),
			bind_group_layouts: &[Some(&bgl)],
			immediate_size: 0,
		});

		let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
			label: Some("rect pipeline"),
			layout: Some(&layout),
			vertex: wgpu::VertexState {
				module: &shader,
				entry_point: Some("vs"),
				compilation_options: Default::default(),
				buffers: &[wgpu::VertexBufferLayout {
					array_stride: std::mem::size_of::<RectInstance>() as u64,
					step_mode: wgpu::VertexStepMode::Instance,
					attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4, 3 => Float32x2],
				}],
			},
			fragment: Some(wgpu::FragmentState {
				module: &shader,
				entry_point: Some("fs"),
				compilation_options: Default::default(),
				targets: &[Some(wgpu::ColorTargetState {
					format,
					// premultiplied so it composites onto a transparent surface;
					// the shader premultiplies, so RGB results match straight alpha
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

		let capacity = 256;
		let instances = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("rect instances"),
			size: capacity * std::mem::size_of::<RectInstance>() as u64,
			usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});

		Self {
			pipeline,
			instances,
			capacity,
			uniform,
			bind_group,
			last_res: std::cell::Cell::new((0.0, 0.0)),
		}
	}

	pub fn set_resolution(&self, queue: &wgpu::Queue, w: f32, h: f32) {
		// called per frame; the uniform only changes on resize
		if self.last_res.get() == (w, h) {
			return;
		}
		self.last_res.set((w, h));
		let uniform_data = Uniform {
			resolution: [w, h],
			_pad: [0.0, 0.0],
		};
		queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&uniform_data));
	}

	pub fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, data: &[RectInstance]) {
		let needed = data.len() as u64;
		if needed > self.capacity {
			self.capacity = needed.next_power_of_two();
			self.instances = device.create_buffer(&wgpu::BufferDescriptor {
				label: Some("rect instances"),
				size: self.capacity * std::mem::size_of::<RectInstance>() as u64,
				usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
				mapped_at_creation: false,
			});
		}
		if !data.is_empty() {
			queue.write_buffer(&self.instances, 0, bytemuck::cast_slice(data));
		}
	}

	pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, range: std::ops::Range<u32>) {
		if range.is_empty() {
			return;
		}
		pass.set_pipeline(&self.pipeline);
		pass.set_bind_group(0, &self.bind_group, &[]);
		pass.set_vertex_buffer(0, self.instances.slice(..));
		pass.draw(0..4, range);
	}
}

pub(crate) const RECT_WGSL: &str = r"
struct Uniform { resolution: vec2<f32>, _pad: vec2<f32> };
@group(0) @binding(0) var<uniform> u: Uniform;

struct VsIn {
    @location(0) pos: vec2<f32>,
    @location(1) size: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) params: vec2<f32>,
    @builtin(vertex_index) vi: u32,
};
struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) local: vec2<f32>,
    @location(2) size: vec2<f32>,
    @location(3) params: vec2<f32>,
};

@vertex
fn vs(in: VsIn) -> VsOut {
    var corner = vec2<f32>(f32(in.vi & 1u), f32((in.vi >> 1u) & 1u));
    var px = in.pos + corner * in.size;
    var ndc = vec2<f32>(px.x / u.resolution.x * 2.0 - 1.0, 1.0 - px.y / u.resolution.y * 2.0);
    var out: VsOut;
    out.clip = vec4<f32>(ndc, 0.0, 1.0);
    out.color = in.color;
    out.local = corner * in.size;
    out.size = in.size;
    out.params = in.params;
    return out;
}

// One 45-degree bar of the X: q is the pixel offset from the quad center in the
// bar's rotated frame (x along the bar, y across it). Box-SDF with ~1px edges,
// so the bar ends are square caps perpendicular to the stroke - i.e. cut on the
// diagonal, not flat like a letter X.
fn xbar(q: vec2<f32>, half_len: f32, half_th: f32) -> f32 {
    let d = max(abs(q.x) - half_len, abs(q.y) - half_th);
    return clamp(0.5 - d, 0.0, 1.0);
}

// fraction of the quad's short side left as padding around the X mark
const X_INSET: f32 = 0.26;

// Signed distance to a rounded box, negative inside. p is the offset from the
// quad center, half the quad's extent, r the corner radius.
fn round_box(p: vec2<f32>, half: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - (half - vec2<f32>(r, r));
    return length(max(q, vec2<f32>(0.0, 0.0))) + min(max(q.x, q.y), 0.0) - r;
}

// Signed distance to a right-pointing isoceles triangle that fills the quad,
// negative inside. p is the offset from the quad center, half its extent. The
// two slanted edges are one line mirrored across the x axis; the third is the
// flat base at the left.
fn right_triangle(p: vec2<f32>, half: vec2<f32>) -> f32 {
    let q = vec2<f32>(p.x, abs(p.y));
    let n = normalize(vec2<f32>(half.y, 2.0 * half.x));
    return max(dot(q - vec2<f32>(half.x, 0.0), n), -half.x - q.x);
}

// Turn the sample point instead of the shape, so one triangle serves all four
// directions. An odd number of quarter-turns also swaps the half-extents, or a
// non-square box would point the arrow at a corner.
fn turned(p: vec2<f32>, half: vec2<f32>, turns: f32) -> vec2<f32> {
    let t = i32(round(turns)) & 3;
    if (t == 1) { return vec2<f32>(p.y, -p.x); }        // down
    if (t == 2) { return vec2<f32>(-p.x, p.y); }        // left
    if (t == 3) { return vec2<f32>(-p.y, p.x); }        // up
    return p;
}
fn turned_half(half: vec2<f32>, turns: f32) -> vec2<f32> {
    let t = i32(round(turns)) & 3;
    if (t == 1 || t == 3) { return vec2<f32>(half.y, half.x); }
    return half;
}

// sRGB -> linear, per channel. The surface encodes on write, so a color the
// shader builds itself has to arrive linear like every other one.
fn to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

// The fully lit color at this hue, sRGB. Mirrored in pick.rs, which needs the
// same answer on the CPU to place the marker.
fn hue_rgb(h: f32) -> vec3<f32> {
    let k = fract(h) * 6.0;
    return vec3<f32>(
        clamp(abs(k - 3.0) - 1.0, 0.0, 1.0),
        clamp(2.0 - abs(k - 2.0), 0.0, 1.0),
        clamp(2.0 - abs(k - 4.0), 0.0, 1.0),
    );
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    var rgb = in.color.rgb;
    var a = in.color.a;
    if (in.params.x > 4.5) {
        rgb = to_linear(hue_rgb(in.local.y / in.size.y));
    } else if (in.params.x > 3.5) {
        let s = in.local.x / in.size.x;
        let v = 1.0 - in.local.y / in.size.y;
        rgb = to_linear(mix(vec3<f32>(1.0), in.color.rgb, s) * v);
    } else if (in.params.x > 2.5) {
        let half = in.size * 0.5;
        let p = turned(in.local - half, half, in.params.y);
        // ~1px linear edge, same convention as the X bars
        a = a * clamp(0.5 - right_triangle(p, turned_half(half, in.params.y)), 0.0, 1.0);
    } else if (in.params.x > 1.5) {
        let half = in.size * 0.5;
        let r = min(in.params.y, min(half.x, half.y));
        // ~1px linear edge, same convention as the X bars
        a = a * clamp(0.5 - round_box(in.local - half, half, r), 0.0, 1.0);
    } else if (in.params.x > 0.5) {
        let p = in.local - in.size * 0.5;
        // both diagonals in one rotation: u = 45-deg frame, u.yx = the other bar
        let q = vec2<f32>(p.x + p.y, p.x - p.y) * 0.7071068;
        let half_ext = min(in.size.x, in.size.y) * (0.5 - X_INSET);
        let half_len = half_ext * 1.4142136;
        let half_th = in.params.y * 0.5;
        a = a * max(xbar(q, half_len, half_th), xbar(vec2<f32>(q.y, q.x), half_len, half_th));
    }
    // premultiply: lets translucent backgrounds composite over the desktop
    return vec4<f32>(rgb * a, a);
}
";

// Fullscreen-triangle flip-blit: samples the offscreen scene and writes it to
// the GL default framebuffer with V flipped (fbo 0 has a bottom-left origin).
// The offscreen already holds premultiplied rgba, so this is a straight copy.
const BLIT_WGSL: &str = r#"
struct VsOut { @builtin(position) clip: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex
fn vs(@builtin(vertex_index) i: u32) -> VsOut {
    var xy = array<vec2<f32>, 3>(vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0));
    let p = xy[i];
    var o: VsOut;
    o.clip = vec4<f32>(p, 0.0, 1.0);
    // default framebuffer (fbo 0) is bottom-origin, so DON'T apply the usual
    // top-left flip: clip.y=+1 maps to the window bottom and should sample the
    // offscreen bottom (uv.y=1) - i.e. uv.y rises with clip.y.
    o.uv = vec2<f32>((p.x + 1.0) * 0.5, (p.y + 1.0) * 0.5);
    return o;
}
@group(0) @binding(0) var t: texture_2d<f32>;
@group(0) @binding(1) var s: sampler;
// linear -> sRGB. The GL default framebuffer (fbo 0) is NOT sRGB-capable here, so
// wgpu won't encode on write; without this every pixel comes out ~half-bright (opaque
// text then reads as "faded/transparent"). Encode manually and write to a non-sRGB
// target so there's no double conversion. rgb is premultiplied; encode per-channel.
fn lin2srgb(c: vec3<f32>) -> vec3<f32> {
    let cl = max(c, vec3<f32>(0.0));
    let lo = cl * 12.92;
    let hi = 1.055 * pow(cl, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, cl <= vec3<f32>(0.0031308));
}
// cheap per-pixel hash for ordered dithering
fn hash12(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.x, p.y, p.x) * 0.1031);
    p3 += dot(p3, vec3<f32>(p3.y, p3.z, p3.x) + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}
@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let c = textureSample(t, s, in.uv);
    // TPDF dither (~1 LSB) before the 8-bit fbo write breaks gradient banding
    // (the offscreen is high-precision linear; the final framebuffer is 8-bit).
    let p = in.clip.xy;
    let d = (hash12(p) - hash12(p + vec2<f32>(13.7, 91.3))) / 255.0;
    return vec4<f32>(lin2srgb(c.rgb) + vec3<f32>(d), c.a);
}
"#;

#[cfg(test)]
mod tests {
	use super::*;

	// What the compositor shows through a pane fill, as an encoded value: the
	// fill's premultiplied pixel after the one encode, plus the desktop at 1 - a.
	fn shown(fill: [f32; 4], desktop: f32) -> f32 {
		crate::config::from_linear(fill[0] * fill[3]) + (1.0 - fill[3]) * desktop
	}

	// A light fill has to let as much of the desktop through as a dark one at the
	// same opacity. Before, white at 80% showed under half of what black did.
	// Test ID: ErDEQFR
	#[test]
	fn a_light_fill_is_as_see_through_as_a_dark_one() {
		for a in [0.3f32, 0.5, 0.8, 0.95] {
			for c in [0.0f32, 0.01, 0.2, 0.8, 1.0] {
				let fill = see_through([c, c, c, a]);
				let want = a * crate::config::from_linear(c);
				let got = shown(fill, 0.0);
				assert!((got - want).abs() < 1e-4, "c {c} a {a}: {got} vs {want}");
			}
			let dark = shown(see_through([0.0, 0.0, 0.0, a]), 1.0);
			let light = shown(see_through([1.0, 1.0, 1.0, a]), 0.0);
			let (dark_range, light_range) = (dark - 0.0, 1.0 - light);
			assert!(
				(dark_range - light_range).abs() < 1e-4,
				"a {a}: dark shows {dark_range}, light {light_range}"
			);
		}
		let opaque = [0.3, 0.6, 0.9, 1.0];
		assert_eq!(see_through(opaque), opaque);
	}

	// What each platform's surface offers, and what the window must ask for.
	// Metal lists Opaque first and never PreMultiplied, which left a Mac window
	// opaque. The other rows are what Linux and Windows already picked.
	// Test ID: ErUlBTl
	#[test]
	fn each_platform_picks_a_see_through_alpha_mode_where_it_has_one() {
		use wgpu::Backend;
		use wgpu::CompositeAlphaMode::{Inherit, Opaque, PostMultiplied, PreMultiplied};
		#[rustfmt::skip]
		let cases = [
			(Backend::Metal,  vec![Opaque, PostMultiplied],                         (PostMultiplied, true)),
			(Backend::Metal,  vec![Opaque],                                         (Opaque, false)),
			(Backend::Dx12,   vec![PreMultiplied],                                  (PreMultiplied, true)),
			(Backend::Dx12,   vec![Opaque],                                         (Opaque, false)),
			(Backend::Vulkan, vec![Opaque],                                         (Opaque, false)),
			(Backend::Vulkan, vec![Inherit],                                        (Inherit, false)),
			(Backend::Vulkan, vec![Opaque, PreMultiplied, PostMultiplied, Inherit], (PreMultiplied, true)),
			(Backend::Vulkan, vec![Opaque, PostMultiplied],                         (Opaque, false)),
			(Backend::Gl,     vec![PreMultiplied, Opaque],                          (PreMultiplied, true)),
		];
		for (backend, offered, want) in cases {
			assert_eq!(
				pick_alpha_mode(&offered, backend),
				want,
				"{backend:?} offering {offered:?}"
			);
		}
	}

	// Numbers from the NVIDIA box where a stray GLX error used to kill the
	// window at its next focus change: GLX 152 with errors from 158, NV-GLX 156.
	// Test ID: EqFRbdI
	#[cfg(target_os = "linux")]
	#[test]
	fn gl_errors_are_claimed_and_others_are_not() {
		let nvidia = GlxCodes {
			requests: [152, 156],
			first_error: 158,
		};
		assert!(
			nvidia.claims(152, 170),
			"GLXBadWindow from glXDestroyWindow"
		);
		assert!(nvidia.claims(156, 9), "BadDrawable from NV-GLX");
		assert!(nvidia.claims(1, 160), "a GLX error on some other request");
		assert!(!nvidia.claims(20, 3), "BadWindow from GetProperty");
		assert!(!nvidia.claims(1, 172), "past GLX's error range");
		let mesa = GlxCodes {
			requests: [152, 0],
			first_error: 158,
		};
		assert!(!mesa.claims(0, 9));
		assert!(!mesa.claims(156, 9), "no NV-GLX here");
	}

	// A built context has to survive being asked for. It didn't once: the state
	// was taken unconditionally to join the worker and only put back on the
	// Building arm, so the second dialog open dropped the context and every open
	// after that paid for a cold one.
	// Test ID: Ep1mr9M
	#[test]
	fn a_settled_context_survives_being_settled_again() {
		let mut w = Warm::Building(std::thread::spawn(|| Some(7)));
		for _ in 0..3 {
			w = w.settled();
			assert!(matches!(w, Warm::Ready(7)));
		}
	}

	// A worker that came back empty stays Failed rather than falling back to
	// Idle, which would let `start` spawn another probe on the next pass.
	// Test ID: Ep1mr9N
	#[test]
	fn a_failed_warm_up_stays_failed() {
		let mut w = Warm::Building(std::thread::spawn(|| Option::<u32>::None));
		for _ in 0..3 {
			w = w.settled();
			assert!(matches!(w, Warm::Failed));
		}
	}

	// The sentinel only detects loss if its pattern can't be mistaken for
	// trashed VRAM: right size, deterministic, and not a trivial fill.
	// Test ID: Ekm1rM0
	#[test]
	fn sentinel_pattern_is_deterministic_and_varied() {
		let a = sentinel_pattern();
		assert_eq!(a.len(), SENTINEL_BYTES);
		assert_eq!(a, sentinel_pattern());
		// a byte permutation tiled: every value present, so neither zeroed nor
		// constant-fill memory matches
		let mut seen = [false; 256];
		for &b in &a[..256] {
			seen[b as usize] = true;
		}
		assert!(seen.iter().all(|&s| s));
		assert_ne!(a, vec![0u8; SENTINEL_BYTES]);
	}

	// Test ID: Ekm1rM1
	#[test]
	fn sentinel_row_is_copy_aligned() {
		// stride == unpadded row, so the readback compares without de-padding
		assert_eq!(SENTINEL_ROW % wgpu::COPY_BYTES_PER_ROW_ALIGNMENT, 0);
		// the second witness reads back at this buffer offset
		assert_eq!(SENTINEL_BYTES as u64 % wgpu::COPY_BUFFER_ALIGNMENT, 0);
	}

	// Test ID: Er2UJeQ
	#[test]
	fn every_gpu_is_hardware_and_only_the_cpu_is_software() {
		use wgpu::DeviceType;
		assert_eq!(acceleration(DeviceType::Cpu), "Software (CPU)");
		for (kind, said) in [
			(DeviceType::DiscreteGpu, "Hardware (discrete GPU)"),
			(DeviceType::IntegratedGpu, "Hardware (integrated GPU)"),
			(DeviceType::VirtualGpu, "Hardware (virtual GPU)"),
		] {
			assert_eq!(acceleration(kind), said);
		}
		assert!(!acceleration(DeviceType::Other).starts_with("Hardware"));
	}
}
