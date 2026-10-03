use crate::{Error, ErrorKind};
use fusor::OwnerHandle;
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};

pub(crate) type Listener = Rc<RefCell<dyn FnMut(Event) -> Result<(), Error>>>;

#[derive(Default)]
pub(crate) struct Scene {
    pub dirty: Cell<bool>,
    pub errors: RefCell<Vec<Error>>,
    pub omitted: Cell<usize>,
    pub faulted: Cell<bool>,
    pub requested_focus: RefCell<std::rc::Weak<NodeData>>,
    pub route_focus: RefCell<std::rc::Weak<NodeData>>,
}

impl Scene {
    pub fn changed(&self) {
        self.dirty.set(true);
    }
    pub fn report(&self, error: Error) {
        let mut errors = self.errors.borrow_mut();
        if errors.len() == 64 {
            errors.remove(0);
            self.omitted.set(self.omitted.get().saturating_add(1));
        }
        errors.push(error);
        self.changed();
    }
}

#[derive(Clone, Debug)]
pub enum EventPayload {
    None,
    Input(crate::Input),
    Resize { width: u16, height: u16 },
}

/// Key and scroll events visit the target then its ancestors until handled.
#[derive(Clone)]
pub struct Event {
    pub name: &'static str,
    pub target: Node,
    pub payload: EventPayload,
    handled: Rc<Cell<bool>>,
}

impl Event {
    pub(crate) fn new(name: &'static str, target: Node, payload: EventPayload) -> Self {
        Self {
            name,
            target,
            payload,
            handled: Rc::default(),
        }
    }

    /// Stop ancestor handlers and the controller's default gesture.
    pub fn prevent_default(&self) {
        self.handled.set(true);
    }

    pub fn default_prevented(&self) -> bool {
        self.handled.get()
    }
}

/// Stable identity. A retained handle becomes inert when its owner is disposed.
#[derive(Clone)]
pub struct Node(pub(crate) Rc<NodeData>);

pub(crate) struct NodeData {
    pub owner: OwnerHandle,
    pub scene: Rc<Scene>,
    pub styles: crate::style::StyleSheet,
    pub component: Rc<crate::scope::Instance>,
    pub tag: &'static str,
    pub attrs: RefCell<BTreeMap<String, String>>,
    pub text: RefCell<String>,
    pub children: RefCell<Vec<Node>>,
    pub listeners: RefCell<BTreeMap<&'static str, Vec<Listener>>>,
    pub value: RefCell<String>,
    pub editor: RefCell<crate::editor::EditorState>,
    pub scroll_request: Cell<Option<(u16, u16)>>,
    pub checked: Cell<bool>,
    pub gate: RefCell<Option<Rc<crate::coherent::Gate>>>,
}

impl PartialEq for Node {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for Node {}
impl std::fmt::Debug for Node {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Node")
            .field("tag", &self.tag())
            .field("id", &self.attribute("id"))
            .finish()
    }
}

pub(crate) struct ScopeShared {
    pub owner: OwnerHandle,
    pub scene: Rc<Scene>,
    pub styles: crate::style::StyleSheet,
    pub component: Rc<crate::scope::Instance>,
}

