//! Staged scene publication for fusor's executor-independent coherence protocol.
use crate::{
    Children, Component, Error, ErrorKind, Event, EventResult, Node, Scope, scene::Listener,
};
use fusor::{
    ContextKey, OwnerHandle, Signal,
    coherence::{AsyncBoundary, Attempt, BoundaryStatus, Publication},
    versions::Versions,
};
use std::{
    any::Any,
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet},
    fmt::Display,
    rc::Rc,
};

pub(crate) struct Context;
impl ContextKey for Context {
    type Value = Rc<Gate>;
}
pub(crate) struct Gate {
    pub boundary: AsyncBoundary,
    pub published: Cell<bool>,
}
type Renderer = dyn Fn(&mut Frame<'_>) -> Result<(), String>;
type Listeners = BTreeMap<&'static str, Vec<Listener>>;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum SlotKey {
    Branch(usize),
    Rows(usize),
    Component(usize),
    Children(usize),
}

pub(crate) struct Tree {
    owner: OwnerHandle,
    elements: BTreeMap<usize, Node>,
    texts: BTreeMap<usize, Node>,
    mounts: BTreeMap<usize, Node>,
    renderer: RefCell<Option<Rc<Renderer>>>,
    slots: RefCell<BTreeMap<SlotKey, Rc<dyn Any>>>,
}
impl Tree {
    pub(crate) fn inherited(
        owner: &OwnerHandle,
        elements: &BTreeMap<usize, Node>,
        texts: &BTreeMap<usize, Node>,
        mounts: &BTreeMap<usize, Node>,
    ) -> Option<Rc<Self>> {
        owner.context::<Context>().map(|_| {
            Rc::new(Self {
                owner: owner.clone(),
                elements: elements.clone(),
                texts: texts.clone(),
                mounts: mounts.clone(),
                renderer: RefCell::default(),
                slots: RefCell::default(),
            })
        })
    }
    pub(crate) fn clear(&self) {
        drop((self.slots.take(), self.renderer.take()));
    }
}

