# Native execution

The native runner runs a Hypercmd application in a terminal on macOS and Linux. It sets up the
terminal session, decodes input, schedules local tasks and restores the terminal on exit.
See [application startup](../README.md#author-an-application) for mounting and running a
root. Disabling the `native` feature keeps scene, controls and layout without OS I/O.

## Resource limits

`run(scope)` uses the default options. Use `run_with(scope, &NativeOptions)` to
configure the resource bounds the runner enforces:

- `max_paste_bytes`: 64 KiB by default, enforced while raw paste bytes arrive.
- `max_edit_bytes`: 1 MiB by default, enforced by the control editor.
- `layout`: stylesheet overrides and renderable-content limits in `LayoutOptions`.

Zero input limits fail before terminal setup. The input decoder holds at most 64 bytes of
an unfinished escape sequence, four bytes of an unfinished UTF-8 character, or the paste
limit plus six terminator bytes. An overflow is a visible error returned after
restoration; no partial paste or remainder executes as keys. Invalid UTF-8 keystrokes
return an error; malformed bytes inside bracketed paste become replacement characters.
[Local services](services.md) provide separate task, timer and worker result bounds.

## Input and focus

See [the key table](controls.md) for focus, editing and activation. Escape is delivered
to application key handlers; it has no default Back action.

### Paste

Bracketed paste is enabled for the session: the terminal marks pasted text so the
application can tell it from typing. A recognized paste produces one edit; CRLF and other
line breaks become spaces in single-line inputs. Textareas normalize CRLF/CR to newlines
and preserve tabs and line breaks. Embedded Ctrl+C, Enter and escape sequences are stored
as replacement characters or spaces, so they cannot execute shortcuts.

Terminals that don't report bracketed paste fall back to ordinary key handling. In that
case an unmarked paste can't be reliably distinguished from typing and can run shortcuts.

### Keys and escape sequences

An incomplete legacy escape sequence expires after 40 ms. Both normal (CSI) and
application (SS3) cursor-key sequences are accepted. A bare Escape produces a key event
when the sequence deadline expires.

The session requests the
[Kitty keyboard protocol's disambiguation mode](https://sw.kovidgoyal.net/kitty/keyboard-protocol/)
and restores the previous mode on exit or suspend. Terminals that support it can report
Command/Super combinations independently of ordinary keys. A shortcut reserved by the
terminal or OS must be remapped there before the application can receive it. Reported
CSI-u modifiers and repeat/release events are decoded; button activation ignores
repeats/releases. Legacy repeats may look like separate presses.

### Mouse

SGR mouse reporting is enabled for pointer focus, button activation, wheel scrolling and
Shift+wheel horizontal scrolling. Holding Shift for terminal text selection depends on the
emulator.

## Color and attributes

The visible focus marker doesn't depend on color. Foreground/background colors are
disabled whenever `NO_COLOR` is set, including an empty value, or when neither
`TERM` nor `COLORTERM` indicates supported color capabilities. `TERM` containing `256color`
enables named/indexed colors. RGB colors use true color when `COLORTERM` is `truecolor`
or `24bit`; in a 256-color terminal they map to the nearest color in the
[xterm color cube or grayscale ramp](https://github.com/ThomasDickey/xterm-snapshots/blob/master/256colres.h).
The first 16, theme-dependent palette entries are excluded from that mapping.

Font/attribute rendering ultimately depends on the emulator. See the
[HTML and CSS reference](profile.md) for Unicode policy.

## Terminal lifetime and scheduling

### Requirements

Interactive execution requires both stdin and stdout to be TTYs, and rejects
`TERM=dumb`; redirected output receives no screen-control sequences. If your application
also produces machine-readable output, implement that as a separate mode that does not start
the terminal runner.

### Scheduling

On Unix, Mio (a Rust library for waiting on I/O events) waits for input, a signal wake
socket or a task notification. At most 256 input bytes are processed before checking
shutdown, resize and painting again. A settled application submits no frames and blocks
without periodic polling.

Escape resolution and service timers use monotonic deadlines. Local futures run only
after admission or a wake; pending futures do not keep the loop polling. Native readiness
is checked between bounded task turns, including self-waking tasks. See
[local services](services.md) for what futures and wakes mean here.

The runner uses one native input owner, a bounded VT decoder, Crossterm (a Rust terminal
library) for mode/output operations and Ratatui (a Rust terminal UI library) for buffer
diffing. It does not call Crossterm's event reader.

### Signals

In this section, signals are Unix process signals, not fusor's reactive signals. SIGINT,
SIGTERM and SIGHUP request orderly shutdown. SIGTSTP/Ctrl+Z restores the terminal, stops
the process, then re-enters and redraws after SIGCONT. Signal handlers only set atomic
flags and wake a socket through `signal-hook`; UI work stays on the UI thread. Modes and
callback registrations are released at exit.

The runner owns process signal handling. The upstream `signal-hook` registry removes
callbacks but does not restore an earlier/default OS signal disposition (what the OS does
when a signal arrives) after its last registration is removed. Use `run` as the terminal
application lifetime; a long-lived host that continues after it returns must establish
its own subsequent signal policy. Arbitrary embedding and preservation of another
library's signal-disposition ownership are not supported by this adapter.

### Restoration and errors

A session guard restores raw mode, alternate screen, cursor visibility and
bracketed-paste, mouse and keyboard modes, and closes hyperlinks on ordinary exit, I/O
errors, partial setup failure and UI panic unwinding. A Rust panic interrupts normal
execution; unwinding walks back through the call stack and runs cleanup. UI panic text
is delayed until restoration, then the panic resumes. Managed worker panics become service
errors through
their result future.

Recoverable reactive errors preserve the committed subtree, occupy a reserved status row
and are printed with their error kind after restoration. The diagnostic history keeps the
newest 64 entries and visibly reports how many earlier errors were omitted. Input/I/O
integrity errors terminate interaction. SIGKILL, process abort, power loss, or a
disconnected terminal cannot carry a restoration guarantee.

## Verification

The [native PTY test](../crates/hypercmd/src/native/unix/tests.rs) checks session
restoration, input, signals, diagnostics and wake-driven services. (A PTY is a
pseudo-terminal, which lets a test run the application as if in a real terminal.) See the
[platform table](../README.md#platform-status) for coverage.
