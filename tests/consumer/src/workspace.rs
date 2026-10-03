use fusor::{FromInputs, Signal, signal};
use hypercmd::{Controller, Event, EventPayload, Input, Key, KeyKind, Modifiers, Scope};

#[derive(FromInputs)]
struct Workspace {
    #[input]
    draft: Signal<String>,
    #[input]
    runs: Signal<usize>,
    #[input]
    size: Signal<(u16, u16)>,
}

impl Workspace {
    fn shortcut(&self, event: &Event) {
        let EventPayload::Input(Input::Key { key, modifiers, .. }) = event.payload else {
            return;
        };
        if modifiers.control && key == Key::Char('r') {
            self.runs.update(|runs| *runs += 1);
        } else if modifiers.control && key == Key::Char('l') {
            event
                .target
                .component_root()
                .find("viewport")
                .unwrap()
                .request_focus();
        } else {
            return;
        }
        event.prevent_default();
    }
    fn resized(&self, event: &Event) {
        if let EventPayload::Resize { width, height } = event.payload {
            self.size.set((width, height));
        }
    }
}

fusor::template!(backend = "hypercmd", "ui/workspace.html");

#[test]
fn multiline_editor_and_scrollable_workspace_preserve_input_and_focus() {
    let draft = signal(String::new());
    let runs = signal(0);
    let size = signal((0, 0));
    let scope = hypercmd::mount::<Workspace>(WorkspaceInputs {
        draft: draft.clone(),
        runs: runs.clone(),
        size: size.clone(),
    })
    .unwrap();
    scope.publish();
    let root = scope.root();
    let mut controls = Controller::new(root.clone());
    super::draw_at(&scope, &mut controls, (40, 16));
    assert_eq!(controls.focus(), root.find("multiline"));
    controls
        .handle(Input::Paste("-- comment\r\nSELECT '東京'".into()))
        .unwrap();
    assert_eq!(draft.get(), "-- comment\nSELECT '東京'");
    super::key(&mut controls, Key::Home, false);
    super::key(&mut controls, Key::Up, false);
    super::key(&mut controls, Key::End, false);
    super::key(&mut controls, Key::Char('!'), false);
    assert_eq!(draft.get(), "-- comment!\nSELECT '東京'");
    modified(&mut controls, 'r');
    assert_eq!(runs.get(), 1);
    assert_eq!(draft.get(), "-- comment!\nSELECT '東京'");
    modified(&mut controls, 'l');
    assert_eq!(controls.focus(), root.find("viewport"));
    super::key(&mut controls, Key::Down, false);
    assert_eq!(
        controls
            .scrolls_mut()
            .offset(&root.find("viewport").unwrap()),
        (0, 1)
    );
    workspace_geometry(&scope, &mut controls, &size);
    pointer_scroll(&scope, &mut controls);
    single_line_draft(&scope, &mut controls);
    link_destinations(&scope, &mut controls, &draft);
    assert_eq!(hypercmd::text::ellipsize("東京👩‍💻abcdef", 7), "東京👩‍💻…");
    assert_eq!(scope.take_errors().len(), 0);
}

fn modified(controls: &mut Controller, character: char) {
    controls
        .handle(Input::Key {
            key: Key::Char(character),
            modifiers: Modifiers {
                control: true,
                ..Modifiers::default()
            },
            kind: KeyKind::Press,
        })
        .unwrap();
}

