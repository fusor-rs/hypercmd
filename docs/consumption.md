# Independent consumption

A component library can share its Rust state and HTML between terminal and browser
applications, with separate styles for each. This page uses `hypercmd-job-controls`
as an example, then covers the package and browser checks for contributors.

## A shared component

[`hypercmd-job-controls`](../examples/job-controls) owns its Rust, HTML, `build.rs` and
separate terminal and browser styles. `JobRow` receives a memoized job (a cached value
computed from reactive state), the job
collection and a typed `Rc<dyn Fn(u32)>` inspection callback. (`Rc` is Rust's
single-threaded reference-counted pointer; the type here is a shared pointer to a
function that takes a `u32`.) Its local inspection counter survives keyed reorder.

The [native job inspector](../examples/job-inspector) and a browser application use the
same Rust type and HTML source.

The component has two additive Cargo features, `terminal` and `browser`. A feature is an
optional compile-time switch. These two independently enable their build helpers and
generated includes, and both may be enabled in one Cargo feature union, which is what
Cargo does when different parts of a build ask for different features. The component has
no native event-loop dependency.

Neither `hypercmd-build` nor `fusor-build` scans a dependent package's source tree:
the library builds its own templates and ships all inputs in its archive. Browser CSS is
explicitly exported;
terminal styles are compiled into the library's generated component.

## Cargo archives

Hypercmd gets fusor from crates.io, pinned to an exact version (`=0.1.5`; the `=` tells
Cargo to accept only that version).

`just packages` creates Cargo `.crate` archives, the source packages Cargo publishes, for
Hypercmd, its build helper, the CLI and the shared component. Cargo normalizes the
manifests and removes development paths. Rust 1.85 then builds and runs a new
application in a temporary directory outside the checkout. Its only local overrides point
to extracted archives. The check reads the dependency metadata and rejects any path back
into a source checkout, and any dependency that doesn't come from crates.io.

[packages.mjs](../scripts/packages.mjs) checks package execution, styles, dependency
isolation and authored app/library diagnostics.

Artifacts remain under `target/package-acceptance/`: the archives, `SHA256SUMS` and a
JSON manifest with each archive's hash. The temporary application and extracted sources
are removed. This check runs offline, so registry dependencies must already be cached.
Run `just fetch` first on a fresh machine.

## Browser acceptance

With Rust, Node 22+ and the `just` command runner installed, run these commands
from the repository root. The setup recipe installs the Wasm targets, matching
wasm-bindgen and Fusor CLIs, npm dependencies and Chromium:

```sh
just setup-browser
just browser
```

`just browser` builds the separate `tests/browser-consumer` workspace with both component
features enabled and executes it in Chromium. The [browser script](../scripts/browser.mjs)
checks the shared component's behavior and CSS; platform coverage is listed in the
[README](../README.md#platform-status).

`just setup-browser` installs the matching fusor CLI from crates.io; `FUSOR_BIN` can
select another build of it. The browser application has its own lockfile.
