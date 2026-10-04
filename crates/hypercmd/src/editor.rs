use crate::{Error, ErrorKind, Key, Modifiers, Node, text};
use ratatui::{buffer::Buffer, layout::Rect, style::Modifier};
use std::ops::{Range, RangeBounds};
use unicode_segmentation::UnicodeSegmentation;

/// Byte offsets refer to extended grapheme boundaries; scroll offsets are terminal cells.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EditorState {
    pub cursor: usize,
    pub anchor: Option<usize>,
    pub scroll: usize,
    pub scroll_row: usize,
}

impl EditorState {
    pub(crate) fn clamp(&mut self, value: &str) {
        self.cursor = boundary(value, ..=self.cursor);
        self.anchor = self.anchor.map(|anchor| boundary(value, ..=anchor));
    }
    fn selection(&self) -> std::ops::Range<usize> {
        let anchor = self.anchor.unwrap_or(self.cursor);
        anchor.min(self.cursor)..anchor.max(self.cursor)
    }
}

fn boundaries(value: &str) -> impl Iterator<Item = usize> + '_ {
    value
        .grapheme_indices(true)
        .map(|(index, _)| index)
        .chain([value.len()])
}

fn boundary(value: &str, range: impl RangeBounds<usize>) -> usize {
    boundaries(value)
        .take_while(|index| range.contains(index))
        .last()
        .unwrap_or(0)
}

fn next_boundary(value: &str, cursor: usize) -> usize {
    boundaries(value)
        .find(|index| *index > cursor)
        .unwrap_or(value.len())
}

impl Node {
    /// Byte bounds must be extended-grapheme boundaries. The caret follows the inserted text.
    /// Emits `input`; invalid bounds or exceeding the controller's edit limit
    /// leave the draft unchanged.
    pub fn replace_range(&self, range: Range<usize>, inserted: &str) -> Result<(), Error> {
        if !self.is_editor() {
            return Err(Error::new(
                ErrorKind::Edit,
                "text replacement requires a text control",
            ));
        }
        if !self.is_interactive() || self.is_disabled() || self.attribute("readonly").is_some() {
            return Ok(());
        }
        let mut value = self.value();
        if range.start > range.end
            || !boundaries(&value).any(|offset| offset == range.start)
            || !boundaries(&value).any(|offset| offset == range.end)
        {
            return Err(Error::new(
                ErrorKind::Edit,
                "text replacement bounds must be ordered grapheme boundaries within the draft",
            ));
        }
        let limit = self
            .0
            .scene
            .edit_limit
            .get()
            .unwrap_or(crate::controls::EDIT_LIMIT);
        if value.len() - range.len() + inserted.len() > limit {
            return Err(Error::limit("edited value exceeds configured byte limit"));
        }
        value.replace_range(range.clone(), inserted);
        let mut editor = self.editor();
        editor.cursor = boundary(&value, ..=range.start + inserted.len());
        editor.anchor = None;
        self.0.editor.replace(editor);
        self.edit(value)
    }

    fn select(&self, editor: EditorState) -> Result<(), Error> {
        let previous = self.0.editor.replace(editor);
        if previous.cursor != editor.cursor || previous.anchor != editor.anchor {
            self.0.scene.changed();
            self.dispatch("select")?;
        }
        Ok(())
    }
}

pub(crate) fn insert(node: &Node, inserted: &str) -> Result<(), Error> {
    let selection = node.editor().selection();
    node.replace_range(selection, inserted)
}

pub(crate) fn key(node: &Node, key: Key, modifiers: Modifiers) -> Result<(), Error> {
    if modifiers.alt {
        return Ok(());
    }
    if !modifiers.control {
        match key {
            Key::Char(character) if !character.is_control() => {
                return insert(node, &character.to_string());
            }
            Key::Enter if node.tag() == "textarea" => return insert(node, "\n"),
            Key::Backspace | Key::Delete => return erase(node, key),
            _ => {}
        }
    }
    let value = node.value();
    let mut editor = node.clamped_editor(&value);
    if modifiers.control && key == Key::Char('a') {
        editor.anchor = Some(0);
        editor.cursor = value.len();
    } else if let Some(cursor) = destination(&value, &editor, key, modifiers, node) {
        editor.anchor = modifiers
            .shift
            .then_some(editor.anchor.unwrap_or(editor.cursor));
        editor.cursor = cursor;
    } else {
        return Ok(());
    }
    node.select(editor)
}

fn destination(
    value: &str,
    editor: &EditorState,
    key: Key,
    modifiers: Modifiers,
    node: &Node,
) -> Option<usize> {
    let cursor = match key {
        Key::Home if modifiers.control || node.tag() == "input" => 0,
        Key::End if modifiers.control || node.tag() == "input" => value.len(),
        _ if modifiers.control => return None,
        Key::Left if !modifiers.shift && !editor.selection().is_empty() => editor.selection().start,
        Key::Right if !modifiers.shift && !editor.selection().is_empty() => editor.selection().end,
        Key::Left => boundary(value, ..editor.cursor),
        Key::Right => next_boundary(value, editor.cursor),
        Key::Home => line_range(value, editor.cursor).start,
        Key::End => line_range(value, editor.cursor).end,
        Key::Up | Key::Down => adjacent_line(value, editor.cursor, key),
        _ => return None,
    };
    Some(cursor)
}

