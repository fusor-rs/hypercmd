//! Local tasks, monotonic timers and bounded workers for one application owner.
use crate::Error;
use fusor::{ContextKey, OwnerHandle};
use fusor_async::CancellationToken;
use futures_channel::oneshot;
use futures_util::{
    future::{AbortHandle, Abortable, LocalBoxFuture},
    stream::FuturesUnordered,
    task::AtomicWaker,
};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    future::Future,
    panic::{self, AssertUnwindSafe},
    pin::Pin,
    rc::{Rc, Weak},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
    time::{Duration, Instant},
};

#[cfg(any(feature = "native", test))]
use futures_util::stream::Stream;

#[cfg(any(feature = "native", test))]
const COMPLETIONS_PER_TURN: usize = 32;

/// Bounds include tasks waiting to start, registered timers, and worker results
/// not yet consumed. A cancelled worker retains its slot until its thread exits.
#[derive(Clone, Copy, Debug)]
pub struct ServiceLimits {
    pub max_tasks: usize,
    pub max_timers: usize,
    pub max_workers: usize,
}
impl Default for ServiceLimits {
    fn default() -> Self {
        Self {
            max_tasks: 256,
            max_timers: 256,
            max_workers: 4,
        }
    }
}

#[derive(Default)]
struct Notify {
    ready: AtomicBool,
    outer: AtomicWaker,
}
impl Wake for Notify {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.ready.store(true, Ordering::Release);
        self.outer.wake();
    }
}

type Tasks = FuturesUnordered<LocalBoxFuture<'static, ()>>;
struct State {
    limits: Cell<ServiceLimits>,
    closed: Cell<bool>,
    count: Rc<Cell<usize>>,
    incoming: RefCell<Vec<LocalBoxFuture<'static, ()>>>,
    tasks: RefCell<Tasks>,
    fault: RefCell<Option<Error>>,
    notify: Arc<Notify>,
    clock: Box<dyn Fn() -> Instant>,
    epoch: Instant,
    next_timer: Cell<u64>,
    timers: RefCell<BTreeMap<u64, (Instant, Option<Waker>)>>,
    workers: Arc<AtomicUsize>,
    worker_slots: RefCell<Vec<std::sync::Weak<WorkerSlot>>>,
}

/// An application-local handle. Obtain it from the constructor's owner; child
/// owners inherit the same services. Retained handles cannot revive a disposed app.
#[derive(Clone)]
pub struct Services(Rc<State>);
impl ContextKey for Services {
    type Value = Self;
}
impl Services {
    pub(crate) fn new() -> Self {
        Self::with_clock(Instant::now)
    }

    fn with_clock(clock: impl Fn() -> Instant + 'static) -> Self {
        Self(Rc::new(State {
            limits: Cell::new(ServiceLimits::default()),
            closed: Cell::new(false),
            count: Rc::new(Cell::new(0)),
            incoming: RefCell::default(),
            tasks: RefCell::default(),
            fault: RefCell::default(),
            notify: Arc::default(),
            epoch: clock(),
            clock: Box::new(clock),
            next_timer: Cell::new(0),
            timers: RefCell::default(),
            workers: Arc::new(AtomicUsize::new(0)),
            worker_slots: RefCell::default(),
        }))
    }

    pub fn from_owner(owner: &OwnerHandle) -> Result<Self, Error> {
        owner
            .context::<Self>()
            .map(|services| (*services).clone())
            .ok_or_else(|| Error::disposed("owner has no live Hypercmd services"))
    }

    /// Configure bounds in the application constructor. Lowering a bound below
    /// current usage fails without changing any limit.
    pub fn set_limits(&self, limits: ServiceLimits) -> Result<(), Error> {
        self.check_live()?;
        if limits.max_tasks == 0 || limits.max_timers == 0 || limits.max_workers == 0 {
            return Err(Error::limit("service limits must be greater than zero"));
        }
        if self.0.count.get() > limits.max_tasks
            || self.0.timers.borrow().len() > limits.max_timers
            || self.0.workers.load(Ordering::Acquire) > limits.max_workers
        {
            return Err(Error::limit("service limits are below current usage"));
        }
        self.0.limits.set(limits);
        Ok(())
    }

