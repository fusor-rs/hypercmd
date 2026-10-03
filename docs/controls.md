# Keyboard and controls

`Controller` translates host-normalized `Input` into generated listeners. It
owns focus and the last presented geometry. Call `decorate` before output and
`presented` after output succeeds.

| Key | Behavior |
| --- | --- |
| Tab / Shift+Tab | Wrap through enabled visible controls in document order |
| Up / Down | Move between textarea lines, scroll a focused pane, or focus the nearest control |
| Enter / Space | Activate a button once per reported press |
| Space on checkbox | Update the model before authored change handlers |
| Left / Right, Home / End | Move the text cursor; Shift selects |
| Backspace / Delete | Remove a complete grapheme or selection |
| Enter in textarea | Insert a newline |
| Ctrl+A in a text control | Select the whole draft |
| Ctrl+Home / Ctrl+End | Move to the start/end of the whole draft |
| PageUp / PageDown | Scroll the nearest containing viewport |
| Home / End on a pane | Scroll to its first/last line |
| Mouse wheel / Shift+wheel | Scroll vertically/horizontally under the pointer |
| Ctrl+C | Request native shutdown |
| Ctrl+Z | Request native Unix suspend |

Vertical navigation compares top-left positions, then row distance, column
distance and document order. It skips controls on the same row; Tab reaches
adjacent toolbar controls. Scrollable offscreen controls scroll into view.
Releases do not move focus; reported repeats do. Distinguishable repeat/release
reports do not activate buttons or checkboxes.

Any container can receive focus with `tabindex="0"`; `tabindex="-1"` requires
explicit focus. Read-only text remains focusable. `autofocus` selects a control
when it first appears in a presentation. `Node::request_focus()` takes precedence
over autofocus and applies immediately when that node has presented geometry.
`Node::scroll_to(column, row)` requests an offset for the next layout; content
bounds clamp it.
Labels resolve text controls within their component instance, across structural
branches. After removal, focus moves to the next surviving control, then the
preceding control; with none available, focus clears. Keyed reorder retains
focus, draft, cursor and selection. Pointer actions use the last presented
geometry and reject disposed targets.

Model writes do not synthesize input/change. Bindings precede authored listeners,
and each listener runs in one reactive batch. Handlers may return `()` or
`Result<(), Error>`; returned errors enter the scene's diagnostic window.
Direct events are `click` on buttons, `input` on text controls, `change` on
checkboxes, and `focus`/`blur`. `keydown` and `scroll` visit the focused or
hit-tested target followed by its presented ancestors. `event.prevent_default()`
stops ancestor handlers and the default gesture. Tab retains the controller's
focus traversal; key releases do not dispatch `keydown`.

`EventPayload::Input` carries the normalized `Input`, including `Modifiers`
(`shift`, `control`, `alt`, `super_key`) and vertical/horizontal scroll deltas.
`super_key` represents Command on macOS and Windows/Super elsewhere. Unhandled
Control/Alt/Super combinations do not insert text or activate controls. Ctrl+C and
Ctrl+Z remain native shutdown/suspend commands.

`on:resize` receives `EventPayload::Resize { width, height }` after presentation,
when the node's content-box size changes. It runs once on first presentation and
lets applications size a virtual result window without depending on the host.
`event.target.component_root().find(id)` resolves a control in the template
instance for explicit focus requests. No browser event object is involved.

See [native input](native.md) for paste normalization, byte limits and legacy
terminal behavior, and [the profile](profile.md) for Unicode policy. The
[consumer contracts](../tests/consumer/src/lib.rs) exercise generated controls,
focus and editing.