fn workspace_geometry(scope: &Scope, controls: &mut Controller, size: &Signal<(u16, u16)>) {
    let root = scope.root();
    let frame = super::frame(&root, controls, (40, 16));
    let pane = frame
        .entries
        .iter()
        .find(|entry| entry.node == root.find("viewport").unwrap())
        .unwrap();
    assert_eq!(size.get(), (38, 4));
    assert_eq!(pane.extent, (0, 4));
    assert_eq!(frame.buffer[(pane.rect.x, pane.rect.y)].symbol(), "╭");
    assert_eq!(frame.buffer[(pane.content.x, pane.content.y)].symbol(), "s");
    let first = frame
        .entries
        .iter()
        .find(|entry| entry.node == root.find("first-heading").unwrap())
        .unwrap();
    let second = frame
        .entries
        .iter()
        .find(|entry| entry.node == root.find("second-cell").unwrap())
        .unwrap();
    assert_eq!(second.content.x - first.content.x, 16);
    controls.presented(frame).unwrap();
    super::draw_at(scope, controls, (30, 16));
    assert_eq!(size.get(), (28, 4));
}

fn pointer_scroll(scope: &Scope, controls: &mut Controller) {
    let root = scope.root();
    let frame = super::frame(&root, controls, (30, 16));
    let pane = frame
        .entries
        .iter()
        .find(|entry| entry.node == root.find("viewport").unwrap())
        .unwrap();
    let (column, row) = (pane.content.x, pane.content.y);
    controls.presented(frame).unwrap();
    controls
        .handle(Input::Scroll {
            column,
            row,
            rows: 2,
            columns: 0,
        })
        .unwrap();
    let frame = super::frame(&root, controls, (30, 16));
    assert_eq!(frame.buffer[(column, row)].symbol(), "f");
    assert_eq!(
        (column..column + 6)
            .map(|x| frame.buffer[(x, row)].symbol())
            .collect::<String>(),
        "fourth"
    );
    controls.presented(frame).unwrap();
    root.find("viewport").unwrap().scroll_to(0, 2);
    let frame = super::frame(&root, controls, (30, 16));
    assert_eq!(
        (column..column + 5)
            .map(|x| frame.buffer[(x, row)].symbol())
            .collect::<String>(),
        "third"
    );
    controls.presented(frame).unwrap();
    super::key(controls, Key::Home, false);
    let frame = super::frame(&root, controls, (30, 16));
    assert_eq!(frame.buffer[(column, row)].symbol(), "f");
    assert_eq!(
        (column..column + 5)
            .map(|x| frame.buffer[(x, row)].symbol())
            .collect::<String>(),
        "first"
    );
    controls.presented(frame).unwrap();
    assert_eq!(
        controls
            .scrolls_mut()
            .offset(&root.find("viewport").unwrap()),
        (0, 0)
    );
}

fn single_line_draft(scope: &Scope, controls: &mut Controller) {
    let root = scope.root();
    let input = root.find("single").unwrap();
    controls.set_focus(&input).unwrap();
    super::key(controls, Key::End, false);
    let frame = super::frame(&root, controls, (30, 16));
    let entry = frame
        .entries
        .iter()
        .find(|entry| entry.node == input)
        .unwrap();
    assert_eq!(
        frame.buffer[(entry.content.x, entry.content.y)].symbol(),
        "L"
    );
    assert_eq!(
        frame.buffer[(entry.content.x + 8, entry.content.y)].symbol(),
        "京"
    );
    controls.presented(frame).unwrap();
}

fn link_destinations(scope: &Scope, controls: &mut Controller, draft: &Signal<String>) {
    for (target, expected) in [
        (
            "https://example.com/first/full-target",
            Some("https://example.com/first/full-target"),
        ),
        (
            "https://example.com/second/full-target",
            Some("https://example.com/second/full-target"),
        ),
        ("", None),
        ("https://example.com/\u{1b}]52;c;bad\u{7}", None),
    ] {
        draft.set(target.into());
        scope.publish();
        let frame = super::frame(&scope.root(), controls, (40, 16));
        let targets: std::collections::BTreeSet<_> = frame
            .hyperlinks
            .values()
            .map(|target| target.as_ref())
            .collect();
        assert_eq!(targets, expected.into_iter().collect());
        assert_eq!(
            scope.root().find("link-label").unwrap().text(),
            "https://example…"
        );
    }
}
