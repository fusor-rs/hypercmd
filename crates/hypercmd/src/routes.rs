use crate::{Error, Node, Scope};
use fusor::{ContextKey, OwnerHandle};
use fusor_router::{
    AppUrl,
    view::{Navigation, RouteScope, RouteView, ViewRouter},
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

const _: () = assert!(fusor_router::view::VERSION == 1);
const HISTORY_LIMIT: usize = 64;

struct HistoryContext;
impl ContextKey for HistoryContext {
    type Value = Rc<HistoryState>;
}
struct RouteContext;
impl ContextKey for RouteContext {
    type Value = ();
}
struct HistoryState {
    entries: RefCell<Vec<AppUrl>>,
    index: Cell<usize>,
    changing: Cell<bool>,
    router: RefCell<Option<(OwnerHandle, Navigation<Scope>)>>,
}

impl HistoryState {
    fn current(&self) -> AppUrl {
        self.entries.borrow()[self.index.get()].clone()
    }
}

/// In-memory screen history, shared by a router and its nested views.
/// Retains at most 64 locations; pushing after Back discards forward history.
/// Handles do not extend the supplying owner's lifetime.
#[derive(Clone)]
pub struct History {
    owner: OwnerHandle,
    state: Rc<HistoryState>,
}

impl History {
    /// Select the initial location before this owner's root Router mounts.
    /// Without an explicit installation, Router starts at `/`.
    pub fn install(owner: &OwnerHandle, initial: &str) -> Result<Self, Error> {
        let state = Rc::new(HistoryState {
            entries: RefCell::new(vec![parse(initial)?]),
            index: Cell::new(0),
            changing: Cell::new(false),
            router: RefCell::new(None),
        });
        owner.provide::<HistoryContext>(state.clone())?;
        Ok(Self {
            owner: owner.clone(),
            state,
        })
    }

    /// Resolve the nearest screen history. A view-local handle becomes inert
    /// when that view leaves; use a root-owner handle for application navigation.
    pub fn from_owner(owner: &OwnerHandle) -> Option<Self> {
        Some(Self {
            owner: owner.clone(),
            state: (*owner.context::<HistoryContext>()?).clone(),
        })
    }

    /// Read the reactive location, including a prepared view's destination.
    pub fn location(&self) -> Result<AppUrl, Error> {
        Ok(self.navigation()?.location().get())
    }

    pub fn push(&self, path: &str) -> Result<(), Error> {
        let url = parse(path)?;
        self.change(move |entries, index| {
            entries.truncate(*index + 1);
            entries.push(url);
            if entries.len() > HISTORY_LIMIT {
                entries.remove(0);
            }
            *index = entries.len() - 1;
            true
        })
    }

    pub fn replace(&self, path: &str) -> Result<(), Error> {
        let url = parse(path)?;
        self.change(move |entries, index| {
            entries[*index] = url;
            true
        })
    }

    /// No-op at the oldest retained location; errors for a disposed router.
    pub fn back(&self) -> Result<(), Error> {
        self.change(|_, index| {
            if *index == 0 {
                return false;
            }
            *index -= 1;
            true
        })
    }

    /// No-op at the newest location; errors for a disposed router.
    pub fn forward(&self) -> Result<(), Error> {
        self.change(|entries, index| {
            if *index + 1 == entries.len() {
                return false;
            }
            *index += 1;
            true
        })
    }

    fn navigation(&self) -> Result<Navigation<Scope>, Error> {
        let unavailable = || Error::navigation("screen router is not mounted or was disposed");
        if self.owner.is_disposed() {
            return Err(unavailable());
        }
        if let Some(navigation) = Navigation::from_owner(&self.owner) {
            return Ok(navigation);
        }
        let router = self.state.router.borrow();
        let (owner, navigation) = router.as_ref().ok_or_else(unavailable)?;
        Navigation::<Scope>::from_owner(owner).ok_or_else(unavailable)?;
        Ok(navigation.clone())
    }

    fn change(
        &self,
        update: impl FnOnce(&mut Vec<AppUrl>, &mut usize) -> bool,
    ) -> Result<(), Error> {
        let navigation = self.navigation()?;
        if self.state.changing.replace(true) {
            return Err(Error::navigation(
                "reentrant history navigation is not supported",
            ));
        }
        struct Reset<'a>(&'a Cell<bool>);
        impl Drop for Reset<'_> {
            fn drop(&mut self) {
                self.0.set(false);
            }
        }
        let _reset = Reset(&self.state.changing);
        let mut entries = self.state.entries.borrow().clone();
        let mut index = self.state.index.get();
        if !update(&mut entries, &mut index) {
            return Ok(());
        }
        let prepared = navigation.prepare_navigation(&entries[index])?;
        // Constructors can dispose an outlet; never publish history for that stage.
        self.navigation()?;
        self.state.entries.replace(entries);
        self.state.index.set(index);
        prepared.commit();
        Ok(())
    }
}

fn parse(path: &str) -> Result<AppUrl, Error> {
    AppUrl::parse(path).map_err(|error| Error::navigation(error.to_string()))
}

impl RouteScope for Scope {
    type Target = Node;
    type Error = Error;

    fn error(message: &str) -> Error {
        Error::navigation(message)
    }

    fn prepare_at(&mut self, target: &Node, _parent_active: bool) -> Result<(), Error> {
        if self.attached.is_some() || self.owner().is_active() || !target.is_alive() {
            return Err(Error::navigation(
                "route view must be detached and prepared",
            ));
        }
        let mut children = target.children();
        children.push(self.root());
        target.set_children(children)?;
        self.attached = Some(target.clone());
        Ok(())
    }

    fn commit(&self) {
        let focus = &self.root.0.scene.route_focus;
        if !self.owner().is_active()
            && focus
                .borrow()
                .upgrade()
                .is_none_or(|node| node.owner.is_disposed())
        {
            focus.replace(Rc::downgrade(&self.root.0));
        }
        self.publish();
    }
}

impl Scope {
    /// Mark factories supplied by fusor so history distinguishes nested outlets.
    #[doc(hidden)]
    pub fn route_factory(
        prepare: impl Fn(&OwnerHandle, &fusor_router::pattern::Match) -> Result<Self, Error> + 'static,
    ) -> impl Fn(&OwnerHandle, &fusor_router::pattern::Match) -> Result<Self, Error> {
        move |owner, matched| {
            owner.provide::<RouteContext>(())?;
            prepare(owner, matched)
        }
    }

    /// Generated outlet installation. Route matching and retention belong to fusor.
    #[doc(hidden)]
    pub fn routes(&mut self, id: usize, routes: Vec<RouteView<Self>>) -> Result<(), Error> {
        if self.is_coherent() {
            return Err(Error::template(
                "Router cannot mount inside a coherent region; move it outside Async",
            ));
        }
        let owner = self.owner();
        let history = match History::from_owner(&owner) {
            Some(history) => history,
            None => History::install(&owner, "/")?,
        };
        let initial = history.state.current();
        let nested = owner.context::<RouteContext>().is_some();
        if !nested
            && history
                .state
                .router
                .borrow()
                .as_ref()
                .is_some_and(|(owner, _)| Navigation::<Scope>::from_owner(owner).is_some())
        {
            return Err(Error::navigation(
                "a history supports one root Router; install a separate History for another root",
            ));
        }
        let router = ViewRouter::mount(&owner, &self.mount(id)?, routes, initial)?;
        if !nested {
            history
                .state
                .router
                .replace(Some((owner, router.navigation())));
        }
        self.retain(router);
        Ok(())
    }
}
