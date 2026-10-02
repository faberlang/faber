//! The lazy sync-generator carrier for generated Rust (`fiunt`).
//!
//! A generator body is an `async` block, which rustc lowers to a state
//! machine; `cede` parks the item in a shared slot and suspends for one poll.
//! [`Gen::next`](Iterator::next) polls once with a no-op waker, so the body
//! only advances when the consumer pulls. No thread, executor, or `Send` bound
//! is involved.
//!
//! Generated code spells the carrier types at the crate root (`faber::Gen`,
//! `faber::Yield`); the module is `r#gen` because `gen` is a reserved keyword
//! of the 2024 edition.

use std::cell::RefCell;
use std::fmt::{Debug, Formatter, Result as FmtResult};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
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

#[cfg(test)]
#[path = "gen_test.rs"]
mod tests;
