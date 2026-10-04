# Keyboard and controls

Hypercmd controls support keyboard navigation, text editing and mouse input. Bind them
to application state in HTML, and handle events with Rust expressions. The native runner
manages focus and input dispatch.

## Bindings and handlers

A signal holds a value and updates the interface when that value changes. Use `bind`
to connect a control to a signal, and `on:click` or `on:input` to handle an event.
These lines come from the [Panel component](../tests/consumer/ui/components.html):

```html
<button id="increment" on:click="state.count.update(|n| *n += 1)">{{ state.count.get() }}</button>
<input id="number" type="text" bind="state.number" on:input="state.record_input()">
<input id="member" type="checkbox" value="batch" bind="state.selection">
```

Here, `count` and `number` are numeric signals. The button increments `count`;
editing the input updates `number` before `record_input()` runs. `selection` is
`Signal<Vec<String>>`, a reactive list of strings. Checking the box adds
`"batch"` to that list; clearing it removes the value. The component's
[Rust state](../tests/consumer/src/lib.rs) shows the field declarations.

## Key bindings

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

A grapheme is one user-perceived character, which can span several Unicode code points.

Vertical navigation compares top-left positions, then row distance, column distance and
document order. It skips controls on the same row; Tab reaches adjacent toolbar
controls. Scrollable offscreen controls scroll into view.

Releases don't move focus; reported repeats do. When a terminal reports repeats or
releases distinguishably, they don't activate buttons or checkboxes.

## Focus and scrolling

Any container can receive focus with `tabindex="0"`; `tabindex="-1"` requires explicit
focus. Text controls with `readonly` remain focusable. `autofocus` selects a control when it first
appears in a presentation. `Node::request_focus()` takes precedence over `autofocus` and
applies immediately when that node has presented geometry.
`event.target.component_root().find(id)` resolves a control in the template instance for
explicit focus requests. Labels resolve text controls within their component instance,
across structural branches.

`Node::scroll_to(column, row)` requests an offset for the next layout; content bounds
clamp it.

After a focused control is removed, focus moves to the next surviving control, then the
preceding control; with none available, focus clears. Keyed reorder retains focus, draft,
cursor and selection. Pointer actions use the last presented geometry and reject
disposed targets.

## Events and handlers

Bindings precede authored listeners, and each listener runs in one reactive batch. Model
writes don't synthesize `input` or `change` events. Handlers may return `()` (nothing) or
`Result<(), Error>`; returned errors enter the scene's diagnostic window (see
[native execution](native.md)).

Direct events are `click` on buttons, `input` on text controls, `change` on checkboxes,
and `focus`/`blur`. `keydown` and `scroll` visit the focused or hit-tested target
followed by its presented ancestors. `event.prevent_default()` stops ancestor handlers
and the default gesture, including Tab focus traversal. Key releases do not dispatch
`keydown`. Text controls emit `select` when keyboard or pointer gestures change the
caret or selection without editing the draft.

### Editing from Rust

`Node::editor()` exposes the caret and selection as UTF-8 byte offsets at extended
grapheme boundaries. `Node::replace_range(range, text)` replaces that byte range,
clears the selection, and places the caret after the insertion before emitting
`input`. Invalid boundaries and edits exceeding the controller's configured byte
limit return an error without changing the draft. Read-only and disabled controls
ignore replacements. Mouse clicks place the caret using the presented content box
and retained editor scroll offsets.

### Input payloads

`EventPayload::Input` carries the normalized `Input`, including `Modifiers` (`shift`,
`control`, `alt`, `super_key`) and vertical/horizontal scroll deltas. `super_key`
represents Command on macOS and Windows/Super elsewhere. Unhandled Control/Alt/Super
combinations don't insert text or activate controls. Ctrl+C and Ctrl+Z remain native
shutdown/suspend commands.

### Resize events

`on:resize` receives `EventPayload::Resize { width, height }` after presentation, when
the node's content-box size changes. It runs once on first presentation and lets
applications size a virtual result window without depending on the host. The
[HTML and CSS reference](profile.md) describes using it to render only the visible
rows and columns of a table. No browser event object is involved.

## Driving the controller

`Controller` translates host-normalized `Input` into listeners and owns focus and
the last presented geometry. If you write a custom host, call `decorate` before
output and `presented` after output succeeds. The native runner does this for you.

## See also

See [native input](native.md) for paste normalization, byte limits and legacy terminal
behavior, and the [HTML and CSS reference](profile.md) for Unicode policy. The
[consumer tests](../tests/consumer/src/lib.rs) cover generated controls, focus and
editing.
