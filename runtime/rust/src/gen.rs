//! The generator and async carriers for generated Rust: the lazy
//! sync-generator (`fiunt`), the async cursor behind `fient`, and `block_on`.
//!
//! A generator body is an `async` block, which rustc lowers to a state
//! machine; `cede` parks the item in a shared slot and suspends for one poll.
//! [`Gen::next`](Iterator::next) polls once with a no-op waker, so the body
//! only advances when the consumer pulls. No thread, executor, or `Send` bound
//! is involved.
//!
//! Generated code spells the carrier types at the crate root (`faber::Gen`,
//! `faber::Yield`, `faber::AsyncCursor`, ...); the module is `r#gen` because
//! `gen` is a reserved keyword of the 2024 edition, so the free functions are
//! spelled `faber::r#gen::block_on`.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::fmt::{Debug, Formatter, Result as FmtResult};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::{Arc, Condvar, Mutex};
use std::task::{Context, Poll, Waker};

/// The `cede` side of a generator: parks one item in the shared slot.
pub struct Yield<T> {
    slot: Rc<RefCell<Option<T>>>,
}

/// The future `cede` awaits: pending once, ready on the next poll.
pub struct YieldNow(bool);

impl Future for YieldNow {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<()> {
        if self.0 {
            Poll::Ready(())
        } else {
            self.0 = true;
            Poll::Pending
        }
    }
}

impl<T> Yield<T> {
    /// The yield handle over the slot the generator shares with its body.
    #[must_use]
    pub fn new(slot: Rc<RefCell<Option<T>>>) -> Self {
        Self { slot }
    }

    /// Park `value` and suspend the body for one poll.
    pub fn put(&self, value: T) -> YieldNow {
        *self.slot.borrow_mut() = Some(value);
        YieldNow(false)
    }
}

/// A lazy generator: an iterator over the items its `async` body yields.
pub struct Gen<'a, T> {
    slot: Rc<RefCell<Option<T>>>,
    body: RefCell<Option<Pin<Box<dyn Future<Output = ()> + 'a>>>>,
}

impl<'a, T> Gen<'a, T> {
    /// A generator over `body`, which yields through `slot`.
    #[must_use]
    pub fn new(slot: Rc<RefCell<Option<T>>>, body: impl Future<Output = ()> + 'a) -> Self {
        Self {
            slot,
            body: RefCell::new(Some(Box::pin(body))),
        }
    }

    fn advance(&self) -> Option<T> {
        let mut guard = self.body.borrow_mut();
        let body = guard.as_mut()?;
        let mut cx = Context::from_waker(Waker::noop());
        match body.as_mut().poll(&mut cx) {
            Poll::Pending => self.slot.borrow_mut().take(),
            Poll::Ready(()) => {
                *guard = None;
                None
            }
        }
    }
}

impl<T> Iterator for Gen<'_, T> {
    type Item = T;

    fn next(&mut self) -> Option<T> {
        self.advance()
    }
}

impl<T: Debug> Debug for Gen<'_, T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        let mut rest = Vec::new();
        while let Some(item) = self.advance() {
            rest.push(item);
        }
        Debug::fmt(&rest, f)
    }
}

/// The pull side of an async generator (`fient`): the consumer queues one
/// request per `next`, the producer thread-side body answers it.
pub struct AsyncCursor<T, E> {
    shared: Arc<AsyncCursorShared<T, E>>,
}

/// The state the producer body and the consumer cursor share.
pub struct AsyncCursorShared<T, E> {
    state: Mutex<AsyncCursorState<T, E>>,
    requests_available: Condvar,
}

struct AsyncCursorState<T, E> {
    requests: VecDeque<Sender<Result<Option<T>, E>>>,
    terminal: Option<Result<(), E>>,
    closed: bool,
}

/// The future one `next` call awaits.
pub struct AsyncCursorNext<T, E> {
    ready: Option<Result<Option<T>, E>>,
    receiver: Option<Receiver<Result<Option<T>, E>>>,
}

