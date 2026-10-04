// Keeps a GPU busy, full, or both, so a window can be watched under load.
//
//	gpu-stress [--vram-mb N | --vram-pct P] [--chunk-mb N] [--touch]
//	           [--busy F] [--slice-ms N] [--secs N] [--stop-file PATH]
//	           [--backend all|vulkan|dx12|gl]
//
// --vram-mb takes that much in chunks until the driver refuses one. --vram-pct
// asks wgpu to refuse past that percent of the budget the driver reports, and
// takes chunks until it does. --busy is the share of time a compute shader
// keeps the GPU working, 0 to 1. Ends after --secs, or once the stop file
// exists, and prints what it holds every two seconds.

use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Debug)]
struct Opts {
	vram_mb: Option<u64>,
	vram_pct: Option<u8>,
	chunk_mb: u64,
	touch: bool,
	busy: f64,
	slice_ms: f64,
	secs: Option<f64>,
	stop_file: Option<PathBuf>,
	backends: wgpu::Backends,
}

fn usage() -> ! {
	eprintln!(
		"usage: gpu-stress [--vram-mb N | --vram-pct P] [--chunk-mb N] [--touch] [--busy F] [--slice-ms N] [--secs N] [--stop-file PATH] [--backend all|vulkan|dx12|gl]"
	);
	std::process::exit(2);
}

fn parse() -> Opts {
	let mut opts = Opts {
		vram_mb: None,
		vram_pct: None,
		chunk_mb: 256,
		touch: false,
		busy: 0.0,
		slice_ms: 40.0,
		secs: None,
		stop_file: None,
		backends: wgpu::Backends::all(),
	};
	let mut args = std::env::args().skip(1);
	while let Some(arg) = args.next() {
		let mut value = || args.next().unwrap_or_else(|| usage());
		match arg.as_str() {
			"--vram-mb" => opts.vram_mb = value().parse().ok(),
			"--vram-pct" => opts.vram_pct = value().parse().ok(),
			"--chunk-mb" => opts.chunk_mb = value().parse().unwrap_or_else(|_| usage()),
			"--touch" => opts.touch = true,
			"--busy" => opts.busy = value().parse().unwrap_or_else(|_| usage()),
			"--slice-ms" => opts.slice_ms = value().parse().unwrap_or_else(|_| usage()),
			"--secs" => opts.secs = value().parse().ok(),
			"--stop-file" => opts.stop_file = Some(PathBuf::from(value())),
			"--backend" => {
				opts.backends = match value().as_str() {
					"vulkan" => wgpu::Backends::VULKAN,
					"dx12" => wgpu::Backends::DX12,
					"gl" => wgpu::Backends::GL,
					"all" => wgpu::Backends::all(),
					_ => usage(),
				}
			}
			_ => usage(),
		}
	}
	opts.busy = opts.busy.clamp(0.0, 1.0);
	opts.chunk_mb = opts.chunk_mb.max(1);
	opts
}

// Every invocation spins on dependent multiply-adds, so the work cannot be
// folded away, and writes one word so it cannot be dropped either.
const SHADER: &str = r"
struct Params { iters: u32, pad0: u32, pad1: u32, pad2: u32 }
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read_write> sink: array<f32>;

@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
	var a = f32(id.x) * 0.0001 + 1.0;
	var b = 0.9999;
	for (var i = 0u; i < params.iters; i++) {
		a = fma(a, b, 0.0001);
		b = fma(b, a, -0.0001);
	}
	sink[id.x & 1023u] = a + b;
}
";

const GROUPS: u32 = 8192;

struct Burner {
	pipeline: wgpu::ComputePipeline,
	bind: wgpu::BindGroup,
	params: wgpu::Buffer,
	iters: u32,
}

