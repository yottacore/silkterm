# GPU load rig

Keeps the GPU busy, full, or both, so a terminal window can be watched under load. It is its own crate, outside the workspace, and builds into `target/gpu-stress/`.

~~~sh
cd cicd/utility/gpu-stress
cargo build --release --target x86_64-pc-windows-gnu
gpu-stress --vram-mb 4600 --touch --busy 1 --slice-ms 40 --secs 300 --stop-file stop.txt
~~~

`--vram-mb` takes memory in `--chunk-mb` pieces until it has that much or the driver refuses one. `--vram-pct` takes pieces until wgpu refuses past that share of the driver's reported budget. `--touch` clears every piece once a second so it stays in video memory. `--busy` is the share of time a compute shader keeps the GPU working, from 0 to 1, in dispatches of about `--slice-ms`. It stops after `--secs` or once the stop file exists, and prints what it holds every two seconds.

The wingui scenarios `gpuload-off` and `gpuload-on` run it beside a terminal on a Windows box. Send it along with `WINGUI_EXTRA`, with a `build-tag.txt` naming the build:

~~~sh
WINGUI_EXE=<silkterm.exe> WINGUI_EXTRA="target/gpu-stress/x86_64-pc-windows-gnu/release/gpu-stress.exe <dir>/build-tag.txt" \
	cicd/tests/wingui/run.bash --host <box> gpuload-off gpuload-on
~~~

A `gpuload-args.txt` sent the same way replaces the load's arguments, and a `gpuload-config.txt` adds lines to the terminal's config, with tabs written as `\t`.

Linux under a private Xvfb draws in software, so the load does nothing to it there.
