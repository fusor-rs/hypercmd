//! Dirty-frame layout and painting; the retained scene owns identity and lifetimes.
use crate::{
    Error, Node,
    style::{Computed, Overflow, StyleSheet, WhiteSpace},
    text::{self, Run},
};
use ratatui::{buffer::Buffer, layout::Rect};
use taffy::{AvailableSpace, Dimension, FlexDirection, NodeId, Size, TaffyTree};

pub struct LayoutOptions {
    /// Explicit application rules applied after each component's local rules.
    pub overrides: StyleSheet,
    pub max_content_bytes: usize,
    pub max_nodes: usize,
    pub max_cells: usize,
}
impl Default for LayoutOptions {
    fn default() -> Self {
        Self {
            overrides: &[],
            max_content_bytes: 1_048_576,
            max_nodes: 16_384,
            max_cells: 1_048_576,
        }
    }
}

pub struct Presentation {
    pub buffer: Buffer,
    pub entries: Vec<LayoutEntry>,
    viewport: Rect,
}

pub struct LayoutEntry {
    pub node: Node,
    pub rect: Rect,
    pub content: Rect,
    pub clip: Rect,
    pub extent: (u16, u16),
    pub scroll: (u16, u16),
    pub scrollable: bool,
    pub(crate) logical: LogicalRect,
    logical_content: LogicalRect,
    overflow: Overflow,
    ancestors: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FocusGeometry {
    target: LogicalRect,
    clips: Vec<(Node, LogicalRect, Overflow, (u16, u16))>,
    viewport: Rect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LogicalRect {
    pub(crate) x: i32,
    pub(crate) y: i32,
    width: u16,
    height: u16,
}

impl LogicalRect {
    fn translated(self, x: i32, y: i32) -> Self {
        Self {
            x: self.x + x,
            y: self.y + y,
            ..self
        }
    }
    fn intersection(self, other: Self) -> Self {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = (self.x + i32::from(self.width)).min(other.x + i32::from(other.width));
        let bottom = (self.y + i32::from(self.height)).min(other.y + i32::from(other.height));
        Self {
            x,
            y,
            width: (right - x).max(0) as u16,
            height: (bottom - y).max(0) as u16,
        }
    }
    fn empty(self) -> bool {
        self.width == 0 || self.height == 0
    }
}

impl Presentation {
    /// Whether scrolling its ancestors can expose this control in this layout.
    /// Hidden overflow and the root viewport are permanent clipping boundaries.
    pub(crate) fn can_focus(&self, node: &Node) -> bool {
        node.is_active() && self.reveal_plan(node).is_some()
    }

    /// Logical geometry independent of manual scrolling, for resize/reorder detection.
    pub(crate) fn focus_geometry(&self, node: &Node) -> Option<FocusGeometry> {
        let entry = self.entries.iter().find(|entry| entry.node == *node)?;
        let unscrolled = |entry: &LayoutEntry, rect: LogicalRect| {
            let (x, y) = entry.ancestors.iter().fold((0, 0), |(x, y), index| {
                let scroll = self.entries[*index].scroll;
                (x + i32::from(scroll.0), y + i32::from(scroll.1))
            });
            rect.translated(x, y)
        };
        Some(FocusGeometry {
            target: unscrolled(entry, entry.logical),
            clips: entry
                .ancestors
                .iter()
                .filter_map(|index| {
                    let ancestor = &self.entries[*index];
                    (ancestor.overflow != Overflow::Visible).then(|| {
                        (
                            ancestor.node.clone(),
                            unscrolled(ancestor, ancestor.logical_content),
                            ancestor.overflow,
                            ancestor.extent,
                        )
                    })
                })
                .collect(),
            viewport: self.viewport,
        })
    }

    fn reveal_plan(&self, node: &Node) -> Option<Vec<(Node, (u16, u16))>> {
        let entry = self.entries.iter().find(|entry| entry.node == *node)?;
        let mut target = entry.logical;
        let mut visible = target;
        let mut plan = Vec::new();
        for index in entry.ancestors.iter().rev() {
            let ancestor = &self.entries[*index];
            if ancestor.scrollable {
                let content = ancestor.logical_content;
                let next = (
                    (i32::from(ancestor.scroll.0)
                        + scroll_delta(target.x, target.width, content.x, content.width))
                    .clamp(0, i32::from(ancestor.extent.0)) as u16,
                    (i32::from(ancestor.scroll.1)
                        + scroll_delta(target.y, target.height, content.y, content.height))
                    .clamp(0, i32::from(ancestor.extent.1)) as u16,
                );
                let movement = (
                    i32::from(ancestor.scroll.0) - i32::from(next.0),
                    i32::from(ancestor.scroll.1) - i32::from(next.1),
                );
                target = target.translated(movement.0, movement.1);
                visible = visible.translated(movement.0, movement.1);
                plan.push((ancestor.node.clone(), next));
            }
            if ancestor.overflow != Overflow::Visible {
                visible = visible.intersection(ancestor.logical_content);
            }
            if visible.empty() {
                return None;
            }
        }
        (!visible.intersection(self.viewport.into()).empty()).then_some(plan)
    }
}

#[derive(Default)]
pub struct ScrollState(Vec<(Node, (u16, u16))>);

impl ScrollState {
    pub fn offset(&self, node: &Node) -> (u16, u16) {
        self.0
            .iter()
            .find(|(candidate, _)| candidate == node)
            .map_or((0, 0), |(_, offset)| *offset)
    }
    pub fn scroll(&mut self, node: &Node, dx: i32, dy: i32) {
        let (x, y) = self.offset(node);
        let next = (
            (i32::from(x) + dx).clamp(0, i32::from(u16::MAX)) as u16,
            (i32::from(y) + dy).clamp(0, i32::from(u16::MAX)) as u16,
        );
        self.set(node, next);
    }
    pub fn reveal(&mut self, node: &Node, presentation: &Presentation) {
        if let Some(plan) = presentation.reveal_plan(node) {
            for (node, offset) in plan {
                self.set(&node, offset);
            }
        }
    }
    fn set(&mut self, node: &Node, offset: (u16, u16)) {
        if let Some((_, current)) = self.0.iter_mut().find(|(candidate, _)| candidate == node) {
            if *current == offset {
                return;
            }
            *current = offset;
        } else {
            self.0.push((node.clone(), offset));
        }
        node.0.scene.changed();
    }
}

struct Item {
    node: Option<Node>,
    style: Computed,
    text: Vec<Run>,
    children: Vec<usize>,
    layout: NodeId,
}

struct Builder<'a> {
    tree: TaffyTree<usize>,
    items: Vec<Item>,
    focus: Option<&'a Node>,
    options: &'a LayoutOptions,
    bytes: usize,
    nodes: usize,
    depth: usize,
}

/// Compute and paint one complete committed scene. No application callback runs here.
pub fn render(
    root: &Node,
    size: (u16, u16),
    focus: Option<&Node>,
    scrolls: &mut ScrollState,
    options: &LayoutOptions,
) -> Result<Presentation, Error> {
    Error::limit_if(
        usize::from(size.0) * usize::from(size.1) > options.max_cells,
        "viewport exceeds max_cells",
    )?;
    let mut builder = Builder {
        tree: TaffyTree::new(),
        items: Vec::new(),
        focus,
        options,
        bytes: 0,
        nodes: 0,
        depth: 0,
    };
    builder.count_node()?;
    let mut style = Computed::for_node(root, None, focus, options.overrides);
    let children = builder.children(root, &style, (true, true))?;
    style.layout.size = Size {
        width: Dimension::Length(f32::from(size.0)),
        height: Dimension::Length(f32::from(size.1)),
    };
    let root_index = builder.push(None, style, Vec::new(), children.clone())?;
    let root_id = builder.items[root_index].layout;
    let items = &builder.items;
    builder.tree.compute_layout_with_measure(
        root_id,
        Size {
            width: AvailableSpace::Definite(f32::from(size.0)),
            height: AvailableSpace::Definite(f32::from(size.1)),
        },
        |known, available, _, context, _| {
            context.map_or(Size::ZERO, |index| {
                measure(&items[*index].text, known, available)
            })
        },
    )?;
    let area = Rect::new(0, 0, size.0, size.1);
    let mut presentation = Presentation {
        buffer: Buffer::empty(area),
        entries: Vec::new(),
        viewport: area,
    };
    let top = Placement {
        origin: (0, 0),
        clip: area,
        ancestors: &[],
    };
    for child in children {
        builder.paint(child, &top, scrolls, &mut presentation)?;
    }
    scrolls.0.retain(|(node, _)| {
        presentation
            .entries
            .iter()
            .any(|entry| entry.scrollable && entry.node == *node)
    });
    Ok(presentation)
}

impl Builder<'_> {
    fn children(
        &mut self,
        node: &Node,
        parent: &Computed,
        definite: (bool, bool),
    ) -> Result<Vec<usize>, Error> {
        self.depth += 1;
        check_depth(self.depth)?;
        let mut children = Vec::new();
        let mut inline = Vec::new();
        let mut pending = vec![(node.clone(), 0)];
        while let Some((parent_node, index)) = pending.last_mut() {
            let child = parent_node.0.children.borrow().get(*index).cloned();
            *index += 1;
            let Some(child) = child else {
                pending.pop();
                continue;
            };
            self.count_node()?;
            if !rendered(&child) {
                continue;
            }
            if matches!(child.tag(), "#scope" | "#mount") {
                check_depth(self.depth + pending.len())?;
                pending.push((child, 0));
                continue;
            }
            if is_inline(&child) {
                self.inline(&child, parent, &mut inline, self.depth + pending.len())?;
            } else {
                self.flush(&mut inline, parent, &mut children)?;
                if let Some(index) = self.element(&child, parent, definite)? {
                    children.push(index);
                }
            }
        }
        self.flush(&mut inline, parent, &mut children)?;
        self.depth -= 1;
        Ok(children)
    }

