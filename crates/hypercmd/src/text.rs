//! Unicode policy shared by text measurement, painting and editing.
use crate::style::WhiteSpace;
use ratatui::style::Style;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Replace terminal controls with U+FFFD; normalize CRLF/CR to one newline.
/// Single-line text replaces line breaks and tabs with spaces.
pub fn sanitize(text: &str, multiline: bool) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                result.push(if multiline { '\n' } else { ' ' });
            }
            '\n' | '\t' if !multiline => result.push(' '),
            '\n' | '\t' => result.push(character),
            character if character.is_control() => result.push('\u{fffd}'),
            _ => result.push(character),
        }
    }
    result
}

/// Unicode-width 0.2.0's non-CJK policy: ambiguous characters use one cell.
pub fn width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

#[derive(Clone)]
pub(crate) struct Run {
    pub text: String,
    pub style: Style,
    pub line_break: bool,
    pub whitespace: WhiteSpace,
}

#[derive(Clone)]
pub(crate) struct Glyph {
    pub text: String,
    pub style: Style,
    pub width: usize,
}

pub(crate) fn lines(runs: &[Run], width: Option<usize>) -> Vec<Vec<Glyph>> {
    if width == Some(0) || runs.is_empty() {
        return Vec::new();
    }
    // Interpolation and inline styling can split a grapheme across text nodes.
    // The style and whitespace policy of its first byte apply to the whole cluster.
    let mut text = String::new();
    for run in runs {
        if run.line_break {
            text.push('\n');
        } else {
            text.push_str(&run.text);
        }
    }
    let run_len = |run: &Run| if run.line_break { 1 } else { run.text.len() };
    let mut run_index = 0;
    let mut run_end = run_len(&runs[0]);
    let mut lines = vec![Vec::new()];
    let mut column = 0;
    let mut pending_space = None;
    for (offset, cluster) in text.grapheme_indices(true) {
        while run_end <= offset {
            run_index += 1;
            run_end += run_len(&runs[run_index]);
        }
        let run = &runs[run_index];
        let whitespace = run.whitespace;
        if run.line_break {
            lines.push(Vec::new());
            column = 0;
            pending_space = None;
            continue;
        }
        if whitespace == WhiteSpace::Normal && cluster.chars().all(char::is_whitespace) {
            if column > 0 {
                pending_space = Some(run.style);
            }
            continue;
        }
        if let Some(style) = pending_space.take() {
            push(
                &mut lines,
                &mut column,
                " ",
                style,
                width,
                WhiteSpace::Normal,
            );
        }
        if cluster == "\n" {
            lines.push(Vec::new());
            column = 0;
        } else if cluster == "\t" {
            for _ in 0..(4 - column % 4) {
                push(&mut lines, &mut column, " ", run.style, width, whitespace);
            }
        } else {
            push(
                &mut lines,
                &mut column,
                cluster,
                run.style,
                width,
                whitespace,
            );
        }
    }
    if lines.len() == 1 && lines[0].is_empty() {
        Vec::new()
    } else {
        lines
    }
}

fn push(
    lines: &mut Vec<Vec<Glyph>>,
    column: &mut usize,
    cluster: &str,
    style: Style,
    limit: Option<usize>,
    whitespace: WhiteSpace,
) {
    let cells = width(cluster);
    if cells == 0 {
        return;
    }
    if whitespace != WhiteSpace::Pre
        && limit.is_some_and(|limit| *column > 0 && *column + cells > limit)
    {
        lines.push(Vec::new());
        *column = 0;
        if cluster == " " && whitespace == WhiteSpace::Normal {
            return;
        }
    }
    lines.last_mut().expect("a text line exists").push(Glyph {
        text: cluster.into(),
        style,
        width: cells,
    });
    *column += cells;
}

pub(crate) fn toggle_class(classes: &str, name: &str, on: bool) -> String {
    classes
        .split_whitespace()
        .filter(|class| *class != name)
        .chain(on.then_some(name))
        .collect::<Vec<_>>()
        .join(" ")
}
