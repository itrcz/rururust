//! Background work → app state: a channel for network requests and streaming.
//!
//! A [`Link`] carries closures `FnOnce(&mut S)` from any thread into the UI thread's
//! inbox; the UI applies them at the start of the next frame (or on [`Ui::pump`]).
//! [`spawn`] runs a future off the UI thread with a link and returns a [`Task`] that
//! cancels it when dropped.

use std::any::Any;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use crate::ui::Ui;

type Waker = Arc<dyn Fn() + Send + Sync>;
type Entry = (Arc<AtomicBool>, Box<dyn Any + Send>);

struct Inbox {
    queue: Mutex<Vec<Entry>>,
    /// Set by the first `send` after a pump, so a burst of sends wakes the loop once.
    woken: AtomicBool,
    /// Per inbox rather than global: each UI thread (and each test) wakes its own loop.
    waker: Mutex<Option<Waker>>,
}

thread_local! {
    static INBOX: Arc<Inbox> = Arc::new(Inbox {
        queue: Mutex::new(Vec::new()),
        woken: AtomicBool::new(false),
        waker: Mutex::new(None),
    });
}

fn inbox() -> Arc<Inbox> {
    INBOX.with(Arc::clone)
}

/// Sets what wakes the UI thread's event loop when a [`Link`] sends something.
/// Call it on the UI thread (the runner does this; embedders call it themselves).
/// It is called from the sending thread, at most once between two pumps.
pub fn set_waker(f: impl Fn() + Send + Sync + 'static) {
    let inbox = inbox();
    *inbox.waker.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(f));
}

/// A handle that sends state updates to the UI thread from anywhere.
///
/// Clone it and move it into another thread freely; `S` itself does not need to be `Send`
/// because only the closures cross threads.
pub struct Link<S> {
    inbox: Arc<Inbox>,
    cancel: Arc<AtomicBool>,
    _s: PhantomData<fn(&mut S)>,
}

impl<S> Clone for Link<S> {
    fn clone(&self) -> Self {
        Link { inbox: self.inbox.clone(), cancel: self.cancel.clone(), _s: PhantomData }
    }
}

/// Creates a [`Link`] to the current thread's UI. Call it on the UI thread — in an event
/// handler, like [`after`](crate::after) — then move the link to the worker:
///
/// ```no_run
/// # struct App { reply: String }
/// let link = ruru::link::<App>();
/// std::thread::spawn(move || link.send(|s| s.reply = "done".into()));
/// ```
///
/// Called on a thread that runs no `Ui`, it links to that thread's own inbox, which nobody
/// pumps: the updates are silently lost.
pub fn link<S: 'static>() -> Link<S> {
    Link { inbox: inbox(), cancel: Arc::new(AtomicBool::new(false)), _s: PhantomData }
}

impl<S: 'static> Link<S> {
    /// Queues `f` to run on the state; call it once or many times, updates apply in order.
    /// Does nothing once the owning [`Task`] is cancelled.
    pub fn send(&self, f: impl FnOnce(&mut S) + Send + 'static) {
        if self.cancelled() {
            return;
        }
        let f: Box<dyn FnOnce(&mut S) + Send> = Box::new(f);
        self.inbox.queue.lock().unwrap_or_else(|e| e.into_inner()).push((self.cancel.clone(), Box::new(f)));
        // After the push: a pump that resets `woken` before taking the queue either sees
        // this entry or is followed by our wake.
        if !self.inbox.woken.swap(true, Ordering::AcqRel) {
            let waker = self.inbox.waker.lock().unwrap_or_else(|e| e.into_inner()).clone();
            if let Some(w) = waker {
                w();
            }
        }
    }
}

impl<S> Link<S> {
    /// True once the owning [`Task`] was cancelled or dropped: the worker should stop.
    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Acquire)
    }
}

