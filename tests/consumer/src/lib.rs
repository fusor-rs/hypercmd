#![cfg(test)]
use fusor::{Effect, FromInputs, Memo, OwnerHandle, Registration, Signal, effect, signal};
mod async_views;
mod routing;
use fusor_std::forms::TextField;
use hypercmd::{Controller, Error, ErrorKind, Input, Key, KeyKind, Node};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

struct AppState {
    count: Signal<u32>,
    message: Signal<String>,
    _effect: Effect,
}

impl AppState {
    fn new(owner: &OwnerHandle) -> Self {
        assert!(!owner.is_active());
        let message = signal(String::new());
        let written = message.clone();
        Self {
            count: signal(5),
            message,
            _effect: effect(move || written.set("Authored constructor ran".into())),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Job {
    id: u32,
    label: String,
}

#[derive(Clone, Default)]
struct Probes {
    effects: Rc<Cell<usize>>,
    cleanups: Rc<Cell<usize>>,
    cleanup_rows: Rc<Cell<usize>>,
}

#[derive(FromInputs)]
struct Panel {
    #[input]
    title: String,
    #[input]
    visible: Signal<bool>,
    #[input]
    message: Signal<Option<String>>,
    #[input]
    jobs: Signal<Vec<Job>>,
    #[input]
    pulse: Signal<u32>,
    #[input]
    probes: Probes,
    #[input]
    number: Signal<i32>,
    #[input]
    selection: Signal<Vec<String>>,
    #[input]
    audit: Signal<u32>,
    #[input]
    observed: Rc<RefCell<Vec<i32>>>,
    #[local(init = signal(0))]
    count: Signal<u32>,
}

impl Panel {
    fn record_input(&self) {
        self.observed.borrow_mut().push(self.number.get());
        self.audit.update(|n| *n += 1);
    }
}

#[derive(FromInputs)]
struct Frame {}

struct JobRow {
    job: Memo<Job>,
    jobs: Signal<Vec<Job>>,
    clicks: Signal<u32>,
    draft: Signal<String>,
    _effect: Effect,
    _cleanup: Registration,
}

struct JobRowInputs {
    job: Memo<Job>,
    jobs: Signal<Vec<Job>>,
    pulse: Signal<u32>,
    probes: Probes,
}

// Constructor errors need no Display implementation.
struct RejectedJob;
impl From<RejectedJob> for Error {
    fn from(_: RejectedJob) -> Self {
        Error::new(ErrorKind::Construction, "job labels must not be empty")
    }
}

impl FromInputs for JobRow {
    type Inputs = JobRowInputs;
    type Error = RejectedJob;

    fn from_inputs(inputs: Self::Inputs, owner: OwnerHandle) -> Result<Self, Self::Error> {
        if inputs.job.get().label.is_empty() {
            return Err(RejectedJob);
        }
        let Probes {
            effects,
            cleanups,
            cleanup_rows,
        } = inputs.probes;
        let jobs = inputs.jobs.clone();
        Ok(Self {
            draft: signal(inputs.job.get().label),
            job: inputs.job,
            jobs: inputs.jobs,
            clicks: signal(0),
            _effect: effect(move || {
                inputs.pulse.get();
                effects.set(effects.get() + 1);
            }),
            _cleanup: owner.on_cleanup(move || {
                cleanups.set(cleanups.get() + 1);
                cleanup_rows.set(jobs.get().len());
            }),
        })
    }
}

impl JobRow {
    fn remove(&self) {
        let id = self.job.get().id;
        self.jobs.update(|jobs| jobs.retain(|job| job.id != id));
    }
}

#[derive(FromInputs)]
struct Controls {
    #[input]
    draft: TextField<String>,
    #[input]
    duplicate: Signal<bool>,
    #[input]
    visible: Signal<bool>,
    #[input]
    remove_on_focus: Signal<bool>,
    #[input]
    disabled: Signal<bool>,
    #[input]
    clicks: Signal<u32>,
    #[input]
    blurred: Signal<bool>,
}

fusor::template!(backend = "hypercmd", "ui/app.html");
fusor::template!(backend = "hypercmd", "ui/components.html");

fn elements(node: &Node, tag: &str) -> Vec<Node> {
    node.descendants()
        .filter(|node| node.tag() == tag)
        .collect()
}

fn job(id: u32, label: &str) -> Job {
    Job {
        id,
        label: label.into(),
    }
}

#[test]
fn app_contract() {
    let app = hypercmd_app().unwrap();
    let root = app.root();
    assert_eq!(
        root.find("constructed").unwrap().text(),
        "Authored constructor ran"
    );
    let increment = root.find("app-increment").unwrap();
    increment.dispatch("click").unwrap();
    assert_eq!(
        increment.text(),
        "Count 5",
        "unpublished App blocks callbacks"
    );
    app.publish();
    increment.dispatch("click").unwrap();
    assert_eq!(increment.text(), "Count 6");
    app.dispose();
    increment.dispatch("click").unwrap();
    assert_eq!(increment.text(), "Count 6");
}

struct PanelHarness {
    scope: hypercmd::Scope,
    root: Node,
    jobs: Signal<Vec<Job>>,
    visible: Signal<bool>,
    message: Signal<Option<String>>,
    pulse: Signal<u32>,
    probes: Probes,
    number: Signal<i32>,
    selection: Signal<Vec<String>>,
    audit: Signal<u32>,
    observed: Rc<RefCell<Vec<i32>>>,
}

fn mount_panel() -> PanelHarness {
    let jobs = signal(vec![job(1, "First"), job(2, "Second")]);
    let (visible, message) = (signal(true), signal(Some("ready".to_owned())));
    let (pulse, number, audit) = (signal(0), signal(3), signal(0));
    let selection = signal(Vec::<String>::new());
    let probes = Probes::default();
    let observed = Rc::new(RefCell::new(Vec::new()));
    let scope = hypercmd::mount::<Panel>(PanelInputs {
        title: "Jobs".into(),
        visible: visible.clone(),
        message: message.clone(),
        jobs: jobs.clone(),
        pulse: pulse.clone(),
        probes: probes.clone(),
        number: number.clone(),
        selection: selection.clone(),
        audit: audit.clone(),
        observed: observed.clone(),
    })
    .unwrap();
    PanelHarness {
        root: scope.root(),
        scope,
        jobs,
        visible,
        message,
        pulse,
        probes,
        number,
        selection,
        audit,
        observed,
    }
}

// The rows captured before a keyed reorder, and their order after it.
struct Rows {
    before: Vec<Node>,
    first_counter: Node,
    after: Vec<Node>,
}

#[test]
fn components_contract() {
    let panel = mount_panel();
    projection_and_branches(&panel);
    bindings_and_batches(&panel);
    let rows = keyed_reorder(&panel);
    rejected_row_updates(&panel, &rows);
    removal_and_disposal(&panel, &rows);
}

fn projection_and_branches(panel: &PanelHarness) {
    let root = &panel.root;
    assert_eq!(
        panel.probes.effects.get(),
        2,
        "ordinary child effects run before publication"
    );
    assert_eq!(root.find("projected").unwrap().text(), "Jobs");
    assert!(root.text().contains("Jobs:0:First"));
    let payload = root.find("payload").unwrap();
    assert_eq!(payload.text(), "Jobs:ready");
    panel.scope.publish();
    root.find("increment").unwrap().dispatch("click").unwrap();
    assert_eq!(root.find("increment").unwrap().text(), "1");
    panel.message.set(Some("updated".into()));
    assert_eq!(root.find("payload").unwrap(), payload);
    assert_eq!(payload.text(), "Jobs:updated");
    panel.message.set(None);
    assert!(root.find("absent").is_some());
    panel.visible.set(false);
    assert!(root.find("absent").is_none());
}

fn bindings_and_batches(panel: &PanelHarness) {
    let schedule = Rc::new(RefCell::new(Vec::new()));
    let _ordering = {
        let (number, audit, schedule) =
            (panel.number.clone(), panel.audit.clone(), schedule.clone());
        effect(move || schedule.borrow_mut().push((number.get(), audit.get())))
    };
    schedule.borrow_mut().clear();
    let input = panel.root.find("number").unwrap();
    input.edit("12").unwrap();
    assert_eq!(
        *panel.observed.borrow(),
        [12],
        "bind runs before authored handler"
    );
    assert_eq!(
        *schedule.borrow(),
        [(12, 0), (12, 1)],
        "each listener owns one batch"
    );
    input.edit("012").unwrap();
    panel.pulse.set(1);
    assert_eq!(
        input.value(),
        "012",
        "equivalent draft survives unrelated updates"
    );
    input.edit("-").unwrap();
    assert_eq!(panel.number.get(), 12);
    assert_eq!(input.value(), "-");
    let calls = panel.observed.borrow().len();
    panel.number.set(20);
    assert_eq!(input.value(), "20");
    assert_eq!(
        panel.observed.borrow().len(),
        calls,
        "model writes do not synthesize input"
    );
    let member = panel.root.find("member").unwrap();
    member.check(true).unwrap();
    assert_eq!(panel.selection.get(), ["batch"]);
    panel.selection.set(vec![]);
    assert!(!member.checked());
}

fn keyed_reorder(panel: &PanelHarness) -> Rows {
    let before = elements(&panel.root, "li");
    let first_counter = before[0].find("row-count").unwrap();
    let row_editor = before[0].find("row-draft").unwrap();
    let mut controls = Controller::new(panel.root.clone());
    draw(&panel.scope, &mut controls);
    controls.set_focus(&row_editor).unwrap();
    key(&mut controls, Key::End, false);
    key(&mut controls, Key::Left, true);
    let editor_before = row_editor.editor();
    first_counter.dispatch("click").unwrap();
    panel.jobs.set(vec![job(2, "Second"), job(1, "First")]);
    let after = elements(&panel.root, "li");
    draw(&panel.scope, &mut controls);
    assert_eq!(controls.focus(), Some(row_editor.clone()));
    assert_eq!(
        row_editor.editor(),
        editor_before,
        "keyed reorder retains cursor and selection"
    );
    assert_eq!(after, [before[1].clone(), before[0].clone()]);
    assert_eq!(
        first_counter.text(),
        "First:1",
        "keyed local state survives reorder"
    );
    assert!(after[0].text().contains("Jobs:0:Second"));
    assert_eq!(
        panel.probes.effects.get(),
        4,
        "row factories did not subscribe to constructor reads"
    );
    Rows {
        before,
        first_counter,
        after,
    }
}

fn rejected_row_updates(panel: &PanelHarness, rows: &Rows) {
    panel.jobs.set(vec![job(2, "Duplicate"), job(2, "Second")]);
    assert_eq!(
        elements(&panel.root, "li"),
        rows.after,
        "duplicate keys preserve committed rows"
    );
    assert_eq!(panel.scope.take_errors()[0].kind, ErrorKind::DuplicateKey);
    assert_eq!(rows.first_counter.text(), "First:1");
    panel.jobs.set(vec![job(2, "Second"), job(3, "")]);
    assert_eq!(
        elements(&panel.root, "li"),
        rows.after,
        "failed new constructor preserves committed rows"
    );
    assert_eq!(panel.scope.take_errors()[0].kind, ErrorKind::Construction);
    panel.jobs.set(vec![job(2, "Second"), job(1, "First")]);
}

fn removal_and_disposal(panel: &PanelHarness, rows: &Rows) {
    let probes = &panel.probes;
    let remove = rows.before[0].find("row-remove").unwrap();
    remove.dispatch("click").unwrap();
    assert_eq!(panel.jobs.get(), [job(2, "Second")]);
    assert_eq!(
        probes.cleanups.get(),
        1,
        "self-removal disposes the owner exactly once"
    );
    assert_eq!(
        probes.cleanup_rows.get(),
        1,
        "cleanup can reenter reactive state"
    );
    assert!(!remove.is_alive());
    remove.dispatch("click").unwrap();
    rows.first_counter.dispatch("click").unwrap();
    assert_eq!(probes.cleanups.get(), 1);
    panel.jobs.set(vec![job(2, "Second"), job(1, "Returned")]);
    let returned = elements(&panel.root, "li")[1].find("row-count").unwrap();
    assert_ne!(returned, rows.first_counter);
    rows.first_counter.dispatch("click").unwrap();
    assert_eq!(
        returned.text(),
        "Returned:0",
        "stale handles cannot reach replacement rows"
    );
    panel.scope.dispose();
    assert_eq!(probes.cleanups.get(), 3);
    let effects = probes.effects.get();
    panel.pulse.set(2);
    assert_eq!(
        probes.effects.get(),
        effects,
        "disposal releases constructor subscriptions"
    );
    assert!(panel.scope.take_errors().is_empty());
}

#[test]
fn initial_failure_contract() {
    let probes = Probes::default();
    let result = hypercmd::mount::<JobRow>(JobRowInputs {
        job: fusor::memo(|| job(1, "")),
        jobs: signal(vec![]),
        pulse: signal(0),
        probes: probes.clone(),
    });
    assert_eq!(result.err().unwrap().kind, ErrorKind::Construction);
    assert_eq!(probes.effects.get(), 0);
}

fn frame(
    root: &Node,
    controls: &mut Controller,
    size: (u16, u16),
) -> hypercmd::layout::Presentation {
    let focus = controls.focus();
    let mut frame = hypercmd::layout::render(
        root,
        size,
        focus.as_ref(),
        controls.scrolls_mut(),
        &Default::default(),
    )
    .unwrap();
    controls.decorate(&mut frame);
    frame
}

fn draw(scope: &hypercmd::Scope, controls: &mut Controller) {
    draw_at(scope, controls, (80, 40));
}

fn draw_at(scope: &hypercmd::Scope, controls: &mut Controller, size: (u16, u16)) -> bool {
    let focus = controls.focus();
    let frame = frame(&scope.root(), controls, size);
    let focus_visible = focus.is_some_and(|focus| {
        frame
            .entries
            .iter()
            .any(|entry| entry.node == focus && entry.content.intersection(entry.clip).area() > 0)
    });
    controls.presented(frame).unwrap();
    focus_visible
}

fn key(controls: &mut Controller, key: Key, shift: bool) {
    controls
        .handle(Input::Key {
            key,
            shift,
            kind: KeyKind::Press,
        })
        .unwrap();
}

struct ControlsHarness {
    scope: hypercmd::Scope,
    root: Node,
    controls: Controller,
    draft: TextField<String>,
    duplicate: Signal<bool>,
    visible: Signal<bool>,
    disabled: Signal<bool>,
    remove_on_focus: Signal<bool>,
    clicks: Signal<u32>,
    blurred: Signal<bool>,
}

fn mount_controls() -> ControlsHarness {
    let draft = TextField::new(String::new());
    let (duplicate, visible, disabled) = (signal(false), signal(true), signal(true));
    let (remove_on_focus, clicks, blurred) = (signal(false), signal(0), signal(false));
    let scope = hypercmd::mount::<Controls>(ControlsInputs {
        draft: draft.clone(),
        remove_on_focus: remove_on_focus.clone(),
        duplicate: duplicate.clone(),
        visible: visible.clone(),
        disabled: disabled.clone(),
        clicks: clicks.clone(),
        blurred: blurred.clone(),
    })
    .unwrap();
    scope.publish();
    let root = scope.root();
    let mut controls = Controller::new(root.clone());
    draw(&scope, &mut controls);
    ControlsHarness {
        scope,
        root,
        controls,
        draft,
        duplicate,
        visible,
        disabled,
        remove_on_focus,
        clicks,
        blurred,
    }
}

fn input_origin(frame: &hypercmd::layout::Presentation, input: &Node) -> (u16, u16) {
    let entry = frame.entries.iter().find(|entry| entry.node == *input);
    let content = entry.unwrap().content;
    (content.x, content.y)
}

#[test]
fn controls_contract() {
    let mut harness = mount_controls();
    activation_and_vertical_focus(&mut harness);
    placeholder_and_cluster_edits(&mut harness);
    selection_and_paste_limits(&mut harness);
    tab_and_label_focus(&mut harness);
    structural_focus_changes(&mut harness);
    scrolling_reveals_focus(&mut harness);
}

fn activation_and_vertical_focus(harness: &mut ControlsHarness) {
    let (root, controls) = (&harness.root, &mut harness.controls);
    assert_eq!(controls.focus(), root.find("activate"));
    for kind in [KeyKind::Press, KeyKind::Repeat, KeyKind::Release] {
        let enter = Input::Key {
            key: Key::Enter,
            shift: false,
            kind,
        };
        controls.handle(enter).unwrap();
    }
    assert_eq!(
        harness.clicks.get(),
        1,
        "distinguishable repeats/releases do not activate buttons"
    );
    key(controls, Key::Up, false);
    let release = Input::Key {
        key: Key::Down,
        shift: false,
        kind: KeyKind::Release,
    };
    controls.handle(release).unwrap();
    assert_eq!(controls.focus(), root.find("activate"));
    key(controls, Key::Down, false);
    assert_eq!(controls.focus(), root.find("draft"));
    key(controls, Key::Up, false);
    assert_eq!(controls.focus(), root.find("activate"));
}

fn placeholder_and_cluster_edits(harness: &mut ControlsHarness) {
    let (root, controls) = (&harness.root, &mut harness.controls);
    controls
        .focus_label(&root.find("draft-label").unwrap())
        .unwrap();
    let input = root.find("draft").unwrap();
    assert_eq!(
        controls.focus(),
        Some(input.clone()),
        "labels resolve IDs across structural scopes"
    );
    let frame = frame(root, controls, (80, 40));
    let (x, y) = input_origin(&frame, &input);
    let cursor = frame.buffer.cell((x, y)).unwrap();
    let next = frame.buffer.cell((x + 1, y)).unwrap();
    assert_eq!(
        cursor.symbol(),
        "T",
        "focus retains the empty editor's placeholder"
    );
    assert_ne!(
        cursor.modifier, next.modifier,
        "placeholder preserves a distinct cursor"
    );
    assert!(
        harness.draft.raw().is_empty(),
        "placeholder is never an edit"
    );
    controls
        .handle(Input::Paste("e\u{301}界👩‍🚀".into()))
        .unwrap();
    key(controls, Key::End, false);
    key(controls, Key::Backspace, false);
    assert_eq!(
        harness.draft.raw(),
        "e\u{301}界",
        "backspace removes a whole emoji ZWJ cluster"
    );
}

fn selection_and_paste_limits(harness: &mut ControlsHarness) {
    let (root, controls, draft) = (&harness.root, &mut harness.controls, &harness.draft);
    let input = root.find("draft").unwrap();
    key(controls, Key::Left, true);
    key(controls, Key::Left, true);
    let frame = frame(root, controls, (80, 40));
    let (x, y) = input_origin(&frame, &input);
    let cursor = frame.buffer.cell((x, y)).unwrap();
    let selected = frame.buffer.cell((x + 1, y)).unwrap();
    let unselected = frame.buffer.cell((x + 3, y)).unwrap();
    assert_ne!(
        selected.modifier, unselected.modifier,
        "selection remains visible on a reversed, underlined focused input"
    );
    assert_ne!(
        cursor.modifier, selected.modifier,
        "cursor remains distinct with authored underline"
    );
    key(controls, Key::Right, true);
    controls
        .handle(Input::Paste("東京\r\nA\x1b[31m".into()))
        .unwrap();
    assert_eq!(
        draft.raw(),
        "e\u{301}東京 A\u{fffd}[31m",
        "selection replacement and paste remain one data edit"
    );
    key(controls, Key::Home, false);
    key(controls, Key::Delete, false);
    assert_eq!(
        draft.raw(),
        "東京 A\u{fffd}[31m",
        "delete removes a complete combining cluster"
    );
    let saved = draft.raw();
    controls.set_limits(4, 100);
    let rejected = controls.handle(Input::Paste("oversized".into()));
    assert_eq!(rejected.unwrap_err().kind, ErrorKind::Limit);
    assert_eq!(draft.raw(), saved, "rejected paste does not partially edit");
}

fn tab_and_label_focus(harness: &mut ControlsHarness) {
    let (root, controls) = (&harness.root, &mut harness.controls);
    draw(&harness.scope, controls);
    key(controls, Key::Tab, false);
    assert_eq!(controls.focus(), root.find("readonly"));
    assert!(
        harness.blurred.get(),
        "internal touch-on-blur precedes the authored blur listener"
    );
    controls.handle(Input::Paste("no".into())).unwrap();
    assert_eq!(root.find("readonly").unwrap().value(), "Read only");
    key(controls, Key::Down, false);
    assert_eq!(
        controls.focus(),
        root.find("readonly"),
        "vertical navigation skips disabled, hidden and negative tabindex controls without wrapping"
    );
    key(controls, Key::Tab, false);
    assert_eq!(
        controls.focus(),
        root.find("activate"),
        "tab skips disabled, hidden and negative tabindex controls"
    );
    controls
        .focus_label(&root.find("nested-label").unwrap())
        .unwrap();
    assert_eq!(
        controls.focus(),
        root.find("explicit"),
        "negative tabindex permits explicit label focus"
    );
}

fn structural_focus_changes(harness: &mut ControlsHarness) {
    let (root, controls, scope) = (&harness.root, &mut harness.controls, &harness.scope);
    controls.set_focus(&root.find("draft").unwrap()).unwrap();
    harness.visible.set(false);
    draw(scope, controls);
    assert_eq!(
        controls.focus(),
        root.find("readonly"),
        "removed focus moves to the next surviving control"
    );
    harness.visible.set(true);
    harness.remove_on_focus.set(true);
    draw(scope, controls);
    key(controls, Key::Up, false);
    assert_eq!(
        controls.focus(),
        root.find("readonly"),
        "focus callback may remove its own control"
    );
    harness.duplicate.set(true);
    assert_eq!(scope.take_errors()[0].kind, ErrorKind::Template);
    assert_eq!(root.find("duplicate").unwrap().text(), "Committed");
    harness.disabled.set(false);
    draw(scope, controls);
    key(controls, Key::Tab, false);
    assert_eq!(
        controls.focus(),
        root.find("disabled"),
        "dynamic enablement updates document-order focus"
    );
}

fn scrolling_reveals_focus(harness: &mut ControlsHarness) {
    let (root, controls, scope) = (&harness.root, &mut harness.controls, &harness.scope);
    // Existing focus/identity checks did not protect visibility after layout changes.
    draw_at(scope, controls, (80, 3));
    assert!(
        draw_at(scope, controls, (80, 3)),
        "resize reveals retained focus"
    );
    let viewport = root.find("controls").unwrap();
    assert!(controls.scrolls_mut().offset(&viewport).1 > 0);
    key(controls, Key::Up, false);
    assert_eq!(controls.focus(), root.find("readonly"));
    key(controls, Key::Up, false);
    assert_eq!(controls.focus(), root.find("activate"));
    assert!(
        draw_at(scope, controls, (80, 3)),
        "arrows reveal offscreen controls above the viewport"
    );
    key(controls, Key::Down, false);
    assert_eq!(controls.focus(), root.find("readonly"));
    key(controls, Key::Down, false);
    assert_eq!(controls.focus(), root.find("disabled"));
    assert!(
        draw_at(scope, controls, (80, 3)),
        "arrows reveal offscreen controls below the viewport"
    );
    key(controls, Key::PageUp, false);
    draw_at(scope, controls, (80, 3));
    assert_eq!(
        controls.scrolls_mut().offset(&viewport).1,
        0,
        "manual scrolling does not immediately snap back to focus"
    );
    harness.remove_on_focus.set(false);
    harness.visible.set(true);
    draw_at(scope, controls, (80, 3));
    assert!(
        draw_at(scope, controls, (80, 3)),
        "structural geometry changes reveal retained focus"
    );
}