    fn element(
        &mut self,
        node: &Node,
        parent: &Computed,
        definite: (bool, bool),
    ) -> Result<Option<usize>, Error> {
        let style = Computed::for_node(node, Some(parent), self.focus, self.options.overrides);
        if style.layout.display == taffy::Display::None {
            return Ok(None);
        }
        validate_percentages(&style, definite, parent.layout.flex_direction)?;
        let axis = |size, definite, cross| {
            definite_axis(size, definite)
                || definite
                    && size == Dimension::Auto
                    && if parent.layout.flex_direction == cross {
                        stretched(&style, parent)
                    } else {
                        style.layout.flex_grow > 0.0
                    }
        };
        let own_definite = (
            axis(style.layout.size.width, definite.0, FlexDirection::Column),
            axis(style.layout.size.height, definite.1, FlexDirection::Row),
        );
        let mut runs = Vec::new();
        let children = if node.tag() == "input" {
            runs.push(self.run(&node.display_value(), &style, false)?);
            Vec::new()
        } else {
            self.children(node, &style, own_definite)?
        };
        self.push(Some(node.clone()), style, runs, children)
            .map(Some)
    }
    fn push(
        &mut self,
        node: Option<Node>,
        style: Computed,
        text: Vec<Run>,
        children: Vec<usize>,
    ) -> Result<usize, Error> {
        let index = self.items.len();
        let layout = if children.is_empty() {
            self.tree.new_leaf_with_context(style.layout.clone(), index)
        } else {
            let ids: Vec<_> = children
                .iter()
                .map(|index| self.items[*index].layout)
                .collect();
            self.tree.new_with_children(style.layout.clone(), &ids)
        }?;
        self.items.push(Item {
            node,
            style,
            text,
            children,
            layout,
        });
        Ok(index)
    }

