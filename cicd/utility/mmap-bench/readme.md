<!-- markdownlint-disable MD041 -- First line in a file should be a top-level heading -->

# Minimap throughput rig

This measures how much of the terminal's throughput the minimap column costs under a flood. It floods a screen-sized terminal with 32 MiB and times the shell's own `cat`, once with the column on and once with it off.

~~~sh
CICD_HEADLESS_DISPLAY=:98 cicd/utility/gui-headless.bash start
cicd/utility/mmap-bench/run.bash target/release/silkterm off base
cicd/utility/mmap-bench/run.bash target/release/silkterm on  base
~~~

Use an optimized binary. A debug build measures the debug build, not the change.

The 2 config files pin `performance.profile: custom` and turn every other effect off, so the only difference between the runs is the column. They differ in one line.

Scratch files go to `target/mmap-bench/`: the 32 MiB flood file, the generated scene, and one log per run. None of it is tracked, and the flood file is made on the first run.

Measured on 2026-09-19 at 48x160, after the compose throttle went in: 53.0 MiB/s with the column on against 56.9 off, from 30.5 against 57.2 before it.
