use crate::{examples::ExampleKind, terminal::Terminal};
use fusor::{FromInputs, Signal, signal};
use std::rc::Rc;
use wasm_bindgen::JsValue;

const INSTALL: &str = "cargo install hypercmd-cli --locked";
const COPY_SUCCESS: &str = "Copied!";

#[derive(Clone, Copy, PartialEq)]
struct CodeToken {
    id: usize,
    text: &'static str,
    style: &'static str,
}

#[derive(Clone, Copy, PartialEq)]
struct CodeLine {
    number: usize,
    tokens: &'static [CodeToken],
}

#[derive(PartialEq)]
struct Example {
    kind: ExampleKind,
    filename: &'static str,
    label: &'static str,
    hint: &'static str,
    rows: u16,
    lines: &'static [CodeLine],
}

include!(concat!(env!("OUT_DIR"), "/source.rs"));

struct App {
    example: Signal<&'static Example>,
    clipboard: web_sys::Clipboard,
    copy_label: Signal<&'static str>,
}

impl App {
    fn new() -> Result<Self, JsValue> {
        let window = web_sys::window().ok_or_else(|| JsValue::from_str("window is unavailable"))?;
        Ok(Self {
            example: signal(&EXAMPLES[0]),
            clipboard: window.navigator().clipboard(),
            copy_label: signal("Copy command"),
        })
    }

    fn copy_install(&self) {
        let promise = self.clipboard.write_text(INSTALL);
        let label = self.copy_label.clone();
        wasm_bindgen_futures::spawn_local(async move {
            label.set(match wasm_bindgen_futures::JsFuture::from(promise).await {
                Ok(_) => COPY_SUCCESS,
                Err(_) => "Select command to copy",
            });
        });
    }
}

struct Showcase {
    example: &'static Example,
    terminal: Rc<Terminal>,
}

struct ShowcaseInputs {
    example: &'static Example,
}

impl FromInputs for Showcase {
    type Inputs = ShowcaseInputs;
    type Error = JsValue;

    fn from_inputs(inputs: Self::Inputs, _owner: fusor::OwnerHandle) -> Result<Self, Self::Error> {
        Ok(Self {
            example: inputs.example,
            terminal: Rc::new(
                Terminal::new(inputs.example.kind, inputs.example.rows)
                    .map_err(|error| JsValue::from_str(&error.to_string()))?,
            ),
        })
    }
}

#[expect(
    clippy::excessive_nesting,
    reason = "generated DOM scaffolding: https://github.com/fusor-rs/fusor/issues/17"
)]
mod dom {
    use super::{App, COPY_SUCCESS, EXAMPLES, INSTALL, Showcase};
    use crate::terminal::COLUMNS;
    fusor::template!("web/index.html");
}
