# File browser

A read-only browser for folders and UTF-8 text files. Start in the current folder
or pass a starting directory:

```sh
just demo
just demo "/path/with spaces"
```

Type in the filter to narrow filenames. Enter opens the focused entry; folders
have a trailing `/`. Parent opens the parent folder, Refresh reads it again,
and Back returns from a preview with the previous filter intact. Reload rereads
the file. See [keyboard controls](../../docs/controls.md) for navigation.
The browser does not modify files.

The interface uses your terminal's foreground/background, bold toolbar actions,
and reverse video for the focused control. It works without color; entry rows
do not all look selected. The filter shrinks on narrow terminals and the footer
stays outside the scrolling list. Paths wrap to at most three rows. Very long
paths and names can be clipped; the filesystem still receives the original path.

Listings include hidden files, sort directories first and keep native paths
intact, including names that cannot be displayed losslessly. Parent and entering a
folder clear the filter. Symbolic links and special files are displayed as
unsupported and cannot be selected. The starting directory is canonicalized.

The example bounds work: it sorts at most the first 1,000 entries
returned by the filesystem and previews at most 64 KiB, with visible truncation
messages. Filtering searches that loaded subset. Empty files have an explicit
message. NUL-containing or non-UTF-8 previews show an error; use Back to return
or Reload after the file changes. Preview text uses Hypercmd's safe terminal-text
policy and wraps to the viewport.

`main.rs` contains the three view types, and `ui/browser.html` contains their
templates. The root retains the folder and filter; each route owns its async
boundary and requests. `files.rs` performs blocking reads with owned data through
`Services::worker`. Disposal requests cancellation, and fusor rejects stale
results. The demo reuses the existing pinned rustix dependency to open previews
without following a final symlink or blocking on a FIFO, then validates the open
handle as a regular file. Cancellation is checked before and after the bounded preview read and between directory entries; an outstanding
filesystem syscall cannot be forcibly interrupted. See [platform coverage](../../README.md#platform-status).

The [filesystem tests](src/files.rs) and [PTY navigation test](tests/navigation.rs)
cover loading and the generated application; run `cargo test -p file-browser --locked`.
