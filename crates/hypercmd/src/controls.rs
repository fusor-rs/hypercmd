use crate::{
    Error, ErrorKind, Node,
    layout::{Presentation, ScrollState},
    text,
};
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::Modifier,
};
use unicode_segmentation::UnicodeSegmentation;

pub(crate) const PASTE_LIMIT: usize = 64 * 1024;
pub(crate) const EDIT_LIMIT: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Enter,
    Tab,
    Backspace,
    Delete,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyKind {
    Press,
    Repeat,
    Release,
}

/// Host-normalized input. A reported paste is always one text edit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Input {
    Key {
        key: Key,
        shift: bool,
        kind: KeyKind,
    },
    Paste(String),
    Click {
        column: u16,
        row: u16,
    },
    Scroll {
        column: u16,
        row: u16,
        rows: i32,
    },
}

/// Byte offsets always refer to extended grapheme boundaries in the current draft.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EditorState {
    pub cursor: usize,
    pub anchor: Option<usize>,
    pub scroll: usize,
}

impl EditorState {
    pub(crate) fn clamp(&mut self, value: &str) {
        self.cursor = boundary(value, self.cursor);
        self.anchor = self.anchor.map(|anchor| boundary(value, anchor));
    }
    fn selection(&self) -> std::ops::Range<usize> {
        let anchor = self.anchor.unwrap_or(self.cursor);
        anchor.min(self.cursor)..anchor.max(self.cursor)
    }
}

fn boundaries(value: &str) -> impl Iterator<Item = usize> {
    value
        .grapheme_indices(true)
        .map(|(i, _)| i)
        .chain([value.len()])
}
fn boundary(value: &str, offset: usize) -> usize {
    boundaries(value)
        .take_while(|i| *i <= offset)
        .last()
        .unwrap_or(0)
}

/// Focus, editor gestures and hit testing for one retained scene.
/// Call `decorate` before output, and `presented` only after that output succeeds.
pub struct Controller {
    root: Node,
    focus: Option<Node>,
    presentation: Option<Presentation>,
    order: Vec<Node>,
    scrolls: ScrollState,
    paste_limit: usize,
    edit_limit: usize,
}

impl Controller {
    pub fn new(root: Node) -> Self {
        Self {
            root,
            focus: None,
            presentation: None,
            order: Vec::new(),
            scrolls: ScrollState::default(),
            paste_limit: PASTE_LIMIT,
            edit_limit: EDIT_LIMIT,
        }
    }
    pub fn set_limits(&mut self, paste_bytes: usize, edit_bytes: usize) {
        self.paste_limit = paste_bytes;
        self.edit_limit = edit_bytes;
    }
    pub fn focus(&self) -> Option<Node> {
        self.focus.clone()
    }
    pub fn scrolls_mut(&mut self) -> &mut ScrollState {
        &mut self.scrolls
    }

    pub fn presented(&mut self, presentation: Presentation) -> Result<(), Error> {
        let previous = self.order.clone();
        let geometry_changed = self.focus.as_ref().is_some_and(|focus| {
            self.presentation.as_ref().is_some_and(|old| {
                old.buffer.area != presentation.buffer.area
                    || old.focus_geometry(focus) != presentation.focus_geometry(focus)
            })
        });
        self.order = presentation
            .entries
            .iter()
            .filter(|entry| entry.node.is_control())
            .map(|entry| entry.node.clone())
            .collect();
        self.presentation = Some(presentation);
        let destination = self.root.0.scene.route_focus.take().upgrade().map(Node);
        let retained = self.focus.as_ref().is_some_and(|node| self.focusable(node));
        if !retained {
            let first = destination.and_then(|destination| {
                self.tab_order()
                    .into_iter()
                    .find(|node| contains(&destination, node))
            });
            if let Some(first) = first {
                self.change_focus(Some(first))?;
            } else {
                self.reconcile(&previous)?;
            }
        }
        if let Some(focus) = self.focus.clone().filter(|_| geometry_changed) {
            self.reveal(&focus);
        }
        Ok(())
    }

