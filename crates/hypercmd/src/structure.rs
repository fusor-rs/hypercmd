use crate::{Children, Component, Error, ErrorKind, Scope};
use fusor::{OwnerHandle, Signal, batch, signal, untrack};
use std::collections::{BTreeMap, BTreeSet};

impl Scope {
    pub fn branch<T: Clone + PartialEq + 'static>(
        &mut self,
        id: usize,
        read: impl Fn() -> (usize, T) + 'static,
        prepare: impl Fn(usize, Signal<T>, &OwnerHandle) -> Result<Scope, Error> + 'static,
    ) -> Result<(), Error> {
        let (target, owner) = (self.mount(id)?, self.owner());
        let mut current: Option<(usize, Signal<T>, Scope)> = None;
        self.watch(move || {
            let (index, value) = read();
            if let Some((old, data, _)) = &current {
                if *old == index {
                    data.set(value);
                    return Ok(());
                }
            }
            let data = signal(value);
            let next = untrack(|| prepare(index, data.clone(), &owner))?;
            target.set_children(vec![next.root()])?;
            let previous = current.replace((index, data, next));
            current.as_ref().unwrap().2.publish();
            drop(previous);
            Ok(())
        })
    }

    pub fn keyed<T: Clone + PartialEq + 'static, K: Ord + Clone + 'static>(
        &mut self,
        id: usize,
        read: impl Fn() -> Vec<T> + 'static,
        key: impl Fn(&T) -> K + 'static,
        prepare: impl Fn(Signal<T>, &OwnerHandle) -> Result<Scope, Error> + 'static,
    ) -> Result<(), Error> {
        let (target, owner) = (self.element(id)?, self.owner());
        let mut rows: BTreeMap<K, (Signal<T>, Scope)> = BTreeMap::new();
        self.watch(move || {
            let entries: Vec<_> = read()
                .into_iter()
                .map(|value| (key(&value), value))
                .collect();
            let mut unique = BTreeSet::new();
            for (key, _) in &entries {
                if !unique.insert(key) {
                    return Err(Error::new(
                        ErrorKind::DuplicateKey,
                        "ForEach contains a duplicate key; use unique rust:key values",
                    ));
                }
            }
            let mut added = BTreeMap::new();
            for (key, value) in &entries {
                if !rows.contains_key(key) {
                    let data = signal(value.clone());
                    let scope = untrack(|| prepare(data.clone(), &owner))?;
                    added.insert(key.clone(), (data, scope));
                }
            }
            let next_roots: Vec<_> = entries
                .iter()
                .map(|(key, _)| {
                    rows.get(key)
                        .or_else(|| added.get(key))
                        .expect("validated row exists")
                        .1
                        .root()
                })
                .collect();
            target.set_children(next_roots)?;
            batch(|| {
                let mut next = BTreeMap::new();
                for (key, value) in entries {
                    let (data, scope) = rows
                        .remove(&key)
                        .or_else(|| added.remove(&key))
                        .expect("validated row exists");
                    data.set(value);
                    next.insert(key, (data, scope));
                }
                let removed = std::mem::replace(&mut rows, next);
                for (_, scope) in rows.values() {
                    scope.publish();
                }
                drop(removed);
            });
            Ok(())
        })
    }

    pub fn component<
        T: Component,
        K: Clone + PartialEq + 'static,
        I: Fn() -> Option<K> + 'static,
        M: Fn(OwnerHandle) -> Result<T, Error> + 'static,
    >(
        &mut self,
        id: usize,
        identity: I,
        make: M,
        children: Children,
    ) -> Result<(), Error> {
        let (target, owner) = (self.mount(id)?, self.owner());
        let mut current: Option<(K, Scope)> = None;
        self.watch(move || {
            let key = identity();
            if current.as_ref().map(|(key, _)| key) == key.as_ref() {
                return Ok(());
            }
            let next = key
                .map(|key| {
                    untrack(|| T::prepare(Some(&owner), Box::new(&make), children.clone()))
                        .map(|scope| (key, scope))
                })
                .transpose()?;
            target.set_children(next.iter().map(|(_, scope)| scope.root()).collect())?;
            let previous = std::mem::replace(&mut current, next);
            if let Some((_, scope)) = &current {
                scope.publish();
            }
            drop(previous);
            Ok(())
        })
    }
    pub fn children(&mut self, id: usize, children: Children) -> Result<(), Error> {
        let target = self.mount(id)?;
        if let Some(scope) = untrack(|| children.prepare(&self.owner()))? {
            target.set_children(vec![scope.root()])?;
            scope.publish();
            self.retain(scope);
        }
        Ok(())
    }
}
