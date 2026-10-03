# Generated native consumer

This independent Cargo workspace consumes the portable runtime with
`default-features = false`. Run `just consumer` from the repository root.

Seven tests in [lib.rs](src/lib.rs), [routing.rs](src/routing.rs) and
[async_views.rs](src/async_views.rs) exercise generated HTML and its construction,
control, routing and coherence contracts. Templates use package-owned discovery
and namespaced includes; assertions inspect mounted nodes and reactive state.
Native terminal I/O is covered by the separate PTY suites.
