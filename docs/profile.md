# HTML and CSS reference

Hypercmd accepts a fixed subset of HTML and CSS and lays it out in terminal cells. Anything
outside that subset is rejected with an error instead of being ignored. Run `hypercmd profile`
to print the same lists of elements and properties that are checked when you build.

## Supported HTML elements {#supported-elements}

| Elements | Purpose and terminal behavior |
| --- | --- |
| `main`, `section`, `div` | Generic containers. Children are laid out in a column by default. |
| `ul`, `li` | List container and list item. Laid out like other containers. |
| `p` | Paragraph of text. |
| `h1`, `h2`, `h3` | Headings. Rendered bold. |
| `pre` | Preformatted text. Keeps spaces, tabs and newlines and does not wrap. |
| `label` | Container for a control and its text. Children are laid out in a row by default. |
| `span` | Inline text run. Accepts text styling and `display: none` only. |
| `strong` | Inline text, bold. |
| `em` | Inline text, italic. |
| `br` | Starts a new line in text. |
| `a` | Inline link. Set its destination with `href`. |
| `button` | Clickable, focusable button. |
| `input` | Text input or checkbox: `type="text"` (the default) or `type="checkbox"`. |
| `textarea` | Multi-line text field. Three rows by default. |
| `table`, `thead`, `tbody` | Table structure. |
| `tr` | Table row. Children are laid out in a row by default. |
| `th`, `td` | Header cell (bold) and data cell. Both default to 16×1 cells, clipped and unwrapped. |

This table lists ordinary HTML elements. Fusor component and control-flow tags, such
as `Router`, `If` and `ForEach`, are compiled separately. Other ordinary HTML elements
are unsupported. This includes document shells such as `html`, `head` and `body`, as well as
media, SVG, canvas, scripts and `style` tags in HTML. Put stylesheets in the package metadata
instead, as described below. For control attributes, bindings and events, see the
[control reference](controls.md).

## Supported CSS properties {#exact-css-values}

Unsupported properties and values cause a build error. Font families, margins,
positioning, grids and animations are not supported.

| Property | Accepted values |
| --- | --- |
| `display` | `flex`, `none` |
| `flex-direction` | `row`, `column` |
| `flex-grow` | Finite number, 0 or greater |
| `flex-shrink` | Finite number, 0 or greater |
| `flex-basis` | `auto`, `0`, or a length |
| `width` | `auto`, `0`, or a length |
| `height` | `auto`, `0`, or a length |
| `min-width` | `auto`, `0`, or a length |
| `min-height` | `auto`, `0`, or a length |
| `max-width` | `auto`, `0`, or a length |
| `max-height` | `auto`, `0`, or a length |
| `gap` | One or two values, each `0` or a length |
| `row-gap` | `0` or a length |
| `column-gap` | `0` or a length |
| `padding` | One to four values, each `0` or a length |
| `padding-top` | `0` or a length |
| `padding-right` | `0` or a length |
| `padding-bottom` | `0` or a length |
| `padding-left` | `0` or a length |
| `align-items` | `flex-start`, `center`, `flex-end`, `stretch` |
| `align-self` | `auto`, `flex-start`, `center`, `flex-end`, `stretch` |
| `justify-content` | `flex-start`, `center`, `flex-end`, `space-between`, `space-around`, `space-evenly` |
| `overflow` | `visible`, `hidden`, `auto` |
| `white-space` | `normal`, `pre`, `pre-wrap` |
| `color` | A color |
| `background-color` | A color |
| `font-weight` | `bold`, `normal` |
| `font-style` | `italic`, `normal` |
| `text-decoration` | `underline`, `none` |
| `border-style` | `none`, `solid`, `rounded` |
| `border-color` | A color |

### Lengths

