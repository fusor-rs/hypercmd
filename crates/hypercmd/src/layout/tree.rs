use super::{Builder, Item};
use crate::{
    Error, Node,
    style::{Computed, Overflow, WhiteSpace},
    text::{self, Run},
};
use taffy::{AvailableSpace, Dimension, FlexDirection, Size};

impl Builder<'_> {
    pub(super) fn children(
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
        let children = if matches!(node.tag(), "input" | "textarea") {
            runs.push(self.run(&node.display_value(), &style, node.tag() == "textarea")?);
            Vec::new()
        } else {
            self.children(node, &style, own_definite)?
        };
        self.push(Some(node.clone()), style, runs, children)
            .map(Some)
    }
    pub(super) fn push(
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
                hyperlink: style.hyperlink.clone(),
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

    pub(super) fn count_node(&mut self) -> Result<(), Error> {
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
            hyperlink: style.hyperlink.clone(),
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
        style.border = crate::style::BorderStyle::None;
        children.push(self.push(None, style, runs, Vec::new())?);
        Ok(())
    }
}

// Min-content offers one cell, so every cluster wraps onto its own line.
pub(super) fn measure(
    runs: &[Run],
    known: Size<Option<f32>>,
    available: Size<AvailableSpace>,
) -> Size<f32> {
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

fn check_depth(depth: usize) -> Result<(), Error> {
    Error::limit_if(depth > 256, "scene nesting exceeds 256 levels")
}
fn is_inline(node: &Node) -> bool {
    matches!(node.tag(), "#text" | "span" | "strong" | "em" | "br" | "a")
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

fn rendered(node: &Node) -> bool {
    node.is_visible() && node.tag() != "#comment"
}
