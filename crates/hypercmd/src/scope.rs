use crate::{
    Error, Node,
    scene::{Scene, ScopeShared},
};
use fusor::{ContextKey, Effect, Owner, OwnerHandle, Registration};
use std::{
    any::Any,
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};

pub type Children = fusor::render::Children<Scope, Error>;

/// Implemented by generated HTML in the component's own Rust module.
pub trait Component: Sized + 'static {
    fn prepare(
        parent: Option<&OwnerHandle>,
        make: Box<dyn FnOnce(OwnerHandle) -> Result<Self, Error> + '_>,
        children: Children,
    ) -> Result<Scope, Error>;
}

pub struct StaticNode {
    pub parent: Option<usize>,
    pub kind: Kind,
}
pub enum Kind {
    Element(
        &'static str,
        &'static [(&'static str, &'static str)],
        Option<usize>,
    ),
    Text(&'static str, Option<usize>),
    Mount(usize),
    Comment,
}

struct SceneContext;
impl ContextKey for SceneContext {
    type Value = Rc<Scene>;
}

#[derive(Default)]
pub(crate) struct Instance {
    pub root: RefCell<std::rc::Weak<crate::scene::NodeData>>,
}
struct InstanceContext;
impl ContextKey for InstanceContext {
    type Value = Rc<Instance>;
}
thread_local! {
    static COMPONENT: RefCell<Option<Rc<Instance>>> = const { RefCell::new(None) };
}

/// Establish an instance boundary for a generated component's first scope.
#[doc(hidden)]
pub fn component_scope<T>(make: impl FnOnce() -> T) -> T {
    struct Restore(Option<Rc<Instance>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            COMPONENT.with(|current| {
                current.replace(self.0.take());
            });
        }
    }
    let instance = Rc::default();
    let _restore = Restore(COMPONENT.with(|current| current.replace(Some(instance))));
    make()
}

#[derive(Default)]
pub(crate) struct Retained {
    pub effects: Vec<Effect>,
    pub values: Vec<Rc<dyn Any>>,
}

/// A prepared component. Publish only after all fallible preparation succeeds.
pub struct Scope {
    pub(crate) owner: Owner,
    pub(crate) root: Node,
    pub(crate) elements: BTreeMap<usize, Node>,
    pub(crate) texts: BTreeMap<usize, Node>,
    pub(crate) mounts: BTreeMap<usize, Node>,
    pub(crate) retained: Rc<RefCell<Retained>>,
    pub(crate) coherent: Option<Rc<crate::coherent::Tree>>,
    pub(crate) attached: Option<Node>,
    _cleanup: Registration,
}

impl Drop for Scope {
    fn drop(&mut self) {
        self.owner.dispose();
        if let Some(target) = self.attached.take() {
            target.replace_children(
                target
                    .children()
                    .into_iter()
                    .filter(|node| node != &self.root)
                    .collect(),
            );
        }
    }
}
impl fusor::render::Scope for Scope {
    fn owner(&self) -> OwnerHandle {
        self.owner()
    }
    fn retain_state<T: 'static>(&mut self, value: T) -> Rc<T> {
        self.retain(value)
    }
    fn prepares_effects(&self) -> bool {
        self.is_coherent()
    }
}

impl Scope {
    pub fn new(parent: Option<&OwnerHandle>, specs: &[StaticNode]) -> Result<Self, Error> {
        Self::with_styles(parent, specs, &[])
    }

    pub fn with_styles(
        parent: Option<&OwnerHandle>,
        specs: &[StaticNode],
        styles: crate::style::StyleSheet,
    ) -> Result<Self, Error> {
        let owner = parent.map_or_else(Owner::new, Owner::child);
        if owner.handle().is_disposed() {
            return Err(Error::template("cannot mount below a disposed owner"));
        }
        if owner
            .handle()
            .context::<crate::coherent::Context>()
            .is_some()
            && specs
                .iter()
                .any(|spec| matches!(spec.kind, Kind::Element("input", _, _)))
        {
            return Err(Error::template(
                "editable controls are unsupported inside coherent regions",
            ));
        }
        let (shared, shutdown_services) = provide_contexts(&owner.handle(), parent, styles)?;
        let root = Node::new(&shared, "#scope", "", &[]);
        if shared.component.root.borrow().upgrade().is_none() {
            shared.component.root.replace(Rc::downgrade(&root.0));
        }
        let (
            nodes,
            Anchors {
                elements,
                texts,
                mounts,
            },
        ) = instantiate(&shared, &root, specs)?;
        let scene = shared.scene;
        let retained = Rc::new(RefCell::new(Retained::default()));
        let weak = Rc::downgrade(&retained);
        let coherent =
            crate::coherent::Tree::inherited(&owner.handle(), &elements, &texts, &mounts);
        let weak_coherent = coherent.as_ref().map(Rc::downgrade);
        let cleanup = owner.handle().on_cleanup(move || {
            // Invalidation precedes cleanup; release all borrows before user drops.
            let listeners: Vec<_> = nodes.iter().map(|node| node.0.listeners.take()).collect();
            let values = weak.upgrade().map(|values| values.take());
            if let Some(services) = &shutdown_services {
                services.shutdown();
            }
            if let Some(tree) = weak_coherent.as_ref().and_then(std::rc::Weak::upgrade) {
                tree.clear();
            }
            scene.changed();
            drop(listeners);
            drop(values);
        });
        Ok(Self {
            owner,
            root,
            elements,
            texts,
            mounts,
            retained,
            coherent,
            attached: None,
            _cleanup: cleanup,
        })
    }
    pub fn owner(&self) -> OwnerHandle {
        self.owner.handle()
    }
    pub fn root(&self) -> Node {
        self.root.clone()
    }
    pub fn publish(&self) {
        self.owner.commit();
        self.root.0.scene.changed();
    }
    pub fn dispose(&self) {
        self.owner.dispose();
    }
    pub fn take_errors(&self) -> Vec<Error> {
        self.root.0.scene.errors.take()
    }
    pub fn take_dirty(&self) -> bool {
        self.root.0.scene.dirty.replace(false)
    }
    /// A publication fault makes further interaction unsafe; the host must exit.
    pub fn is_faulted(&self) -> bool {
        self.root.0.scene.faulted.get()
    }
    pub fn retain<T: 'static>(&mut self, value: T) -> Rc<T> {
        let value = Rc::new(value);
        self.retained.borrow_mut().values.push(value.clone());
        value
    }
    pub(crate) fn element(&self, id: usize) -> Result<Node, Error> {
        anchor(&self.elements, "element", id)
    }
    pub(crate) fn mount(&self, id: usize) -> Result<Node, Error> {
        anchor(&self.mounts, "mount", id)
    }
    pub(crate) fn watch(
        &mut self,
        mut update: impl FnMut() -> Result<(), Error> + 'static,
    ) -> Result<(), Error> {
        let initial = Rc::new(RefCell::new(None));
        let result = initial.clone();
        let scene = self.root.0.scene.clone();
        let mut first = true;
        let owner = self.owner();
        let effect = fusor::effect(move || {
            if owner.is_disposed() {
                return;
            }
            if let Err(error) = update() {
                if first {
                    result.replace(Some(error));
                } else {
                    scene.report(error);
                }
            }
            first = false;
        });
        if let Some(error) = initial.take() {
            return Err(error);
        }
        self.retained.borrow_mut().effects.push(effect);
        Ok(())
    }
}

