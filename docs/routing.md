# Terminal screens

Routing lets an application move between screens, such as a list and a detail view.
`Router` and `Route` use `fusor_router::view` for route matching, decoded parameters and
nested view retention. A terminal screen is an application location, not a network
request.

## Declaring routes

```html
<main><Router>
  <Route path="/"><JobList></JobList></Route>
  <Route path="/jobs/:id" let="job">
    <JobDetails id="{{ job.id.clone() }}"></JobDetails>
  </Route>
  <Route fallback><p>Unknown screen</p></Route>
</Router></main>
```

The `:id` segment captures a path parameter. `let="job"` makes the decoded
parameters available to the template, so `/jobs/42` passes `"42"` to `JobDetails`.
The fallback displays when no route matches. The compiler supplies the factories
that construct route components.

## Navigating

`History::install(&owner, "/")` in the application constructor selects the initial
location. A Router without an explicit history starts at `/`. The owner is the fusor
object that scopes the application's state and tasks, and `&owner` is Rust's syntax for
borrowing it.

Components resolve `History::from_owner(&owner)` and call `push`, `replace`, `back` or
`forward`. For example, a
component with a `history` field can navigate home with this button from the
[routing example](../tests/consumer/ui/routing.html):

```html
<button id="leave" on:click='state.history.push("/")'>Leave</button>
```

Escape is not reserved.
`location()` reads the reactive `AppUrl`, including the destination during prepared view
construction. Invalid URLs and unavailable/disposed routers return `ErrorKind::Navigation`.

## History limits

A history retains at most 64 locations. Push after Back truncates the forward entries;
pushing beyond the limit evicts the oldest location. Back and Forward at the ends are
no-ops. History is memory-only, with no shell or browser history integration. Each history
belongs to one root `Router`. Nested routers share it; an outlet is where a router renders its
matched view.

## How a navigation commits

Destination preparation must succeed before history changes. The shared router then
commits synchronously, before a frame is presented. Failed and abandoned stages preserve
the previous location, views and focus. Trying to navigate again while another navigation
is in progress returns an error.

Use `History` for navigation when history must stay synchronized; directly using fusor's
lower-level `Navigation` bypasses this adapter's entries.

## State and focus across navigations

Query/fragment changes preserve view identity. Changed parameters replace only the
affected view, retaining surviving parents and their local state. Focus remains in a
surviving control; otherwise the next presentation chooses the destination's first
eligible control. Candidates attached during preparation remain invisible. History stores
locations, never disposed node handles.

## Lifetimes and handles

A handle obtained inside a view cannot keep that view alive and becomes invalid when it
leaves. Keep a handle from the application owner for persistent navigation. Dropping an
outlet or its owner (in Rust, letting it go out of scope and be destroyed) disposes the
route tree even if navigation handles remain. View-owned resources cancel through fusor
ownership; work owned by the application can continue.

`Async` regions are supported inside routes, but router outlets inside coherent regions
are not. See [async views](async.md) for the region restrictions.

## Verification

The [routing consumer](../tests/consumer/src/routing.rs) checks history and view
lifecycle; run `just consumer`.
