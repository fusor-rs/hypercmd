use crate::{
    Error, ErrorKind, Event, EventPayload, Node,
    layout::{Presentation, ScrollState},
};
use ratatui::layout::Position;

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
    Escape,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyKind {
    Press,
    Repeat,
    Release,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "keyboard modifier keys are independent and can be held together"
)]
pub struct Modifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    /// Command on macOS; Windows/Super on other platforms.
    pub super_key: bool,
}

/// Host-normalized input. A reported paste is always one text edit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Input {
    Key {
        key: Key,
        modifiers: Modifiers,
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
        columns: i32,
    },
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
        }
    }
    pub fn set_limits(&mut self, paste_bytes: usize, edit_bytes: usize) {
        self.paste_limit = paste_bytes;
        self.root.0.scene.edit_limit.set(Some(edit_bytes));
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
        let autofocus = presentation
            .entries
            .iter()
            .find(|entry| {
                entry.node.attribute("autofocus").is_some() && !previous.contains(&entry.node)
            })
            .map(|entry| entry.node.clone());
        self.resize_events(&presentation)?;
        self.presentation = Some(presentation);
        if self
            .root
            .0
            .scene
            .requested_focus
            .borrow()
            .upgrade()
            .is_none()
        {
            if let Some(autofocus) = autofocus {
                autofocus.request_focus();
            }
        }
        self.apply_focus_request()?;
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

    fn apply_focus_request(&mut self) -> Result<(), Error> {
        let requested = self
            .root
            .0
            .scene
            .requested_focus
            .borrow()
            .upgrade()
            .map(Node);
        if let Some(node) = requested.filter(|node| self.focusable(node)) {
            self.root.0.scene.requested_focus.take();
            self.set_focus(&node)?;
        }
        Ok(())
    }

    fn resize_events(&self, presentation: &Presentation) -> Result<(), Error> {
        for entry in &presentation.entries {
            if !entry.node.0.listeners.borrow().contains_key("resize") {
                continue;
            }
            let size = (entry.content.width, entry.content.height);
            let previous = self
                .presentation
                .as_ref()
                .and_then(|previous| previous.entries.iter().find(|old| old.node == entry.node))
                .map(|old| (old.content.width, old.content.height));
            if previous != Some(size) {
                entry.node.emit(&Event::new(
                    "resize",
                    entry.node.clone(),
                    EventPayload::Resize {
                        width: size.0,
                        height: size.1,
                    },
                ))?;
            }
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
                        matches!(entry.node.tag(), "input" | "textarea")
                            && entry.node.attribute("id").as_ref() == Some(&id)
                            && entry.node.same_component(label)
                    })
                    .map(|entry| entry.node.clone())
            })
        } else {
            label.descendants().find(|node| {
                matches!(node.tag(), "input" | "textarea") && node.same_component(label)
            })
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
                key,
                modifiers,
                kind,
            } => {
                self.keyboard(key, modifiers, kind)?;
            }
            Input::Paste(value) => {
                if value.len() > self.paste_limit {
                    return Err(Error::limit("paste exceeds configured byte limit"));
                }
                if let Some(node) = self.focus.as_ref().filter(|node| node.is_editor()) {
                    let value = crate::text::sanitize(&value, node.tag() == "textarea");
                    crate::editor::insert(node, &value)?;
                }
            }
            Input::Click { column, row } => self.click(column, row)?,
            input @ Input::Scroll {
                column,
                row,
                rows,
                columns,
            } => {
                if let Some(node) = self.hit(column, row, true) {
                    if !self.bubble("scroll", &node, input)? {
                        self.scrolls.scroll(&node, columns, rows);
                    }
                }
            }
        }
        self.apply_focus_request()?;
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
    fn keyboard(&mut self, key: Key, modifiers: Modifiers, kind: KeyKind) -> Result<(), Error> {
        if kind == KeyKind::Release {
            return Ok(());
        }
        let Some(node) = self.focus.clone().filter(Node::is_interactive) else {
            if key == Key::Tab && !modifiers.control && !modifiers.alt && !modifiers.super_key {
                return self.tab(modifiers.shift);
            }
            return Ok(());
        };
        if self.bubble(
            "keydown",
            &node,
            Input::Key {
                key,
                modifiers,
                kind,
            },
        )? || modifiers.super_key
        {
            return Ok(());
        }
        if key == Key::Tab && !modifiers.control && !modifiers.alt {
            return self.tab(modifiers.shift);
        }
        if matches!(key, Key::PageUp | Key::PageDown) && !modifiers.control && !modifiers.alt {
            self.page_viewport_containing(&node, key == Key::PageUp);
            return Ok(());
        }
        if matches!(key, Key::Up | Key::Down)
            && node.tag() == "input"
            && !modifiers.control
            && !modifiers.alt
        {
            return self.vertical(key == Key::Up);
        }
        if node.is_editor() {
            return crate::editor::key(&node, key, modifiers);
        }
        if modifiers.control || modifiers.alt {
            return Ok(());
        }
        if self.scroll_panel(&node, key) {
            return Ok(());
        } else if matches!(key, Key::Up | Key::Down) {
            self.vertical(key == Key::Up)?;
        } else if kind == KeyKind::Press
            && (key == Key::Char(' ') || (key == Key::Enter && node.tag() == "button"))
        {
            activate(&node)?;
        }
        Ok(())
    }

    fn scroll_panel(&mut self, node: &Node, key: Key) -> bool {
        if node.attribute("tabindex").is_none() {
            return false;
        }
        let (columns, rows) = match key {
            Key::Up => (0, -1),
            Key::Down => (0, 1),
            Key::Left => (-1, 0),
            Key::Right => (1, 0),
            Key::Home => (0, -i32::from(u16::MAX)),
            Key::End => (0, i32::from(u16::MAX)),
            _ => return false,
        };
        self.scrolls.scroll(node, columns, rows);
        true
    }

    fn bubble(&self, name: &'static str, target: &Node, input: Input) -> Result<bool, Error> {
        let event = Event::new(name, target.clone(), EventPayload::Input(input));
        target.emit(&event)?;
        let Some(presentation) = &self.presentation else {
            return Ok(event.default_prevented());
        };
        let Some(entry) = presentation
            .entries
            .iter()
            .find(|entry| entry.node == *target)
        else {
            return Ok(event.default_prevented());
        };
        for index in entry.ancestors.iter().rev() {
            if event.default_prevented() {
                break;
            }
            presentation.entries[*index].node.emit(&event)?;
        }
        Ok(event.default_prevented())
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
    #[cfg(all(feature = "native", unix))]
    pub(crate) fn link_at(&self, column: u16, row: u16) -> Option<&str> {
        let node = self
            .hit(column, row, false)
            .unwrap_or_else(|| self.root.clone());
        if !node.is_interactive() || node.is_disabled() {
            return None;
        }
        self.presentation
            .as_ref()?
            .hyperlinks
            .get(&(column, row))
            .map(AsRef::as_ref)
    }

    fn click(&mut self, column: u16, row: u16) -> Result<(), Error> {
        if let Some(node) = self.hit(column, row, false) {
            if node.tag() == "label" {
                return self.focus_label(&node);
            }
            let target = self.presentation.as_ref().and_then(|presentation| {
                let entry = presentation
                    .entries
                    .iter()
                    .find(|entry| entry.node == node)?;
                std::iter::once(&node)
                    .chain(
                        entry
                            .ancestors
                            .iter()
                            .rev()
                            .map(|index| &presentation.entries[*index].node),
                    )
                    .find(|node| self.focusable(node))
                    .cloned()
            });
            let Some(target) = target else {
                return Ok(());
            };
            self.set_focus(&target)?;
            if self.focus.as_ref() != Some(&target) || !self.focusable(&target) {
                return Ok(());
            }
            if target.is_editor() {
                self.place_cursor(&target, column, row)?;
            }
            activate(&target)?;
        }
        Ok(())
    }

    fn place_cursor(&self, node: &Node, column: u16, row: u16) -> Result<(), Error> {
        if let Some(entry) = self.presentation.as_ref().and_then(|presentation| {
            presentation
                .entries
                .iter()
                .find(|entry| entry.node == *node)
        }) {
            crate::editor::place_cursor(
                node,
                (
                    usize::from(column.saturating_sub(entry.content.x)),
                    usize::from(row.saturating_sub(entry.content.y)),
                ),
            )?;
        }
        Ok(())
    }

    /// Paint the retained draft, selection and cursor into the focused text control.
    pub fn decorate(&self, presentation: &mut Presentation) {
        let Some(node) = self
            .focus
            .as_ref()
            .filter(|node| node.is_editor() && node.is_active())
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
        crate::editor::paint(node, &mut presentation.buffer, rect);
    }
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