impl Burner {
	fn new(device: &wgpu::Device) -> Self {
		let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
			label: Some("burn"),
			source: wgpu::ShaderSource::Wgsl(SHADER.into()),
		});
		let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
			label: Some("burn"),
			layout: None,
			module: &module,
			entry_point: Some("main"),
			compilation_options: wgpu::PipelineCompilationOptions::default(),
			cache: None,
		});
		let params = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("params"),
			size: 16,
			usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});
		let sink = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("sink"),
			size: 4096,
			usage: wgpu::BufferUsages::STORAGE,
			mapped_at_creation: false,
		});
		let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("burn"),
			layout: &pipeline.get_bind_group_layout(0),
			entries: &[
				wgpu::BindGroupEntry {
					binding: 0,
					resource: params.as_entire_binding(),
				},
				wgpu::BindGroupEntry {
					binding: 1,
					resource: sink.as_entire_binding(),
				},
			],
		});
		Burner {
			pipeline,
			bind,
			params,
			iters: 256,
		}
	}

	// One dispatch, waited out. Returns how long the GPU took.
	fn slice(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Duration {
		let words = [self.iters, 0, 0, 0];
		let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
		queue.write_buffer(&self.params, 0, &bytes);
		let mut encoder =
			device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
		{
			let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
				label: None,
				timestamp_writes: None,
			});
			pass.set_pipeline(&self.pipeline);
			pass.set_bind_group(0, &self.bind, &[]);
			pass.dispatch_workgroups(GROUPS, 1, 1);
		}
		let start = Instant::now();
		queue.submit(Some(encoder.finish()));
		let _ = device.poll(wgpu::PollType::wait_indefinitely());
		start.elapsed()
	}

	// Steer the loop count so one dispatch lasts about `target`. Kept well under
	// the two seconds Windows allows before it resets the GPU.
	fn steer(&mut self, took: Duration, target: Duration) {
		let ratio = target.as_secs_f64() / took.as_secs_f64().max(1e-4);
		let next = (f64::from(self.iters) * ratio.clamp(0.5, 2.0)).round();
		self.iters = next.clamp(16.0, 4_000_000.0) as u32;
	}
}

// Take memory chunk by chunk until the target, a refusal, or the budget line.
// Each chunk is cleared on the GPU so its pages are really there.
fn fill(
	device: &wgpu::Device,
	queue: &wgpu::Queue,
	opts: &Opts,
	max_chunk: u64,
) -> Vec<wgpu::Buffer> {
	let mut held = Vec::new();
	if opts.vram_mb.is_none() && opts.vram_pct.is_none() {
		return held;
	}
	let chunk = (opts.chunk_mb << 20).min(max_chunk) & !3;
	let want = opts.vram_mb.map_or(u64::MAX, |mb| mb << 20);
	let mut have = 0u64;
	while have < want {
		let size = chunk.min(want - have).max(4);
		let scope = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
		let buf = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("hold"),
			size,
			usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});
		if let Some(err) = pollster::block_on(scope.pop()) {
			say(&format!("refused at {} MB: {err}", have >> 20));
			break;
		}
		let mut encoder =
			device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
		encoder.clear_buffer(&buf, 0, None);
		queue.submit(Some(encoder.finish()));
		let _ = device.poll(wgpu::PollType::wait_indefinitely());
		have += size;
		held.push(buf);
	}
	say(&format!(
		"holding {} MB in {} chunks",
		have >> 20,
		held.len()
	));
	held
}

fn touch(device: &wgpu::Device, queue: &wgpu::Queue, held: &[wgpu::Buffer]) {
	if held.is_empty() {
		return;
	}
	let mut encoder =
		device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
	for buf in held {
		encoder.clear_buffer(buf, 0, None);
	}
	queue.submit(Some(encoder.finish()));
}

fn say(msg: &str) {
	let mut out = std::io::stdout().lock();
	let _ = writeln!(out, "gpu-stress: {msg}");
	let _ = out.flush();
}

