# Terminal profile v1

`hypercmd profile` prints the element and CSS property/value table used by
validation (`hypercmd_build::profile`). Unsupported markup, control bindings,
properties, values and selector syntax fail explicitly. Rust expressions are
lowered by fusor, not rewritten by the terminal compiler.

Containers are `main`, `section`, `div`, `ul`, `li`, `p`, `h1`–`h3`, `pre` and
`label`. Inline text supports `span`, `strong`, `em`, `br`, and `a href="…"`. Controls are
`button`, `input type="text"` (also the default type) and
`input type="checkbox"`, and `textarea`. Tables use `table`, `thead`, `tbody`, `tr`,
`th`, and `td`. See [the control reference](controls.md) for bindings and events;
static attributes are validated in [backend.rs](../crates/hypercmd-build/src/backend.rs). Document shells, browser layout repair,
media, SVG, canvas, scripts, styles in HTML, hydration and JavaScript
are rejected. Put stylesheets in package metadata.

## Stylesheets and delivery

Declare styles in the [application metadata](../README.md#author-an-application).

Stylesheets are parsed during the package's build, in their declared order, and
embedded in its generated Rust under `OUT_DIR/fusor_backends/hypercmd/`.
Applications never read library source files at runtime. Include HTML, CSS and
`build.rs` in a library's package archive. Each generated file stores the package stylesheet once;
components reference it and selectors apply only to its nodes. Text color, emphasis
and whitespace can inherit through component boundaries.

Rules support a tag, `.class`, `#id`, `:focus` or `:disabled`, and compounds such
as `button.action:focus`. Comma-separated selector lists are accepted.
Specificity compares ID count, then class/pseudo count, then tag count; ties
follow source order. Whitespace combinators, attribute selectors, universal
selectors, `!important`, variables, imports, URLs and CSS functions are rejected.

Application overrides are explicit: set `LayoutOptions.overrides` to a compiled
stylesheet. An authored App exports `hypercmd_styles()` alongside
`hypercmd_app()` for this purpose. Overrides apply after local rules regardless
of local specificity. Merely mounting a library does not change another
component's cascade order.

## Exact CSS values

The validation-backed `hypercmd profile` table is authoritative:

ANSI names are `black`, `red`, `green`, `yellow`, `blue`, `magenta`, `cyan`,
`white` and their `bright-` variants. `gray` aliases `white`. Font families,
margins, positioning, grids and animations are outside this profile.
`border-style: solid | rounded | none` draws a one-cell border, and `border-color`
accepts the same colors as text. Borders occupy layout space. Bordered automatic
scroll panes show a vertical scroll-position indicator on their right edge.

One `ch` is one terminal cell, including one row vertically. Percentages
require a definite parent axis: the viewport, an explicit resolved size, a
stretched cross axis, or a growing item in a definite flex main axis. A
percentage against an intrinsic/indefinite parent size fails before presentation;
it does not silently become `auto`. As in the CSS sizing used by Taffy, percentage
padding on all four sides uses the parent's width. `px`, `em`, viewport and
physical units are rejected.

Containers default to column layout; labels and table rows default to rows.
Table cells default to 16×1 cells with clipped, unwrapped content; headers are
bold. Set matching widths on `th` and `td`, or use equal flex growth for columns
that share the available width. There is no browser intrinsic column sizing or
cell spanning. Tables use ordinary retained nodes; applications can use the
content-box `resize` event to render only the visible rows and columns. Flex items default
to `flex-shrink: 0` so content remains scrollable instead of disappearing under
vertical pressure. Buttons have one cell of horizontal padding and bold labels
when enabled; authored CSS can override both. Controls inherit terminal colors
instead of assuming a dark background. Inputs default to 16×1 cells; checkboxes
to 3×1. Textareas default to three rows, preserve newlines and tabs, and scroll
the draft to keep the caret visible. Headings and
`strong` are bold, and `em` is italic. Inline runs accept text styling and
`display:none`; geometry, flex and overflow declarations on inline runs fail
with a recommendation to style a surrounding container.

Anchors carry the complete `href` independently of the visible label. Native
output uses [OSC 8 hyperlinks](https://iterm2.com/feature-reporting/Hyperlinks_in_Terminal_Emulators.html);
activation follows the terminal's link gesture. Clipping does not shorten the
destination. Empty destinations or destinations containing control characters or
whitespace render as ordinary text. Hyperlinks inherit through nested inline
styling, and a reactive destination change updates the link even when its label
stays the same. Hosts using `layout::render` receive destinations in
`Presentation::hyperlinks`, indexed by each visible glyph's starting cell.

## Text, clipping and limits

`normal` collapses whitespace across adjacent inline runs and removes leading
and trailing whitespace. Indentation-only container text produces no row.
`pre` preserves spaces, tabs and newlines without wrapping. `pre-wrap` preserves
them and wraps. Wrapping occurs on extended grapheme boundaries, including across
interpolations and inline elements; a joined cluster uses the style and whitespace
policy where it starts. Tabs advance
to a four-cell stop; `br` starts a new line. Measurement and painting use the
same normalized runs. Input values and placeholders preserve spaces, stay on one
line regardless of inherited whitespace rules, and clip to the input's content
box. Button labels use authored whitespace rules; use `white-space: pre` and
`overflow: hidden` for single-row lists.

Text uses unicode-segmentation 1.12.0 and unicode-width 0.2.0's non-CJK width
policy (ambiguous characters use one cell). Combining clusters and emoji/ZWJ
clusters remain indivisible. A clipped wide glyph is omitted as a whole;
continuation cells are cleared on each complete frame. Isolated zero-width
clusters are omitted. Terminal emulators can disagree with this width policy;
use ordinary narrow characters when exact alignment across emulators matters.

ESC, DEL and other C0/C1 controls become the visible replacement character
U+FFFD before output. Only declared newline/tab handling is preserved. User text
cannot emit terminal control sequences. `LayoutOptions` bounds source text bytes,
visited nodes and viewport cells; exceeding a limit returns a visible error to
the runner. Structural regions count toward the visit budget. Nested layout
processing is capped at 256 levels. Zero-sized viewports produce empty buffers.

`overflow:auto` stores scroll offsets by stable node identity, clamps them after
resize/removal and lets keyboard focus reveal a control. Painting clips against
the viewport and every clipping ancestor. Hit testing uses the last presented
layout. Full dirty frames use Ratatui's buffer diff; settled state submits no
new frames. Buttons and single-line inputs use reverse video for focus.
Textareas show an underlined caret; focusable containers use authored `:focus`
styles, such as changing a rounded border to a solid border.
Controls entirely behind hidden overflow or the root viewport are excluded from
focus unless scrolling an automatic viewport can reveal them. Manual scrolling
does not reset focus visibility; resize and keyed geometry changes reveal the
focused control again.
Unsupported color/emphasis capabilities may degrade to terminal defaults;
keyboard correctness never depends on color or italic support. Emulator and
screen-reader testing status is tracked in the [platform table](../README.md#platform-status).