    fn focusable(&self, node: &Node) -> bool {
        node.is_active()
            && node.is_control()
            && !node.is_disabled()
            && contains(&self.root, node)
            && self
                .presentation
                .as_ref()
                .is_some_and(|presentation| presentation.can_focus(node))
    }
    fn tabbable(&self, node: &Node) -> bool {
        self.focusable(node)
            && node.is_interactive()
            && node.attribute("tabindex").as_deref() != Some("-1")
    }
    fn tab_order(&self) -> Vec<Node> {
        self.presentation
            .as_ref()
            .map(|presentation| {
                presentation
                    .entries
                    .iter()
                    .filter(|entry| self.tabbable(&entry.node))
                    .map(|entry| entry.node.clone())
                    .collect()
            })
            .unwrap_or_default()
    }
    fn reveal(&mut self, node: &Node) {
        if let Some(presentation) = &self.presentation {
            self.scrolls.reveal(node, presentation);
        }
    }
    fn reconcile(&mut self, previous: &[Node]) -> Result<(), Error> {
        if self.focus.as_ref().is_some_and(|node| self.focusable(node)) {
            return Ok(());
        }
        if self.root.0.scene.route_focus.borrow().upgrade().is_some() {
            // Destination focus needs its new layout; stale event targets stay inert.
            return Ok(());
        }
        self.change_focus(self.replacement(self.focus.as_ref(), previous))
    }
    fn replacement(&self, focus: Option<&Node>, previous: &[Node]) -> Option<Node> {
        let from = focus.and_then(|focus| previous.iter().position(|node| node == focus));
        nearest(previous, from, |node| self.tabbable(node))
            .or_else(|| self.tab_order().first().cloned())
    }
    pub fn set_focus(&mut self, node: &Node) -> Result<(), Error> {
        if self.focusable(node) && node.is_interactive() {
            self.change_focus(Some(node.clone()))?;
        }
        Ok(())
    }
    fn change_focus(&mut self, mut next: Option<Node>) -> Result<(), Error> {
        if self.focus == next {
            return Ok(());
        }
        self.root.0.scene.changed();
        for _ in 0..=self.order.len() {
            if let Some(previous) = self.focus.take() {
                previous.dispatch("blur")?;
            }
            let Some(target) = next else {
                return Ok(());
            };
            if self.focusable(&target) && target.is_interactive() {
                self.focus = Some(target.clone());
                target.dispatch("focus")?;
                if self.focusable(&target) {
                    self.reveal(&target);
                    return Ok(());
                }
            }
            if self.root.0.scene.route_focus.borrow().upgrade().is_some() {
                return Ok(());
            }
            next = self.replacement(Some(&target), &self.order);
        }
        Err(Error::new(
            ErrorKind::ReentrantEvent,
            "focus handlers did not settle after visiting the presented controls",
        ))
    }
    pub fn focus_label(&mut self, label: &Node) -> Result<(), Error> {
        if !label.is_interactive() || label.tag() != "label" {
            return Ok(());
        }
        let control = if let Some(id) = label.attribute("for") {
            self.presentation.as_ref().and_then(|presentation| {
                presentation
                    .entries
                    .iter()
                    .find(|entry| {
                        entry.node.tag() == "input"
                            && entry.node.attribute("id").as_ref() == Some(&id)
                            && entry.node.same_component(label)
                    })
                    .map(|entry| entry.node.clone())
            })
        } else {
            label
                .descendants()
                .find(|node| node.tag() == "input" && node.same_component(label))
        };
        if let Some(control) = control {
            self.set_focus(&control)?;
        }
        Ok(())
    }
    pub fn handle(&mut self, input: Input) -> Result<(), Error> {
        let previous = self.order.clone();
        self.reconcile(&previous)?;
        match input {
            Input::Key {
                kind: KeyKind::Release,
                ..
            } => {}
            Input::Key {
                key: Key::Tab,
                shift,
                ..
            } => self.tab(shift)?,
            Input::Key {
                key: key @ (Key::Up | Key::Down),
                ..
            } => self.vertical(key == Key::Up)?,
            Input::Key { key, shift, kind } => self.key(key, shift, kind)?,
            Input::Paste(value) => {
                if value.len() > self.paste_limit {
                    return Err(Error::limit("paste exceeds configured byte limit"));
                }
                let value = text::sanitize(&value, false);
                self.insert(&value)?;
            }
            Input::Click { column, row } => self.click(column, row)?,
            Input::Scroll { column, row, rows } => {
                if let Some(node) = self.hit(column, row, true) {
                    self.scrolls.scroll(&node, 0, rows);
                }
            }
        }
        self.reconcile(&previous)
    }
    fn tab(&mut self, reverse: bool) -> Result<(), Error> {
        let current = self
            .focus
            .as_ref()
            .and_then(|focus| self.order.iter().position(|node| node == focus));
        let next = cycle(&self.order, current, !reverse, |node| self.tabbable(node));
        self.change_focus(next)
    }
    fn vertical(&mut self, up: bool) -> Result<(), Error> {
        let Some(presentation) = &self.presentation else {
            return Ok(());
        };
        let Some(current) = presentation
            .entries
            .iter()
            .find(|entry| Some(&entry.node) == self.focus.as_ref())
        else {
            return Ok(());
        };
        let next = presentation
            .entries
            .iter()
            .filter(|entry| {
                self.tabbable(&entry.node)
                    && if up {
                        entry.logical.y < current.logical.y
                    } else {
                        entry.logical.y > current.logical.y
                    }
            })
            .min_by_key(|entry| {
                (
                    entry.logical.y.abs_diff(current.logical.y),
                    entry.logical.x.abs_diff(current.logical.x),
                )
            })
            .map(|entry| entry.node.clone());
        if let Some(next) = next {
            self.change_focus(Some(next))?;
        }
        Ok(())
    }
    fn page_viewport_containing(&mut self, node: &Node, up: bool) {
        let Some(entry) = self.presentation.as_ref().and_then(|presentation| {
            presentation
                .entries
                .iter()
                .rev()
                .find(|entry| entry.scrollable && contains(&entry.node, node))
        }) else {
            return;
        };
        let rows = i32::from(entry.content.height.max(1));
        self.scrolls
            .scroll(&entry.node, 0, if up { -rows } else { rows });
    }
    fn key(&mut self, key: Key, shift: bool, kind: KeyKind) -> Result<(), Error> {
        let Some(node) = self.focus.clone().filter(Node::is_interactive) else {
            return Ok(());
        };
        if matches!(key, Key::PageUp | Key::PageDown) {
            self.page_viewport_containing(&node, key == Key::PageUp);
        } else if node.tag() == "button" {
            if kind == KeyKind::Press && matches!(key, Key::Enter | Key::Char(' ')) {
                activate(&node)?;
            }
        } else if node.is_checkbox() {
            if kind == KeyKind::Press && key == Key::Char(' ') {
                activate(&node)?;
            }
        } else if let Key::Char(character) = key {
            if !character.is_control() {
                self.insert(&character.to_string())?;
            }
        } else {
            self.edit_key(&node, key, shift)?;
        }
        Ok(())
    }
    fn insert(&mut self, inserted: &str) -> Result<(), Error> {
        let Some(node) = self
            .focus
            .clone()
            .filter(is_editor)
            .filter(Node::is_interactive)
        else {
            return Ok(());
        };
        if node.attribute("readonly").is_some() {
            return Ok(());
        }
        let mut value = node.value();
        let mut editor = node.clamped_editor(&value);
        let selection = editor.selection();
        if value.len() - selection.len() + inserted.len() > self.edit_limit {
            return Err(Error::limit("edited value exceeds configured byte limit"));
        }
        value.replace_range(selection.clone(), inserted);
        editor.cursor = boundary(&value, selection.start + inserted.len());
        editor.anchor = None;
        node.0.editor.replace(editor);
        node.edit(value)
    }
    fn edit_key(&mut self, node: &Node, key: Key, shift: bool) -> Result<(), Error> {
        let mut value = node.value();
        let mut editor = node.clamped_editor(&value);
        let next = boundaries(&value)
            .find(|i| *i > editor.cursor)
            .unwrap_or(value.len());
        let previous = boundaries(&value)
            .take_while(|i| *i < editor.cursor)
            .last()
            .unwrap_or(0);
        if matches!(key, Key::Backspace | Key::Delete) {
            if node.attribute("readonly").is_some() {
                return Ok(());
            }
            let mut range = editor.selection();
            if range.is_empty() {
                range = if key == Key::Backspace {
                    previous..editor.cursor
                } else {
                    editor.cursor..next
                };
            }
            if range.is_empty() {
                return Ok(());
            }
            value.replace_range(range.clone(), "");
            editor.cursor = range.start;
            editor.anchor = None;
            node.0.editor.replace(editor);
            return node.edit(value);
        }
        let cursor = match key {
            Key::Left if !shift && !editor.selection().is_empty() => editor.selection().start,
            Key::Right if !shift && !editor.selection().is_empty() => editor.selection().end,
            Key::Left => previous,
            Key::Right => next,
            Key::Home => 0,
            Key::End => value.len(),
            _ => return Ok(()),
        };
        editor.anchor = if shift {
            Some(editor.anchor.unwrap_or(editor.cursor))
        } else {
            None
        };
        editor.cursor = cursor;
        node.0.editor.replace(editor);
        self.root.0.scene.changed();
        Ok(())
    }
    fn hit(&self, column: u16, row: u16, scroll: bool) -> Option<Node> {
        self.presentation
            .as_ref()?
            .entries
            .iter()
            .rev()
            .find(|entry| {
                entry.node.is_active()
                    && (!scroll || entry.scrollable)
                    && entry
                        .rect
                        .intersection(entry.clip)
                        .contains(Position::new(column, row))
            })
            .map(|entry| entry.node.clone())
    }
    fn click(&mut self, column: u16, row: u16) -> Result<(), Error> {
        if let Some(node) = self.hit(column, row, false) {
            if node.tag() == "label" {
                return self.focus_label(&node);
            }
            self.set_focus(&node)?;
            if self.focus.as_ref() != Some(&node) || !self.focusable(&node) {
                return Ok(());
            }
            activate(&node)?;
        }
        Ok(())
    }

