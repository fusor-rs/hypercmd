use fusor_build::{
    ExtractError,
    backend::{
        Anchor, Backend, Capability, ComponentCode, Control, NodeKind, Operation, OperationKind,
        OperationMode, Origin, Runtime, Template,
    },
};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use std::cell::Cell;
use syn::parse_quote;

/// The terminal markup vocabulary used by validation and the profile reference.
pub mod profile {
    pub use crate::css::PROPERTIES as CSS_PROPERTIES;
    /// This profile has an independent version from fusor's compiler contracts.
    pub const NAME: &str = "terminal-v1";
    /// Supported native HTML tags. Structural tags are lowered by fusor.
    pub const ELEMENTS: &[&str] = &[
        "main", "section", "div", "ul", "li", "p", "h1", "h2", "h3", "span", "strong", "em", "pre",
        "a", "br", "label", "button", "input", "textarea", "table", "thead", "tbody", "tr", "th",
        "td",
    ];
    /// An asterisk matches every element; resize reports the content box.
    pub const EVENTS: &[(&str, &[&str])] = &[
        ("*", &["keydown", "scroll", "resize", "focus", "blur"]),
        ("button", &["click"]),
        ("text", &["input"]),
        ("checkbox", &["change"]),
    ];
}

/// Token-based emitter for the bounded Hypercmd terminal profile.
pub(crate) struct HypercmdBackend {
    pub(crate) styles: TokenStream,
    pub(crate) file: Cell<u64>,
}

