# Coherent async views

A coherent region doesn't show partial results. It publishes a new view only when the
reads it depends on have completed; until then it keeps the last published view on
screen. Use it when several pieces of loaded data need to appear together. See the
[restrictions](#restrictions) before placing controls inside the region.

## Declaring an async region

Declare reads with an owner and a local spawner, and put loading, error and retry
controls outside the coherent region. The owner is the fusor object that scopes the
lifetime of the app's state and tasks. The spawner starts the futures (Rust's values
for work that finishes later) that perform the reads. [Local services](services.md)
explains how to get a spawner from the component
owner supplied by Fusor.

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
`fusor_async::AsyncValue`. In the example, the status line and the Retry button sit
outside `Async`, so they stay visible and usable while the region is hidden or blocked.
`status()` reports `Detached`, `Pending`, `Ready`, `Error`, `Faulted` or `Disposed`.
`retry()` retries failed reads while allowing compatible successful reads to be reused.
For a complete Rust constructor, see `JobDetails` in the
[job inspector](../examples/job-inspector/src/main.rs).

A standalone `Await` starts an independent boundary; inside an existing coherent scope
it joins that boundary. `Async` and `Await` each need one ordinary HTML
root element: `<section>` and `<p>` in this example. Put component tags inside that root.
Reads in independent `Await` branches are discovered in the same pass, and completion
of only some reads doesn't paint a partial result.

## Behavior while reads are pending

Before the first successful publication, the entire region, including static content,
is hidden. After that, pending reads and recoverable errors retain the last committed
scene (the retained tree of terminal elements), and that region's event handlers
are blocked until the boundary is ready.

Focus can remain on a pending control, or Tab can leave the region. Completing the read
doesn't steal focus back from another control. Navigation and controls outside the
region remain available.

## Constructors and effects

Components in a coherent region are built as candidates before they're shown. Prepared
component constructors retain their ordinary Rust semantics, except for effects that
`fusor::coherence::prepare_state` explicitly defers through `render::construct`.
Candidate effects begin only when adopted owners activate. Arbitrary constructor side
effects are not transactions. Input/structural factories run untracked.

Within one input epoch, pending candidates survive completion and retry. Obsolete
candidates, including slots that are no longer visited, are released through
`Attempt::on_invalidate`.

## Keyed rows

Keyed rows keep committed node and component identity. Candidate reads project row
signals (fusor's reactive values) through `Signal::with_render_value` and `Versions`,
so cached and nested memos (values computed from signals) see speculative data without
modifying committed caches. A read can restart when captured source versions change, even if
its key compares equal.

## How publication works

Hypercmd validates all proposed child replacements together before publishing. Duplicate
keys, duplicate component-local IDs and failed component preparation preserve the
previous scene.

Publication swaps prepared values without calling application code. Afterward, in a
finish phase, it updates row signals, activates adopted owners and retires old scopes.
Old event captures remain retained until this phase, and scene borrows are released
before callbacks or destructors can run.

## Restrictions

Nested `Async` boundaries, editable controls and router outlets inside coherent regions
are unsupported. Direct authored violations produce compiler diagnostics; incompatible
reusable components fail coherent preparation while retaining the last scene. `Async`
regions inside routes are supported; see [terminal screens](routing.md).

## Node and scope state for hosts

`Node::is_visible()` distinguishes a hidden initial candidate from retained content.
`Node::is_interactive()` also checks owner activation, boundary readiness and scene
health. An unexpected publication fault disables interaction for the whole scene.

Custom hosts must stop when `Scope::is_faulted()` becomes true; the native runner
restores the terminal and exits. `Scope::take_errors()` exposes structured publication
errors alongside ordinary update errors.

## Verification

The [async consumer tests](../tests/consumer/src/async_views.rs) cover the generated
coherence behavior with a controlled executor. Run `just consumer` to execute them.
