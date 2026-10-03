# Terminal screens

`Router` and `Route` use `fusor_router::view` for route matching, decoded
parameters and nested view retention. Route factories are the compiler's supplied
factories. A terminal screen is an application location, not a network request.

```html
<main><Router>
  <Route path="/"><JobList></JobList></Route>
  <Route path="/jobs/:id" let="job">
    <JobDetails id="{{ job.id.clone() }}"></JobDetails>
  </Route>
  <Route fallback><p>Unknown screen</p></Route>
</Router></main>
```

`History::install(&owner, "/")` in the application constructor selects the
initial location. A Router without an explicit history starts at `/`.
Components resolve `History::from_owner(&owner)` and call `push`, `replace`,
`back` or `forward`. Bind these commands to application buttons; Escape is not
reserved. `location()` reads the reactive `AppUrl`, including the destination
during prepared view construction. Invalid URLs and unavailable/disposed routers
return `ErrorKind::Navigation`.

A history retains at most 64 locations. Push after Back truncates the forward
entries; pushing beyond the limit evicts the oldest location. Back and Forward
at the ends are no-ops. History is memory-only, with no shell or browser history
integration. One root outlet uses each history; nested outlets share it.

Destination preparation must succeed before history changes. The shared router
then commits synchronously, before a frame is presented. Failed and abandoned
stages preserve the previous location, views and focus. Reentrant history
changes return an error. Use `History` for navigation when history must stay
synchronized; directly using fusor's lower-level `Navigation` bypasses this
adapter's entries.

Query/fragment changes preserve view identity. Changed parameters replace only
the affected view, retaining surviving parents and their local state. Focus
remains in a surviving control; otherwise the next presentation chooses the
destination's first eligible control. Candidates attached during preparation
remain invisible. History stores locations, never disposed node handles.

A handle obtained inside a view cannot keep that view alive and becomes invalid
when it leaves. Keep a handle from the application owner for persistent navigation.
Dropping an outlet or its owner disposes the route tree even if navigation
handles remain. View-owned resources cancel through fusor ownership; work owned
by the application can continue. See [async views](async.md) for region restrictions.

The [routing consumer](../tests/consumer/src/routing.rs) checks history and
view lifecycle; run `just consumer`.
