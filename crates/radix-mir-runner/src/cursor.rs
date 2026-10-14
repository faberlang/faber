//! Suspended generator cursors (D6.6: cursors are lazy by definition).
//!
//! A `fiunt`/`fient` call builds a generator frame that is not on the runner
//! stack. Each pull pushes that frame back, runs it until its next `cede` (the
//! run loop parks the frame after the yielding statement) or until it
//! returns, and pops it again. The frame is the whole generator state: `cede`
//! only runs in the generator's own frame, and nested calls finish inside one
//! pull, so no other frame outlives a suspension.
//!
//! A stepped `itera ex` walk reads the cursor lazily (`length` demands one
//! more item, the index reads it); every other read materializes the cursor
//! into a `lista<T>` by pulling until the generator returns. Dropping the
//! cursor abandons the generator — an early `rumpe` leaves it unreferenced.
//!
//! A `tuus.cursor()` view reuses the same stepped walk over a conversation
//! (A4/U9): each pull takes the next content frame from the sermo, which in
//! turn resumes a cursor handler only when its queue is empty, so a `fiunt`
//! handler produces while the caller consumes.

use super::{Frame, Host, Runner, RunnerError, Value};
use crate::sermo::SermoValue;
use radix_hir::DefId;
use radix_lexer::Span;
use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

/// Shared handle to one suspended generator (`Value::Cursor`).
#[derive(Clone)]
pub struct GeneratorCursor(Rc<RefCell<CursorState>>);

struct CursorState {
    /// Parked generator frame; `None` once the generator returned or failed.
    frame: Option<Frame>,
    /// Conversation this cursor reads content frames from instead of a
    /// generator (`tuus.cursor()`), with the `scrinium` shape to wrap them in.
    sermo: Option<SermoSource>,
    /// The item most recently pulled, not yet necessarily read.
    current: Option<Value>,
    /// Items pulled so far (the walk's `length`).
    pulled: usize,
    /// Items read so far through the walk's index projection.
    read: usize,
    /// Error raised through the cursor's `⇥` channel, handed out once.
    raised: Option<Value>,
}

struct SermoSource {
    sermo: Rc<RefCell<SermoValue>>,
    frame: DefId,
}

/// One resumption of a generator frame.
pub(crate) enum GeneratorStep {
    /// The generator parked at a `cede` with this item.
    Yield(Value),
    /// The generator returned; its own return value is not an item.
    Return,
    /// The generator raised through its `⇥` channel with this error value.
    Raise(Value),
}

impl GeneratorCursor {
    pub(crate) fn new(frame: Frame) -> Self {
        Self(Rc::new(RefCell::new(CursorState {
            frame: Some(frame),
            sermo: None,
            current: None,
            pulled: 0,
            read: 0,
            raised: None,
        })))
    }

    /// Lazy content-frame cursor over a conversation's inbound direction.
    pub(crate) fn sermo(sermo: Rc<RefCell<SermoValue>>, frame: DefId) -> Self {
        Self(Rc::new(RefCell::new(CursorState {
            frame: None,
            sermo: Some(SermoSource { sermo, frame }),
            current: None,
            pulled: 0,
            read: 0,
            raised: None,
        })))
    }

    /// Whether the generator can still be resumed.
    pub(crate) fn is_live(&self) -> bool {
        self.0.borrow().frame.is_some()
    }

    /// The error raised through the cursor's `⇥` channel, handed out once.
    // WHY: the consumer (the cursor error path) lands separately; the unit
    // test pins the hand-out-once contract until then.
    #[allow(dead_code)]
    pub(crate) fn take_raised(&self) -> Option<Value> {
        self.0.borrow_mut().raised.take()
    }

    /// Stepped index read: only the item most recently pulled is readable.
    pub(crate) fn index(&self, index: usize, span: Span) -> Result<Value, RunnerError> {
        let mut state = self.0.borrow_mut();
        if index + 1 != state.pulled {
            return Err(RunnerError::internal(
                span,
                "stepped cursor index is not the current item",
            ));
        }
        state.read = state.pulled;
        state
            .current
            .clone()
            .ok_or_else(|| RunnerError::internal(span, "stepped cursor has no current item"))
    }
}

