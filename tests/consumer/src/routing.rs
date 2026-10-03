use fusor::{FromInputs, OwnerHandle, Registration, Signal, signal};
use fusor_async::{Resource, Spawner};
use fusor_router::{AppUrl, view::Navigation};
use fusor_test::{ControlledLoader, TestExecutor};
use hypercmd::{Controller, Error, ErrorKind, History, Scope};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

#[derive(Clone)]
struct Work {
    loader: ControlledLoader<String, (), String>,
    spawn: Rc<Spawner>,
    cleanups: Rc<Cell<usize>>,
    navigation: Rc<RefCell<Option<Navigation<Scope>>>>,
}

struct Screens {
    visible: Signal<bool>,
    work: Work,
    history: History,
}
struct ScreensInputs {
    visible: Signal<bool>,
    work: Work,
}
impl FromInputs for Screens {
    type Inputs = ScreensInputs;
    type Error = Error;
    fn from_inputs(inputs: Self::Inputs, owner: OwnerHandle) -> Result<Self, Error> {
        Ok(Self {
            visible: inputs.visible,
            work: inputs.work,
            history: History::install(&owner, "/")?,
        })
    }
}

struct Member {
    name: String,
    clicks: Signal<u32>,
    history: History,
    _work: Resource<String, (), String>,
    _cleanup: Registration,
}
#[derive(FromInputs)]
struct Team {
    #[input]
    name: String,
    #[input]
    work: Work,
    #[local(init = signal(0))]
    visits: Signal<u32>,
}
struct MemberInputs {
    team: String,
    id: String,
    work: Work,
}
impl FromInputs for Member {
    type Inputs = MemberInputs;
    type Error = Error;
    fn from_inputs(inputs: Self::Inputs, owner: OwnerHandle) -> Result<Self, Error> {
        if inputs.id == "broken" {
            return Err(Error::new(ErrorKind::Construction, "member is unavailable"));
        }
        let history = History::from_owner(&owner).unwrap();
        if inputs.id == "reentrant" {
            assert_eq!(history.push("/").unwrap_err().kind, ErrorKind::Navigation);
        }
        let key = inputs.id.clone();
        let Work {
            loader,
            spawn,
            cleanups,
            ..
        } = inputs.work;
        Ok(Self {
            name: format!("{}:{}", inputs.team, inputs.id),
            clicks: signal(0),
            history,
            _work: Resource::new(
                &owner,
                move || Some(key.clone()),
                move |key, token| loader.load(key, token),
                move |future| spawn(future),
            ),
            _cleanup: owner.on_cleanup(move || cleanups.set(cleanups.get() + 1)),
        })
    }
}

fusor::template!(backend = "hypercmd", "ui/routing.html");

