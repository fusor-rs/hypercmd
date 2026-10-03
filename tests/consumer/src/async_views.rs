use fusor::coherence::{AsyncBoundary, BoundaryStatus};
use fusor::{Effect, FromInputs, Memo, OwnerHandle, Registration, Signal, effect, memo, signal};
use fusor_async::AsyncValue;
use fusor_test::{ControlledLoader, TestExecutor};
use hypercmd::{Children, Component, Controller, Error, ErrorKind, Key, Node};
use std::{cell::Cell, rc::Rc};

use super::{Job, job};

#[derive(Clone)]
struct Environment {
    executor: Rc<TestExecutor>,
    rows: ControlledLoader<String, String, String>,
    starts: Rc<Cell<usize>>,
    constructors: Rc<Cell<usize>>,
    cleanups: Signal<usize>,
}
struct Panel {
    boundary: AsyncBoundary,
    revision: Signal<u32>,
    left: AsyncValue<u32, String, String>,
    right: AsyncValue<u32, String, String>,
    jobs: Signal<Vec<Job>>,
    show: Signal<bool>,
    speculative: Signal<bool>,
    clicks: Signal<u32>,
    environment: Environment,
}
struct ReadRow {
    read: AsyncValue<String, String, String>,
    _effect: Effect,
    _cleanup: Registration,
}
struct ReadRowInputs {
    job: Memo<Job>,
    environment: Environment,
}
impl FromInputs for ReadRow {
    type Inputs = ReadRowInputs;
    type Error = Error;
    fn from_inputs(inputs: Self::Inputs, owner: OwnerHandle) -> Result<Self, Error> {
        let starts = inputs.environment.starts.clone();
        let activation = effect(move || starts.set(starts.get() + 1));
        if inputs.job.get().label == "reject" {
            return Err(Error::new(ErrorKind::Construction, "rejected async job"));
        }
        let environment = inputs.environment;
        environment
            .constructors
            .set(environment.constructors.get() + 1);
        let label = memo(move || inputs.job.get().label);
        let uppercase = memo(move || label.get().to_uppercase());
        assert!(!uppercase.get().is_empty()); // Populate both committed memo caches.
        let loader = environment.rows.clone();
        let cleanups = environment.cleanups;
        Ok(Self {
            read: AsyncValue::new(
                &owner,
                move || uppercase.get(),
                move |key, cancel| loader.load(key, cancel),
                environment.executor.spawner(),
            ),
            _effect: activation,
            _cleanup: owner.on_cleanup(move || cleanups.update(|value| *value += 1)),
        })
    }
}

#[derive(FromInputs)]
struct Editable {
    #[local(init = signal(String::new()))]
    value: Signal<String>,
}
#[derive(FromInputs)]
struct Nested {
    #[local(init = AsyncBoundary::coherent())]
    boundary: AsyncBoundary,
}
#[derive(FromInputs)]
struct Routed {}
#[derive(FromInputs)]
struct InvalidRegion {
    #[input]
    boundary: AsyncBoundary,
    #[input]
    choice: Signal<u8>,
}

fusor::template!(backend = "hypercmd", "ui/async.html");

struct AsyncHarness {
    scope: hypercmd::Scope,
    root: Node,
    controls: Controller,
    left: ControlledLoader<u32, String, String>,
    right: ControlledLoader<u32, String, String>,
    environment: Environment,
    boundary: AsyncBoundary,
    revision: Signal<u32>,
    jobs: Signal<Vec<Job>>,
    show: Signal<bool>,
    speculative: Signal<bool>,
    clicks: Signal<u32>,
}

impl AsyncHarness {
    fn settle(&self) {
        self.environment.executor.run_until_stalled();
    }
    fn draw(&mut self) {
        super::draw(&self.scope, &mut self.controls);
    }
    fn coherent_text(&self) -> String {
        self.root.find("coherent").unwrap().text()
    }
}

