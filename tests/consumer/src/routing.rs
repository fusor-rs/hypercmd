use fusor::{FromInputs, OwnerHandle, Registration, Signal, signal};
use fusor_async::{Resource, Spawner};
use fusor_router::{AppUrl, view::Navigation};
use fusor_test::{ControlledLoader, PendingRequest, TestExecutor};
use hypercmd::{Controller, Error, ErrorKind, History, Node, Scope};
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
    let mut routing = mount_routing();
    let member = enter_member(&mut routing);
    same_route_keeps_identity(&mut routing, &member);
    let before = rejected_navigation_rolls_back(&mut routing, &member);
    abandoned_stage_retires(&mut routing, &member, &before);
    let team = leaving_cancels_work(&mut routing, member);
    history_traversal(&routing, &team, &before);
    redirect_moves_focus(&mut routing);
    disposal(&routing);
}

struct RoutingHarness {
    controls: Controller,
    root: Node,
    history: History,
    scope: Scope,
    navigation: Rc<RefCell<Option<Navigation<Scope>>>>,
    visible: Signal<bool>,
    cleanups: Rc<Cell<usize>>,
    loader: ControlledLoader<String, (), String>,
    executor: TestExecutor,
}

struct MemberScreen {
    team: Node,
    member: Node,
    counter: Node,
    first_request: PendingRequest<String, (), String>,
}

fn mount_routing() -> RoutingHarness {
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
    RoutingHarness {
        controls,
        root,
        history,
        scope,
        navigation,
        visible,
        cleanups,
        loader,
        executor,
    }
}

fn enter_member(routing: &mut RoutingHarness) -> MemberScreen {
    let RoutingHarness {
        controls,
        root,
        history,
        scope,
        ..
    } = routing;
    history.push("/teams/alpha/members/1").unwrap();
    super::draw(scope, controls);
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
    routing.executor.run_until_stalled();
    let first_request = routing.loader.next_request().unwrap();
    assert_eq!(first_request.key, "1");
    MemberScreen {
        team,
        member,
        counter,
        first_request,
    }
}

fn same_route_keeps_identity(routing: &mut RoutingHarness, screen: &MemberScreen) {
    let RoutingHarness {
        controls,
        root,
        history,
        scope,
        ..
    } = routing;
    history
        .push("/teams/alpha/members/1?q=one#details")
        .unwrap();
    super::draw(scope, controls);
    assert_eq!(root.find("team"), Some(screen.team.clone()));
    assert_eq!(root.find("member"), Some(screen.member.clone()));
    assert_eq!(controls.focus(), Some(screen.counter.clone()));
    assert!(screen.counter.text().contains("1"));
    assert_eq!(
        history.location().unwrap().query_first("q").as_deref(),
        Some("one")
    );
}

fn rejected_navigation_rolls_back(routing: &mut RoutingHarness, screen: &MemberScreen) -> AppUrl {
    let RoutingHarness {
        controls,
        root,
        history,
        scope,
        ..
    } = routing;
    let before = history.location().unwrap();
    assert!(history.push("/teams/alpha/members/broken").is_err());
    assert!(history.push("/%zz").is_err());
    super::draw(scope, controls);
    assert_eq!(history.location().unwrap(), before);
    assert_eq!(root.find("member"), Some(screen.member.clone()));
    assert_eq!(controls.focus(), Some(screen.counter.clone()));
    assert!(!screen.first_request.is_cancelled());
    before
}

// Stage through the supported API without changing the terminal history.
fn abandoned_stage_retires(routing: &mut RoutingHarness, screen: &MemberScreen, before: &AppUrl) {
    let RoutingHarness {
        controls,
        root,
        history,
        navigation,
        cleanups,
        ..
    } = routing;
    let outlet = root.find("routing").unwrap();
    let navigation = navigation.borrow().as_ref().unwrap().clone();
    let stage = navigation
        .prepare_navigation(&AppUrl::parse("/teams/alpha/members/2").unwrap())
        .unwrap();
    let before_cleanup = cleanups.get();
    assert_eq!(&history.location().unwrap(), before);
    let frame = super::frame(root, controls, (80, 40));
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
    assert_eq!(root.find("member"), Some(screen.member.clone()));
    assert!(!screen.first_request.is_cancelled());
    assert!(outlet.is_alive());
}

fn leaving_cancels_work(routing: &mut RoutingHarness, screen: MemberScreen) -> Node {
    let RoutingHarness {
        controls,
        root,
        history,
        scope,
        loader,
        executor,
        ..
    } = routing;
    history.push("/teams/alpha/members/2").unwrap();
    executor.run_until_stalled();
    assert!(screen.first_request.is_cancelled());
    assert!(screen.first_request.complete(Ok(())).is_err());
    assert_eq!(root.find("team"), Some(screen.team.clone()));
    assert_eq!(root.find("team-counter").unwrap().text(), "alpha 1");
    assert_ne!(root.find("member"), Some(screen.member));
    super::draw(scope, controls);
    assert_eq!(controls.focus(), root.find("member-counter"));
    let second_request = loader.next_request().unwrap();
    let stale = root.find("leave").unwrap();
    controls.set_focus(&stale).unwrap();
    super::key(controls, hypercmd::Key::Enter, false);
    assert!(!stale.is_alive());
    super::draw(scope, controls);
    assert_eq!(controls.focus(), root.find("home"));
    stale.dispatch("click").unwrap();
    assert!(second_request.is_cancelled());
    executor.run_until_stalled();
    assert!(second_request.complete(Ok(())).is_err());
    screen.team
}

fn history_traversal(routing: &RoutingHarness, team: &Node, before: &AppUrl) {
    let RoutingHarness { root, history, .. } = routing;
    history.back().unwrap();
    assert_eq!(history.location().unwrap().path, "/teams/alpha/members/2");
    history.back().unwrap();
    assert_eq!(&history.location().unwrap(), before);
    history.forward().unwrap();
    history.push("/teams/beta/members/reentrant").unwrap();
    assert_ne!(root.find("team"), Some(team.clone()));
    history.forward().unwrap();
    assert_eq!(
        history.location().unwrap().path,
        "/teams/beta/members/reentrant"
    );
    history.replace("/missing").unwrap();
    assert!(root.find("missing").is_some());
    history.back().unwrap();
    assert_eq!(history.location().unwrap().path, "/teams/alpha/members/2");
}

fn redirect_moves_focus(routing: &mut RoutingHarness) {
    let RoutingHarness {
        controls,
        root,
        history,
        scope,
        ..
    } = routing;
    history.push("/redirect").unwrap();
    super::draw(scope, controls);
    assert_eq!(history.location().unwrap().path, "/teams/gamma/members/3");
    super::draw(scope, controls);
    assert_eq!(
        controls.focus(),
        root.find("team-counter"),
        "focus navigation waits for the destination layout"
    );
}

fn disposal(routing: &RoutingHarness) {
    let RoutingHarness {
        history,
        scope,
        visible,
        cleanups,
        loader,
        executor,
        ..
    } = routing;
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
