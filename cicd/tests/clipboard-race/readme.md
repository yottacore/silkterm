<!-- markdownlint-disable MD041 -- First line in a file should be a top-level heading -->

# Clipboard race reproduction

This reproduces the x11-clipboard 0.9.3 defect behind the recurring copy bug. A `SelectionClear` left over from an earlier hand-over arrives after the next `store` has taken the selection back, and the crate drops the stored value anyway. SilkTerm then owns the selection with nothing behind it, so a program that asks to paste doesn't get any reply at all, and waits out its own timeout.

Run it by hand against any display. It isn't part of the pipeline, because it needs a display, takes about 15 seconds, and what it checks is a dependency rather than SilkTerm.

~~~sh
cargo build
DISPLAY=:98 ./target/debug/clipracer 300
~~~

It builds against the stock crate from crates.io on purpose, so it still shows the defect. The fix itself is pinned by `a_stale_selection_clear_does_not_wipe_a_newer_store` in the fork.

It stores, lets a second connection in the same process take the selection, takes it straight back, and asks `xclip -o` for the text. A round where xclip comes back empty after its timeout is the defect.

Measured on 2026-09-20, Xvfb on `:98`: 19 broken rounds out of 900 on the stock crate, 0 out of 900 with the fix.

The fix is to delete the `setmap.remove` from the `SelectionClear` arm in the crate's `run.rs`, but keep the INCR cleanup beside it. The value is never served while another program owns the selection, and the next store overwrites it.

Don't try to fix it by asking the server who owns the selection now. The crate's event loop blocks on `poll(fd, -1)`, so a request/reply round trip there drains the socket, leaves a pending event in x11rb's own queue with the fd no longer readable, and the thread never wakes again. That version didn't reply to any requests at all.
