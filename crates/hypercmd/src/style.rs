//! The bounded terminal stylesheet data emitted by `hypercmd-build`.
use crate::Node;
pub use ratatui::style::Color;
use ratatui::style::{Modifier, Style};
use taffy::{AlignItems, AlignSelf, Dimension, FlexDirection, JustifyContent, LengthPercentage};

pub type StyleSheet = &'static [Rule];

#[derive(Clone, Copy, Debug)]
pub struct Rule {
    pub selector: &'static [Selector],
    pub declarations: &'static [Declaration],
}

#[derive(Clone, Copy, Debug)]
pub enum Selector {
    Tag(&'static str),
    Class(&'static str),
    Id(&'static str),
    Focus,
    Disabled,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Length {
    Auto,
    Cells(f32),
    Percent(f32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WhiteSpace {
    Normal,
    Pre,
    PreWrap,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Overflow {
    Visible,
    Hidden,
    Auto,
}

#[derive(Clone, Copy, Debug)]
pub enum Alignment {
    Start,
    Center,
    End,
    Stretch,
    Auto,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}

#[derive(Clone, Copy, Debug)]
pub enum Declaration {
    Display(bool),
    Row(bool),
    Grow(f32),
    Shrink(f32),
    Basis(Length),
    Width(Length),
    Height(Length),
    MinWidth(Length),
    MinHeight(Length),
    MaxWidth(Length),
    MaxHeight(Length),
    RowGap(Length),
    ColumnGap(Length),
    PaddingTop(Length),
    PaddingRight(Length),
    PaddingBottom(Length),
    PaddingLeft(Length),
    AlignItems(Alignment),
    AlignSelf(Alignment),
    Justify(Alignment),
    Overflow(Overflow),
    WhiteSpace(WhiteSpace),
    Foreground(Color),
    Background(Color),
    Bold(bool),
    Italic(bool),
    Underline(bool),
}

#[derive(Clone)]
pub(crate) struct Computed {
    pub layout: taffy::Style,
    pub visual: Style,
    pub whitespace: WhiteSpace,
    pub overflow: Overflow,
}

impl Computed {
    pub(crate) fn for_node(
        node: &Node,
        parent: Option<&Self>,
        focus: Option<&Node>,
        overrides: StyleSheet,
    ) -> Self {
        let mut value = Self {
            layout: taffy::Style {
                flex_direction: if node.tag() == "label" {
                    FlexDirection::Row
                } else {
                    FlexDirection::Column
                },
                ..base_layout()
            },
            visual: parent.map_or(Style::default(), |parent| parent.visual),
            whitespace: parent.map_or(WhiteSpace::Normal, |parent| parent.whitespace),
            overflow: Overflow::Visible,
        };
        match node.tag() {
            "pre" => value.whitespace = WhiteSpace::Pre,
            "h1" | "h2" | "h3" | "strong" => {
                value.visual = value.visual.add_modifier(Modifier::BOLD)
            }
            "em" => value.visual = value.visual.add_modifier(Modifier::ITALIC),
            "button" => {
                value.layout.padding.left = LengthPercentage::Length(1.0);
                value.layout.padding.right = LengthPercentage::Length(1.0);
                if node.attribute("disabled").is_none() {
                    value.visual = value.visual.add_modifier(Modifier::BOLD);
                }
            }
            "input" => {
                value.layout.size.width =
                    Dimension::Length(if node.is_checkbox() { 3.0 } else { 16.0 });
                value.layout.size.height = Dimension::Length(1.0);
            }
            _ => {}
        }
        value.cascade(node.0.styles, node, focus);
        value.cascade(overrides, node, focus);
        if focus == Some(node) {
            value.visual = value.visual.add_modifier(Modifier::REVERSED);
        }
        value
    }

    fn cascade(&mut self, sheet: StyleSheet, node: &Node, focus: Option<&Node>) {
        let mut matching: Vec<_> = sheet
            .iter()
            .enumerate()
            .filter(|(_, rule)| rule.matches(node, focus))
            .collect();
        matching.sort_by_key(|(index, rule)| (rule.specificity(), *index));
        for (_, rule) in matching {
            for declaration in rule.declarations {
                self.apply(*declaration);
            }
        }
    }

    fn apply(&mut self, declaration: Declaration) {
        match declaration {
            Declaration::Display(visible) => {
                self.layout.display = if visible {
                    taffy::Display::Flex
                } else {
                    taffy::Display::None
                }
            }
            Declaration::Row(row) => {
                self.layout.flex_direction = if row {
                    FlexDirection::Row
                } else {
                    FlexDirection::Column
                }
            }
            Declaration::Grow(value) => self.layout.flex_grow = value,
            Declaration::Shrink(value) => self.layout.flex_shrink = value,
            Declaration::Basis(value) => self.layout.flex_basis = dimension(value),
            Declaration::Width(value) => self.layout.size.width = dimension(value),
            Declaration::Height(value) => self.layout.size.height = dimension(value),
            Declaration::MinWidth(value) => self.layout.min_size.width = dimension(value),
            Declaration::MinHeight(value) => self.layout.min_size.height = dimension(value),
            Declaration::MaxWidth(value) => self.layout.max_size.width = dimension(value),
            Declaration::MaxHeight(value) => self.layout.max_size.height = dimension(value),
            Declaration::RowGap(value) => self.layout.gap.height = spacing(value),
            Declaration::ColumnGap(value) => self.layout.gap.width = spacing(value),
            Declaration::PaddingTop(value) => self.layout.padding.top = spacing(value),
            Declaration::PaddingRight(value) => self.layout.padding.right = spacing(value),
            Declaration::PaddingBottom(value) => self.layout.padding.bottom = spacing(value),
            Declaration::PaddingLeft(value) => self.layout.padding.left = spacing(value),
            Declaration::AlignItems(value) => self.layout.align_items = align(value),
            Declaration::AlignSelf(value) => {
                self.layout.align_self = align(value).map(|value| match value {
                    AlignItems::FlexStart => AlignSelf::FlexStart,
                    AlignItems::FlexEnd => AlignSelf::FlexEnd,
                    AlignItems::Center => AlignSelf::Center,
                    _ => AlignSelf::Stretch,
                })
            }
            Declaration::Justify(value) => {
                self.layout.justify_content = Some(match value {
                    Alignment::Center => JustifyContent::Center,
                    Alignment::End => JustifyContent::FlexEnd,
                    Alignment::SpaceBetween => JustifyContent::SpaceBetween,
                    Alignment::SpaceAround => JustifyContent::SpaceAround,
                    Alignment::SpaceEvenly => JustifyContent::SpaceEvenly,
                    _ => JustifyContent::FlexStart,
                })
            }
            Declaration::Overflow(value) => {
                self.overflow = value;
                let value = match value {
                    Overflow::Visible => taffy::Overflow::Visible,
                    Overflow::Hidden => taffy::Overflow::Hidden,
                    Overflow::Auto => taffy::Overflow::Scroll,
                };
                self.layout.overflow = taffy::geometry::Point { x: value, y: value };
            }
            Declaration::WhiteSpace(value) => self.whitespace = value,
            Declaration::Foreground(value) => self.visual = self.visual.fg(value),
            Declaration::Background(value) => self.visual = self.visual.bg(value),
            Declaration::Bold(value) => self.modifier(Modifier::BOLD, value),
            Declaration::Italic(value) => self.modifier(Modifier::ITALIC, value),
            Declaration::Underline(value) => self.modifier(Modifier::UNDERLINED, value),
        }
    }

    fn modifier(&mut self, modifier: Modifier, enabled: bool) {
        self.visual = if enabled {
            self.visual.add_modifier(modifier)
        } else {
            self.visual.remove_modifier(modifier)
        };
    }
}

impl Rule {
    fn matches(&self, node: &Node, focus: Option<&Node>) -> bool {
        self.selector.iter().all(|selector| match selector {
            Selector::Tag(tag) => node.tag() == *tag,
            Selector::Class(class) => node.attribute("class").is_some_and(|classes| {
                classes
                    .split_ascii_whitespace()
                    .any(|value| value == *class)
            }),
            Selector::Id(id) => node.attribute("id").as_deref() == Some(*id),
            Selector::Focus => focus == Some(node),
            Selector::Disabled => node.is_disabled(),
        })
    }
    fn specificity(&self) -> (usize, usize, usize) {
        self.selector.iter().fold((0, 0, 0), |mut value, selector| {
            match selector {
                Selector::Id(_) => value.0 += 1,
                Selector::Tag(_) => value.2 += 1,
                _ => value.1 += 1,
            }
            value
        })
    }
}

fn dimension(value: Length) -> Dimension {
    match value {
        Length::Auto => Dimension::Auto,
        Length::Cells(value) => Dimension::Length(value),
        Length::Percent(value) => Dimension::Percent(value),
    }
}
fn spacing(value: Length) -> LengthPercentage {
    match value {
        Length::Percent(value) => LengthPercentage::Percent(value),
        Length::Cells(value) => LengthPercentage::Length(value),
        Length::Auto => LengthPercentage::Length(0.0),
    }
}
fn align(value: Alignment) -> Option<AlignItems> {
    match value {
        Alignment::Auto => None,
        Alignment::Start => Some(AlignItems::FlexStart),
        Alignment::End => Some(AlignItems::FlexEnd),
        Alignment::Center => Some(AlignItems::Center),
        _ => Some(AlignItems::Stretch),
    }
}

pub(crate) fn base_layout() -> taffy::Style {
    taffy::Style {
        flex_shrink: 0.0,
        ..Default::default()
    }
}
