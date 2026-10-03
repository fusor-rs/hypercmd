use super::{LayoutOptions, Presentation, ScrollState, render};
use crate::{
    Error, ErrorKind, Kind, Node, Scope, StaticNode,
    style::{Color, Declaration, Length, Overflow, Rule, Selector, WhiteSpace},
};
use ratatui::{layout::Rect, style::Modifier};

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
