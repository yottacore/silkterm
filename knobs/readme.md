<!-- markdownlint-disable MD041 -- First line in a file should be a top-level heading -->

# knobs

Settings declared in a shcl file, and the rules for what a change to one does to the others. Nothing here draws a screen. The program draws the controls, and asks knobs what each one shows and what a change does.

## What's in it

- `src/lib.rs`: the spec parser, the rules, and the config and state files.

- `demo/demo.shcl`: an example spec. Its header documents the format.

- `demo/`: a small egui app with nothing but a settings dialog, for trying a spec out.

## Running the demo

~~~sh
cd knobs/demo
cargo run -- --spec demo.shcl --dir /tmp/knobs-demo
~~~

- With `--spec`, the demo rereads the file whenever it changes, so a spec can be edited while the demo runs. A spec with mistakes lists them at the top and keeps the last good one.

- `--dir` is where `config.shcl` and `state.shcl` are written, after every change.

- The side panel stands in for the desktop and the machine test. It also shows both files and logs what each change did to every setting.

The demo is kept out of SilkTerm's workspace, so egui never becomes part of SilkTerm's build.