impl<S: 'static> Ui<S> {
    /// Applies everything sent through links without building a frame.
    /// Returns true if any update was applied (the state changed, a redraw is due).
    /// [`Ui::frame`] calls it first thing.
    pub fn pump(&mut self) -> bool {
        let inbox = inbox();
        inbox.woken.store(false, Ordering::Release);
        let taken = std::mem::take(&mut *inbox.queue.lock().unwrap_or_else(|e| e.into_inner()));
        if taken.is_empty() {
            return false;
        }
        let mut applied = false;
        let mut foreign = Vec::new();
        for (cancel, f) in taken {
            match f.downcast::<Box<dyn FnOnce(&mut S) + Send>>() {
                Ok(f) => {
                    if !cancel.load(Ordering::Acquire) {
                        f(&mut self.state);
                        applied = true;
                    }
                }
                // Another `Ui` with a different state type shares this thread; keep it for them.
                Err(f) => foreign.push((cancel, f)),
            }
        }
        if !foreign.is_empty() {
            let mut q = inbox.queue.lock().unwrap_or_else(|e| e.into_inner());
            foreign.append(&mut q);
            *q = foreign;
        }
        applied
    }
}

/// A running background job. Dropping it cancels the job; [`Task::detach`] lets it run on.
#[must_use = "dropping a Task cancels it; call .detach() to let it run"]
pub struct Task {
    cancel: Arc<AtomicBool>,
    detached: bool,
    #[cfg(not(target_arch = "wasm32"))]
    thread: Option<std::thread::Thread>,
}

impl Task {
    /// Stops the job: its pending updates are discarded, further sends are ignored and the
    /// future is not polled again.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
        // Wake a worker parked in `block_on` so it sees the flag and exits.
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(t) = &self.thread {
            t.unpark();
        }
    }

    /// Lets the job run to completion even though the handle is dropped.
    pub fn detach(mut self) {
        self.detached = true;
    }
}

impl Drop for Task {
    fn drop(&mut self) {
        if !self.detached {
            self.cancel();
        }
    }
}

/// Resolves to `()` as soon as the cancel flag is seen, without polling `inner` again.
struct Cancellable<Fut> {
    inner: Pin<Box<Fut>>,
    cancel: Arc<AtomicBool>,
}

impl<Fut: Future<Output = ()>> Future for Cancellable<Fut> {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.cancel.load(Ordering::Acquire) {
            return Poll::Ready(());
        }
        self.inner.as_mut().poll(cx)
    }
}

/// Runs `work` in the background with a [`Link`] to the UI. Call it on the UI thread.
///
/// Natively the future is created and driven on its own thread, so it may block (e.g. a
/// synchronous HTTP client) and need not be `Send`. On wasm it runs on the page's event
/// loop through the executor given to [`set_spawner`].
///
/// ```no_run
/// # struct App { text: String, job: Option<ruru::Task> }
/// # fn on_click(s: &mut App) {
/// s.job = Some(ruru::spawn(|link: ruru::Link<App>| async move {
///     for word in ["streamed", " reply"] {
///         if link.cancelled() { return; }
///         link.send(move |s| s.text.push_str(word));
///     }
/// }));
/// # }
/// ```
pub fn spawn<S: 'static, F, Fut>(work: F) -> Task
where
    F: FnOnce(Link<S>) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + 'static,
{
    let link = link::<S>();
    let cancel = link.cancel.clone();
    imp::spawn(link, work, cancel)
}

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use super::*;
    use std::task::{Wake, Waker};
    use std::thread::Thread;
    use std::time::{Duration, Instant};

    struct Unpark(Thread);
    impl Wake for Unpark {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }

    thread_local! {
        /// Set on task threads: `sleep` can hand its deadline to `block_on` instead of blocking.
        static IN_TASK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
        /// The earliest deadline a pending `sleep` asked to be polled again at.
        static DEADLINE: std::cell::Cell<Option<Instant>> = const { std::cell::Cell::new(None) };
    }

    fn block_on(fut: impl Future<Output = ()>) {
        IN_TASK.with(|t| t.set(true));
        let waker = Waker::from(Arc::new(Unpark(std::thread::current())));
        let mut cx = Context::from_waker(&waker);
        let mut fut = std::pin::pin!(fut);
        while fut.as_mut().poll(&mut cx).is_pending() {
            // Parking (not sleeping) keeps `Task::cancel` instant: its unpark ends the wait
            // early. Spurious unparks only cost an extra poll.
            match DEADLINE.with(|d| d.take()) {
                Some(t) => std::thread::park_timeout(t.saturating_duration_since(Instant::now())),
                None => std::thread::park(),
            }
        }
    }

    pub(super) struct Sleep(pub(super) Instant);

    impl Future for Sleep {
        type Output = ();
        fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<()> {
            let now = Instant::now();
            if now >= self.0 {
                return Poll::Ready(());
            }
            if IN_TASK.with(|t| t.get()) {
                DEADLINE.with(|d| d.set(Some(d.get().map_or(self.0, |t| t.min(self.0)))));
                Poll::Pending
            } else {
                // Some other executor: no deadline hook, so block this thread.
                std::thread::sleep(self.0 - now);
                Poll::Ready(())
            }
        }
    }

    pub(super) fn sleep(seconds: f32) -> Sleep {
        Sleep(Instant::now() + Duration::from_secs_f32(seconds.max(0.0)))
    }

    pub(super) fn spawn<S: 'static, F, Fut>(link: Link<S>, work: F, cancel: Arc<AtomicBool>) -> Task
    where
        F: FnOnce(Link<S>) -> Fut + Send + 'static,
        Fut: Future<Output = ()> + 'static,
    {
        let flag = cancel.clone();
        let handle = std::thread::Builder::new()
            .name("ruru-task".into())
            .spawn(move || block_on(Cancellable { inner: Box::pin(work(link)), cancel: flag }))
            .expect("ruru::spawn: failed to start a thread");
        Task { cancel, detached: false, thread: Some(handle.thread().clone()) }
    }

    /// Natively tasks run on their own threads; the spawner is not used.
    pub fn set_spawner(_f: impl Fn(Pin<Box<dyn Future<Output = ()>>>) + 'static) {}
}

