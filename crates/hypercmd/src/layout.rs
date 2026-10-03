//! Dirty-frame layout and painting; the retained scene owns identity and lifetimes.
mod geometry;
mod paint;
mod scroll;
mod tree;

#[cfg(test)]
pub(crate) mod tests;

use crate::{
    Error, Node,
    style::{Computed, Overflow, StyleSheet},
    text::Run,
};
use geometry::LogicalRect;
use paint::Placement;
use ratatui::{buffer::Buffer, layout::Rect};
pub use scroll::ScrollState;
use std::{collections::BTreeMap, rc::Rc};
use taffy::{AvailableSpace, Dimension, NodeId, Size, TaffyTree};
use tree::measure;

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
    /// Complete link destinations keyed by the visible glyph's (column, row).
    pub hyperlinks: BTreeMap<(u16, u16), Rc<str>>,
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
    pub(crate) ancestors: Vec<usize>,
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
        hyperlinks: BTreeMap::new(),
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
