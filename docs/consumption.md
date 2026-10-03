# Independent consumption

fusor comes from crates.io, pinned to an exact version (`=0.1.4`).

## Cargo archives

`just packages` creates Cargo `.crate` archives for Hypercmd, its build
helper, the CLI and the shared component. Cargo normalizes the manifests and
removes development paths. Rust 1.85 then builds and executes a new application
in a temporary directory outside the checkout. Its only local overrides point to
extracted archives. Dependency metadata is checked to reject any path back into
a source checkout, and any dependency that does not come from crates.io.

[packages.mjs](../scripts/packages.mjs) checks package execution, styles,
dependency isolation and authored app/library diagnostics.

Artifacts remain under `target/package-acceptance/`: the archives,
`SHA256SUMS` and a JSON manifest with each archive's hash. The temporary
application and extracted sources are removed. Registry dependencies must already
be cached: this gate runs offline. Run `just fetch` first on a fresh
machine.

## A shared component

[`hypercmd-job-controls`](../examples/job-controls) owns its Rust, HTML,
`build.rs` and separate terminal/browser styles. `JobRow` receives a memoized job,
the job collection and a typed `Rc<dyn Fn(u32)>` inspection callback. Its local
inspection counter survives keyed reorder. The native job inspector and a browser application use the same Rust type and HTML source.

Its additive `terminal` and `browser` features independently enable their build
helpers and generated includes. Both may be enabled in one Cargo feature union.
The component has no native event-loop dependency. Neither build helper crawls
a dependent package's source tree: the library builds its own templates and
ships all inputs in its archive. Browser CSS is explicitly exported; terminal
styles are compiled into the library's generated component.

## Browser acceptance

With Node 22+, install the Rust Wasm target, matching wasm-bindgen CLI and
Chromium, then execute the consumer:

```sh
just setup-browser
just browser
```

`just browser` builds the separate `tests/browser-consumer` workspace with both
component features enabled and executes it in Chromium. The [browser gate](../scripts/browser.mjs) checks the shared component's behavior
and CSS; platform coverage is listed in the [README](../README.md#platform-status).

`just setup-browser` installs the matching fusor CLI from crates.io; `FUSOR_BIN`
can select another build of it. The browser application has its own lockfile.