impl Node {
    pub(crate) fn new(
        shared: &ScopeShared,
        tag: &'static str,
        text: &str,
        attrs: &[(&str, &str)],
    ) -> Self {
        let attrs: BTreeMap<String, String> = attrs
            .iter()
            .map(|(name, value)| ((*name).into(), (*value).into()))
            .collect();
        let gate = shared
            .owner
            .context::<crate::coherent::Context>()
            .map(|gate| (*gate).clone());
        Self(Rc::new(NodeData {
            owner: shared.owner.clone(),
            scene: shared.scene.clone(),
            styles: shared.styles,
            component: shared.component.clone(),
            tag,
            value: RefCell::new(attrs.get("value").cloned().unwrap_or_default()),
            checked: Cell::new(attrs.contains_key("checked")),
            editor: RefCell::default(),
            scroll_request: Cell::default(),
            attrs: RefCell::new(attrs),
            text: RefCell::new(text.into()),
            children: RefCell::default(),
            listeners: RefCell::default(),
            gate: RefCell::new(gate),
        }))
    }
    pub fn tag(&self) -> &'static str {
        self.0.tag
    }
    pub fn is_alive(&self) -> bool {
        !self.0.owner.is_disposed()
    }
    pub fn is_active(&self) -> bool {
        self.0.owner.is_active()
    }
    /// Pending regions retain their committed content but hide their first candidate.
    pub fn is_visible(&self) -> bool {
        self.is_active()
            && self
                .0
                .gate
                .borrow()
                .as_ref()
                .is_none_or(|gate| gate.published.get())
    }
    /// Coherent pending/error regions remain visible but cannot receive events.
    pub fn is_interactive(&self) -> bool {
        self.is_visible()
            && !self.0.scene.faulted.get()
            && self
                .0
                .gate
                .borrow()
                .as_ref()
                .is_none_or(|gate| gate.boundary.is_interactive())
    }
    pub fn attribute(&self, name: &str) -> Option<String> {
        self.0.attrs.borrow().get(name).cloned()
    }
    pub fn children(&self) -> Vec<Node> {
        self.0.children.borrow().clone()
    }
    pub(crate) fn same_component(&self, other: &Node) -> bool {
        Rc::ptr_eq(&self.0.component, &other.0.component)
    }

    pub fn component_root(&self) -> Node {
        self.0
            .component
            .root
            .borrow()
            .upgrade()
            .map(Node)
            .unwrap_or_else(|| self.clone())
    }
    pub(crate) fn validate_children(&self, children: &[Node]) -> Result<(), Error> {
        // Ids are scoped per component; other components' subtrees validate themselves.
        let children_of = |node: &Node| {
            if !node.same_component(self) {
                Vec::new()
            } else if node == self {
                children.to_vec()
            } else {
                node.children()
            }
        };
        let id_of = |node: &Node| node.attribute("id");
        std::iter::once(self.component_root())
            .chain(children.iter().cloned())
            .try_for_each(|root| validate_tree(&root, children_of, id_of))
    }
    pub(crate) fn set_children(&self, children: Vec<Node>) -> Result<(), Error> {
        self.validate_children(&children)?;
        self.replace_children(children);
        Ok(())
    }
    pub fn descendants(&self) -> impl Iterator<Item = Node> {
        let mut pending = vec![self.clone()];
        std::iter::from_fn(move || {
            let node = pending.pop()?;
            pending.extend(node.children().into_iter().rev());
            Some(node)
        })
    }
    pub fn text(&self) -> String {
        self.descendants()
            .map(|node| node.0.text.borrow().clone())
            .collect()
    }
    pub fn find(&self, id: &str) -> Option<Node> {
        self.descendants()
            .find(|node| node.attribute("id").as_deref() == Some(id))
    }
    pub(crate) fn is_checkbox(&self) -> bool {
        self.attribute("type").as_deref() == Some("checkbox")
    }
    pub(crate) fn is_editor(&self) -> bool {
        self.tag() == "textarea" || (self.tag() == "input" && !self.is_checkbox())
    }
    pub(crate) fn is_control(&self) -> bool {
        matches!(self.tag(), "button" | "input" | "textarea")
            || self.attribute("tabindex").is_some()
    }
    pub(crate) fn is_disabled(&self) -> bool {
        self.attribute("disabled").is_some()
    }
    pub(crate) fn display_value(&self) -> String {
        if self.is_checkbox() {
            if self.checked() { "[x]" } else { "[ ]" }.into()
        } else {
            let value = self.value();
            if value.is_empty() {
                self.attribute("placeholder").unwrap_or_default()
            } else {
                value
            }
        }
    }
    pub(crate) fn clamped_editor(&self, value: &str) -> crate::editor::EditorState {
        let mut editor = self.editor();
        editor.clamp(value);
        editor
    }
    pub fn value(&self) -> String {
        self.0.value.borrow().clone()
    }
    pub fn editor(&self) -> crate::editor::EditorState {
        *self.0.editor.borrow()
    }
    pub fn checked(&self) -> bool {
        self.0.checked.get()
    }

    pub(crate) fn replace_children(&self, children: Vec<Node>) {
        let previous = self.0.children.replace(children);
        self.0.scene.changed();
        drop(previous);
    }
    pub(crate) fn set_text(&self, text: String) {
        if *self.0.text.borrow() != text {
            self.0.text.replace(text);
            self.0.scene.changed();
        }
    }
    pub(crate) fn set_value(&self, value: String) {
        if *self.0.value.borrow() != value {
            self.0.editor.borrow_mut().clamp(&value);
            self.0.value.replace(value);
            self.0.scene.changed();
        }
    }
    pub(crate) fn set_checked(&self, checked: bool) {
        if self.0.checked.replace(checked) != checked {
            self.0.scene.changed();
        }
    }
    pub(crate) fn set_attribute(&self, name: &str, value: Option<String>) {
        let changed = {
            let mut attrs = self.0.attrs.borrow_mut();
            match value {
                Some(value) if attrs.get(name) == Some(&value) => false,
                Some(value) => {
                    attrs.insert(name.into(), value);
                    true
                }
                None => attrs.remove(name).is_some(),
            }
        };
        if changed {
            self.0.scene.changed();
        }
    }
    /// Apply a cell offset on the next layout, clamped to the scrollable content.
    pub fn scroll_to(&self, column: u16, row: u16) {
        self.0.scroll_request.set(Some((column, row)));
        self.0.scene.changed();
    }

    pub fn request_focus(&self) {
        self.0.scene.requested_focus.replace(Rc::downgrade(&self.0));
        self.0.scene.changed();
    }

    pub fn dispatch(&self, name: &'static str) -> Result<(), Error> {
        self.emit(&Event::new(name, self.clone(), EventPayload::None))
    }

    pub(crate) fn emit(&self, event: &Event) -> Result<(), Error> {
        let name = event.name;
        if !self.is_interactive()
            || (matches!(name, "click" | "input" | "change") && self.is_disabled())
        {
            return Ok(());
        }
        let listeners = self
            .0
            .listeners
            .borrow()
            .get(name)
            .cloned()
            .unwrap_or_default();
        for listener in listeners {
            if !self.is_interactive() {
                break;
            }
            let mut callback = listener.try_borrow_mut().map_err(|_| {
                Error::new(
                    ErrorKind::ReentrantEvent,
                    "a listener cannot recursively dispatch itself",
                )
            })?;
            if let Err(error) = fusor::untrack(|| fusor::batch(|| callback(event.clone()))) {
                self.0.scene.report(error);
            }
        }
        Ok(())
    }
    /// Apply a user edit; programmatic bindings never call this method.
    pub fn edit(&self, value: impl Into<String>) -> Result<(), Error> {
        if self.is_interactive() && !self.is_disabled() && self.attribute("readonly").is_none() {
            self.set_value(value.into());
            self.dispatch("input")?;
        }
        Ok(())
    }
    pub fn check(&self, checked: bool) -> Result<(), Error> {
        if self.is_interactive() && !self.is_disabled() {
            self.set_checked(checked);
            self.dispatch("change")?;
        }
        Ok(())
    }
}

pub(crate) fn validate_tree(
    root: &Node,
    children_of: impl Fn(&Node) -> Vec<Node>,
    id_of: impl Fn(&Node) -> Option<String>,
) -> Result<(), Error> {
    let mut pending = vec![root.clone()];
    let mut seen = BTreeSet::new();
    let mut ids: BTreeMap<usize, BTreeSet<String>> = BTreeMap::new();
    while let Some(node) = pending.pop() {
        if !seen.insert(Rc::as_ptr(&node.0)) {
            return Err(Error::template("scene contains a repeated node or cycle"));
        }
        if !node.is_alive() {
            return Err(Error::template("scene contains a disposed node"));
        }
        if let Some(id) = id_of(&node) {
            if !ids
                .entry(Rc::as_ptr(&node.0.component) as usize)
                .or_default()
                .insert(id.clone())
            {
                return Err(Error::template(format!(
                    "duplicate id {id:?} in mounted component"
                )));
            }
        }
        pending.extend(children_of(&node));
    }
    Ok(())
}