#[cfg(target_arch = "wasm32")]
mod imp {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    type Spawner = Rc<dyn Fn(Pin<Box<dyn Future<Output = ()>>>)>;

    thread_local! {
        static SPAWNER: RefCell<Option<Spawner>> = const { RefCell::new(None) };
    }

    pub(super) fn spawn<S: 'static, F, Fut>(link: Link<S>, work: F, cancel: Arc<AtomicBool>) -> Task
    where
        F: FnOnce(Link<S>) -> Fut + Send + 'static,
        Fut: Future<Output = ()> + 'static,
    {
        let spawner = SPAWNER.with(|s| s.borrow().clone()).expect(
            "ruru::spawn: no executor on wasm. Call ruru::set_spawner(|f| wasm_bindgen_futures::spawn_local(f)) \
             before spawning",
        );
        spawner(Box::pin(Cancellable { inner: Box::pin(work(link)), cancel: cancel.clone() }));
        Task { cancel, detached: false }
    }

    #[cfg(feature = "app")]
    pub(super) async fn sleep(seconds: f32) {
        use wasm_bindgen_futures::js_sys::{Function, Promise};
        let ms = (seconds.max(0.0) * 1000.0).round() as i32;
        let promise = Promise::new(&mut |resolve: Function, _reject: Function| {
            if let Some(w) = web_sys::window() {
                let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms);
            }
        });
        let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
    }

    /// Sets the executor [`spawn`](super::spawn) uses on wasm, where there are no threads,
    /// e.g. `ruru::set_spawner(|f| wasm_bindgen_futures::spawn_local(f))`.
    pub fn set_spawner(f: impl Fn(Pin<Box<dyn Future<Output = ()>>>) + 'static) {
        SPAWNER.with(|s| *s.borrow_mut() = Some(Rc::new(f)));
    }
}

pub use imp::set_spawner;

/// Waits `seconds` inside a [`spawn`]ed task without blocking the UI.
///
/// Natively the task thread parks until the deadline (a cancel still stops it at once);
/// on wasm it awaits a `setTimeout`, which is why it needs the `app` feature there.
///
/// ```no_run
/// # struct App { text: String }
/// let task = ruru::spawn(|link: ruru::Link<App>| async move {
///     for word in ["one ", "two ", "three"] {
///         ruru::sleep(0.05).await;
///         link.send(move |s| s.text.push_str(word));
///     }
/// });
/// # task.detach();
/// ```
#[cfg(any(not(target_arch = "wasm32"), feature = "app"))]
pub fn sleep(seconds: f32) -> impl Future<Output = ()> {
    imp::sleep(seconds)
}