    /// Paint the retained draft, selection and cursor into the focused text control.
    pub fn decorate(&self, presentation: &mut Presentation) {
        let Some(node) = self
            .focus
            .as_ref()
            .filter(|node| is_editor(node) && node.is_active())
        else {
            return;
        };
        let Some(entry) = presentation
            .entries
            .iter()
            .find(|entry| entry.node == *node)
        else {
            return;
        };
        let rect = entry
            .content
            .intersection(entry.clip)
            .intersection(presentation.buffer.area);
        if rect.is_empty() {
            return;
        }
        let value = node.value();
        let mut editor = node.clamped_editor(&value);
        let cursor = text::width(&text::sanitize(&value[..editor.cursor], false));
        editor.scroll = editor.scroll.min(cursor);
        if cursor >= editor.scroll + usize::from(rect.width) {
            editor.scroll = cursor + 1 - usize::from(rect.width);
        }
        let displayed = node.display_value();
        paint_draft(&mut presentation.buffer, rect, &displayed, &editor, cursor);
        node.0.editor.replace(editor);
    }
}

fn paint_draft(
    buffer: &mut Buffer,
    rect: Rect,
    displayed: &str,
    editor: &EditorState,
    cursor: usize,
) {
    let selection = editor.selection();
    for x in rect.x..rect.right() {
        if let Some(cell) = buffer.cell_mut((x, rect.y)) {
            cell.set_symbol(" ");
        }
    }
    let mut column = 0;
    for (byte, grapheme) in displayed.grapheme_indices(true) {
        let safe = text::sanitize(grapheme, false);
        let width = text::width(&safe);
        let start = column;
        column += width;
        let visible = width > 0
            && start >= editor.scroll
            && column <= editor.scroll + usize::from(rect.width);
        let x = rect.x + start.saturating_sub(editor.scroll) as u16;
        let Some(cell) = visible.then(|| buffer.cell_mut((x, rect.y))).flatten() else {
            continue;
        };
        cell.set_symbol(&safe);
        if selection.contains(&byte) {
            cell.modifier.toggle(Modifier::REVERSED);
        }
    }
    let x = rect.x + (cursor - editor.scroll) as u16;
    if let Some(cell) = buffer.cell_mut((x, rect.y)) {
        cell.modifier.toggle(Modifier::UNDERLINED);
    }
}