    /// Adapter for fusor-async/query. Their futures retain their own cancellation
    /// and generation checks. Because their spawner cannot return an error,
    /// admission failure faults the runner instead of leaving a silent pending read.
    pub fn spawner(&self) -> impl Fn(LocalBoxFuture<'static, ()>) + 'static {
        let services = self.clone();
        move |future| {
            if let Err(error) = services.enqueue(future) {
                services.0.fault.borrow_mut().get_or_insert(error);
                services.0.notify.wake_by_ref();
            }
        }
    }

    /// Spawn application work that stops being polled after owner disposal.
    pub fn spawn(
        &self,
        owner: &OwnerHandle,
        future: impl Future<Output = ()> + 'static,
    ) -> Result<(), Error> {
        if owner.is_disposed() {
            return Err(Error::disposed("task owner was disposed"));
        }
        let (abort, registration) = AbortHandle::new_pair();
        let cleanup = owner.on_cleanup(move || abort.abort());
        let (activate, activated) = oneshot::channel();
        let activation = owner.on_activate(move || {
            let _ = activate.send(());
        });
        self.enqueue(Box::pin(async move {
            let (_cleanup, _activation) = (cleanup, activation);
            let _ = Abortable::new(
                async move {
                    if activated.await.is_ok() {
                        future.await;
                    }
                },
                registration,
            )
            .await;
        }))
    }

    fn enqueue(&self, future: LocalBoxFuture<'static, ()>) -> Result<(), Error> {
        self.check_live()?;
        if self.0.count.get() >= self.0.limits.get().max_tasks {
            return Err(Error::limit("local task queue is full"));
        }
        self.0.count.set(self.0.count.get() + 1);
        let permit = TaskPermit(self.0.count.clone());
        self.0.incoming.borrow_mut().push(Box::pin(async move {
            let _permit = permit;
            future.await;
        }));
        self.0.notify.wake_by_ref();
        Ok(())
    }

    /// Monotonic elapsed time, suitable for a fusor-query clock closure.
    pub fn now(&self) -> Duration {
        (self.0.clock)().saturating_duration_since(self.0.epoch)
    }

    /// Register a deadline without starting a thread. Dropping the future removes
    /// the deadline, including when a fusor request is cancelled.
    pub fn sleep(&self, duration: Duration) -> Result<Sleep, Error> {
        self.check_live()?;
        let deadline = (self.0.clock)()
            .checked_add(duration)
            .ok_or_else(|| Error::limit("timer deadline is out of range"))?;
        if self.0.timers.borrow().len() >= self.0.limits.get().max_timers {
            return Err(Error::limit("timer queue is full"));
        }
        let id = self
            .0
            .next_timer
            .get()
            .checked_add(1)
            .ok_or_else(|| Error::limit("timer identifiers exhausted"))?;
        self.0.next_timer.set(id);
        self.0.timers.borrow_mut().insert(id, (deadline, None));
        self.0.notify.wake_by_ref();
        Ok(Sleep {
            state: Rc::downgrade(&self.0),
            id,
            deadline,
        })
    }

    /// Run blocking work with an owned result and a cooperative cancellation flag.
    /// Only the closure, flag and result cross threads. No UI handles are Send.
    /// Cancellation cannot undo writes or forcibly stop arbitrary worker code.
    pub fn worker<T: Send + 'static>(
        &self,
        cancel: &CancellationToken,
        work: impl FnOnce(Arc<AtomicBool>) -> T + Send + 'static,
    ) -> Result<Worker<T>, Error> {
        self.check_live()?;
        if cancel.is_cancelled() {
            return Err(Error::service("worker request was cancelled"));
        }
        if self.0.workers.load(Ordering::Acquire) >= self.0.limits.get().max_workers {
            return Err(Error::limit("worker result queue is full"));
        }
        self.0.workers.fetch_add(1, Ordering::AcqRel);
        let slot = Arc::new(WorkerSlot {
            count: self.0.workers.clone(),
            cancel: Arc::new(AtomicBool::new(false)),
            waker: AtomicWaker::new(),
        });
        let cancelled = slot.clone();
        let registration = cancel.on_cancel(move || cancelled.cancel());
        let (send, receive) = oneshot::channel();
        let thread_slot = slot.clone();
        std::thread::Builder::new()
            .name("hypercmd-worker".into())
            .spawn(move || {
                MANAGED_WORKER.with(|managed| managed.set(true));
                let result =
                    panic::catch_unwind(AssertUnwindSafe(|| work(thread_slot.cancel.clone())))
                        .map_err(|payload| Error::service(panic_message(payload.as_ref())));
                // The receiver is gone when the request was cancelled first.
                let _ = send.send(result);
            })
            .map_err(|error| Error::service(format!("cannot start worker: {error}")))?;
        let mut slots = self.0.worker_slots.borrow_mut();
        slots.retain(|slot| slot.strong_count() != 0);
        slots.push(Arc::downgrade(&slot));
        Ok(Worker {
            receive,
            slot,
            _registration: registration,
        })
    }