impl fmt::Debug for GeneratorCursor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = self.0.borrow();
        formatter
            .debug_struct("GeneratorCursor")
            .field("suspended", &state.frame.is_some())
            .field("pulled", &state.pulled)
            .finish()
    }
}

impl PartialEq for GeneratorCursor {
    /// Identity: two handles are equal only when they share one generator.
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl<H: Host + ?Sized> Runner<'_, H> {
    /// Stepped `length`: pull the next item unless one is pulled and unread,
    /// then report the items pulled so far. The walk's `index < length`
    /// check therefore fails exactly when the generator has returned.
    pub(crate) fn cursor_step_length(
        &mut self,
        cursor: &GeneratorCursor,
        span: Span,
    ) -> Result<usize, RunnerError> {
        let pending = {
            let state = cursor.0.borrow();
            state.pulled > state.read
        };
        if !pending && let Some(item) = self.cursor_pull(cursor, span)? {
            let mut state = cursor.0.borrow_mut();
            state.current = Some(item);
            state.pulled += 1;
        }
        Ok(cursor.0.borrow().pulled)
    }

    /// Materialize the rest of the cursor: pull until the generator returns.
    pub(crate) fn cursor_collect(
        &mut self,
        cursor: &GeneratorCursor,
        span: Span,
    ) -> Result<Vec<Value>, RunnerError> {
        let mut items = Vec::new();
        while let Some(item) = self.cursor_pull(cursor, span)? {
            items.push(item);
        }
        Ok(items)
    }

    /// Next item: the generator's next `cede` (`Some(item)`), or `None` once
    /// it returns. A conversation cursor yields the next content frame. A
    /// runtime failure inside the generator retires it and propagates to the
    /// consumer.
    fn cursor_pull(
        &mut self,
        cursor: &GeneratorCursor,
        span: Span,
    ) -> Result<Option<Value>, RunnerError> {
        let source = cursor
            .0
            .borrow()
            .sermo
            .as_ref()
            .map(|source| (Rc::clone(&source.sermo), source.frame));
        if let Some((sermo, frame)) = source {
            return self.sermo_frame_next(frame, &sermo, span);
        }
        match self.generator_resume(cursor, span)? {
            GeneratorStep::Yield(item) => Ok(Some(item)),
            GeneratorStep::Return => Ok(None),
            GeneratorStep::Raise(error) => {
                cursor.0.borrow_mut().raised = Some(error);
                Ok(None)
            }
        }
    }

    /// Resume the generator until its next `cede` or until it returns or
    /// raises. A runtime failure inside the generator retires it and
    /// propagates to the caller.
    pub(crate) fn generator_resume(
        &mut self,
        cursor: &GeneratorCursor,
        span: Span,
    ) -> Result<GeneratorStep, RunnerError> {
        let Some(frame) = cursor.0.borrow_mut().frame.take() else {
            return Ok(GeneratorStep::Return);
        };
        let depth = self.frames.len();
        self.resume_frame(frame);
        self.run_until_depth(depth);
        if let Some(error) = self.errors.pop() {
            while self.frames.len() > depth {
                self.pop_frame();
            }
            return Err(error);
        }
        if self.frames.len() > depth {
            // WHY: the run loop stopped with the generator parked after a
            // yielding statement — the frame goes back into the cursor.
            let frame = self
                .pop_frame()
                .ok_or_else(|| RunnerError::internal(span, "suspended generator frame missing"))?;
            let item = self
                .yielded
                .take()
                .ok_or_else(|| RunnerError::internal(span, "suspended generator has no yield"))?;
            cursor.0.borrow_mut().frame = Some(frame);
            return Ok(GeneratorStep::Yield(item));
        }
        // The generator's own return value is not an item.
        self.return_value = None;
        Ok(match self.return_error.take() {
            Some(error) => GeneratorStep::Raise(error),
            None => GeneratorStep::Return,
        })
    }
}

#[cfg(test)]
#[path = "cursor_test.rs"]
mod tests;