fn mount_async() -> AsyncHarness {
    let executor = Rc::new(TestExecutor::new());
    let (left, right) = (ControlledLoader::new(), ControlledLoader::new());
    let environment = Environment {
        executor: executor.clone(),
        rows: ControlledLoader::new(),
        starts: Rc::default(),
        constructors: Rc::default(),
        cleanups: signal(0),
    };
    let boundary = AsyncBoundary::coherent();
    let (revision, jobs) = (signal(1), signal(vec![job(1, "one"), job(2, "two")]));
    let (show, speculative, clicks) = (signal(true), signal(false), signal(0));
    let read = |loader: &ControlledLoader<u32, String, String>, owner: &OwnerHandle| {
        let (key, loader) = (revision.clone(), loader.clone());
        let load = move |key, cancel| loader.load(key, cancel);
        AsyncValue::new(owner, move || key.get(), load, executor.spawner())
    };
    let panel = |owner: OwnerHandle| {
        Ok(Panel {
            boundary: boundary.clone(),
            revision: revision.clone(),
            jobs: jobs.clone(),
            show: show.clone(),
            speculative: speculative.clone(),
            clicks: clicks.clone(),
            environment: environment.clone(),
            left: read(&left, &owner),
            right: read(&right, &owner),
        })
    };
    let scope = Panel::prepare(None, Box::new(panel), Children::default()).unwrap();
    scope.publish();
    let root = scope.root();
    AsyncHarness {
        controls: Controller::new(root.clone()),
        root,
        scope,
        left,
        right,
        environment,
        boundary,
        revision,
        jobs,
        show,
        speculative,
        clicks,
    }
}

#[test]
fn contract() {
    let mut harness = mount_async();
    first_publication_waits_for_every_read(&mut harness);
    let inside = refresh_keeps_committed_content(&mut harness);
    keyed_rows_and_focus(&mut harness);
    rejected_candidates(&harness);
    disposal_cancels_reads(&mut harness, &inside);
}

fn first_publication_waits_for_every_read(harness: &mut AsyncHarness) {
    harness.settle();
    assert_eq!(harness.boundary.status(), BoundaryStatus::Pending);
    let started = (
        harness.left.counts().started,
        harness.right.counts().started,
        harness.environment.rows.counts().started,
    );
    assert_eq!(started, (1, 1, 2), "independent reads start in parallel");
    assert_eq!(harness.environment.starts.get(), 0);
    assert!(
        !harness.root.find("coherent").unwrap().is_visible(),
        "no static candidate content is shown before first publication"
    );
    harness.draw();
    let outside = harness.root.find("outside").unwrap();
    assert_eq!(harness.controls.focus(), Some(outside));
    answer(&harness.left, Ok("left-1".into()));
    answer(&harness.right, Ok("right-1".into()));
    let first_row = harness.environment.rows.next_request().unwrap();
    let second_row = harness.environment.rows.next_request().unwrap();
    assert_eq!((&*first_row.key, &*second_row.key), ("ONE", "TWO"));
    first_row.complete(Ok("ONE".into())).unwrap();
    harness.settle();
    assert_eq!(harness.boundary.status(), BoundaryStatus::Pending);
    assert!(!harness.root.find("coherent").unwrap().is_visible());
    second_row.complete(Ok("TWO".into())).unwrap();
    harness.settle();
    assert_eq!(harness.boundary.status(), BoundaryStatus::Ready);
    let environment = &harness.environment;
    assert_eq!(
        (environment.constructors.get(), environment.starts.get()),
        (2, 2)
    );
    assert_eq!(harness.root.find("left").unwrap().text(), "left-1");
}

fn refresh_keeps_committed_content(harness: &mut AsyncHarness) -> Node {
    let inside = harness.root.find("inside").unwrap();
    harness.draw();
    super::key(&mut harness.controls, Key::Tab, false);
    assert_eq!(harness.controls.focus(), Some(inside.clone()));
    let committed = harness.coherent_text();
    harness.revision.set(2);
    harness.settle();
    harness.draw();
    assert_eq!(
        harness.controls.focus(),
        Some(inside.clone()),
        "pending remembers focus"
    );
    inside.dispatch("click").unwrap();
    assert_eq!(harness.clicks.get(), 0, "pending handlers are gated");
    assert_eq!(harness.coherent_text(), committed);
    answer(&harness.left, Ok("left-2".into()));
    answer(&harness.right, Err("offline".into()));
    harness.settle();
    assert!(matches!(
        harness.boundary.status(),
        BoundaryStatus::Error(_)
    ));
    assert_eq!(harness.coherent_text(), committed);
    harness.boundary.retry();
    harness.settle();
    assert!(
        harness.left.next_request().is_none(),
        "retry reuses a compatible successful read"
    );
    answer(&harness.right, Ok("right-2".into()));
    harness.settle();
    harness.draw();
    assert_eq!(harness.boundary.status(), BoundaryStatus::Ready);
    assert_eq!(harness.controls.focus(), Some(inside.clone()));
    inside.dispatch("click").unwrap();
    assert_eq!(harness.clicks.get(), 1);
    inside
}

