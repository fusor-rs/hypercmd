use crate::{Error, Key, Modifiers, Node, text};
use ratatui::{buffer::Buffer, layout::Rect, style::Modifier};
use std::ops::RangeBounds;
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

pub(crate) fn insert(node: &Node, inserted: &str, limit: usize) -> Result<(), Error> {
    if !node.is_interactive() || node.attribute("readonly").is_some() {
        return Ok(());
    }
    let mut value = node.value();
    let mut editor = node.clamped_editor(&value);
    let selection = editor.selection();
    if value.len() - selection.len() + inserted.len() > limit {
        return Err(Error::limit("edited value exceeds configured byte limit"));
    }
    value.replace_range(selection.clone(), inserted);
    editor.cursor = boundary(&value, ..=selection.start + inserted.len());
    editor.anchor = None;
    node.0.editor.replace(editor);
    node.edit(value)
}

pub(crate) fn key(node: &Node, key: Key, modifiers: Modifiers, limit: usize) -> Result<(), Error> {
    if modifiers.alt {
        return Ok(());
    }
    if !modifiers.control {
        match key {
            Key::Char(character) if !character.is_control() => {
                return insert(node, &character.to_string(), limit);
            }
            Key::Enter if node.tag() == "textarea" => return insert(node, "\n", limit),
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
    node.0.editor.replace(editor);
    node.0.scene.changed();
    Ok(())
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
