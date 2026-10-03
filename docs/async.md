# Coherent async views

Hypercmd uses fusor's `AsyncBoundary`, `AsyncValue`, attempt/read-lease and source
version APIs. Declare reads with an owner and a local spawner; put loading,
error and retry controls outside the coherent region:

```html
<p>{{ format!("{:?}", state.boundary.status()) }}</p>
<button on:click="state.boundary.retry()">Retry</button>
<Async boundary="{{ state.boundary }}"><section>
  <Await value="{{ state.details }}" let="details">
    <p>{{ details.title }}</p>
  </Await>
</section></Async>
```

The state declares `boundary: fusor::coherence::AsyncBoundary` and an owned
`fusor_async::AsyncValue`. A standalone `Await` starts an independent boundary;
inside an existing coherent scope it joins that boundary. `Async` and `Await`
need a native HTML root, as in the example. Reads in independent Await branches
are discovered in the same pass. Completion of only some reads does not paint a
partial result.

Before the first successful publication the entire region, including static
content, is hidden. Pending reads and recoverable errors retain the last
committed scene. Its event handlers are blocked until the boundary is ready.
Focus can remain on a pending control, or Tab can leave the region; completing
the read does not steal focus back from another control. Navigation and controls
outside the region remain available.

Prepared component constructors retain their ordinary Rust semantics except
for effects explicitly deferred by `fusor::coherence::prepare_state` through
`render::construct`. Candidate effects begin only when adopted owners activate.
Arbitrary constructor side effects are not transactions. Input/structural
factories run untracked. Pending candidates survive completion and retry within
one input epoch; obsolete candidates, including slots no longer visited, are
released through `Attempt::on_invalidate`.

Keyed rows keep committed node and component identity. Candidate reads project
row signals through `Signal::with_render_value` and `Versions`, so cached and
nested memos see speculative data without modifying committed caches. A read
can restart when captured source versions change even if its key compares equal.

Hypercmd validates all proposed child replacements together before publishing.
Duplicate keys, duplicate component-local IDs and failed component preparation
preserve the previous scene. Publication swaps prepared values without calling
application code; afterward it updates row signals, activates adopted owners
and retires old scopes. Old event captures remain retained until this finish
phase, with scene borrows released before callbacks or destructors can run.

Nested Async boundaries, editable controls and router outlets inside coherent
regions are unsupported. Direct authored violations produce compiler diagnostics;
incompatible reusable components fail coherent preparation while retaining the
last scene. Async regions inside routes are supported.

`Node::is_visible()` distinguishes a hidden initial candidate from retained
content. `Node::is_interactive()` also checks owner activation, boundary readiness
and scene health. An unexpected publication fault poisons interaction for the
whole scene. Custom hosts must stop when `Scope::is_faulted()` becomes true;
the native runner restores the terminal and exits. `Scope::take_errors()` exposes
structured publication errors alongside ordinary update errors.

The [async consumer tests](../tests/consumer/src/async_views.rs) exercise the
generated coherence contract with a controlled executor. Run `just consumer`.