fn is_editor(node: &Node) -> bool {
    node.tag() == "input" && !node.is_checkbox()
}
fn contains(root: &Node, target: &Node) -> bool {
    root.descendants().any(|node| node == *target)
}
fn activate(node: &Node) -> Result<(), Error> {
    if node.tag() == "button" {
        node.dispatch("click")
    } else if node.is_checkbox() {
        node.check(!node.checked())
    } else {
        Ok(())
    }
}
// Tab order: the next eligible node in one direction, wrapping past either end.
fn cycle(
    list: &[Node],
    from: Option<usize>,
    forward: bool,
    eligible: impl Fn(&Node) -> bool,
) -> Option<Node> {
    let (before, after) = from.map_or((&[][..], &[][..]), |index| {
        (&list[..index], &list[index + 1..])
    });
    let found = if forward {
        after.iter().chain(list).find(|node| eligible(node))
    } else {
        before
            .iter()
            .rev()
            .chain(list.iter().rev())
            .find(|node| eligible(node))
    };
    found.cloned()
}

// A replacement for a removed focus: the nearest following node, then the nearest preceding one.
fn nearest(list: &[Node], from: Option<usize>, eligible: impl Fn(&Node) -> bool) -> Option<Node> {
    let index = from?;
    list[index + 1..]
        .iter()
        .find(|node| eligible(node))
        .or_else(|| list[..index].iter().rev().find(|node| eligible(node)))
        .cloned()
}