    fn inline(
        &mut self,
        node: &Node,
        parent: &Computed,
        runs: &mut Vec<Run>,
        depth: usize,
    ) -> Result<(), Error> {
        check_depth(depth)?;
        let style = Computed::for_node(node, Some(parent), self.focus, self.options.overrides);
        if style.layout.display == taffy::Display::None {
            return Ok(());
        }
        let inline_layout = taffy::Style {
            flex_direction: FlexDirection::Column,
            ..crate::style::base_layout()
        };
        if style.layout != inline_layout {
            return Err(Error::template(
                "terminal inline runs support text styling and display:none; apply geometry, flex and overflow styles to a surrounding container",
            ));
        }
        if node.tag() == "br" {
            runs.push(Run {
                text: String::new(),
                style: style.visual,
                line_break: true,
                whitespace: style.whitespace,
            });
        } else if node.tag() == "#text" {
            runs.push(self.run(&node.0.text.borrow(), &style, true)?);
        } else {
            for child in node.0.children.borrow().iter() {
                self.count_node()?;
                if !rendered(child) {
                    continue;
                }
                if !is_inline(child) && !child.tag().starts_with('#') {
                    return Err(Error::template(
                        "terminal inline text cannot contain block elements or controls; move the block beside the inline run",
                    ));
                }
                self.inline(child, &style, runs, depth + 1)?;
            }
        }
        Ok(())
    }

    fn count_node(&mut self) -> Result<(), Error> {
        self.nodes += 1;
        Error::limit_if(
            self.nodes > self.options.max_nodes,
            "scene exceeds max_nodes",
        )
    }

    fn run(&mut self, source: &str, style: &Computed, multiline: bool) -> Result<Run, Error> {
        self.bytes = self
            .bytes
            .checked_add(source.len())
            .ok_or_else(|| Error::limit("text size overflow"))?;
        Error::limit_if(
            self.bytes > self.options.max_content_bytes,
            "scene text exceeds max_content_bytes",
        )?;
        Ok(Run {
            text: text::sanitize(source, multiline),
            style: style.visual,
            line_break: false,
            whitespace: if multiline {
                style.whitespace
            } else {
                WhiteSpace::Pre
            },
        })
    }

