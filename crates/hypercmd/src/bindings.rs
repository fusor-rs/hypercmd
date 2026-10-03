use crate::{Error, Event, EventResult, Node, Scope, scene::Listener};
use fusor::bind::{Checkbox, TextValue};
use std::{cell::RefCell, fmt::Display, rc::Rc};

impl Scope {
    fn effect(&mut self, node: Node, mut update: impl FnMut(&Node) + 'static) -> Result<(), Error> {
        self.watch(move || {
            update(&node);
            Ok(())
        })
    }
    pub fn text<T: Display>(
        &mut self,
        id: usize,
        read: impl Fn() -> T + 'static,
    ) -> Result<(), Error> {
        self.effect(
            crate::scope::anchor(&self.texts, "text", id)?,
            move |node| node.set_text(read().to_string()),
        )
    }
    pub fn on<R: EventResult>(
        &mut self,
        id: usize,
        name: &'static str,
        mut callback: impl FnMut(Event) -> R + 'static,
    ) -> Result<(), Error> {
        let node = self.element(id)?;
        let listener: Listener = Rc::new(RefCell::new(move |event| callback(event).into_result()));
        node.0
            .listeners
            .borrow_mut()
            .entry(name)
            .or_default()
            .push(listener);
        Ok(())
    }
    pub fn bind_text(&mut self, id: usize, model: impl TextValue) -> Result<(), Error> {
        let input_model = model.clone();
        self.on(id, "input", move |event| {
            input_model.edit(event.target.value())
        })?;
        let blur_model = model.clone();
        self.on(id, "blur", move |_| blur_model.touch())?;
        let node = self.element(id)?;
        self.watch(move || {
            if !model.shows(&node.value()) {
                node.set_value(model.text());
            }
            Ok(())
        })
    }
    pub fn bind_checkbox(
        &mut self,
        id: usize,
        model: impl Checkbox,
        choice: impl Fn() -> String + 'static,
    ) -> Result<(), Error> {
        let choice = Rc::new(choice);
        let (input_choice, input_model) = (choice.clone(), model.clone());
        self.on(id, "change", move |event| {
            input_model.check(&input_choice(), event.target.checked())
        })?;
        let node = self.element(id)?;
        self.watch(move || {
            node.set_checked(model.checked(&choice()));
            Ok(())
        })
    }
    pub fn value<T: Display>(
        &mut self,
        id: usize,
        read: impl Fn() -> T + 'static,
    ) -> Result<(), Error> {
        self.effect(self.element(id)?, move |node| {
            node.set_value(read().to_string())
        })
    }
    pub fn checked(&mut self, id: usize, read: impl Fn() -> bool + 'static) -> Result<(), Error> {
        self.effect(self.element(id)?, move |node| node.set_checked(read()))
    }
    pub fn attribute<T: Display>(
        &mut self,
        id: usize,
        name: &'static str,
        read: impl Fn() -> T + 'static,
    ) -> Result<(), Error> {
        self.effect(self.element(id)?, move |node| {
            node.set_attribute(name, Some(read().to_string()))
        })
    }
    pub fn boolean(
        &mut self,
        id: usize,
        name: &'static str,
        read: impl Fn() -> bool + 'static,
    ) -> Result<(), Error> {
        self.effect(self.element(id)?, move |node| {
            node.set_attribute(name, read().then(String::new))
        })
    }
    pub fn class(
        &mut self,
        id: usize,
        name: &'static str,
        read: impl Fn() -> bool + 'static,
    ) -> Result<(), Error> {
        self.effect(self.element(id)?, move |node| {
            node.set_attribute(
                "class",
                Some(crate::text::toggle_class(
                    &node.attribute("class").unwrap_or_default(),
                    name,
                    read(),
                )),
            )
        })
    }
}
