use super::{LayoutEntry, LogicalRect, Presentation};
use crate::{Node, style::Overflow};
use ratatui::layout::Rect;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FocusGeometry {
    target: LogicalRect,
    clips: Vec<(Node, LogicalRect, Overflow, (u16, u16))>,
    viewport: Rect,
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
pub struct ScrollState(pub(super) Vec<(Node, (u16, u16))>);

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
    pub(super) fn resolve(&mut self, node: &Node, extent: (u16, u16)) -> (u16, u16) {
        let requested = node
            .0
            .scroll_request
            .take()
            .unwrap_or_else(|| self.offset(node));
        let clamped = (requested.0.min(extent.0), requested.1.min(extent.1));
        self.set(node, clamped);
        clamped
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

// How far a scroll offset moves to bring `[start, start + length)` into view
// within `[lower, lower + available)`.
fn scroll_delta(start: i32, length: u16, lower: i32, available: u16) -> i32 {
    if start < lower || length > available {
        start - lower
    } else {
        (start + i32::from(length) - lower - i32::from(available)).max(0)
    }
}