impl<T, E> Default for AsyncCursorShared<T, E> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T, E> AsyncCursorShared<T, E> {
    /// An open shared state with no queued request and no terminal result.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Mutex::new(AsyncCursorState {
                requests: VecDeque::new(),
                terminal: None,
                closed: false,
            }),
            requests_available: Condvar::new(),
        }
    }

    /// The next queued consumer request, waiting for one; `None` once closed.
    ///
    /// # Panics
    ///
    /// Panics if the state mutex is poisoned.
    pub fn next_request(&self) -> Option<Sender<Result<Option<T>, E>>> {
        let mut state = self.state.lock().expect("async cursor state poisoned");
        loop {
            if state.closed {
                return None;
            }
            if let Some(request) = state.requests.pop_front() {
                return Some(request);
            }
            state = self
                .requests_available
                .wait(state)
                .expect("async cursor state poisoned");
        }
    }

    /// Close the cursor with the body's terminal `result`, answering every
    /// pending request.
    ///
    /// # Panics
    ///
    /// Panics if the state mutex is poisoned.
    pub fn finish(&self, result: Result<(), E>) {
        let mut state = self.state.lock().expect("async cursor state poisoned");
        state.closed = true;
        let mut requests = std::mem::take(&mut state.requests);
        match result {
            Ok(()) => {
                for request in requests {
                    let _ = request.send(Ok(None));
                }
                state.terminal = Some(Ok(()));
            }
            Err(err) => {
                if let Some(first) = requests.pop_front() {
                    let _ = first.send(Err(err));
                    for request in requests {
                        let _ = request.send(Ok(None));
                    }
                    state.terminal = Some(Ok(()));
                } else {
                    state.terminal = Some(Err(err));
                }
            }
        }
        self.requests_available.notify_all();
    }
}

impl<T, E> AsyncCursor<T, E> {
    /// The consumer cursor over `shared`.
    #[must_use]
    pub fn new(shared: Arc<AsyncCursorShared<T, E>>) -> Self {
        Self { shared }
    }

    /// Queue one request and return the future that resolves it.
    ///
    /// # Panics
    ///
    /// Panics if the state mutex is poisoned.
    #[must_use]
    #[allow(clippy::should_implement_trait)]
    pub fn next(&self) -> AsyncCursorNext<T, E> {
        let (sender, receiver) = channel();
        let mut state = self
            .shared
            .state
            .lock()
            .expect("async cursor state poisoned");
        if let Some(terminal) = state.terminal.take() {
            return match terminal {
                Ok(()) => {
                    state.terminal = Some(Ok(()));
                    AsyncCursorNext {
                        ready: Some(Ok(None)),
                        receiver: None,
                    }
                }
                Err(err) => {
                    state.terminal = Some(Ok(()));
                    AsyncCursorNext {
                        ready: Some(Err(err)),
                        receiver: None,
                    }
                }
            };
        }
        if state.closed {
            return AsyncCursorNext {
                ready: Some(Ok(None)),
                receiver: None,
            };
        }
        state.requests.push_back(sender);
        drop(state);
        self.shared.requests_available.notify_one();
        AsyncCursorNext {
            ready: None,
            receiver: Some(receiver),
        }
    }
}

impl<T, E> Drop for AsyncCursor<T, E> {
    fn drop(&mut self) {
        let mut state = self
            .shared
            .state
            .lock()
            .expect("async cursor state poisoned");
        state.closed = true;
        state.requests.clear();
        self.shared.requests_available.notify_all();
    }
}

impl<T: Unpin, E: Unpin> Future for AsyncCursorNext<T, E> {
    type Output = Result<Option<T>, E>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let Some(ready) = self.ready.take() {
            return Poll::Ready(ready);
        }
        let Some(receiver) = self.receiver.as_ref() else {
            return Poll::Ready(Ok(None));
        };
        match receiver.try_recv() {
            Ok(step) => {
                self.receiver = None;
                Poll::Ready(step)
            }
            Err(TryRecvError::Empty) => {
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            Err(TryRecvError::Disconnected) => {
                self.receiver = None;
                Poll::Ready(Ok(None))
            }
        }
    }
}

/// Drive `future` to completion on the calling thread: poll with a no-op
/// waker, yielding the thread between polls. No runtime is involved.
pub fn block_on<F: Future>(future: F) -> F::Output {
    let mut context = Context::from_waker(Waker::noop());
    let mut future = Box::pin(future);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::yield_now(),
        }
    }
}

/// Drive `future` to completion on a current-thread Tokio runtime, for
/// programs whose library bindings need one.
///
/// # Panics
///
/// Panics if the runtime cannot be created.
#[cfg(feature = "tokio")]
pub fn block_on_tokio<F: Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to create Tokio runtime")
        .block_on(future)
}

#[cfg(test)]
#[path = "gen_test.rs"]
mod tests;