fn keyed_rows_and_focus(harness: &mut AsyncHarness) {
    harness.jobs.set(vec![job(2, "changed"), job(1, "one")]);
    harness.settle();
    let changed = harness.environment.rows.next_request().unwrap();
    assert_eq!(
        changed.key, "CHANGED",
        "nested cached memos see projected row values"
    );
    assert_eq!(
        harness.environment.constructors.get(),
        2,
        "keyed reorder retains constructors"
    );
    harness.draw();
    super::key(&mut harness.controls, Key::Tab, false);
    let outside = Some(harness.root.find("outside").unwrap());
    assert_eq!(
        harness.controls.focus(),
        outside,
        "Tab can leave pending content"
    );
    changed.complete(Ok("CHANGED".into())).unwrap();
    complete_rows(&harness.environment);
    assert_eq!(harness.boundary.status(), BoundaryStatus::Ready);
    harness.draw();
    assert_eq!(
        harness.controls.focus(),
        outside,
        "readiness does not steal focus back"
    );
    let rows: Vec<_> = super::elements(&harness.root, "li")
        .iter()
        .map(|node| node.text().trim().to_owned())
        .collect();
    assert_eq!(rows, ["CHANGED", "ONE"]);
}

fn rejected_candidates(harness: &AsyncHarness) {
    let environment = &harness.environment;
    harness.speculative.set(true);
    harness.settle();
    let obsolete = environment.rows.next_request().unwrap();
    assert_eq!(obsolete.key, "OBSOLETE");
    assert_eq!(environment.starts.get(), 2);
    harness.speculative.set(false);
    harness.settle();
    assert!(obsolete.is_cancelled());
    assert_eq!(
        environment.cleanups.get(),
        1,
        "an unvisited candidate slot is invalidated"
    );
    let _ = obsolete.complete(Ok("LATE".into()));
    harness.settle();
    assert!(!harness.root.text().contains("LATE"));
    let before_failure = harness.coherent_text();
    harness.jobs.set(vec![job(1, "one"), job(1, "one")]);
    assert!(matches!(
        harness.boundary.status(),
        BoundaryStatus::Error(_)
    ));
    assert_eq!(harness.coherent_text(), before_failure);
    harness.jobs.set(vec![job(4, "reject")]);
    assert!(matches!(
        harness.boundary.status(),
        BoundaryStatus::Error(_)
    ));
    assert_eq!(harness.coherent_text(), before_failure);
    assert_eq!(environment.starts.get(), 2);
    harness.jobs.set(vec![job(2, "changed"), job(1, "one")]);
    harness.settle();
    complete_rows(environment);
    assert_eq!(harness.boundary.status(), BoundaryStatus::Ready);
}

fn disposal_cancels_reads(harness: &mut AsyncHarness, inside: &Node) {
    harness.revision.set(3);
    harness.settle();
    let disposed_left = harness.left.next_request().unwrap();
    let disposed_right = harness.right.next_request().unwrap();
    harness.show.set(false);
    harness.settle();
    assert_eq!(harness.boundary.status(), BoundaryStatus::Disposed);
    assert!(disposed_left.is_cancelled() && disposed_right.is_cancelled());
    assert!(!inside.is_alive());
    inside.dispatch("click").unwrap();
    assert_eq!(harness.clicks.get(), 1);
    harness.draw();
    let outside = harness.root.find("outside").unwrap();
    assert_eq!(harness.controls.focus(), Some(outside));
    let _ = disposed_left.complete(Ok("late-left".into()));
    let _ = disposed_right.complete(Ok("late-right".into()));
    harness.scope.dispose();
    harness.settle();
    assert_eq!(harness.environment.cleanups.get(), 3);
    assert!(!harness.root.text().contains("late-left"));
}

fn complete_rows(environment: &Environment) {
    while let Some(request) = environment.rows.next_request() {
        let value = request.key.clone();
        request.complete(Ok(value)).unwrap();
    }
    environment.executor.run_until_stalled();
}

#[test]
fn transitive_rejections() {
    let boundary = AsyncBoundary::coherent();
    let choice = signal(0);
    let scope = hypercmd::mount::<InvalidRegion>(InvalidRegionInputs {
        boundary: boundary.clone(),
        choice: choice.clone(),
    })
    .unwrap();
    scope.publish();
    assert_eq!(boundary.status(), BoundaryStatus::Ready);
    let text = scope.root().text();
    for invalid in [1, 2, 3, 4] {
        choice.set(invalid);
        assert!(
            matches!(boundary.status(), BoundaryStatus::Error(_)),
            "transitive incompatible component must fail preparation"
        );
        assert_eq!(scope.root().text(), text);
        choice.set(0);
        assert_eq!(boundary.status(), BoundaryStatus::Ready);
    }
}

fn answer(loader: &ControlledLoader<u32, String, String>, result: Result<String, String>) {
    loader.next_request().unwrap().complete(result).unwrap();
}