// Existing consumers exercise structures, but cannot establish staged route/history
// rollback, nested identity, or cancellation when a callback leaves its own screen.
#[test]
fn contract() {
    let executor = TestExecutor::new();
    let loader = ControlledLoader::new();
    let cleanups = Rc::new(Cell::new(0));
    let visible = signal(true);
    let navigation = Rc::new(RefCell::new(None));
    let scope = hypercmd::mount::<Screens>(ScreensInputs {
        visible: visible.clone(),
        work: Work {
            loader: loader.clone(),
            spawn: Rc::new(executor.spawner()),
            cleanups: cleanups.clone(),
            navigation: navigation.clone(),
        },
    })
    .unwrap();
    let history = History::from_owner(&scope.owner()).unwrap();
    scope.publish();
    let root = scope.root();
    let mut controls = Controller::new(root.clone());
    super::draw(&scope, &mut controls);
    assert_eq!(controls.focus(), root.find("home"));
    history.push("/teams/alpha/members/1").unwrap();
    super::draw(&scope, &mut controls);
    assert_eq!(controls.focus(), root.find("team-counter"));
    let team = root.find("team").unwrap();
    root.find("team-counter")
        .unwrap()
        .dispatch("click")
        .unwrap();
    let member = root.find("member").unwrap();
    let counter = root.find("member-counter").unwrap();
    counter.dispatch("click").unwrap();
    controls.set_focus(&counter).unwrap();
    executor.run_until_stalled();
    let first_request = loader.next_request().unwrap();
    assert_eq!(first_request.key, "1");

    history
        .push("/teams/alpha/members/1?q=one#details")
        .unwrap();
    super::draw(&scope, &mut controls);
    assert_eq!(root.find("team"), Some(team.clone()));
    assert_eq!(root.find("member"), Some(member.clone()));
    assert_eq!(controls.focus(), Some(counter.clone()));
    assert!(counter.text().contains("1"));
    assert_eq!(
        history.location().unwrap().query_first("q").as_deref(),
        Some("one")
    );

    let before = history.location().unwrap();
    assert!(history.push("/teams/alpha/members/broken").is_err());
    assert!(history.push("/%zz").is_err());
    super::draw(&scope, &mut controls);
    assert_eq!(history.location().unwrap(), before);
    assert_eq!(root.find("member"), Some(member.clone()));
    assert_eq!(controls.focus(), Some(counter.clone()));
    assert!(!first_request.is_cancelled());

    // Stage through the supported API without changing the terminal history.
    let outlet = root.find("routing").unwrap();
    let navigation = navigation.borrow().as_ref().unwrap().clone();
    let stage = navigation
        .prepare_navigation(&AppUrl::parse("/teams/alpha/members/2").unwrap())
        .unwrap();
    let before_cleanup = cleanups.get();
    assert_eq!(history.location().unwrap(), before);
    let frame = super::frame(&root, &mut controls, (80, 40));
    let painted: String = frame
        .buffer
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(
        !painted.contains("alpha:2"),
        "inactive destination is never presented"
    );
    drop(stage);
    assert_eq!(
        cleanups.get(),
        before_cleanup + 1,
        "abandoned destination retires once"
    );
    assert_eq!(root.find("member"), Some(member.clone()));
    assert!(!first_request.is_cancelled());
    assert!(outlet.is_alive());

    history.push("/teams/alpha/members/2").unwrap();
    executor.run_until_stalled();
    assert!(first_request.is_cancelled());
    assert!(first_request.complete(Ok(())).is_err());
    assert_eq!(root.find("team"), Some(team.clone()));
    assert_eq!(root.find("team-counter").unwrap().text(), "alpha 1");
    assert_ne!(root.find("member"), Some(member));
    super::draw(&scope, &mut controls);
    assert_eq!(controls.focus(), root.find("member-counter"));
    let second_request = loader.next_request().unwrap();
    let stale = root.find("leave").unwrap();
    controls.set_focus(&stale).unwrap();
    super::key(&mut controls, hypercmd::Key::Enter, false);
    assert!(!stale.is_alive());
    super::draw(&scope, &mut controls);
    assert_eq!(controls.focus(), root.find("home"));
    stale.dispatch("click").unwrap();
    assert!(second_request.is_cancelled());
    executor.run_until_stalled();
    assert!(second_request.complete(Ok(())).is_err());

    history.back().unwrap();
    assert_eq!(history.location().unwrap().path, "/teams/alpha/members/2");
    history.back().unwrap();
    assert_eq!(history.location().unwrap(), before);
    history.forward().unwrap();
    history.push("/teams/beta/members/reentrant").unwrap();
    assert_ne!(root.find("team"), Some(team));
    history.forward().unwrap();
    assert_eq!(
        history.location().unwrap().path,
        "/teams/beta/members/reentrant"
    );
    history.replace("/missing").unwrap();
    assert!(root.find("missing").is_some());
    history.back().unwrap();
    assert_eq!(history.location().unwrap().path, "/teams/alpha/members/2");

    history.push("/redirect").unwrap();
    super::draw(&scope, &mut controls);
    assert_eq!(history.location().unwrap().path, "/teams/gamma/members/3");
    super::draw(&scope, &mut controls);
    assert_eq!(
        controls.focus(),
        root.find("team-counter"),
        "focus navigation waits for the destination layout"
    );

    visible.set(false);
    assert!(history.back().is_err());
    assert!(history.forward().is_err());
    scope.dispose();
    executor.run_until_stalled();
    assert_eq!(loader.counts().live, 0);
    assert!(history.push("/").is_err());
    assert!(cleanups.get() >= 5);
    assert!(scope.take_errors().is_empty());
}

struct Home {
    visits: Signal<u32>,
}
struct HomeInputs {
    navigation: Rc<RefCell<Option<Navigation<Scope>>>>,
}
impl FromInputs for Home {
    type Inputs = HomeInputs;
    type Error = std::convert::Infallible;
    fn from_inputs(inputs: Self::Inputs, owner: OwnerHandle) -> Result<Self, Self::Error> {
        inputs.navigation.replace(Navigation::from_owner(&owner));
        Ok(Self { visits: signal(0) })
    }
}