A length is a non-negative number followed by `ch` (for example `20ch`) or `%` (for example
`50%`). A bare `0` is also accepted. Percentages need a parent with a definite size; see
[units and percentages](#units-and-percentages). Units such as `px` and `em` are rejected.

### Colors

A color is one of:

- `default`, which uses the terminal's own color.
- An ANSI color name: `black`, `red`, `green`, `yellow`, `blue`, `magenta`, `cyan` or `white`,
  or the `bright-` version of any of these, such as `bright-red`.
- A hex value in the form `#rrggbb`, such as `#ff8800`.

`gray` is accepted as another name for `white`.

## Stylesheets and delivery

Declare styles in the [package metadata](../README.md#author-an-application).

Stylesheets are parsed during the package's build, in their declared order, and embedded
in its generated Rust under `OUT_DIR/fusor_backends/hypercmd/`. (`OUT_DIR` is the
directory Cargo gives a build script for generated files.) Applications never read
library source files at runtime. Include HTML, CSS and `build.rs` in a library's package
archive. Each generated file stores the package stylesheet once; components reference it
and selectors apply only to nodes from that package. Text color, emphasis and whitespace can
inherit through component boundaries.

### Selectors and specificity

Rules support a tag, `.class`, `#id`, `:focus` or `:disabled`, and compounds such as
`button.action:focus`. Comma-separated selector lists are accepted. Specificity compares
ID count, then class/pseudo count, then tag count; ties follow source order. Whitespace
combinators, attribute selectors, universal selectors, `!important`, variables, imports,
URLs and CSS functions are rejected.

### Application overrides

Application overrides are explicit: set `LayoutOptions.overrides` to a compiled
stylesheet. An authored App exports `hypercmd_styles()` alongside `hypercmd_app()` for
this purpose. Overrides apply after local rules regardless of local specificity. Merely
mounting a library does not change another component's cascade order.

## Layout and styling

The following rules describe how the supported properties behave in terminal cells.

### Colors and borders

`border-style` takes `solid`, `rounded`
or `none`. `solid` and `rounded` draw a one-cell border; `none` removes it.
`border-color` accepts the same colors as text.
Borders occupy layout space. Bordered automatic scroll panes show a vertical
scroll-position indicator on their right edge.

### Units and percentages

One `ch` is one terminal cell, including one row vertically. Percentages require a
definite parent axis: the viewport, an explicit resolved size, a stretched cross axis, or
a growing item in a definite flex main axis. A percentage against an intrinsic/indefinite
parent size fails before presentation; it does not silently become `auto`. Percentage
padding on all four sides uses the parent's width. `px`,
`em`, viewport and physical units are rejected.

### Layout and element defaults

Containers default to column layout; labels and table rows default to rows. Flex items
default to `flex-shrink: 0` so content remains scrollable instead of disappearing under
vertical pressure. Buttons have one cell of horizontal padding and bold labels when
enabled; authored CSS can override both. Controls inherit terminal colors instead of
assuming a dark background. Inputs default to 16×1 cells; checkboxes to 3×1. Textareas
default to three rows, preserve newlines and tabs, and scroll the draft to keep the caret
visible.

Headings and `strong` are bold, and `em` is italic. Inline runs accept text styling and
`display: none`; geometry, flex and overflow declarations on inline runs fail with a
recommendation to style a surrounding container.

### Tables

Table cells default to 16×1 terminal cells with clipped, unwrapped content; headers are
bold. Set matching widths on `th` and `td`, or use equal flex growth for columns that
share the available width. There is no browser-style intrinsic column sizing or cell
spanning. Tables use ordinary retained nodes; applications can use the content-box
`resize` event to render only the visible rows and columns.

## Hyperlinks

Anchors carry the complete `href` independently of the visible label, and clipping a
label does not shorten the destination. Native output uses
[OSC 8 hyperlinks](https://iterm2.com/feature-reporting/Hyperlinks_in_Terminal_Emulators.html).
A normal left click opens HTTP(S) destinations with `open` on macOS or `xdg-open` on
other Unix systems, without requiring OSC 8 support. Opener failures appear in the
diagnostic bar. Modifier-click gestures belong to the terminal emulator.

Use a non-URL label for shortened destinations: terminal auto-detection can otherwise
also open the visible text as a URL. Empty destinations or destinations containing
control characters or whitespace render as ordinary text. Hyperlinks inherit through
nested inline styling, and a reactive destination change updates the link even when its
label stays the same. Hosts using `layout::render` receive destinations in
`Presentation::hyperlinks`, indexed by each visible glyph's starting cell.

## Text, clipping and limits

### Whitespace and wrapping

With `white-space`, `normal` collapses whitespace across adjacent inline runs and removes
leading and trailing whitespace. Indentation-only container text produces no row. `pre`
preserves spaces, tabs and newlines without wrapping. `pre-wrap` preserves them and
wraps. Wrapping occurs on extended grapheme boundaries, including across interpolations
and inline elements; a joined cluster uses the style and whitespace policy where it
starts. Tabs advance to a four-cell stop; `br` starts a new line. Measurement and
painting use the same normalized runs.

Input values and placeholders preserve spaces, stay on one line regardless of inherited
whitespace rules, and clip to the input's content box. Button labels use authored
whitespace rules; use `white-space: pre` and `overflow: hidden` for single-row lists.

### Unicode width

Text uses unicode-segmentation 1.12.0 and unicode-width 0.2.0's non-CJK width policy
(ambiguous characters use one cell). Combining clusters and emoji/ZWJ clusters remain
indivisible. A clipped wide glyph is omitted as a whole; continuation cells are cleared on
each complete frame. Isolated zero-width clusters are omitted. Terminal emulators can
disagree with this width policy; use ordinary narrow characters when exact alignment
across emulators matters.

### Control characters and limits

ESC, DEL and other C0/C1 controls become the visible replacement character U+FFFD before
output. Only declared newline/tab handling is preserved. User text cannot emit terminal
control sequences. `LayoutOptions` bounds source text bytes, visited nodes and viewport
cells; exceeding a limit returns a visible error to the runner. Structural regions count
toward the visit budget. Nested layout processing is capped at 256 levels. Zero-sized
viewports produce empty buffers.

### Scrolling, focus and painting

`overflow: auto` stores scroll offsets by stable node identity, clamps them after
resize/removal and lets keyboard focus reveal a control. Painting clips against the
viewport and every clipping ancestor. Hit testing uses the last presented layout. Full
dirty frames use Ratatui's buffer diff; settled state submits no new frames.

Buttons and single-line inputs use reverse video for focus unless a matching authored
`:focus` rule supplies their appearance, including application overrides. This keeps
borders and inherited colors under the stylesheet's control. Textareas show an underlined
caret; focusable containers use authored `:focus` styles, such as changing a rounded
border to a solid border.

Controls entirely behind hidden overflow or the root viewport are excluded from focus
unless scrolling an automatic viewport can reveal them. Manual scrolling does not reset
focus visibility; resize and keyed geometry changes reveal the focused control again.
Unsupported color/emphasis capabilities may degrade to terminal defaults; keyboard
correctness never depends on color or italic support. Emulator and screen-reader testing
status is tracked in the [platform table](../README.md#platform-status).
