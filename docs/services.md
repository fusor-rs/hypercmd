# Local services

Every application root installs `Services` in fusor owner context before its
constructor runs. Descendants share that instance. Obtain it with
`Services::from_owner(&owner)?`; a disposed owner cannot supply services. The
native runner drives these services. Disabling the `native` feature keeps the
portable scene and declarations; hosts provide their own executor integration.

`services.spawner()` matches `fusor_async::Spawner`. Pass it directly to
`fusor_async::AsyncValue::new`, `Resource::new` or `fusor_query::QueryClient::new`.
Fusor owns cancellation, read leases, generations and stale-result rejection.
Hypercmd schedules their local futures without moving signals or owners between
threads. `services.now()` returns monotonic elapsed `Duration` for query clocks.

For other local application work, `services.spawn(&owner, future)?` waits until
that owner activates and stops polling after its disposal. This preserves
prepared-candidate timing. An ordinary constructor's existing effects still run
with fusor's ordinary scheduling. `services.sleep(duration)?` returns a future
whose deadline uses `Instant`; dropping it unregisters the deadline.

The constructor can configure `ServiceLimits` through `set_limits`:

- `max_tasks`, default 256, includes pending admissions and running local futures.
- `max_timers`, default 256, bounds registered deadlines.
- `max_workers`, default 4, includes unfinished workers and unconsumed results.

Zero limits and reductions below current usage return `ErrorKind::Limit` without
changing the limits. Direct admission methods return errors. Fusor's spawner has
no error return channel, so admission failure records a fatal runner error; the
native session restores before returning it. It cannot silently strand an Await.

The executor uses the pinned `FuturesUnordered` scheduler, which polls children
only after their wakes. A turn accepts at most the bounded incoming task set and
consumes at most 32 completions; the scheduler itself yields after polling the
live set or repeated self-wakes. Futures must return promptly from `poll`;
cooperative scheduling cannot preempt arbitrary synchronous code. Native input
and signals receive a turn even while tasks keep waking. Pending tasks alone do
not trigger periodic polling or painting. One Mio Waker handles task notifications;
the nearest timer/input deadline determines how long the runner sleeps.

## Blocking workers

`services.worker(&token, move |cancelled| result)?` starts bounded blocking work
and returns a future yielding `Result<T, Error>`, where `T: Send + 'static`.
The closure receives `Arc<AtomicBool>` and should check it between operations.
Only owned input, output and that cancellation flag cross threads; `Rc`-based UI
state remains local. Application-specific errors can be carried in `T`, for
example a `Result<Records, LoadError>`.

Each admitted worker has one result slot. A completed result remains queued
until consumed or cancelled; completion and errors are not dropped to make room
for newer work. Dropping the future, cancelling its request token or disposing
the application sets the flag. A cancelled worker retains its capacity slot
until its thread exits, preventing repeated cancellations from creating
unbounded threads. The UI never joins a worker. Cancellation cannot undo writes
or forcibly interrupt a blocking library call; use cancellation-aware or bounded
I/O within the closure.

Worker panics become `ErrorKind::Service`. During native execution the panic hook
captures managed worker panic text, so it is reported as data through the result
instead of writing into the active terminal. Outside a native session the process
panic hook still applies. Owners and request generations decide whether a late
result is still relevant; cancellation does not authorize updating a removed UI.

## Verification

The [service tests](../crates/hypercmd/src/services.rs) and
[native PTY test](../crates/hypercmd/src/native/unix/tests.rs) cover scheduling,
ownership and worker results.