    fn check_live(&self) -> Result<(), Error> {
        if self.0.closed.get() {
            Err(Error::disposed("application services were disposed"))
        } else {
            Ok(())
        }
    }

    #[cfg(feature = "native")]
    pub(crate) fn register_waker(&self, waker: &Waker) {
        self.0.notify.outer.register(waker);
    }

    #[cfg(feature = "native")]
    pub(crate) fn is_ready(&self) -> bool {
        self.0.notify.ready.load(Ordering::Acquire)
    }

    #[cfg(any(feature = "native", test))]
    pub(crate) fn timeout(&self) -> Option<Duration> {
        let now = (self.0.clock)();
        self.0
            .timers
            .borrow()
            .values()
            .map(|(deadline, _)| deadline.saturating_duration_since(now))
            .min()
    }

    #[cfg(any(feature = "native", test))]
    pub(crate) fn poll_turn(&self) -> Result<(), Error> {
        self.check_live()?;
        if let Some(error) = self.0.fault.take() {
            return Err(error);
        }
        let now = (self.0.clock)();
        let expired: Vec<_> = self
            .0
            .timers
            .borrow()
            .iter()
            .filter(|(_, (deadline, _))| *deadline <= now)
            .map(|(id, _)| *id)
            .collect();
        let wakes: Vec<_> = {
            let mut timers = self.0.timers.borrow_mut();
            expired
                .into_iter()
                .filter_map(|id| timers.remove(&id).and_then(|(_, waker)| waker))
                .collect()
        };
        for waker in wakes {
            waker.wake();
        }
        if !self.0.notify.ready.swap(false, Ordering::AcqRel) {
            return Ok(());
        }
        // User code may enqueue or dispose services during polling. Keep the pool
        // outside RefCell borrows and merge newly queued tasks on the next turn.
        let mut tasks = self.0.tasks.take();
        for future in self.0.incoming.take() {
            tasks.push(future);
        }
        let waker = Waker::from(self.0.notify.clone());
        let mut context = Context::from_waker(&waker);
        let mut completed = 0;
        for _ in 0..COMPLETIONS_PER_TURN {
            match Pin::new(&mut tasks).poll_next(&mut context) {
                Poll::Ready(Some(())) if !self.0.closed.get() => completed += 1,
                _ => break,
            }
        }
        if self.0.closed.get() {
            drop(tasks);
        } else {
            if completed == COMPLETIONS_PER_TURN {
                self.0.notify.wake_by_ref();
            }
            drop(self.0.tasks.replace(tasks));
        }
        if let Some(error) = self.0.fault.take() {
            return Err(error);
        }
        Ok(())
    }

    pub(crate) fn shutdown(&self) {
        if self.0.closed.replace(true) {
            return;
        }
        for slot in self
            .0
            .worker_slots
            .take()
            .into_iter()
            .filter_map(|slot| slot.upgrade())
        {
            slot.cancel();
        }
        drop((
            self.0.timers.take(),
            self.0.incoming.take(),
            self.0.tasks.take(),
        ));
        self.0.notify.wake_by_ref();
    }
}

struct TaskPermit(Rc<Cell<usize>>);
impl Drop for TaskPermit {
    fn drop(&mut self) {
        self.0.set(self.0.get() - 1);
    }
}

/// A cancellable monotonic deadline from [`Services::sleep`].
pub struct Sleep {
    state: Weak<State>,
    id: u64,
    deadline: Instant,
}
impl Future for Sleep {
    type Output = Result<(), Error>;
    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let Some(state) = self.state.upgrade().filter(|state| !state.closed.get()) else {
            return Poll::Ready(Err(Error::disposed("timer services were disposed")));
        };
        if (state.clock)() >= self.deadline {
            state.timers.borrow_mut().remove(&self.id);
            return Poll::Ready(Ok(()));
        }
        if let Some((_, waker)) = state.timers.borrow_mut().get_mut(&self.id) {
            *waker = Some(context.waker().clone());
        }
        Poll::Pending
    }
}
impl Drop for Sleep {
    fn drop(&mut self) {
        if let Some(state) = self.state.upgrade() {
            state.timers.borrow_mut().remove(&self.id);
        }
    }
}

