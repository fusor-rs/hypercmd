//! Unicode policy shared by text measurement, painting and editing.
use crate::style::WhiteSpace;
use ratatui::style::Style;
use std::rc::Rc;
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

/// Single-line cell preview; terminal controls are sanitized and graphemes remain whole.
pub fn ellipsize(value: &str, columns: usize) -> String {
    let safe = sanitize(value, false);
    if width(&safe) <= columns {
        return safe;
    }
    if columns == 0 {
        return String::new();
    }
    let mut used = 0;
    let mut result = String::new();
    for cluster in unicode_segmentation::UnicodeSegmentation::graphemes(safe.as_str(), true) {
        used += width(cluster);
        if used >= columns {
            break;
        }
        result.push_str(cluster);
    }
    result.push('…');
    result
}

#[derive(Clone)]
pub(crate) struct Run {
    pub text: String,
    pub style: Style,
    pub hyperlink: Option<Rc<str>>,
    pub line_break: bool,
    pub whitespace: WhiteSpace,
}

#[derive(Clone)]
pub(crate) struct Glyph {
    pub text: String,
    pub style: Style,
    pub hyperlink: Option<Rc<str>>,
    pub width: usize,
}

pub(crate) fn lines(runs: &[Run], width: Option<usize>) -> Vec<Vec<Glyph>> {
    if width == Some(0) || runs.is_empty() {
        return Vec::new();
    }
    // Interpolation and inline styling can split a grapheme across text nodes.
    // The style and whitespace policy of its first byte apply to the whole cluster.
    let text: String = runs
        .iter()
        .map(|run| if run.line_break { "\n" } else { &run.text })
        .collect();
    let run_len = |run: &Run| if run.line_break { 1 } else { run.text.len() };
    let mut run_index = 0;
    let mut run_end = run_len(&runs[0]);
    let mut wrapped = WrappedLines::new(width);
    let mut pending_space = None;
    for (offset, cluster) in text.grapheme_indices(true) {
        while run_end <= offset {
            run_index += 1;
            run_end += run_len(&runs[run_index]);
        }
        let run = &runs[run_index];
        if run.line_break {
            wrapped.break_line();
            pending_space = None;
            continue;
        }
        if run.whitespace == WhiteSpace::Normal && cluster.chars().all(char::is_whitespace) {
            if wrapped.column > 0 {
                pending_space = Some(run);
            }
            continue;
        }
        if let Some(run) = pending_space.take() {
            wrapped.push(" ", run);
        }
        match cluster {
            "\n" => wrapped.break_line(),
            "\t" => {
                for _ in 0..(TAB_STOP - wrapped.column % TAB_STOP) {
                    wrapped.push(" ", run);
                }
            }
            _ => wrapped.push(cluster, run),
        }
    }
    wrapped.finish()
}

pub(crate) const TAB_STOP: usize = 4;

struct WrappedLines {
    lines: Vec<Vec<Glyph>>,
    column: usize,
    limit: Option<usize>,
}

impl WrappedLines {
    fn new(limit: Option<usize>) -> Self {
        Self {
            lines: vec![Vec::new()],
            column: 0,
            limit,
        }
    }

    fn break_line(&mut self) {
        self.lines.push(Vec::new());
        self.column = 0;
    }

    fn push(&mut self, cluster: &str, run: &Run) {
        let cells = width(cluster);
        if cells == 0 {
            return;
        }
        let overflows = self
            .limit
            .is_some_and(|limit| self.column > 0 && self.column + cells > limit);
        if run.whitespace != WhiteSpace::Pre && overflows {
            self.break_line();
            if cluster == " " && run.whitespace == WhiteSpace::Normal {
                return;
            }
        }
        self.lines
            .last_mut()
            .expect("a text line exists")
            .push(Glyph {
                text: cluster.into(),
                style: run.style,
                hyperlink: run.hyperlink.clone(),
                width: cells,
            });
        self.column += cells;
    }

    fn finish(self) -> Vec<Vec<Glyph>> {
        if self.lines.len() == 1 && self.lines[0].is_empty() {
            Vec::new()
        } else {
            self.lines
        }
    }
}

pub(crate) fn toggle_class(classes: &str, name: &str, on: bool) -> String {
    classes
        .split_whitespace()
        .filter(|class| *class != name)
        .chain(on.then_some(name))
        .collect::<Vec<_>>()
        .join(" ")
}
