use fusor::coherence::{AsyncBoundary, BoundaryStatus};
use fusor::{Effect, FromInputs, Memo, OwnerHandle, Registration, Signal, effect, memo, signal};
use fusor_async::AsyncValue;
use fusor_test::{ControlledLoader, TestExecutor};
use hypercmd::{Children, Component, Controller, Error, ErrorKind, Key};
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

#[test]
fn contract() {
    let executor = Rc::new(TestExecutor::new());
    let left = ControlledLoader::<u32, String, String>::new();
    let right = ControlledLoader::<u32, String, String>::new();
    let environment = Environment {
        executor: executor.clone(),
        rows: ControlledLoader::new(),
        starts: Rc::new(Cell::new(0)),
        constructors: Rc::new(Cell::new(0)),
        cleanups: signal(0),
    };
    let boundary = AsyncBoundary::coherent();
    let revision = signal(1);
    let jobs = signal(vec![job(1, "one"), job(2, "two")]);
    let show = signal(true);
    let speculative = signal(false);
    let clicks = signal(0);
    let scope = Panel::prepare(
        None,
        Box::new(|owner| {
            let left_key = revision.clone();
            let right_key = revision.clone();
            let (left, right) = (left.clone(), right.clone());
            Ok(Panel {
                boundary: boundary.clone(),
                revision: revision.clone(),
                jobs: jobs.clone(),
                show: show.clone(),
                speculative: speculative.clone(),
                clicks: clicks.clone(),
                environment: environment.clone(),
                left: AsyncValue::new(
                    &owner,
                    move || left_key.get(),
                    move |key, cancel| left.load(key, cancel),
                    executor.spawner(),
                ),
                right: AsyncValue::new(
                    &owner,
                    move || right_key.get(),
                    move |key, cancel| right.load(key, cancel),
                    executor.spawner(),
                ),
            })
        }),
        Children::default(),
    )
    .unwrap();
    scope.publish();
    let root = scope.root();
    let mut controls = Controller::new(root.clone());
    executor.run_until_stalled();
    assert_eq!(boundary.status(), BoundaryStatus::Pending);
    assert_eq!(
        (
            left.counts().started,
            right.counts().started,
            environment.rows.counts().started
        ),
        (1, 1, 2),
        "independent reads start in parallel"
    );
    assert_eq!(environment.starts.get(), 0);
    assert!(
        !root.find("coherent").unwrap().is_visible(),
        "no static candidate content is shown before first publication"
    );
    super::draw(&scope, &mut controls);
    assert_eq!(controls.focus().unwrap(), root.find("outside").unwrap());
    answer(&left, Ok("left-1".into()));
    answer(&right, Ok("right-1".into()));
    let first_row = environment.rows.next_request().unwrap();
    let second_row = environment.rows.next_request().unwrap();
    assert_eq!((&*first_row.key, &*second_row.key), ("ONE", "TWO"));
    first_row.complete(Ok("ONE".into())).unwrap();
    executor.run_until_stalled();
    assert_eq!(boundary.status(), BoundaryStatus::Pending);
    assert!(!root.find("coherent").unwrap().is_visible());
    second_row.complete(Ok("TWO".into())).unwrap();
    executor.run_until_stalled();
    assert_eq!(boundary.status(), BoundaryStatus::Ready);
    assert_eq!(
        (environment.constructors.get(), environment.starts.get()),
        (2, 2)
    );
    assert_eq!(root.find("left").unwrap().text(), "left-1");
    let inside = root.find("inside").unwrap();
    super::draw(&scope, &mut controls);
    super::key(&mut controls, Key::Tab, false);
    assert_eq!(controls.focus(), Some(inside.clone()));
    let committed = root.find("coherent").unwrap().text();

    revision.set(2);
    executor.run_until_stalled();
    super::draw(&scope, &mut controls);
    assert_eq!(
        controls.focus(),
        Some(inside.clone()),
        "pending remembers focus"
    );
    inside.dispatch("click").unwrap();
    assert_eq!(clicks.get(), 0, "pending handlers are gated");
    assert_eq!(root.find("coherent").unwrap().text(), committed);
    answer(&left, Ok("left-2".into()));
    answer(&right, Err("offline".into()));
    executor.run_until_stalled();
    assert!(matches!(boundary.status(), BoundaryStatus::Error(_)));
    assert_eq!(root.find("coherent").unwrap().text(), committed);
    boundary.retry();
    executor.run_until_stalled();
    assert!(
        left.next_request().is_none(),
        "retry reuses a compatible successful read"
    );
    answer(&right, Ok("right-2".into()));
    executor.run_until_stalled();
    super::draw(&scope, &mut controls);
    assert_eq!(boundary.status(), BoundaryStatus::Ready);
    assert_eq!(controls.focus(), Some(inside.clone()));
    inside.dispatch("click").unwrap();
    assert_eq!(clicks.get(), 1);

    jobs.set(vec![job(2, "changed"), job(1, "one")]);
    executor.run_until_stalled();
    let changed = environment.rows.next_request().unwrap();
    assert_eq!(
        changed.key, "CHANGED",
        "nested cached memos see projected row values"
    );
    assert_eq!(
        environment.constructors.get(),
        2,
        "keyed reorder retains constructors"
    );
    super::draw(&scope, &mut controls);
    super::key(&mut controls, Key::Tab, false);
    assert_eq!(
        controls.focus(),
        Some(root.find("outside").unwrap()),
        "Tab can leave pending content"
    );
    changed.complete(Ok("CHANGED".into())).unwrap();
    complete_rows(&environment);
    assert_eq!(boundary.status(), BoundaryStatus::Ready);
    super::draw(&scope, &mut controls);
    assert_eq!(
        controls.focus(),
        Some(root.find("outside").unwrap()),
        "readiness does not steal focus back"
    );
    assert_eq!(
        super::elements(&root, "li")
            .iter()
            .map(|node| node.text().trim().to_owned())
            .collect::<Vec<_>>(),
        ["CHANGED", "ONE"]
    );

    speculative.set(true);
    executor.run_until_stalled();
    let obsolete = environment.rows.next_request().unwrap();
    assert_eq!(obsolete.key, "OBSOLETE");
    assert_eq!(environment.starts.get(), 2);
    speculative.set(false);
    executor.run_until_stalled();
    assert!(obsolete.is_cancelled());
    assert_eq!(
        environment.cleanups.get(),
        1,
        "an unvisited candidate slot is invalidated"
    );
    let _ = obsolete.complete(Ok("LATE".into()));
    executor.run_until_stalled();
    assert!(!root.text().contains("LATE"));
    let before_failure = root.find("coherent").unwrap().text();
    jobs.set(vec![job(1, "one"), job(1, "one")]);
    assert!(matches!(boundary.status(), BoundaryStatus::Error(_)));
    assert_eq!(root.find("coherent").unwrap().text(), before_failure);
    jobs.set(vec![job(4, "reject")]);
    assert!(matches!(boundary.status(), BoundaryStatus::Error(_)));
    assert_eq!(root.find("coherent").unwrap().text(), before_failure);
    assert_eq!(environment.starts.get(), 2);
    jobs.set(vec![job(2, "changed"), job(1, "one")]);
    executor.run_until_stalled();
    complete_rows(&environment);
    assert_eq!(boundary.status(), BoundaryStatus::Ready);

    revision.set(3);
    executor.run_until_stalled();
    let disposed_left = left.next_request().unwrap();
    let disposed_right = right.next_request().unwrap();
    show.set(false);
    executor.run_until_stalled();
    assert_eq!(boundary.status(), BoundaryStatus::Disposed);
    assert!(disposed_left.is_cancelled() && disposed_right.is_cancelled());
    assert!(!inside.is_alive());
    inside.dispatch("click").unwrap();
    assert_eq!(clicks.get(), 1);
    super::draw(&scope, &mut controls);
    assert_eq!(controls.focus(), Some(root.find("outside").unwrap()));
    let _ = disposed_left.complete(Ok("late-left".into()));
    let _ = disposed_right.complete(Ok("late-right".into()));
    scope.dispose();
    executor.run_until_stalled();
    assert_eq!(environment.cleanups.get(), 3);
    assert!(!root.text().contains("late-left"));
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
