use crate::examples::ExampleKind;
use fusor::{Signal, signal};
use hypercmd::{Error, Node, Scope, layout};
use std::rc::Rc;
use wasm_bindgen::JsCast;

pub(crate) const COLUMNS: u16 = 36;

pub(crate) struct Terminal {
    scope: Scope,
    rows: u16,
    pub(crate) error: Signal<String>,
    pub(crate) frame: Signal<Rc<Frame>>,
}

#[derive(PartialEq)]
pub(crate) struct Frame {
    pub(crate) text: String,
    pub(crate) buttons: Vec<TerminalControl>,
    pub(crate) inputs: Vec<TerminalControl>,
}

#[derive(Clone, PartialEq)]
pub(crate) struct TerminalControl {
    pub(crate) node: Node,
    pub(crate) label: String,
    pub(crate) style: String,
}

impl Terminal {
    pub(crate) fn new(example: ExampleKind, rows: u16) -> Result<Self, Error> {
        let scope = example.mount()?;
        scope.publish();
        let frame = signal(Rc::new(render(&scope, rows)?));
        Ok(Self {
            scope,
            rows,
            frame,
            error: signal(String::new()),
        })
    }

    pub(crate) fn activate(self: &Rc<Self>, node: &Node) {
        self.repaint(node.dispatch("click"));
    }

    pub(crate) fn edit(self: &Rc<Self>, node: &Node, event: &web_sys::Event) {
        let input = event
            .target()
            .expect("an input event has a target")
            .dyn_into::<web_sys::HtmlInputElement>()
            .expect("text input events originate from an input element");
        self.repaint(node.edit(input.value()));
    }

    fn repaint(self: &Rc<Self>, outcome: Result<(), Error>) {
        if let Err(error) = outcome {
            self.error.set(error.to_string());
            return;
        }
        let terminal = self.clone();
        // DOM handlers batch writes; paint after Hypercmd's bindings finish that batch.
        wasm_bindgen_futures::spawn_local(async move {
            match render(&terminal.scope, terminal.rows) {
                Ok(frame) => terminal.frame.set(Rc::new(frame)),
                Err(error) => terminal.error.set(error.to_string()),
            }
        });
    }
}

fn render(scope: &Scope, rows: u16) -> Result<Frame, Error> {
    let presentation = layout::render(
        &scope.root(),
        (COLUMNS, rows),
        None,
        &mut layout::ScrollState::default(),
        &layout::LayoutOptions::default(),
    )?;
    let mut text = String::new();
    for row in presentation.buffer.content.chunks(usize::from(COLUMNS)) {
        for cell in row {
            text.push_str(cell.symbol());
        }
        text.push('\n');
    }
    let mut buttons = Vec::new();
    let mut inputs = Vec::new();
    for entry in presentation.entries {
        match entry.node.tag() {
            "button" => buttons.push(control(entry)),
            "input" => inputs.push(control(entry)),
            _ => {}
        }
    }
    Ok(Frame {
        text,
        buttons,
        inputs,
    })
}

fn control(entry: layout::LayoutEntry) -> TerminalControl {
    let rectangle = if entry.node.tag() == "input" {
        entry.content
    } else {
        entry.rect
    };
    TerminalControl {
        label: entry
            .node
            .attribute("placeholder")
            .unwrap_or_else(|| entry.node.text()),
        node: entry.node,
        style: format!(
            concat!(
                "left:{}ch;top:calc({} * var(--cell-height));",
                "width:{}ch;height:calc({} * var(--cell-height))",
            ),
            rectangle.x, rectangle.y, rectangle.width, rectangle.height,
        ),
    }
}
