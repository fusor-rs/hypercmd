# Keyboard and controls

`Controller` translates host-normalized `Input` into generated listeners. It
owns focus and the last presented geometry. Call `decorate` before output and
`presented` after output succeeds.

| Key | Behavior |
| --- | --- |
| Tab / Shift+Tab | Wrap through enabled visible controls in document order |
| Up / Down | Focus the nearest control above/below, without wrapping |
| Enter / Space | Activate a button once per reported press |
| Space on checkbox | Update the model before authored change handlers |
| Left / Right, Home / End | Move the text cursor; Shift selects |
| Backspace / Delete | Remove a complete grapheme or selection |
| PageUp / PageDown | Scroll the nearest containing viewport |
| Ctrl+C | Request native shutdown |
| Ctrl+Z | Request native Unix suspend |

Vertical navigation compares top-left positions, then row distance, column
distance and document order. It skips controls on the same row; Tab reaches
adjacent toolbar controls. Scrollable offscreen controls scroll into view.
Releases do not move focus; reported repeats do. Distinguishable repeat/release
reports do not activate buttons or checkboxes.

Read-only text remains focusable. `tabindex="-1"` requires explicit focus.
Labels resolve inputs within their component instance, across structural
branches. After removal, focus moves to the next surviving control, then the
preceding control; with none available, focus clears. Keyed reorder retains
focus, draft, cursor and selection. Pointer actions use the last presented
geometry and reject disposed targets.

Model writes do not synthesize input/change. Bindings precede authored listeners,
and each listener runs in one reactive batch. Handlers may return `()` or
`Result<(), Error>`; returned errors enter the scene's diagnostic window.
Events are `click` on buttons, `input` on text inputs, `change` on checkboxes,
and `focus`/`blur` on inputs. They have no bubbling or browser payload.

See [native input](native.md) for paste normalization, byte limits and legacy
terminal behavior, and [the profile](profile.md) for Unicode policy. The
[consumer contracts](../tests/consumer/src/lib.rs) exercise generated controls,
focus and editing.