pub(crate) fn anchor(map: &BTreeMap<usize, Node>, kind: &str, id: usize) -> Result<Node, Error> {
    map.get(&id)
        .cloned()
        .ok_or_else(|| Error::template(format!("missing {kind} anchor {id}")))
}

// A child scope inherits its parent's services, scene and component instance;
// a root scope creates them and shuts its services down on cleanup.
fn provide_contexts(
    owner: &OwnerHandle,
    parent: Option<&OwnerHandle>,
    styles: crate::style::StyleSheet,
) -> Result<(ScopeShared, Option<crate::Services>), Error> {
    let inherited_services = parent.and_then(|parent| parent.context::<crate::Services>());
    let services = inherited_services
        .as_deref()
        .cloned()
        .unwrap_or_else(crate::Services::new);
    let shutdown_services = inherited_services.is_none().then(|| services.clone());
    owner.provide::<crate::Services>(services)?;
    let scene = parent
        .and_then(|parent| parent.context::<SceneContext>())
        .map(|scene| (*scene).clone())
        .unwrap_or_default();
    owner.provide::<SceneContext>(scene.clone())?;
    let component = COMPONENT
        .with(|current| current.take())
        .or_else(|| {
            parent
                .and_then(|parent| parent.context::<InstanceContext>())
                .map(|value| (*value).clone())
        })
        .unwrap_or_default();
    owner.provide::<InstanceContext>(component.clone())?;
    let shared = ScopeShared {
        owner: owner.clone(),
        scene,
        styles,
        component,
    };
    Ok((shared, shutdown_services))
}

#[derive(Default)]
struct Anchors {
    elements: BTreeMap<usize, Node>,
    texts: BTreeMap<usize, Node>,
    mounts: BTreeMap<usize, Node>,
}

fn instantiate(
    shared: &ScopeShared,
    root: &Node,
    specs: &[StaticNode],
) -> Result<(Vec<Node>, Anchors), Error> {
    let mut nodes: Vec<Node> = Vec::with_capacity(specs.len());
    let mut anchors = Anchors::default();
    let mut ids = BTreeSet::new();
    for spec in specs {
        let (tag, text, attrs) = match spec.kind {
            Kind::Element(tag, attrs, _) => (tag, "", attrs),
            Kind::Text(text, _) => ("#text", text, &[][..]),
            Kind::Mount(_) => ("#mount", "", &[][..]),
            Kind::Comment => ("#comment", "", &[][..]),
        };
        let node = Node::new(shared, tag, text, attrs);
        if node.attribute("id").is_some_and(|id| !ids.insert(id)) {
            return Err(Error::template("duplicate id in component template"));
        }
        let anchor = match spec.kind {
            Kind::Element(_, _, Some(id)) => Some((&mut anchors.elements, id)),
            Kind::Text(_, Some(id)) => Some((&mut anchors.texts, id)),
            Kind::Mount(id) => Some((&mut anchors.mounts, id)),
            _ => None,
        };
        if anchor.is_some_and(|(map, id)| map.insert(id, node.clone()).is_some()) {
            return Err(Error::template("duplicate template anchor"));
        }
        let parent = match spec.parent {
            Some(index) => nodes
                .get(index)
                .ok_or_else(|| Error::template("template parent must precede child"))?,
            None => root,
        };
        parent.0.children.borrow_mut().push(node.clone());
        nodes.push(node);
    }
    Ok((nodes, anchors))
}