struct WorkerSlot {
    count: Arc<AtomicUsize>,
    cancel: Arc<AtomicBool>,
    waker: AtomicWaker,
}
impl WorkerSlot {
    fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
        self.waker.wake();
    }
}
impl Drop for WorkerSlot {
    fn drop(&mut self) {
        self.count.fetch_sub(1, Ordering::AcqRel);
    }
}

/// One owned worker result. Dropping it requests cooperative cancellation.
pub struct Worker<T> {
    receive: oneshot::Receiver<Result<T, Error>>,
    slot: Arc<WorkerSlot>,
    _registration: fusor_async::CancelRegistration,
}
impl<T> Future for Worker<T> {
    type Output = Result<T, Error>;
    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        this.slot.waker.register(context.waker());
        if this.slot.cancel.load(Ordering::Acquire) {
            return Poll::Ready(Err(Error::service("worker request was cancelled")));
        }
        match Pin::new(&mut this.receive).poll(context) {
            Poll::Ready(Ok(result)) => Poll::Ready(result),
            Poll::Ready(Err(_)) => {
                Poll::Ready(Err(Error::service("worker stopped without a result")))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}
impl<T> Drop for Worker<T> {
    fn drop(&mut self) {
        self.slot.cancel();
    }
}

thread_local! {
    static MANAGED_WORKER: Cell<bool> = const { Cell::new(false) };
    static WORKER_PANIC: RefCell<Option<String>> = const { RefCell::new(None) };
}
#[cfg(feature = "native")]
pub(crate) fn capture_worker_panic(info: &panic::PanicHookInfo<'_>) -> bool {
    MANAGED_WORKER.with(|managed| {
        if !managed.get() {
            return false;
        }
        WORKER_PANIC.with(|text| text.replace(Some(info.to_string())));
        true
    })
}
// The native panic hook records the full report; other hosts only have the payload.
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    WORKER_PANIC.with(|text| text.take()).unwrap_or_else(|| {
        payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| {
                payload
                    .downcast_ref::<&str>()
                    .map(|text| (*text).to_owned())
            })
            .unwrap_or_else(|| "worker panicked".into())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ErrorKind;
    use futures_util::future::poll_fn;

    // A scene consumer cannot establish scheduler admission, dormant-future
    // polling or a prepared owner's task lifetime. Exercise these together.
    #[test]
    fn tasks_wait_for_activation_wake_and_owner_disposal() {
        let root = crate::Scope::new(None, &[]).unwrap();
        root.publish();
        let scope = crate::Scope::new(Some(&root.owner()), &[]).unwrap();
        let services = Services::from_owner(&scope.owner()).unwrap();
        let polls = Rc::new(Cell::new(0));
        let wake = Rc::new(RefCell::new(None));
        let task_alive = Rc::new(());
        let (observed, stored, held) = (polls.clone(), wake.clone(), task_alive.clone());
        services
            .spawn(&scope.owner(), async move {
                let _held = held;
                poll_fn(move |context| {
                    observed.set(observed.get() + 1);
                    stored.replace(Some(context.waker().clone()));
                    Poll::<()>::Pending
                })
                .await;
            })
            .unwrap();
        services.poll_turn().unwrap();
        assert_eq!(
            polls.get(),
            0,
            "prepared task ran an application side effect"
        );
        scope.publish();
        services.poll_turn().unwrap();
        assert_eq!(polls.get(), 1);
        services.poll_turn().unwrap();
        services.poll_turn().unwrap();
        assert_eq!(polls.get(), 1, "a dormant future was busy-polled");
        wake.borrow().as_ref().unwrap().wake_by_ref();
        services.poll_turn().unwrap();
        assert_eq!(polls.get(), 2);
        scope.dispose();
        services.poll_turn().unwrap();
        assert_eq!(
            Rc::strong_count(&task_alive),
            1,
            "disposal dropped the task"
        );
        wake.borrow().as_ref().unwrap().wake_by_ref();
        services.poll_turn().unwrap();
        assert_eq!(polls.get(), 2, "disposed child task was polled again");
        assert_eq!(
            services.spawn(&scope.owner(), async {}).unwrap_err().kind,
            ErrorKind::Disposed
        );
        let root_ran = Rc::new(Cell::new(false));
        let observed = root_ran.clone();
        services
            .spawn(&root.owner(), async move {
                observed.set(true);
            })
            .unwrap();
        services.poll_turn().unwrap();
        assert!(
            root_ran.get(),
            "child disposal shut down application services"
        );
    }

    // Self-waking tasks and reentrant spawning must leave a bounded turn; a
    // spawner with no Result channel must surface overload rather than hang Await.
    #[test]
    fn hot_tasks_yield_and_admission_failure_is_visible() {
        let scope = crate::Scope::new(None, &[]).unwrap();
        let services = Services::from_owner(&scope.owner()).unwrap();
        services
            .set_limits(ServiceLimits {
                max_tasks: 2,
                ..ServiceLimits::default()
            })
            .unwrap();
        scope.publish();
        let polls = Rc::new(Cell::new(0));
        let observed = polls.clone();
        services
            .spawn(
                &scope.owner(),
                poll_fn(move |context| {
                    observed.set(observed.get() + 1);
                    context.waker().wake_by_ref();
                    Poll::<()>::Pending
                }),
            )
            .unwrap();
        let finished = Rc::new(Cell::new(false));
        let observed = finished.clone();
        let again = services.clone();
        let owner = scope.owner();
        services
            .spawn(&owner.clone(), async move {
                observed.set(true);
                assert_eq!(
                    again.spawn(&owner, async {}).unwrap_err().kind,
                    ErrorKind::Limit
                );
            })
            .unwrap();
        services.poll_turn().unwrap();
        assert!(finished.get());
        assert!(polls.get() <= 4, "a hot task monopolized one turn");
        services
            .spawn(&scope.owner(), std::future::pending())
            .unwrap();
        services.spawner()(Box::pin(async {}));
        assert_eq!(services.poll_turn().unwrap_err().kind, ErrorKind::Limit);
    }

    // Drive the real timer queue with a controlled monotonic clock; wall-clock
    // sleeps would not establish the idle deadline and cancelled-slot contracts.
    #[test]
    fn deadlines_wake_without_periodic_task_polling() {
        let start = Instant::now();
        let clock = Rc::new(Cell::new(start));
        let reader = clock.clone();
        let services = Services::with_clock(move || reader.get());
        services
            .set_limits(ServiceLimits {
                max_timers: 1,
                ..ServiceLimits::default()
            })
            .unwrap();
        let timer = services.sleep(Duration::from_secs(2)).unwrap();
        assert!(services.sleep(Duration::from_secs(1)).is_err());
        drop(timer);
        let timer = services.sleep(Duration::from_secs(3)).unwrap();
        let done = Rc::new(Cell::new(false));
        let observed = done.clone();
        services.spawner()(Box::pin(async move {
            timer.await.unwrap();
            observed.set(true);
        }));
        services.poll_turn().unwrap();
        assert_eq!(services.timeout(), Some(Duration::from_secs(3)));
        clock.set(start + Duration::from_secs(2));
        services.poll_turn().unwrap();
        assert!(!done.get());
        assert_eq!(services.now(), Duration::from_secs(2));
        clock.set(start + Duration::from_secs(3));
        services.poll_turn().unwrap();
        assert!(done.get());
        assert_eq!(services.timeout(), None);
        services.shutdown();
    }

    // A cancelled result cannot free capacity while arbitrary worker code is
    // still running. Only Send data reaches the worker; cancellation wakes UI.
    #[test]
    fn worker_cancellation_keeps_admission_until_thread_exit() {
        let services = Services::new();
        services
            .set_limits(ServiceLimits {
                max_workers: 1,
                ..ServiceLimits::default()
            })
            .unwrap();
        let source = fusor_async::CancellationSource::default();
        let (release, wait) = std::sync::mpsc::sync_channel(0);
        let (report, observed) = std::sync::mpsc::sync_channel(1);
        let worker = services
            .worker(&source.token(), move |cancel| {
                wait.recv().unwrap();
                report.send(cancel.load(Ordering::Acquire)).unwrap();
                7
            })
            .unwrap();
        let finished = Rc::new(Cell::new(false));
        let done = finished.clone();
        services.spawner()(Box::pin(async move {
            assert_eq!(worker.await.unwrap_err().kind, ErrorKind::Service);
            done.set(true);
        }));
        services.poll_turn().unwrap();
        source.cancel();
        services.poll_turn().unwrap();
        assert!(finished.get(), "cancellation did not wake the waiter");
        assert!(
            services
                .worker(&CancellationToken::default(), |_| ())
                .is_err()
        );
        release.send(()).unwrap();
        assert!(observed.recv_timeout(Duration::from_secs(2)).unwrap());
        services.shutdown();
    }
}