impl Scope {
    pub fn is_coherent(&self) -> bool {
        self.coherent.is_some()
    }
    pub fn set_coherent_renderer(
        &mut self,
        render: impl Fn(&mut Frame<'_>) -> Result<(), String> + 'static,
    ) {
        if let Some(tree) = &self.coherent {
            tree.renderer.replace(Some(Rc::new(render)));
        }
    }
    pub fn async_region(
        &mut self,
        id: usize,
        boundary: AsyncBoundary,
        render: impl Fn(&mut Frame<'_>) -> Result<(), String> + 'static,
    ) -> Result<(), Error> {
        if self.is_coherent() {
            return Err(Error::template("nested Async boundaries are unsupported"));
        }
        let root = self.element(id)?;
        let nodes: Vec<_> = root.descendants().collect();
        if nodes.iter().any(|node| node.tag() == "input") {
            return Err(Error::template(
                "editable controls are unsupported inside coherent regions",
            ));
        }
        let gate = Rc::new(Gate {
            boundary: boundary.clone(),
            published: Cell::new(false),
        });
        let mut region = Scope::new(Some(&self.owner()), &[])?;
        region.owner().provide::<Context>(gate.clone())?;
        let within = |anchors: &BTreeMap<usize, Node>| {
            anchors
                .iter()
                .filter(|(_, node)| nodes.contains(node))
                .map(|(id, node)| (*id, node.clone()))
                .collect()
        };
        let tree = Tree::inherited(
            &region.owner(),
            &within(&self.elements),
            &within(&self.texts),
            &within(&self.mounts),
        )
        .expect("region context was installed");
        region.coherent = Some(tree.clone());
        region.set_coherent_renderer(render);
        for node in nodes {
            node.0.gate.replace(Some(gate.clone()));
        }
        let weak = Rc::downgrade(&tree);
        region.retain(region.owner().on_cleanup(move || {
            if let Some(tree) = weak.upgrade() {
                tree.clear();
            }
        }));
        let scene = self.root.0.scene.clone();
        let scene_for_publication = scene.clone();
        let mounted = boundary
            .attach(&region.owner(), move |attempt| {
                let mut prepared = Prepared {
                    scene: scene_for_publication.clone(),
                    gate: gate.clone(),
                    owners: Vec::new(),
                    updates: BTreeMap::new(),
                    finish: Vec::new(),
                };
                visit(&tree, attempt, &mut prepared)?;
                Ok(Box::new(prepared))
            })
            .map_err(Error::template)?;
        region.retain(mounted);
        region.retain(fusor::effect(move || match boundary.status() {
            BoundaryStatus::Error(message) => {
                scene.report(Error::new(ErrorKind::Publication, message))
            }
            BoundaryStatus::Faulted(message) => {
                scene.faulted.set(true);
                scene.report(Error::new(ErrorKind::Publication, message));
            }
            _ => scene.changed(),
        }));
        region.publish();
        self.retain(region);
        Ok(())
    }
}

fn identity(node: &Node) -> usize {
    Rc::as_ptr(&node.0) as usize
}
struct Update {
    node: Node,
    text: Option<String>,
    attrs: Option<BTreeMap<String, String>>,
    children: Option<Vec<Node>>,
    listeners: Option<Listeners>,
}
struct Prepared {
    scene: Rc<crate::scene::Scene>,
    gate: Rc<Gate>,
    owners: Vec<OwnerHandle>,
    updates: BTreeMap<usize, Update>,
    finish: Vec<Box<dyn FnOnce()>>,
}
impl Prepared {
    fn update(&mut self, node: Node) -> &mut Update {
        self.updates
            .entry(identity(&node))
            .or_insert_with(|| Update {
                node,
                text: None,
                attrs: None,
                children: None,
                listeners: None,
            })
    }
}
impl Publication for Prepared {
    fn validate(&self) -> Result<(), String> {
        if self.scene.faulted.get() || self.owners.iter().any(OwnerHandle::is_disposed) {
            return Err("coherent scene was disposed or faulted before publication".into());
        }
        let mut roots = BTreeMap::new();
        for update in self.updates.values() {
            if !update.node.is_alive() {
                return Err("coherent target was disposed before publication".into());
            }
            let root = update.node.component_root();
            roots.insert(identity(&root), root);
        }
        // Validate all simultaneous child replacements together, including detached
        // candidates. Per-patch checks would miss duplicate IDs introduced by siblings.
        for root in roots.values() {
            crate::scene::validate_tree(
                root,
                |node| {
                    self.updates
                        .get(&identity(node))
                        .and_then(|update| update.children.clone())
                        .unwrap_or_else(|| node.children())
                },
                |node| {
                    self.updates
                        .get(&identity(node))
                        .and_then(|update| update.attrs.as_ref())
                        .map_or_else(|| node.attribute("id"), |attrs| attrs.get("id").cloned())
                },
            )
            .map_err(|error| error.to_string())?;
        }
        Ok(())
    }
    fn apply(&mut self) -> Result<(), String> {
        for update in self.updates.values_mut() {
            if let Some(text) = &mut update.text {
                std::mem::swap(&mut *update.node.0.text.borrow_mut(), text);
            }
            if let Some(attrs) = &mut update.attrs {
                std::mem::swap(&mut *update.node.0.attrs.borrow_mut(), attrs);
            }
            if let Some(children) = &mut update.children {
                std::mem::swap(&mut *update.node.0.children.borrow_mut(), children);
            }
            if let Some(listeners) = &mut update.listeners {
                std::mem::swap(&mut *update.node.0.listeners.borrow_mut(), listeners);
            }
        }
        self.gate.published.set(true);
        self.scene.changed();
        Ok(())
    }
    fn finish(self: Box<Self>) {
        for finish in self.finish {
            finish();
        }
        // Old scene nodes and callback captures stay in updates until activation
        // completes, so user destructors never run during apply.
    }
}

/// A single evaluation. Readers and structural factories may borrow this attempt.
pub struct Frame<'a> {
    pub attempt: &'a Attempt,
    tree: &'a Rc<Tree>,
    prepared: &'a mut Prepared,
    listeners: BTreeMap<usize, Listeners>,
}
fn visit(tree: &Rc<Tree>, attempt: &Attempt, prepared: &mut Prepared) -> Result<(), String> {
    if tree.owner.is_disposed() {
        return Err("coherent scope was disposed during evaluation".into());
    }
    let render = tree
        .renderer
        .borrow()
        .clone()
        .ok_or("component has no coherent renderer")?;
    prepared.owners.push(tree.owner.clone());
    let mut frame = Frame {
        attempt,
        tree,
        prepared,
        listeners: BTreeMap::new(),
    };
    render(&mut frame)?;
    for (id, node) in &tree.elements {
        frame.prepared.update(node.clone()).listeners =
            Some(frame.listeners.remove(id).unwrap_or_default());
    }
    Ok(())
}

struct Slot<V> {
    epoch: u64,
    committed: V,
    candidate: V,
}
impl<V> Slot<V> {
    fn find<R>(&self, find: impl Fn(&V) -> Option<R>) -> Option<R> {
        find(&self.committed).or_else(|| find(&self.candidate))
    }
}
fn stage<V, R>(slot: &RefCell<Slot<V>>, update: impl FnOnce(&mut V) -> R) {
    let retired = update(&mut slot.borrow_mut().candidate);
    drop(retired);
}
fn prepared<T>(prepare: impl FnOnce() -> Result<T, Error>) -> Result<T, String> {
    fusor::untrack(prepare).map_err(|error| error.to_string())
}
type Child<K> = Option<(K, Rc<Scope>)>;
type Row<T> = (Signal<T>, Rc<Scope>);
impl Frame<'_> {
    pub fn reject(&self, reason: &str) -> Result<(), String> {
        Err(reason.into())
    }
    fn slot<V: Default + 'static>(&self, key: SlotKey) -> Result<Rc<RefCell<Slot<V>>>, String> {
        let erased = self
            .tree
            .slots
            .borrow_mut()
            .entry(key)
            .or_insert_with(|| {
                Rc::new(RefCell::new(Slot {
                    epoch: 0,
                    committed: V::default(),
                    candidate: V::default(),
                }))
            })
            .clone();
        let slot = erased
            .downcast::<RefCell<Slot<V>>>()
            .map_err(|_| "coherent slot type changed")?;
        let discarded = {
            let mut state = slot.borrow_mut();
            if state.epoch == self.attempt.epoch() {
                None
            } else {
                state.epoch = self.attempt.epoch();
                Some(std::mem::take(&mut state.candidate))
            }
        };
        if discarded.is_some() {
            let weak = Rc::downgrade(&slot);
            self.attempt.on_invalidate(move || {
                if let Some(slot) = weak.upgrade() {
                    let candidate = {
                        let mut state = slot.borrow_mut();
                        state.epoch = 0;
                        std::mem::take(&mut state.candidate)
                    };
                    drop(candidate);
                }
            });
        }
        drop(discarded);
        Ok(slot)
    }
    fn adopt<V: Default + 'static>(
        &mut self,
        slot: Rc<RefCell<Slot<V>>>,
        next: V,
        activate: impl FnOnce() + 'static,
    ) {
        self.prepared.finish.push(Box::new(move || {
            let retired = {
                let mut state = slot.borrow_mut();
                (
                    std::mem::replace(&mut state.committed, next),
                    std::mem::take(&mut state.candidate),
                )
            };
            activate();
            drop(retired);
        }));
    }
    fn scope(&mut self, scope: &Scope) -> Result<(), String> {
        if !scope.owner().is_child_of(&self.tree.owner) {
            return Err("coherent child belongs to another owner".into());
        }
        visit(
            scope
                .coherent
                .as_ref()
                .ok_or("component cannot render coherently")?,
            self.attempt,
            self.prepared,
        )
    }
    fn element(&self, id: usize) -> Result<Node, String> {
        crate::scope::anchor(&self.tree.elements, "element", id).map_err(|error| error.to_string())
    }
    fn mount(&self, id: usize) -> Result<Node, String> {
        crate::scope::anchor(&self.tree.mounts, "mount", id).map_err(|error| error.to_string())
    }
    pub fn text<T: Display>(&mut self, id: usize, read: impl Fn() -> T) -> Result<(), String> {
        let node = crate::scope::anchor(&self.tree.texts, "text", id)
            .map_err(|error| error.to_string())?;
        self.prepared.update(node).text = Some(read().to_string());
        Ok(())
    }
    pub fn attribute<T: Display>(
        &mut self,
        id: usize,
        name: &'static str,
        read: impl Fn() -> T,
    ) -> Result<(), String> {
        self.attr(id, name, Some(read().to_string()))
    }
    pub fn boolean(
        &mut self,
        id: usize,
        name: &'static str,
        read: impl Fn() -> bool,
    ) -> Result<(), String> {
        self.attr(id, name, read().then(String::new))
    }
    fn staged_attrs(&mut self, id: usize) -> Result<&mut BTreeMap<String, String>, String> {
        let node = self.element(id)?;
        Ok(self
            .prepared
            .update(node.clone())
            .attrs
            .get_or_insert_with(|| node.0.attrs.borrow().clone()))
    }
    fn attr(&mut self, id: usize, name: &str, value: Option<String>) -> Result<(), String> {
        let attrs = self.staged_attrs(id)?;
        if let Some(value) = value {
            attrs.insert(name.into(), value);
        } else {
            attrs.remove(name);
        }
        Ok(())
    }
    pub fn class(
        &mut self,
        id: usize,
        name: &'static str,
        read: impl Fn() -> bool,
    ) -> Result<(), String> {
        let enabled = read();
        let attrs = self.staged_attrs(id)?;
        let class = attrs.get("class").map(String::as_str).unwrap_or_default();
        let class = crate::text::toggle_class(class, name, enabled);
        attrs.insert("class".into(), class);
        Ok(())
    }
    pub fn on<R: EventResult>(
        &mut self,
        id: usize,
        name: &'static str,
        mut callback: impl FnMut(Event) -> R + 'static,
    ) -> Result<(), String> {
        self.element(id)?;
        self.listeners
            .entry(id)
            .or_default()
            .entry(name)
            .or_default()
            .push(Rc::new(RefCell::new(move |event| {
                callback(event).into_result()
            })));
        Ok(())
    }
    pub fn component<T, K, I, M>(
        &mut self,
        id: usize,
        identity: I,
        make: M,
        children: Children,
    ) -> Result<(), String>
    where
        T: Component,
        K: Clone + PartialEq + 'static,
        I: Fn() -> Option<K>,
        M: Fn(OwnerHandle) -> Result<T, Error>,
    {
        let target = self.mount(id)?;
        let slot = self.slot::<Child<K>>(SlotKey::Component(id))?;
        let next = identity()
            .map(|key| {
                let existing = slot.borrow().find(|entry| {
                    entry
                        .as_ref()
                        .filter(|(old, _)| old == &key)
                        .map(|(_, scope)| scope.clone())
                });
                let scope = match existing {
                    Some(scope) => scope,
                    None => Rc::new(prepared(|| {
                        T::prepare(Some(&self.tree.owner), Box::new(&make), children)
                    })?),
                };
                stage(&slot, |candidate| {
                    candidate.replace((key.clone(), scope.clone()))
                });
                self.scope(&scope)?;
                Ok::<_, String>((key, scope))
            })
            .transpose()?;
        self.prepared.update(target).children =
            Some(next.iter().map(|(_, scope)| scope.root()).collect());
        let activate = next.as_ref().map(|(_, scope)| scope.clone());
        self.adopt(slot, next, move || {
            activate.iter().for_each(|scope| scope.publish());
        });
        Ok(())
    }
    pub fn branch<T: Clone + PartialEq + 'static>(
        &mut self,
        id: usize,
        read: impl Fn() -> (usize, T),
        prepare: impl Fn(usize, Signal<T>, &OwnerHandle) -> Result<Scope, Error>,
    ) -> Result<(), String> {
        let target = self.mount(id)?;
        let ((index, value), versions) = Versions::capture(read);
        let slot = self.slot::<Option<(usize, Row<T>)>>(SlotKey::Branch(id))?;
        let existing = slot.borrow().find(|entry| {
            entry
                .as_ref()
                .filter(|(old, _)| *old == index)
                .map(|(_, row)| row.clone())
        });
        let (data, scope) = match existing {
            Some(row) => row,
            None => {
                let data = fusor::signal(value.clone());
                let scope = prepared(|| prepare(index, data.clone(), &self.tree.owner))?;
                (data, Rc::new(scope))
            }
        };
        let next = Some((index, (data.clone(), scope.clone())));
        stage(&slot, |candidate| {
            std::mem::replace(candidate, next.clone())
        });
        data.with_render_value(Rc::new(value.clone()), versions, || self.scope(&scope))?;
        self.prepared.update(target).children = Some(vec![scope.root()]);
        self.adopt(slot, next, move || {
            data.set(value);
            scope.publish();
        });
        Ok(())
    }
    pub fn keyed<T: Clone + PartialEq + 'static, K: Ord + Clone + 'static>(
        &mut self,
        id: usize,
        read: impl Fn() -> Vec<T>,
        key: impl Fn(&T) -> K,
        prepare: impl Fn(Signal<T>, &OwnerHandle) -> Result<Scope, Error>,
    ) -> Result<(), String> {
        let target = self.element(id)?;
        let (values, versions) = Versions::capture(read);
        let entries: Vec<_> = values
            .into_iter()
            .map(|value| (key(&value), value))
            .collect();
        let mut unique = BTreeSet::new();
        if entries.iter().any(|(key, _)| !unique.insert(key)) {
            return Err("duplicate key in coherent ForEach".into());
        }
        let slot = self.slot::<BTreeMap<K, Row<T>>>(SlotKey::Rows(id))?;
        let mut next = BTreeMap::new();
        let mut roots = Vec::new();
        let mut updates = Vec::new();
        for (key, value) in entries {
            let existing = slot.borrow().find(|rows| rows.get(&key).cloned());
            let (data, scope) = match existing {
                Some(row) => row,
                None => {
                    let data = fusor::signal(value.clone());
                    let scope = prepared(|| prepare(data.clone(), &self.tree.owner))?;
                    (data, Rc::new(scope))
                }
            };
            stage(&slot, |candidate| {
                candidate.insert(key.clone(), (data.clone(), scope.clone()))
            });
            data.with_render_value(Rc::new(value.clone()), versions.clone(), || {
                self.scope(&scope)
            })?;
            roots.push(scope.root());
            updates.push((data.clone(), value, scope.clone()));
            next.insert(key, (data, scope));
        }
        self.prepared.update(target).children = Some(roots);
        self.adopt(slot, next, move || {
            for (data, value, scope) in updates {
                data.set(value);
                scope.publish();
            }
        });
        Ok(())
    }
    pub fn children(&mut self, id: usize, children: Children) -> Result<(), String> {
        let target = self.mount(id)?;
        let slot = self.slot::<Option<Rc<Scope>>>(SlotKey::Children(id))?;
        let existing = slot.borrow().find(Clone::clone);
        let next = match existing {
            Some(scope) => Some(scope),
            None => prepared(|| children.prepare(&self.tree.owner))?.map(Rc::new),
        };
        stage(&slot, |candidate| {
            std::mem::replace(candidate, next.clone())
        });
        if let Some(scope) = &next {
            self.scope(scope)?;
        }
        self.prepared.update(target).children =
            Some(next.iter().map(|scope| scope.root()).collect());
        let activate = next.clone();
        self.adopt(slot, next, move || {
            activate.iter().for_each(|scope| scope.publish());
        });
        Ok(())
    }
}
