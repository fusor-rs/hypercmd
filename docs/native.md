# Native execution

See [application startup](../README.md#author-an-application) for mounting and
running a root. Disabling `native` keeps scene, controls and layout without OS I/O.

`run_with(scope, &NativeOptions)` configures the required resource bounds:

- `max_paste_bytes`: 64 KiB by default, enforced while raw paste bytes arrive.
- `max_edit_bytes`: 1 MiB by default, enforced by the control editor.
- `layout`: stylesheet overrides and renderable-content limits in `LayoutOptions`.

Zero input limits fail before terminal setup. The decoder holds at most 64 bytes
of an unfinished escape sequence, four bytes of an unfinished UTF-8 character,
or the paste limit plus six terminator bytes. An overflow is a visible error
returned after restoration; no partial paste or remainder executes as keys.
Invalid UTF-8 keystrokes return an error; malformed bytes inside bracketed paste
become replacement characters. [Local services](services.md) provide separate task, timer and worker
result bounds.

## Input and focus

See [the key table](controls.md) for focus, editing and activation. Escape is delivered to application key handlers; it has no default Back action.

Bracketed paste is enabled for the session. A recognized paste produces one
edit; CRLF and other line breaks become spaces in single-line inputs. Textareas
normalize CRLF/CR to newlines and preserve tabs and line breaks. Embedded
Ctrl+C, Enter and escape sequences are stored as replacement characters
or spaces, so they cannot execute shortcuts. Terminals that do not report
bracketed paste fall back to ordinary key handling: unmarked paste cannot be
reliably distinguished from typing and can run shortcuts. An incomplete legacy
escape sequence expires after 40 ms. Both normal (CSI) and application (SS3)
cursor-key sequences are accepted. A bare Escape produces a key event when the
sequence deadline expires. SGR mouse reporting is enabled for pointer focus,
button activation, wheel scrolling and Shift+wheel horizontal scrolling; holding
Shift for terminal text selection depends on the emulator. The session requests the
[Kitty keyboard protocol's disambiguation mode](https://sw.kovidgoyal.net/kitty/keyboard-protocol/)
and restores the previous mode on exit or suspend. Supporting terminals can
report Command/Super combinations independently of ordinary keys. A shortcut
reserved by the terminal or OS must be remapped there before the application can
receive it. Reported CSI-u modifiers and repeat/release events are decoded;
button activation ignores repeats/releases. Legacy repeats may look like
separate presses.

The visible focus marker does not depend on color. Foreground/background colors
are disabled for `NO_COLOR` or unknown terminal capabilities. `TERM` containing
`256color` enables named/indexed colors. RGB uses `COLORTERM=truecolor` or
`24bit`; in a 256-color terminal it maps to the nearest color in the
[xterm color cube or grayscale ramp](https://github.com/ThomasDickey/xterm-snapshots/blob/master/256colres.h).
The first 16, theme-dependent palette entries are excluded from that mapping. Font/attribute
rendering ultimately depends on the emulator. See [the profile](profile.md) for Unicode policy.

## Terminal lifetime and scheduling

Interactive execution requires both stdin and stdout to be TTYs, and rejects
`TERM=dumb`; redirected output receives no screen-control sequences. Application
machine output must use a separate explicit mode. The runner uses one native
input owner, a bounded VT decoder, Crossterm for mode/output operations and
Ratatui for buffer diffing. It does not call Crossterm's event reader.

On Unix, Mio waits for input, a signal wake socket or a task notification. At most 256 input bytes
are processed before checking shutdown, resize and painting again. A settled
application submits no frames and blocks without periodic polling. Escape
resolution and service timers use monotonic deadlines. Local futures run only
after admission or a wake; pending futures do not keep the loop polling. Native
readiness is checked between bounded task turns, including self-waking tasks.

SIGINT, SIGTERM and SIGHUP request orderly shutdown. SIGTSTP/Ctrl+Z restores the
terminal, stops the process, then re-enters and redraws after SIGCONT. Signal
handlers only set atomic flags and wake a socket through `signal-hook`; UI work
stays on the UI thread. Modes and callback registrations are released at exit.

The runner owns process signal handling. The upstream `signal-hook` registry
removes callbacks but does not restore an earlier/default OS signal disposition
after its last registration is removed. Use `run` as the terminal application
lifetime; a long-lived host that continues after it returns must establish its
own subsequent signal policy. Arbitrary embedding and preservation of another
library's signal-disposition ownership are not supported by this adapter.

A session guard restores raw mode, alternate screen, cursor visibility and
bracketed-paste, mouse and keyboard modes, and closes hyperlinks on ordinary exit,
I/O errors, partial setup failure and UI panic unwinding. UI panic text is delayed
until restoration, then the panic resumes. Managed worker panics become service errors through their result future. Recoverable reactive errors preserve the committed subtree, occupy a
reserved status row and are printed with their error kind after restoration;
the diagnostic history keeps the newest 64 entries and visibly reports how many
earlier errors were omitted. Input/I/O integrity errors
terminate interaction. SIGKILL, process abort, power loss, or a disconnected
terminal cannot carry a restoration guarantee.

## Verification

The [native PTY test](../crates/hypercmd/src/native/unix/tests.rs) checks session
restoration, input, signals, diagnostics and wake-driven services. See the
[platform table](../README.md#platform-status) for coverage.
