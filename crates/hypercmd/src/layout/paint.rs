use super::{
    Builder, Item, LayoutEntry, LogicalRect, Presentation, ScrollState,
    geometry::{Boxes, boxes},
};
use crate::{
    Error, Node,
    style::{Computed, Overflow},
    text::{self, Run},
};
use ratatui::{buffer::Buffer, layout::Rect};

pub(super) struct Placement<'a> {
    pub(super) origin: (i32, i32),
    pub(super) clip: Rect,
    pub(super) ancestors: &'a [usize],
}

impl Item {
    fn entry(
        &self,
        boxes: &Boxes,
        placement: &Placement<'_>,
        scroll: (u16, u16),
    ) -> Option<LayoutEntry> {
        Some(LayoutEntry {
            node: self.node.as_ref()?.clone(),
            rect: boxes.border.into(),
            content: boxes.content.into(),
            clip: placement.clip,
            extent: boxes.extent,
            scroll,
            scrollable: self.style.overflow == Overflow::Auto,
            logical: boxes.border,
            logical_content: boxes.content,
            overflow: self.style.overflow,
            ancestors: placement.ancestors.to_vec(),
        })
    }

    fn clip(&self, content: LogicalRect, inherited: Rect) -> Rect {
        let editor = self.node.as_ref().is_some_and(Node::is_editor);
        if self.style.overflow == Overflow::Visible && !editor {
            inherited
        } else {
            inherited.intersection(content.into())
        }
    }
}

impl Builder<'_> {
    pub(super) fn paint(
        &self,
        index: usize,
        placement: &Placement<'_>,
        scrolls: &mut ScrollState,
        presentation: &mut Presentation,
    ) -> Result<(), Error> {
        let item = &self.items[index];
        let boxes = boxes(self.tree.layout(item.layout)?, placement.origin)?;
        let Boxes {
            border,
            content,
            extent,
        } = boxes;
        let rect: Rect = border.into();
        let scrolling = item.style.overflow == Overflow::Auto;
        let offset = item
            .node
            .as_ref()
            .filter(|_| scrolling)
            .map_or((0, 0), |node| scrolls.resolve(node, extent));
        let mut ancestors = placement.ancestors.to_vec();
        if let Some(entry) = item.entry(&boxes, placement, offset) {
            ancestors.push(presentation.entries.len());
            presentation.entries.push(entry);
        }
        presentation
            .buffer
            .set_style(rect.intersection(placement.clip), item.style.visual);
        paint_border(
            &mut presentation.buffer,
            border,
            placement.clip,
            &item.style,
        );
        let clip = item.clip(content, placement.clip);
        let (dx, dy) = (-i32::from(offset.0), -i32::from(offset.1));
        paint_text(presentation, &item.text, content.translated(dx, dy), clip);
        let children = Placement {
            origin: (border.x + dx, border.y + dy),
            clip,
            ancestors: &ancestors,
        };
        for child in &item.children {
            self.paint(*child, &children, scrolls, presentation)?;
        }
        if scrolling && !matches!(item.style.border, crate::style::BorderStyle::None) {
            paint_scrollbar(
                &mut presentation.buffer,
                rect,
                placement.clip,
                (offset.1, extent.1),
            );
        }
        Ok(())
    }
}

fn paint_scrollbar(buffer: &mut Buffer, rect: Rect, clip: Rect, vertical: (u16, u16)) {
    if vertical.1 == 0 || rect.height < 3 || rect.width < 2 {
        return;
    }
    let track = u32::from(rect.height - 2);
    let thumb = (track * track / (track + u32::from(vertical.1))).max(1);
    let start = u32::from(vertical.0) * (track - thumb) / u32::from(vertical.1);
    for position in 0..track {
        let point = (rect.right() - 1, rect.y + 1 + position as u16);
        if !clip.contains(point.into()) {
            continue;
        }
        if let Some(cell) = buffer.cell_mut(point) {
            cell.set_symbol(if (start..start + thumb).contains(&position) {
                "┃"
            } else {
                "│"
            });
        }
    }
}

fn paint_border(buffer: &mut Buffer, rect: LogicalRect, clip: Rect, style: &Computed) {
    use crate::style::BorderStyle;
    let corners = match style.border {
        BorderStyle::None => return,
        BorderStyle::Solid => ["┌", "┐", "└", "┘"],
        BorderStyle::Rounded => ["╭", "╮", "╰", "╯"],
    };
    if rect.width < 2 || rect.height < 2 {
        return;
    }
    let right = rect.x + i32::from(rect.width) - 1;
    let bottom = rect.y + i32::from(rect.height) - 1;
    let mut draw = |x: i32, y: i32, symbol: &str| {
        if x < i32::from(clip.x)
            || x >= i32::from(clip.right())
            || y < i32::from(clip.y)
            || y >= i32::from(clip.bottom())
        {
            return;
        }
        if let Some(cell) = buffer.cell_mut((x as u16, y as u16)) {
            cell.set_symbol(symbol).set_fg(style.border_color);
        }
    };
    for x in rect.x.max(i32::from(clip.x))..=right.min(i32::from(clip.right()) - 1) {
        let (top, bottom_corner) = if x == rect.x {
            (corners[0], corners[2])
        } else if x == right {
            (corners[1], corners[3])
        } else {
            ("─", "─")
        };
        draw(x, rect.y, top);
        draw(x, bottom, bottom_corner);
    }
    for y in (rect.y + 1).max(i32::from(clip.y))..bottom.min(i32::from(clip.bottom())) {
        draw(rect.x, y, "│");
        draw(right, y, "│");
    }
}

fn paint_text(presentation: &mut Presentation, runs: &[Run], area: LogicalRect, clip: Rect) {
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
                presentation.buffer.set_stringn(
                    column as u16,
                    row as u16,
                    &glyph.text,
                    glyph.width,
                    glyph.style,
                );
                let position = (column as u16, row as u16);
                if let Some(target) = &glyph.hyperlink {
                    presentation.hyperlinks.insert(position, target.clone());
                } else {
                    presentation.hyperlinks.remove(&position);
                }
            }
            column = end;
        }
    }
}