    fn flush(
        &mut self,
        runs: &mut Vec<Run>,
        parent: &Computed,
        children: &mut Vec<usize>,
    ) -> Result<(), Error> {
        if runs.is_empty() {
            return Ok(());
        }
        let runs = std::mem::take(runs);
        if text::lines(&runs, None).is_empty() {
            return Ok(());
        }
        let mut style = parent.clone();
        style.layout = crate::style::base_layout();
        style.overflow = Overflow::Visible;
        children.push(self.push(None, style, runs, Vec::new())?);
        Ok(())
    }

    fn paint(
        &self,
        index: usize,
        placement: &Placement<'_>,
        scrolls: &mut ScrollState,
        presentation: &mut Presentation,
    ) -> Result<(), Error> {
        let item = &self.items[index];
        let Boxes {
            border,
            content,
            extent,
        } = boxes(self.tree.layout(item.layout)?, placement.origin)?;
        let rect: Rect = border.into();
        let scrolling = item.style.overflow == Overflow::Auto;
        let offset = item
            .node
            .as_ref()
            .filter(|_| scrolling)
            .map_or((0, 0), |node| scrolls.offset(node));
        let offset = (offset.0.min(extent.0), offset.1.min(extent.1));
        let mut ancestors = placement.ancestors.to_vec();
        if let Some(node) = &item.node {
            if scrolling {
                scrolls.set(node, offset);
            }
            ancestors.push(presentation.entries.len());
            presentation.entries.push(LayoutEntry {
                node: node.clone(),
                rect,
                content: content.into(),
                clip: placement.clip,
                extent,
                scroll: offset,
                scrollable: scrolling,
                logical: border,
                logical_content: content,
                overflow: item.style.overflow,
                ancestors: placement.ancestors.to_vec(),
            });
        }
        presentation
            .buffer
            .set_style(rect.intersection(placement.clip), item.style.visual);
        let clip = if item.style.overflow == Overflow::Visible
            && item.node.as_ref().is_none_or(|node| node.tag() != "input")
        {
            placement.clip
        } else {
            placement.clip.intersection(content.into())
        };
        let (dx, dy) = (-i32::from(offset.0), -i32::from(offset.1));
        paint_text(
            &mut presentation.buffer,
            &item.text,
            content.translated(dx, dy),
            clip,
        );
        let children = Placement {
            origin: (border.x + dx, border.y + dy),
            clip,
            ancestors: &ancestors,
        };
        for child in &item.children {
            self.paint(*child, &children, scrolls, presentation)?;
        }
        Ok(())
    }
}

struct Placement<'a> {
    origin: (i32, i32),
    clip: Rect,
    ancestors: &'a [usize],
}

struct Boxes {
    border: LogicalRect,
    content: LogicalRect,
    extent: (u16, u16),
}

// `extent` is how far the content overflows the content box.
fn boxes(layout: &taffy::Layout, origin: (i32, i32)) -> Result<Boxes, Error> {
    let x = origin.0 + layout.location.x.round() as i32;
    let y = origin.1 + layout.location.y.round() as i32;
    let (width, height) = (cells(layout.size.width)?, cells(layout.size.height)?);
    let (left, top) = (cells(layout.padding.left)?, cells(layout.padding.top)?);
    let content = LogicalRect {
        x: x + i32::from(left),
        y: y + i32::from(top),
        width: width
            .saturating_sub(left)
            .saturating_sub(cells(layout.padding.right)?),
        height: height
            .saturating_sub(top)
            .saturating_sub(cells(layout.padding.bottom)?),
    };
    let extent = (
        cells(layout.content_size.width)?.saturating_sub(content.width),
        cells(layout.content_size.height)?.saturating_sub(content.height),
    );
    Ok(Boxes {
        border: LogicalRect {
            x,
            y,
            width,
            height,
        },
        content,
        extent,
    })
}

fn paint_text(buffer: &mut Buffer, runs: &[Run], area: LogicalRect, clip: Rect) {
    let lines = text::lines(runs, Some(usize::from(area.width)));
    for (row, line) in lines.iter().enumerate() {
        let row = area.y + row as i32;
        let mut column = area.x;
        for glyph in line {
            let end = column + glyph.width as i32;
            let inside = column >= i32::from(clip.left())
                && end <= i32::from(clip.right())
                && row >= i32::from(clip.top())
                && row < i32::from(clip.bottom());
            if inside {
                buffer.set_stringn(
                    column as u16,
                    row as u16,
                    &glyph.text,
                    glyph.width,
                    glyph.style,
                );
            }
            column = end;
        }
    }
}

// Min-content offers one cell, so every cluster wraps onto its own line.
fn measure(runs: &[Run], known: Size<Option<f32>>, available: Size<AvailableSpace>) -> Size<f32> {
    let limit = known
        .width
        .or(match available.width {
            AvailableSpace::Definite(width) => Some(width),
            AvailableSpace::MinContent => Some(1.0),
            AvailableSpace::MaxContent => None,
        })
        .map(|width| width.max(0.0).floor() as usize);
    let lines = text::lines(runs, limit);
    let widest = lines
        .iter()
        .map(|line| line.iter().map(|glyph| glyph.width).sum::<usize>())
        .max()
        .unwrap_or_default();
    Size {
        width: known.width.unwrap_or(widest as f32),
        height: known.height.unwrap_or(lines.len() as f32),
    }
}