fn erase(node: &Node, key: Key) -> Result<(), Error> {
    if node.attribute("readonly").is_some() {
        return Ok(());
    }
    let mut value = node.value();
    let mut editor = node.clamped_editor(&value);
    let mut range = editor.selection();
    if range.is_empty() {
        range = if key == Key::Backspace {
            boundary(&value, ..editor.cursor)..editor.cursor
        } else {
            editor.cursor..next_boundary(&value, editor.cursor)
        };
    }
    if range.is_empty() {
        return Ok(());
    }
    value.replace_range(range.clone(), "");
    editor.cursor = range.start;
    editor.anchor = None;
    node.0.editor.replace(editor);
    node.edit(value)
}

pub(crate) fn place_cursor(node: &Node, position: (usize, usize)) -> Result<(), Error> {
    let value = node.value();
    let mut editor = node.clamped_editor(&value);
    let target = (position.0 + editor.scroll, position.1 + editor.scroll_row);
    let mut column = 0;
    let mut row = 0;
    editor.cursor = value.len();
    editor.anchor = None;
    for (index, cluster) in value.grapheme_indices(true) {
        if node.tag() == "textarea" && matches!(cluster, "\n" | "\r" | "\r\n") {
            if row == target.1 {
                editor.cursor = index;
                break;
            }
            row += 1;
            column = 0;
            continue;
        }
        column += cluster_width(cluster, column);
        if row == target.1 && column > target.0 {
            editor.cursor = index;
            break;
        }
    }
    node.select(editor)
}

fn line_range(value: &str, cursor: usize) -> std::ops::Range<usize> {
    let start = value[..cursor].rfind('\n').map_or(0, |index| index + 1);
    let end = value[cursor..]
        .find('\n')
        .map_or(value.len(), |index| cursor + index);
    start..end
}

fn adjacent_line(value: &str, cursor: usize, key: Key) -> usize {
    let current = line_range(value, cursor);
    let column = position(&value[current.start..cursor]).0;
    let target = match key {
        Key::Up if current.start > 0 => current.start - 1,
        Key::Down if current.end < value.len() => current.end + 1,
        _ => return cursor,
    };
    let range = line_range(value, target);
    let mut width = 0;
    for (index, cluster) in value[range.clone()].grapheme_indices(true) {
        width += cluster_width(cluster, width);
        if width > column {
            return range.start + index;
        }
    }
    range.end
}

fn cluster_width(cluster: &str, column: usize) -> usize {
    if cluster == "\t" {
        text::TAB_STOP - column % text::TAB_STOP
    } else if cluster.chars().any(char::is_control) {
        1
    } else {
        text::width(cluster)
    }
}

fn position(value: &str) -> (usize, usize) {
    let mut column = 0;
    let mut row = 0;
    for cluster in value.graphemes(true) {
        if matches!(cluster, "\n" | "\r" | "\r\n") {
            row += 1;
            column = 0;
        } else {
            column += cluster_width(cluster, column);
        }
    }
    (column, row)
}

pub(crate) fn paint(node: &Node, buffer: &mut Buffer, rect: Rect) {
    if rect.is_empty() {
        return;
    }
    let value = node.value();
    let mut editor = node.clamped_editor(&value);
    let cursor = if node.tag() == "textarea" {
        position(&value[..editor.cursor])
    } else {
        (
            text::width(&text::sanitize(&value[..editor.cursor], false)),
            0,
        )
    };
    editor.scroll = visible_offset(editor.scroll, cursor.0, usize::from(rect.width));
    editor.scroll_row = visible_offset(editor.scroll_row, cursor.1, usize::from(rect.height));
    for y in rect.y..rect.bottom() {
        for x in rect.x..rect.right() {
            if let Some(cell) = buffer.cell_mut((x, y)) {
                cell.set_symbol(" ");
            }
        }
    }
    paint_value(buffer, rect, node, &editor);
    let x = rect.x + (cursor.0 - editor.scroll) as u16;
    let y = rect.y + (cursor.1 - editor.scroll_row) as u16;
    if let Some(cell) = buffer.cell_mut((x, y)) {
        cell.modifier.toggle(Modifier::UNDERLINED);
    }
    node.0.editor.replace(editor);
}

fn visible_offset(offset: usize, cursor: usize, length: usize) -> usize {
    offset.min(cursor).max((cursor + 1).saturating_sub(length))
}

fn paint_value(buffer: &mut Buffer, rect: Rect, node: &Node, editor: &EditorState) {
    let selection = editor.selection();
    let mut column = 0;
    let mut row = 0;
    for (byte, cluster) in node.display_value().grapheme_indices(true) {
        if node.tag() == "textarea" && matches!(cluster, "\n" | "\r" | "\r\n") {
            row += 1;
            column = 0;
            continue;
        }
        let safe = text::sanitize(cluster, false);
        let width = if node.tag() == "textarea" {
            cluster_width(cluster, column)
        } else {
            text::width(&safe)
        };
        let start = column;
        column += width;
        if width == 0
            || start < editor.scroll
            || column > editor.scroll + usize::from(rect.width)
            || row < editor.scroll_row
            || row >= editor.scroll_row + usize::from(rect.height)
        {
            continue;
        }
        let x = rect.x + (start - editor.scroll) as u16;
        let y = rect.y + (row - editor.scroll_row) as u16;
        if let Some(cell) = buffer.cell_mut((x, y)) {
            cell.set_symbol(&safe);
            if selection.contains(&byte) {
                cell.modifier.toggle(Modifier::REVERSED);
            }
        }
    }
}
