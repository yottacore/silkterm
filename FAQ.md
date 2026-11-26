<!-- markdownlint-disable MD007 -- Unordered list indentation -->
<!-- markdownlint-disable MD010 -- No hard tabs -->
<!-- markdownlint-disable MD033 -- No inline html -->
<!-- markdownlint-disable MD055 -- Table pipe style [Expected: leading_and_trailing; Actual: leading_only; Missing trailing pipe] -->
<!-- markdownlint-disable MD041 -- First line in a file should be a top-level heading -->

<!-- TOC ignore:true -->
# FAQ

<!-- TOC ignore:true -->
## Table of contents

<!-- TOC -->

- [The binary is 10+ MiB. Isn't that huge for a terminal?](#the-binary-is-10-mib-isnt-that-huge-for-a-terminal)
	- [Static vs dynamic linking](#static-vs-dynamic-linking)
	- [An "effective" comparison](#an-effective-comparison)
	- [Why go static at all?](#why-go-static-at-all)

<!-- /TOC -->

## The binary is 10+ MiB. Isn't that huge for a terminal?

In short, SilkTerm is a self-contained program, with everything it needs *statically linked* inside it. It can't be compared against a thin launcher that depends on 10+ MiB of shared libraries that are *dynamically linked at runtime*. Count what has to be present for each one to run, and SilkTerm comes out about the same size, and often smaller.

### Static vs dynamic linking

Most GTK terminals (`xfce4-terminal`, `gnome-terminal`, `terminator`, ...) are small *executables* but not small *programs*. The binary on disk is mostly glue code. The real work is done in shared libraries that come with the desktop, and those don't count toward the binary's size:

- **VTE**: the terminal widget, which is the emulator itself.

- **GTK**, **GLib/GObject**: the toolkit.

- **Pango**, **HarfBuzz**, **FreeType**, **fontconfig**: text shaping and glyph rendering.

- **Cairo**, **pixman**: drawing.

`terminator` goes a step further. It's written in Python, so its "binary" is a launcher script, and the real cost is the Python interpreter plus everything above.

SilkTerm statically links nearly its entire stack into one file. That stack is heavier by design, because SilkTerm is a GPU terminal rather than a GTK widget terminal:

- **wgpu / naga**: the GPU abstraction and WGSL shader compiler. This is the biggest part.

- **cosmic-text / swash / rustybuzz / ttf-parser**: SilkTerm's own text shaping and rasterization, used in place of Pango, HarfBuzz, and FreeType.

- **alacritty_terminal**: the grid, PTY, and escape-sequence engine.

- **winit / glutin**: windowing, plus the GL and X11 layers.

The OS doesn't supply any of that the way it supplies GTK and VTE, so all of it is built into the one binary. In exchange, there's a single file that runs without a toolkit to install, and without anything to keep in version sync.

### An "effective" comparison

The fair question is "how much code has to be present for this thing to run at all?" You can measure it yourself:

~~~bash
ls -l "$(command -v xfce4-terminal)"    # the binary itself
ldd "$(command -v xfce4-terminal)" | awk '/=>/ {print $3}' | sort -u | xargs -r du -bc | tail -1    # plus every shared library it drags in
~~~

On a typical desktop the shared-library side of a GTK terminal adds up to well over 10 MiB for VTE, GTK, Pango, Cairo, and GLib together, and that's before the Python interpreter for `terminator`. So the effective footprint of a GTK terminal is about the same as SilkTerm's, and often larger. The difference is that SilkTerm has all of it in one file, and there isn't anything to install first.

For reference, SilkTerm's release binary is about **11 MiB** (Linux x86_64), already built with fat LTO, `panic = "abort"`, and symbol stripping. Without the size-tuned release profile, a default `cargo build --release` of the same code comes out noticeably larger.

### Why go static at all?

- It's one file. There isn't a runtime to install, or 40 packages to install first.

- Nothing breaks when the distro updates GTK or VTE.

- It isn't tied to one display server either. The same Linux binary runs natively on both X11 and Wayland, which is picked at runtime, so it keeps working through the slow move from one to the other.

- Windows is the same. The build imports only core OS DLLs and doesn't need a redistributable, and there isn't a system VTE or GTK to depend on there anyway.

- Much more resistant to bitrot over time. If you neglect to update SilkTerm, it will still run for many years into the future, even as your distro library requirements evolve out from underneath it in a way that leaves other GUI programs stranded with no viable upgrade path.

So the file is bigger on disk than a glue binary, because it includes everything, but the program as a whole isn't.