// How far a scroll offset moves to bring `[start, start + length)` into view
// within `[lower, lower + available)`.
fn scroll_delta(start: i32, length: u16, lower: i32, available: u16) -> i32 {
    if start < lower || length > available {
        start - lower
    } else {
        (start + i32::from(length) - lower - i32::from(available)).max(0)
    }
}

fn check_depth(depth: usize) -> Result<(), Error> {
    Error::limit_if(depth > 256, "scene nesting exceeds 256 levels")
}
fn is_inline(node: &Node) -> bool {
    matches!(node.tag(), "#text" | "span" | "strong" | "em" | "br")
}
fn definite_axis(dimension: Dimension, parent: bool) -> bool {
    matches!(dimension, Dimension::Length(_))
        || matches!(dimension, Dimension::Percent(_)) && parent
}
fn stretched(style: &Computed, parent: &Computed) -> bool {
    style
        .layout
        .align_self
        .or(parent.layout.align_items)
        .is_none_or(|value| value == taffy::AlignItems::Stretch)
}
fn validate_percentages(
    style: &Computed,
    parent: (bool, bool),
    direction: FlexDirection,
) -> Result<(), Error> {
    let layout = &style.layout;
    let percent = |value| matches!(value, Dimension::Percent(_));
    let horizontal = [
        layout.size.width,
        layout.min_size.width,
        layout.max_size.width,
    ]
    .into_iter()
    .any(percent)
        || percent(layout.gap.width.into())
        || [
            layout.padding.left,
            layout.padding.right,
            layout.padding.top,
            layout.padding.bottom,
        ]
        .into_iter()
        .any(|value| percent(value.into()));
    let vertical = [
        layout.size.height,
        layout.min_size.height,
        layout.max_size.height,
    ]
    .into_iter()
    .any(percent)
        || percent(layout.gap.height.into());
    let basis = percent(layout.flex_basis)
        && if direction == FlexDirection::Row {
            !parent.0
        } else {
            !parent.1
        };
    if horizontal && !parent.0 || vertical && !parent.1 || basis {
        return Err(Error::template(
            "percentage requires a definite parent axis; give the parent an explicit ch or resolved percentage size",
        ));
    }
    Ok(())
}
impl From<LogicalRect> for Rect {
    fn from(
        LogicalRect {
            x,
            y,
            width,
            height,
        }: LogicalRect,
    ) -> Self {
        let right = (x + i32::from(width)).clamp(0, i32::from(u16::MAX));
        let bottom = (y + i32::from(height)).clamp(0, i32::from(u16::MAX));
        let x = x.clamp(0, i32::from(u16::MAX));
        let y = y.clamp(0, i32::from(u16::MAX));
        Rect::new(
            x as u16,
            y as u16,
            (right - x).max(0) as u16,
            (bottom - y).max(0) as u16,
        )
    }
}
impl From<Rect> for LogicalRect {
    fn from(
        Rect {
            x,
            y,
            width,
            height,
        }: Rect,
    ) -> Self {
        Self {
            x: i32::from(x),
            y: i32::from(y),
            width,
            height,
        }
    }
}
fn rendered(node: &Node) -> bool {
    node.is_visible() && node.tag() != "#comment"
}
fn cells(value: f32) -> Result<u16, Error> {
    (value.is_finite() && value >= 0.0 && value <= f32::from(u16::MAX))
        .then(|| value.round() as u16)
        .ok_or_else(|| Error::template("layout exceeds the supported terminal coordinate range"))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{
        ErrorKind, Kind, Scope, StaticNode,
        style::{Color, Declaration, Length, Rule, Selector},
    };
    use ratatui::style::Modifier;

    pub(crate) const fn el(parent: Option<usize>, tag: &'static str) -> StaticNode {
        tagged(parent, tag, &[])
    }
    pub(crate) const fn txt(parent: usize, text: &'static str) -> StaticNode {
        StaticNode {
            parent: Some(parent),
            kind: Kind::Text(text, None),
        }
    }
    const fn tagged(
        parent: Option<usize>,
        tag: &'static str,
        attributes: &'static [(&'static str, &'static str)],
    ) -> StaticNode {
        StaticNode {
            parent,
            kind: Kind::Element(tag, attributes, None),
        }
    }
    const fn anchored_text(parent: usize, anchor: usize) -> StaticNode {
        StaticNode {
            parent: Some(parent),
            kind: Kind::Text("", Some(anchor)),
        }
    }
    const fn mount(parent: Option<usize>, anchor: usize) -> StaticNode {
        StaticNode {
            parent,
            kind: Kind::Mount(anchor),
        }
    }
    fn with(rule: Rule) -> LayoutOptions {
        LayoutOptions {
            overrides: Box::leak(Box::new([rule])),
            ..Default::default()
        }
    }
    fn cells(presentation: &Presentation, points: &[(u16, u16)]) -> Vec<String> {
        points
            .iter()
            .map(|point| presentation.buffer[*point].symbol().to_owned())
            .collect()
    }
    fn failure(root: &Node, size: (u16, u16), options: &LayoutOptions) -> Error {
        let mut scroll = ScrollState::default();
        render(root, size, None, &mut scroll, options)
            .err()
            .unwrap()
    }

    fn unicode_scene() -> (Scope, fusor::Signal<String>) {
        let mut scope = Scope::with_styles(
            None,
            &[
                el(None, "main"),
                txt(0, "\n  "),
                el(Some(0), "p"),
                txt(2, " A "),
                el(Some(2), "strong"),
                txt(4, "B"),
                txt(2, " C\u{1b}"),
                el(Some(0), "pre"),
                anchored_text(7, 0),
                el(Some(0), "p"),
                txt(9, "e"),
                anchored_text(9, 1),
                el(Some(9), "strong"),
                txt(12, "👩"),
                txt(9, "\u{200d}💻"),
                tagged(Some(0), "p", &[("id", "wrap")]),
                txt(15, "ab c"),
                tagged(Some(0), "input", &[("placeholder", "a  bcdefghi")]),
                el(Some(0), "p"),
                txt(18, "tail"),
            ],
            &[
                Rule {
                    selector: &[Selector::Id("wrap")],
                    declarations: &[
                        Declaration::Width(Length::Cells(2.0)),
                        Declaration::WhiteSpace(WhiteSpace::PreWrap),
                    ],
                },
                Rule {
                    selector: &[Selector::Tag("input")],
                    declarations: &[Declaration::Width(Length::Cells(3.0))],
                },
            ],
        )
        .unwrap();
        let value = fusor::signal(String::from("e\u{301}\t界👩\u{200d}💻"));
        let text = value.clone();
        scope.text(0, move || text.get()).unwrap();
        scope.text(1, || "\u{301}").unwrap();
        scope.publish();
        (scope, value)
    }

    #[test]
    fn inline_unicode_whitespace_and_control_text_share_measured_cells() {
        let (scope, _) = unicode_scene();
        let mut scroll = ScrollState::default();
        let first = render(
            &scope.root(),
            (12, 8),
            None,
            &mut scroll,
            &Default::default(),
        )
        .unwrap();
        // Row by row: collapsed spaces and a replaced control character, a tab
        // and wide graphemes, a grapheme split across runs, wrapped pre-wrap text,
        // a placeholder whose spaces are preserved and which clips, then the tail.
        let points = [
            (0, 0),
            (2, 0),
            (5, 0),
            (0, 1),
            (4, 1),
            (6, 1),
            (0, 2),
            (1, 2),
        ];
        let expected = [
            "A",
            "B",
            "�",
            "e\u{301}",
            "界",
            "👩\u{200d}💻",
            "e\u{301}",
            "👩\u{200d}💻",
        ];
        assert_eq!(cells(&first, &points), expected);
        let points = [
            (0, 3),
            (1, 3),
            (0, 4),
            (1, 4),
            (0, 5),
            (2, 5),
            (3, 5),
            (0, 6),
        ];
        assert_eq!(
            cells(&first, &points),
            ["a", "b", " ", "c", "a", " ", " ", "t"]
        );
        assert!(first.buffer[(2, 0)].modifier.contains(Modifier::BOLD));
        assert!(
            first.buffer[(1, 2)].modifier.contains(Modifier::BOLD),
            "a cross-run grapheme uses the style where it starts"
        );
        assert!(
            first.buffer.content[7 * 12..]
                .iter()
                .all(|cell| cell.symbol() == " "),
            "a narrow input must not wrap over following content"
        );
    }

    #[test]
    fn text_updates_rerender_and_an_empty_viewport_paints_nothing() {
        let (scope, value) = unicode_scene();
        let mut scroll = ScrollState::default();
        let options = LayoutOptions::default();
        render(&scope.root(), (12, 8), None, &mut scroll, &options).unwrap();
        value.set("ab".into());
        let next = render(&scope.root(), (12, 4), None, &mut scroll, &options).unwrap();
        assert_eq!(cells(&next, &[(0, 1), (6, 1), (7, 1)]), ["a", " ", " "]);
        let empty = render(&scope.root(), (0, 0), None, &mut scroll, &options).unwrap();
        assert!(empty.buffer.content.is_empty());
    }

    static SCROLL_SHEET: &[Rule] = &[
        Rule {
            selector: &[Selector::Tag("main")],
            declarations: &[
                Declaration::Height(Length::Percent(1.0)),
                Declaration::Overflow(Overflow::Auto),
            ],
        },
        Rule {
            selector: &[Selector::Tag("button")],
            declarations: &[
                Declaration::Height(Length::Cells(1.0)),
                Declaration::Width(Length::Percent(1.0)),
                Declaration::Foreground(Color::Red),
            ],
        },
        Rule {
            selector: &[Selector::Id("last")],
            declarations: &[Declaration::Foreground(Color::Blue)],
        },
    ];

    fn scroll_scene() -> (Scope, Node) {
        let scope = Scope::with_styles(
            None,
            &[
                el(None, "main"),
                el(Some(0), "button"),
                txt(1, "one"),
                el(Some(0), "button"),
                txt(3, "two"),
                tagged(Some(0), "button", &[("id", "last")]),
                txt(5, "界三"),
                mount(Some(0), 0),
            ],
            SCROLL_SHEET,
        )
        .unwrap();
        scope.publish();
        let last = scope.root().find("last").unwrap();
        (scope, last)
    }

    #[test]
    fn revealing_a_control_keeps_its_logical_geometry_until_resize() {
        let (scope, last) = scroll_scene();
        let root = scope.root();
        let mut scroll = ScrollState::default();
        let options = LayoutOptions::default();
        let first = render(&root, (8, 2), None, &mut scroll, &options).unwrap();
        assert_eq!(cells(&first, &[(0, 0), (1, 0)]), [" ", "o"]);
        let label = &first.buffer[(1, 0)];
        assert_eq!(label.bg, Color::Reset);
        assert!(label.modifier.contains(Modifier::BOLD));
        assert!(!label.modifier.contains(Modifier::REVERSED));
        assert!(first.can_focus(&last));
        let geometry = first.focus_geometry(&last);
        scroll.reveal(&last, &first);
        let revealed = render(&root, (8, 2), Some(&last), &mut scroll, &options).unwrap();
        assert_eq!(
            revealed.focus_geometry(&last),
            geometry,
            "manual scroll does not change logical focus geometry"
        );
        assert_eq!(cells(&revealed, &[(1, 0), (1, 1)]), ["t", "界"]);
        assert_eq!(revealed.buffer[(0, 1)].fg, Color::Blue);
        assert!(
            revealed.buffer[(0, 1)]
                .modifier
                .contains(Modifier::REVERSED)
        );
        let resized = render(&root, (8, 4), None, &mut scroll, &options).unwrap();
        assert_eq!(resized.entries[0].scroll, (0, 0));
        assert_ne!(
            resized.focus_geometry(&last),
            geometry,
            "resize changes clipping geometry"
        );
    }

    #[test]
    fn hidden_overflow_and_the_viewport_bound_what_can_be_focused() {
        let (scope, last) = scroll_scene();
        let root = scope.root();
        let mut scroll = ScrollState::default();
        let hidden = with(Rule {
            selector: &[Selector::Tag("main")],
            declarations: &[Declaration::Overflow(Overflow::Hidden)],
        });
        let hidden = render(&root, (8, 1), None, &mut scroll, &hidden).unwrap();
        assert!(
            !hidden.can_focus(&last),
            "hidden overflow cannot expose the last control"
        );
        let visible = with(Rule {
            selector: &[Selector::Tag("main")],
            declarations: &[Declaration::Overflow(Overflow::Visible)],
        });
        let mut clipped = render(&root, (8, 1), None, &mut scroll, &visible).unwrap();
        assert!(
            !clipped.can_focus(&last),
            "viewport clipping also excludes an unreachable control"
        );
        clipped.buffer.resize(Rect::new(0, 0, 8, 4));
        assert!(
            !clipped.can_focus(&last),
            "an output-only diagnostic area does not extend the content viewport"
        );
    }

    #[test]
    fn application_overrides_apply_and_library_styles_stay_scoped() {
        let (mut scope, _) = scroll_scene();
        let root = scope.root();
        let mut scroll = ScrollState::default();
        let green = with(Rule {
            selector: &[Selector::Tag("button")],
            declarations: &[Declaration::Foreground(Color::Green)],
        });
        let overridden = render(&root, (8, 4), None, &mut scroll, &green).unwrap();
        assert_eq!(overridden.buffer[(0, 2)].fg, Color::Green);
        let library = crate::Children::new(|owner| {
            Scope::with_styles(
                Some(owner),
                &[el(None, "button"), txt(0, "library")],
                &[Rule {
                    selector: &[Selector::Tag("button")],
                    declarations: &[Declaration::Foreground(Color::Yellow)],
                }],
            )
        });
        scope.children(0, &library).unwrap();
        let scoped = render(&root, (8, 4), None, &mut scroll, &Default::default()).unwrap();
        assert_eq!(
            (scoped.buffer[(0, 0)].fg, scoped.buffer[(0, 3)].fg),
            (Color::Red, Color::Yellow)
        );
    }

    #[test]
    fn indefinite_percentages_and_oversized_viewports_fail_before_presentation() {
        let scope = Scope::with_styles(
            None,
            &[el(None, "main"), el(Some(0), "p"), txt(1, "bounded")],
            &[Rule {
                selector: &[Selector::Tag("p")],
                declarations: &[Declaration::Height(Length::Percent(0.5))],
            }],
        )
        .unwrap();
        scope.publish();
        let options = LayoutOptions::default();
        let percentage = failure(&scope.root(), (8, 4), &options);
        assert!(percentage.message.contains("definite parent"));
        let oversized = failure(&scope.root(), (u16::MAX, u16::MAX), &options);
        assert_eq!(oversized.kind, ErrorKind::Limit);
    }

    #[test]
    fn nested_inline_nodes_consume_the_budget_and_reject_geometry() {
        let inline = Scope::with_styles(
            None,
            &[
                el(None, "div"),
                el(Some(0), "span"),
                el(Some(1), "strong"),
                txt(2, "nested"),
            ],
            &[Rule {
                selector: &[Selector::Tag("strong")],
                declarations: &[Declaration::Width(Length::Cells(1.0))],
            }],
        )
        .unwrap();
        inline.publish();
        let budget = LayoutOptions {
            max_nodes: 2,
            ..LayoutOptions::default()
        };
        assert_eq!(
            failure(&inline.root(), (8, 4), &budget).kind,
            ErrorKind::Limit,
            "nested inline nodes consume the render budget"
        );
        let unsupported = failure(&inline.root(), (8, 4), &LayoutOptions::default());
        assert!(unsupported.message.contains("surrounding container"));
    }

    #[test]
    fn structural_only_trees_consume_the_visit_budget() {
        let structure = Scope::new(
            None,
            &[mount(None, 0), mount(Some(0), 1), mount(Some(1), 2)],
        )
        .unwrap();
        structure.publish();
        let budget = LayoutOptions {
            max_nodes: 1,
            ..LayoutOptions::default()
        };
        let limit = failure(&structure.root(), (8, 4), &budget);
        assert_eq!(limit.kind, ErrorKind::Limit);
    }

    #[test]
    fn nested_scroll_reveal_uses_signed_clipping_geometry() {
        let scope = Scope::with_styles(
            None,
            &[
                tagged(None, "main", &[("id", "outer")]),
                tagged(Some(0), "section", &[("id", "inner")]),
                el(Some(1), "button"),
                txt(2, "one"),
                el(Some(1), "button"),
                txt(4, "two"),
                tagged(Some(1), "button", &[("id", "target")]),
                txt(6, "three"),
                el(Some(0), "p"),
                txt(8, "tail"),
            ],
            &[
                Rule {
                    selector: &[Selector::Tag("main")],
                    declarations: &[
                        Declaration::Height(Length::Percent(1.0)),
                        Declaration::Overflow(Overflow::Auto),
                    ],
                },
                Rule {
                    selector: &[Selector::Tag("section")],
                    declarations: &[
                        Declaration::Height(Length::Cells(2.0)),
                        Declaration::Overflow(Overflow::Auto),
                    ],
                },
            ],
        )
        .unwrap();
        scope.publish();
        let root = scope.root();
        let [outer, inner, target] = ["outer", "inner", "target"].map(|id| root.find(id).unwrap());
        let mut scroll = ScrollState::default();
        let options = LayoutOptions::default();
        let first = render(&root, (8, 2), None, &mut scroll, &options).unwrap();
        assert!(first.can_focus(&target));
        scroll.scroll(&outer, 0, 1);
        let displaced = render(&root, (8, 2), None, &mut scroll, &options).unwrap();
        assert_eq!(
            displaced.focus_geometry(&target),
            first.focus_geometry(&target)
        );
        scroll.reveal(&target, &displaced);
        let revealed = render(&root, (8, 2), Some(&target), &mut scroll, &options).unwrap();
        assert_eq!(scroll.offset(&inner), (0, 1));
        assert_eq!(revealed.buffer[(1, 0)].symbol(), "t");
        let hidden = with(Rule {
            selector: &[Selector::Tag("section")],
            declarations: &[Declaration::Overflow(Overflow::Hidden)],
        });
        let hidden = render(&root, (8, 2), None, &mut scroll, &hidden).unwrap();
        assert!(!hidden.can_focus(&target));
    }
}