fn main() {
	let opts = parse();
	let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
		backends: opts.backends,
		flags: wgpu::InstanceFlags::default(),
		memory_budget_thresholds: wgpu::MemoryBudgetThresholds {
			for_resource_creation: opts.vram_pct,
			for_device_loss: None,
		},
		backend_options: wgpu::BackendOptions::default(),
		display: None,
	});
	let adapter = match pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
		power_preference: wgpu::PowerPreference::HighPerformance,
		compatible_surface: None,
		force_fallback_adapter: false,
	})) {
		Ok(adapter) => adapter,
		Err(e) => {
			say(&format!("no adapter: {e}"));
			std::process::exit(1);
		}
	};
	let info = adapter.get_info();
	say(&format!(
		"adapter {} ({:?}, {:?})",
		info.name, info.backend, info.device_type
	));
	let limits = adapter.limits();
	let (device, queue) =
		match pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
			label: Some("gpu-stress"),
			required_features: wgpu::Features::empty(),
			required_limits: wgpu::Limits {
				max_buffer_size: limits.max_buffer_size,
				max_storage_buffer_binding_size: limits.max_storage_buffer_binding_size,
				..wgpu::Limits::default()
			},
			experimental_features: wgpu::ExperimentalFeatures::default(),
			memory_hints: wgpu::MemoryHints::Performance,
			trace: wgpu::Trace::Off,
		})) {
			Ok(pair) => pair,
			Err(e) => {
				say(&format!("no device: {e}"));
				std::process::exit(1);
			}
		};
	// a refusal is reported by its error scope; anything else is only printed
	device.on_uncaptured_error(std::sync::Arc::new(|e| say(&format!("gpu error: {e}"))));

	let held = fill(&device, &queue, &opts, limits.max_buffer_size);
	let held_mb: u64 = held.iter().map(|b| b.size() >> 20).sum();
	let mut burner = (opts.busy > 0.0).then(|| Burner::new(&device));
	let target = Duration::from_secs_f64(opts.slice_ms / 1000.0);
	let started = Instant::now();
	let mut report = Instant::now();
	let mut last_touch = Instant::now();
	let (mut busy_time, mut window_start) = (Duration::ZERO, Instant::now());
	let mut last_slice = Duration::ZERO;
	let why = loop {
		if opts
			.secs
			.is_some_and(|s| started.elapsed().as_secs_f64() >= s)
		{
			break "timer";
		}
		if opts.stop_file.as_ref().is_some_and(|p| p.exists()) {
			break "stop file";
		}
		if opts.touch && last_touch.elapsed() >= Duration::from_secs(1) {
			touch(&device, &queue, &held);
			last_touch = Instant::now();
		}
		match burner.as_mut() {
			Some(b) => {
				let took = b.slice(&device, &queue);
				b.steer(took, target);
				busy_time += took;
				last_slice = took;
				if opts.busy < 1.0 {
					let rest = took.as_secs_f64() * (1.0 - opts.busy) / opts.busy;
					std::thread::sleep(Duration::from_secs_f64(rest.min(5.0)));
				}
			}
			None => std::thread::sleep(Duration::from_millis(100)),
		}
		if report.elapsed() >= Duration::from_secs(2) {
			let share = busy_time.as_secs_f64() / window_start.elapsed().as_secs_f64();
			say(&format!(
				"holding {held_mb} MB in {} chunks; busy {:.0}%, slice {:.1} ms, {} iters",
				held.len(),
				share * 100.0,
				last_slice.as_secs_f64() * 1000.0,
				burner.as_ref().map_or(0, |b| b.iters)
			));
			report = Instant::now();
			busy_time = Duration::ZERO;
			window_start = Instant::now();
		}
	};
	drop(burner);
	drop(held);
	let _ = device.poll(wgpu::PollType::wait_indefinitely());
	say(&format!(
		"stopped ({why}) after {:.0} s",
		started.elapsed().as_secs_f64()
	));
}
