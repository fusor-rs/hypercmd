# Shared job controls

This library owns `Job`, the stateful `JobRow` component, its HTML and both
stylesheets. Its `terminal` and `browser` features are additive: each compiles
the same Rust state and `ui/job.html` into its own rendering implementation.
Neither feature implies an execution target. Terminal dependencies disable
Hypercmd's native I/O feature so both implementations can coexist on Wasm.

`JobRowInputs` accepts a projected `Memo<Job>`, its `Signal<Vec<Job>>` collection
and an `Rc<dyn Fn(u32)>` inspection callback. Inspection count is private local
state; cancel and remove update the shared collection. Hosts choose what an
inspection does. The terminal inspector navigates; the browser companion
updates a status message.

Terminal styles are compiled into the component scope. Browser hosts embed
`BROWSER_CSS` in their document. Cargo archives include Rust, HTML and styles;
consumers never inspect another crate's source tree or generate its trait impls.
