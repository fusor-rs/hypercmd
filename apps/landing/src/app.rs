use crate::{examples::ExampleKind, terminal::Terminal};
use fusor::{FromInputs, Signal, signal};
use std::rc::Rc;
use wasm_bindgen::JsValue;

#[derive(PartialEq)]
struct InstallMethod {
    name: &'static str,
    command: &'static str,
}

const INSTALL_METHODS: &[InstallMethod] = &[
    InstallMethod {
        name: "Cargo",
        command: "cargo install hypercmd-cli --locked",
    },
    InstallMethod {
        name: "Linux",
        command: "curl -fsSL https://cmd.fusor.build/install.sh | sh",
    },
];
const COPY_PROMPT: &str = "Copy command";
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
struct SourceFile {
    filename: &'static str,
    language: &'static str,
    lines: &'static [CodeLine],
}

#[derive(PartialEq)]
struct Example {
    kind: ExampleKind,
    label: &'static str,
    hint: &'static str,
    rows: u16,
    files: [SourceFile; 2],
}

include!(concat!(env!("OUT_DIR"), "/source.rs"));

struct App {
    example: Signal<&'static Example>,
    install_method: Signal<&'static InstallMethod>,
    clipboard: web_sys::Clipboard,
    copy_label: Signal<&'static str>,
}

impl App {
    fn new() -> Result<Self, JsValue> {
        let window = web_sys::window().ok_or_else(|| JsValue::from_str("window is unavailable"))?;
        Ok(Self {
            example: signal(&EXAMPLES[0]),
            install_method: signal(&INSTALL_METHODS[0]),
            clipboard: window.navigator().clipboard(),
            copy_label: signal(COPY_PROMPT),
        })
    }

    fn select_install(&self, method: &'static InstallMethod) {
        self.install_method.set(method);
        self.copy_label.set(COPY_PROMPT);
    }

    fn copy_install(&self) {
        let method = self.install_method.get();
        let promise = self.clipboard.write_text(method.command);
        let selected = self.install_method.clone();
        let label = self.copy_label.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let outcome = match wasm_bindgen_futures::JsFuture::from(promise).await {
                Ok(_) => COPY_SUCCESS,
                Err(_) => "Select command to copy",
            };
            if selected.get() == method {
                label.set(outcome);
            }
        });
    }
}

struct Showcase {
    example: &'static Example,
    source: Signal<&'static SourceFile>,
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
            source: signal(&inputs.example.files[0]),
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
    use super::{App, COPY_SUCCESS, EXAMPLES, INSTALL_METHODS, Showcase};
    use crate::terminal::COLUMNS;
    fusor::template!("web/index.html");
}