impl HypercmdBackend {
    fn styles_name(&self) -> proc_macro2::Ident {
        format_ident!("__hypercmd_styles_{}", self.file.get())
    }
}
impl Backend for HypercmdBackend {
    fn file(&self) -> TokenStream {
        let (name, styles) = (self.styles_name(), &self.styles);
        quote! {
            const _: () = assert!(::hypercmd::VERSION == 1);
            const _: () = assert!(::fusor::render::VERSION == 1);
            const _: () = assert!(::fusor_components::BACKEND_VERSION == 1);
            fn #name() -> ::hypercmd::style::StyleSheet { #styles }
        }
    }
    fn version(&self) -> u32 {
        2
    }
    fn name(&self) -> &str {
        "hypercmd terminal-v1"
    }
    fn runtime(&self) -> Runtime {
        Runtime {
            scope: parse_quote!(::hypercmd::Scope),
            error: parse_quote!(::hypercmd::Error),
            children: parse_quote!(::hypercmd::Children),
            convert_error: parse_quote!(::std::convert::Into::into),
            coherent_frame: Some(parse_quote!(::hypercmd::coherent::Frame)),
        }
    }
    fn supports(&self, capability: Capability<'_>) -> bool {
        matches!(
            capability,
            Capability::App
                | Capability::Text
                | Capability::Attribute("placeholder" | "href")
                | Capability::Boolean("disabled" | "readonly")
                | Capability::Value
                | Capability::Checked
                | Capability::Class(_)
                | Capability::Event(_)
                | Capability::Bind(Control::Text | Control::Checkbox)
                | Capability::Branch
                | Capability::Keyed
                | Capability::Component
                | Capability::Children
                | Capability::Async
                | Capability::Router
        )
    }
    fn validate_binding(
        &self,
        template: &Template,
        capability: Capability<'_>,
        anchor: Anchor,
        origin: &Origin,
    ) -> Result<(), ExtractError> {
        let element = template.nodes.iter().find_map(|node| match &node.kind {
            NodeKind::Element {
                tag,
                attributes,
                anchor: Some(id),
            } if anchor == Anchor::Element(*id) => Some((tag.as_str(), attributes)),
            _ => None,
        });
        let control = element.map(|(tag, attrs)| control_kind(tag, attrs));
        let supported = match capability {
            Capability::Event(event) => control.is_some_and(|kind| {
                profile::EVENTS.iter().any(|(control, events)| {
                    (*control == "*" || *control == kind) && events.contains(&event)
                })
            }),
            Capability::Bind(Control::Text) | Capability::Value => control == Some("text"),
            Capability::Attribute(name) | Capability::Boolean(name) => {
                return match element.zip(control) {
                    Some((element, kind)) if attribute_allowed(element.0, kind, name) => Ok(()),
                    _ => Err(origin.error(format!(
                        "terminal-v1 does not support binding {name} on this element"
                    ))),
                };
            }
            Capability::Bind(Control::Checkbox) | Capability::Checked => {
                control == Some("checkbox")
            }
            _ => true,
        };
        if supported {
            Ok(())
        } else {
            Err(origin.error(
                "terminal-v1 rejects this binding/control pair; use click on button, \
                 input on a text control, change on checkbox, or keydown/scroll/resize/focus/blur",
            ))
        }
    }
    fn validate(&self, template: &Template) -> Result<(), ExtractError> {
        let mut ids = std::collections::BTreeSet::new();
        for node in &template.nodes {
            let NodeKind::Element {
                tag, attributes, ..
            } = &node.kind
            else {
                continue;
            };
            if !profile::ELEMENTS.contains(&tag.as_str()) {
                return Err(node.origin.error(format!(
                    "terminal-v1 does not support <{tag}>; \
                     use a supported container, text element, button or input"
                )));
            }
            let kind = control_kind(tag, attributes);
            for attribute in attributes {
                validate_attribute(tag, kind, attribute)?;
                if attribute.name == "id" && !ids.insert(&attribute.value) {
                    return Err(attribute.origin.error(
                        "duplicate id in this template; \
                         give each element in a component a unique id",
                    ));
                }
            }
        }
        Ok(())
    }
    fn mount(&self, template: &Template) -> TokenStream {
        let nodes = template.nodes.iter().map(|node| {
            let parent = option(node.parent);
            let kind = match &node.kind {
                NodeKind::Element {
                    tag,
                    attributes,
                    anchor,
                } => {
                    let anchor = option(*anchor);
                    let attributes = attributes.iter().map(|attribute| {
                        let (name, value) = (&attribute.name, &attribute.value);
                        quote! { (#name, #value) }
                    });
                    quote! { ::hypercmd::Kind::Element(#tag, &[#(#attributes),*], #anchor) }
                }
                NodeKind::Text { value, anchor } => {
                    let anchor = option(*anchor);
                    quote! { ::hypercmd::Kind::Text(#value, #anchor) }
                }
                NodeKind::Mount { anchor } => quote! { ::hypercmd::Kind::Mount(#anchor) },
                NodeKind::Comment(_) => quote! { ::hypercmd::Kind::Comment },
            };
            quote! { ::hypercmd::StaticNode { parent: #parent, kind: #kind } }
        });
        let styles = self.styles_name();
        quote! { ::hypercmd::Scope::with_styles(parent, &[#(#nodes),*], #styles()) }
    }
    fn operation(&self, operation: Operation) -> TokenStream {
        let scope = match operation.mode {
            OperationMode::Reactive => quote! { __fusor_scope },
            OperationMode::Coherent => quote! { __fusor_frame },
        };
        let (Anchor::Element(id) | Anchor::Text(id) | Anchor::Mount(id)) = operation.anchor;
        let (method, args) = match operation.kind {
            OperationKind::Text { read } => (quote!(text), quote!(#read)),
            OperationKind::Attribute { name, read } => (quote!(attribute), quote!(#name, #read)),
            OperationKind::Boolean { name, read } => (quote!(boolean), quote!(#name, #read)),
            OperationKind::Value { read } => (quote!(value), quote!(#read)),
            OperationKind::Checked { read } => (quote!(checked), quote!(#read)),
            OperationKind::Class { name, read } => (quote!(class), quote!(#name, #read)),
            OperationKind::Event { name, handler } => (quote!(on), quote!(#name, #handler)),
            OperationKind::Bind {
                control: Control::Text,
                value,
                ..
            } => (quote!(bind_text), quote!(#value)),
            OperationKind::Bind {
                control: Control::Checkbox,
                value,
                choice,
            } => {
                let choice = choice.unwrap_or_else(|| quote!(|| ::std::string::String::new()));
                (quote!(bind_checkbox), quote!(#value, #choice))
            }
            OperationKind::Branch { read, prepare } => (quote!(branch), quote!(#read, #prepare)),
            OperationKind::Keyed { read, key, prepare } => {
                (quote!(keyed), quote!(#read, #key, #prepare))
            }
            OperationKind::Component {
                ty,
                identity,
                make,
                children,
            } => (
                quote!(component::<#ty, _, _, _>),
                quote!(#identity, #make, #children),
            ),
            OperationKind::Children { children } => (quote!(children), quote!(&#children)),
            OperationKind::Async { boundary, render } => {
                (quote!(async_region), quote!(#boundary, #render))
            }
            OperationKind::Router { routes } => {
                let routes = routes.into_iter().map(|route| {
                    let prepare = route.prepare;
                    match route.pattern {
                        Some(pattern) => quote!(::fusor_router::view::RouteView::new(#pattern, ::hypercmd::Scope::route_factory(#prepare))?),
                        None => quote!(::fusor_router::view::RouteView::fallback(::hypercmd::Scope::route_factory(#prepare))),
                    }
                });
                return quote! {
                    const _: () = assert!(::fusor_router::view::VERSION == 1);
                    #scope.routes(#id, ::std::vec![#(#routes),*])?;
                };
            }
            _ => unreachable!("fusor validates terminal capabilities before emission"),
        };
        quote!(#scope.#method(#id, #args)?;)
    }
    fn component(&self, component: ComponentCode) -> TokenStream {
        let ComponentCode {
            ty,
            body,
            app_state,
        } = component;
        let implementation = if let Some(state) = app_state {
            let styles = self.styles_name();
            quote! {
                pub fn hypercmd_app() -> ::std::result::Result<::hypercmd::Scope, ::hypercmd::Error> {
                    let parent = ::std::option::Option::None;
                    let make = |owner: ::fusor::OwnerHandle| ::std::result::Result::<_, ::hypercmd::Error>::Ok({ #state });
                    ::hypercmd::component_scope(|| { #body })
                }
                pub fn hypercmd_styles() -> ::hypercmd::style::StyleSheet { #styles() }
            }
        } else {
            quote! {
                impl ::hypercmd::Component for #ty {
                    fn prepare(
                        parent: ::std::option::Option<&::fusor::OwnerHandle>,
                        make: ::std::boxed::Box<dyn FnOnce(::fusor::OwnerHandle) -> ::std::result::Result<Self, ::hypercmd::Error> + '_>,
                        children: ::hypercmd::Children,
                    ) -> ::std::result::Result<::hypercmd::Scope, ::hypercmd::Error> {
                        children.with(|| ::hypercmd::component_scope(|| { #body }))
                    }
                }
            }
        };
        quote! {
            #[allow(
                unused_variables,
                unused_braces,
                non_snake_case,
                clippy::unused_unit,
                clippy::unit_arg,
                clippy::clone_on_copy,
                clippy::useless_conversion,
                clippy::redundant_clone,
                clippy::too_many_lines,
                clippy::cognitive_complexity,
                clippy::excessive_nesting,
                reason = "generated from an HTML template; its shape follows the template, not hand-written style"
            )]
            #implementation
        }
    }
}

fn control_kind<'a>(tag: &'a str, attributes: &'a [fusor_build::backend::Attribute]) -> &'a str {
    if tag == "textarea" {
        "text"
    } else if tag == "input" {
        attributes
            .iter()
            .find(|attribute| attribute.name == "type")
            .map_or("text", |attribute| attribute.value.as_str())
    } else {
        tag
    }
}

fn is_control(kind: &str) -> bool {
    matches!(kind, "button" | "text" | "checkbox")
}
fn attribute_allowed(tag: &str, kind: &str, name: &str) -> bool {
    match name {
        "id" | "class" | "tabindex" | "autofocus" => true,
        "type" => tag == "input",
        "value" => matches!(tag, "input" | "textarea"),
        "checked" => kind == "checkbox",
        "disabled" => is_control(kind),
        "readonly" | "placeholder" => kind == "text",
        "for" => tag == "label",
        "href" => tag == "a",
        _ => false,
    }
}

fn validate_attribute(
    tag: &str,
    kind: &str,
    attribute: &fusor_build::backend::Attribute,
) -> Result<(), ExtractError> {
    let supported = attribute_allowed(tag, kind, &attribute.name)
        && match attribute.name.as_str() {
            "type" => matches!(attribute.value.as_str(), "text" | "checkbox"),
            "tabindex" => matches!(attribute.value.as_str(), "0" | "-1"),
            _ => true,
        };
    if supported {
        Ok(())
    } else {
        Err(attribute.origin.error(format!("terminal-v1 rejects {}={:?} on <{tag}>; use a documented attribute/control pair (tabindex accepts 0 or -1)", attribute.name, attribute.value)))
    }
}

fn option(value: Option<usize>) -> TokenStream {
    match value {
        Some(value) => quote!(Some(#value)),
        None => quote!(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // These are target restrictions, not fusor syntax or generated-code tests.
    // The external consumer compiles and executes the accepted operations.
    #[test]
    fn unsupported_terminal_operations_have_authored_diagnostics() {
        for markup in [
            "<canvas></canvas>",
            "<button on:input=\"()\">Wrong event</button>",
            "<input type=\"password\">",
            "<input tabindex=\"1\">",
            "<div disabled></div>",
            "<input bind=\"state.value\" value=\"{{ state.other }}\">",
            "<Async boundary=\"{{ state.boundary }}\"><input bind=\"state.value\"></Async>",
            "<Async boundary=\"{{ state.boundary }}\"><Router><Route fallback><p>nested</p></Route></Router></Async>",
        ] {
            let source = format!("<template rust:component=\"Example\">\n{markup}\n</template>");
            let error = match fusor_build::backend::generate(
                &source,
                &HypercmdBackend {
                    styles: quote!(&[]),
                    file: Cell::default(),
                },
            ) {
                Ok(_) => panic!("unsupported operation was accepted: {markup}"),
                Err(error) => error,
            };
            assert_eq!(error.line, 2, "{markup}: {error}");
            assert!(error.column > 0 && !error.message.is_empty());
        }
    }
}
