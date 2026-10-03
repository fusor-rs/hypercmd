//! Retained terminal interfaces compiled from fusor HTML.
mod bindings;
pub mod coherent;
mod controls;
mod error;
#[cfg(feature = "native")]
mod input;
pub mod layout;
#[cfg(feature = "native")]
pub mod native;
mod routes;
mod scene;
mod scope;
pub mod services;
mod structure;
pub mod style;
pub mod text;

pub use controls::{Controller, EditorState, Input, Key, KeyKind};
pub use error::{Error, ErrorKind, EventResult};
pub use routes::History;
pub use scene::{Event, Node};
#[doc(hidden)]
pub use scope::component_scope;
pub use scope::{Children, Component, Kind, Scope, StaticNode};
pub use services::{ServiceLimits, Services};

/// Hypercmd's generated runtime contract, independent of fusor's contracts.
pub const VERSION: u32 = 1;
const _: () = assert!(fusor::render::VERSION == 1);
const _: () = assert!(fusor::coherence::VERSION == 2);

/// Prepare a reusable component as an application root, without activating it.
pub fn mount<T>(inputs: T::Inputs) -> Result<Scope, Error>
where
    T: Component + fusor::FromInputs,
    T::Error: Into<Error>,
{
    fusor::untrack(|| {
        T::prepare(
            None,
            Box::new(move |owner| T::from_inputs(inputs, owner).map_err(Into::into)),
            Children::default(),
        )
    })
}
