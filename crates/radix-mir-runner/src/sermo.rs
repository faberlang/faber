//! Conversation router for the MIR runner (A4/U9, D6.9).
//!
//! `ad '<route>'` opens a conversation that is answered in a fixed order:
//!
//! 1. **Static Faber table** — the program's `@ ad` handlers
//!    (`MirValidationContext::ad_routes`). The handler runs as a cooperative
//!    task: a plain handler runs to completion when the conversation opens
//!    and queues one item (none for `vacuum`) then `done`; a cursor handler
//!    becomes a suspended generator that is resumed only when the consumer
//!    needs the next frame, so it produces while the caller consumes.
//! 2. **Builtins** — `runtime:echo` answers the opener as one item.
//! 3. **Host** — [`Host::sermo_route`]; hosts that do not answer frame routes
//!    return `None`.
//! 4. **Fail closed** — one `error` terminal; materializing it reports the
//!    route as unsupported.
//!
//! The opener and every item cross the boundary as deep copies (D6.14). A
//! failed handler (`⇥`) sends an `error` terminal carrying its error value.
//! `tuus.fini()` on an undrained stream cancels a live cursor handler (the
//! Go/Rust choice); a finished handler's remaining frames are drained to their
//! terminal instead.
//!
//! Each conversation keeps the §2.7 record: id and route, answering tier, the
//! handler task, inbound and outbound terminals, and one release point.
//!
//! # Lifecycle (D8.3)
//!
//! A conversation takes a router slot when it opens and gives it back at its
//! single release point, which runs the moment the conversation completes:
//! the handler returns or fails (its terminal is known), the consumer reads
//! or cancels to a terminal, or `close()` ends it. Release drops the handler
//! task and the opener; the handle lives on as a finished record whose unread
//! frames still read as its result. `close()` closes early and reports an
//! error terminal. The last reference is the safety net: dropping the final
//! `Rc` of an unreleased conversation releases it exactly then (the runner is
//! reference counted), and any error it carried is lost.

use super::{CallOutcome, Host, MirBlockId, MirFunctionId, Runner, RunnerError, Value, write_slot};
use crate::conversio::MissingFieldPolicy;
use crate::cursor::{GeneratorCursor, GeneratorStep};
use radix_hir::DefId;
use radix_lexer::Span;
use radix_types::Type;
use rustc_hash::FxHashMap;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

/// Builtin route answered in-process (tier 2).
const ECHO_ROUTE: &str = "runtime:echo";

/// Router slots of one runner (D8.3): a conversation takes one at open and
/// gives it back at its release point.
#[derive(Debug, Default, PartialEq)]
pub struct ConversationSlots {
    opened: Cell<u64>,
    live: Cell<usize>,
    peak: Cell<usize>,
}

impl ConversationSlots {
    fn take(&self) {
        self.opened.set(self.opened.get() + 1);
        self.live.set(self.live.get() + 1);
        self.peak.set(self.peak.get().max(self.live.get()));
    }

    fn give_back(&self) {
        self.live.set(self.live.get().saturating_sub(1));
    }

    /// The counts observed so far.
    #[must_use]
    pub fn ledger(&self) -> ConversationLedger {
        ConversationLedger {
            opened: self.opened.get(),
            live_peak: self.peak.get(),
            live: self.live.get(),
        }
    }
}

/// Conversation counts of one run: how many opened, the most that held a
/// router slot at once, and how many still held one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ConversationLedger {
    pub opened: u64,
    pub live_peak: usize,
    pub live: usize,
}

/// Which side of a conversation a handle views.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SermoView {
    /// The `sermo` itself (materialization).
    Whole,
    /// `s.meus<T>()`: the outbound direction.
    Meus,
    /// `s.tuus<T>()`: the inbound direction.
    Tuus,
}

/// The router tier that answered a conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SermoTier {
    Static,
    Builtin,
    Host,
    /// No tier answered; the conversation carries a fail-closed terminal.
    Unrouted,
}

/// A conversation direction's terminal frame.
#[derive(Debug, Clone, PartialEq)]
pub enum SermoTerminal {
    Done,
    Error(Value),
    Cancel,
}

/// One view handle onto a shared conversation (`Value::Sermo`).
#[derive(Debug, Clone, PartialEq)]
pub struct SermoValue {
    pub view: SermoView,
    pub conversation: Rc<RefCell<Conversation>>,
}

impl SermoValue {
    /// A handle viewing the same conversation from `view`.
    #[must_use]
    pub fn with_view(&self, view: SermoView) -> Self {
        Self {
            view,
            conversation: Rc::clone(&self.conversation),
        }
    }
}

/// Conversation record (§2.7).
#[derive(Debug, PartialEq)]
pub struct Conversation {
    pub id: u64,
    pub route: String,
    pub opener: Value,
    /// Tier-1 handler resolved at open, when the static table serves the route.
    handler: Option<MirFunctionId>,
    /// Answering tier; `None` until the conversation starts.
    pub tier: Option<SermoTier>,
    /// Content items produced and not yet received.
    queue: VecDeque<Value>,
    /// Terminal the producer sent, delivered after `queue` drains.
    producer_terminal: Option<SermoTerminal>,
    /// Live cursor-handler task; dropped at the release point.
    task: Option<GeneratorCursor>,
    /// Terminal the consumer observed on the inbound direction.
    pub incoming_terminal: Option<SermoTerminal>,
    /// Whether the outbound direction was closed with `meus.fini()`.
    pub meus_closed: bool,
    /// Router slots of the owning runner; this conversation holds one until
    /// it is released.
    slots: Rc<ConversationSlots>,
    /// Whether the release point has run.
    pub released: bool,
    /// Whether `close()` closed this handle.
    pub closed: bool,
    /// Error the last `close()` reported, until lowering takes it.
    close_error: Option<String>,
}

impl Conversation {
    /// Record the inbound terminal; the conversation is complete.
    fn finish_inbound(&mut self, terminal: SermoTerminal) {
        self.incoming_terminal = Some(terminal);
        self.queue.clear();
        self.release();
    }

    /// The single release point (D8.3): drop the handler task and the
    /// opener, and give the router slot back. Idempotent.
    fn release(&mut self) {
        if self.released {
            return;
        }
        self.released = true;
        self.task = None;
        self.opener = Value::Nil;
        self.slots.give_back();
    }

    /// Release once the producer is finished: its terminal is known and no
    /// live task remains to produce more.
    fn release_if_complete(&mut self) {
        if self.producer_terminal.is_some() && self.task.is_none() {
            self.release();
        }
    }
}

impl Drop for Conversation {
    /// Last-reference safety net: an abandoned conversation is released when
    /// its final handle goes away. Any error it carried is lost.
    fn drop(&mut self) {
        self.release();
    }
}

/// Text of an `error` terminal's data, as `close()` reports it (`⇥ textus`).
fn error_text(data: &Value) -> String {
    match data {
        Value::Str(text) => text.clone(),
        Value::Int(value) => value.to_string(),
        Value::Float(value) => value.to_string(),
        Value::Bool(value) => value.to_string(),
        other => format!("{other:?}"),
    }
}

impl<H: Host + ?Sized> Runner<'_, H> {
    /// `tuus.accipe()`: the next inbound content frame, or `none` at the
    /// terminal.
    pub(crate) fn sermo_receive(
        &mut self,
        frame: DefId,
        sermo: &Rc<RefCell<SermoValue>>,
        span: Span,
    ) -> Result<Value, RunnerError> {
        Ok(self
            .sermo_frame_next(frame, sermo, span)?
            .unwrap_or(Value::Nil))
    }

    /// The next inbound content item, wrapped as a `scrinium` frame.
    pub(crate) fn sermo_frame_next(
        &mut self,
        frame: DefId,
        sermo: &Rc<RefCell<SermoValue>>,
        span: Span,
    ) -> Result<Option<Value>, RunnerError> {
        let Some(data) = self.sermo_next(sermo, span)? else {
            return Ok(None);
        };
        self.sermo_frame(frame, sermo, data, span).map(Some)
    }

    /// A received content frame: every declared `scrinium` field is present.
    ///
    /// WHY: the runner models a conversation's id and route and nothing per
    /// frame. `call` is the route, `parent_id` the conversation id, `status`
    /// is `item`. The runner has no frame id, clock, source label or trace, so
    /// `id`, `created_ms`, `from` and `trace` are `none` (their `sponte`
    /// absence), not invented values.
    pub(crate) fn sermo_frame(
        &self,
        frame: DefId,
        sermo: &Rc<RefCell<SermoValue>>,
        data: Value,
        span: Span,
    ) -> Result<Value, RunnerError> {
        const STATUS_ITEM: usize = 1;
        let declared = self
            .context
            .struct_fields
            .get(&frame)
            .ok_or_else(|| RunnerError::internal(span, "scrinium fields missing"))?;
        let conversation = Rc::clone(&sermo.borrow().conversation);
        let conversation = conversation.borrow();
        let mut data = Some(data);
        let mut fields = FxHashMap::default();
        for (&symbol, ty) in declared {
            let value = match self.interner.resolve(symbol) {
                "id" | "created_ms" | "from" | "trace" => Value::Nil,
                "parent_id" => Value::Str(conversation.id.to_string()),
                "call" => Value::Str(conversation.route.clone()),
                "data" => data.take().unwrap_or(Value::Nil),
                "status" => {
                    let Type::Enum(status) = self.types.get(ty.semantic_id()) else {
                        return Err(RunnerError::internal(
                            span,
                            "scrinium status is not an enum",
                        ));
                    };
                    let variant = self.status_variant_at(*status, STATUS_ITEM, span)?;
                    Value::EnumVariant(variant, Rc::new(RefCell::new(FxHashMap::default())))
                }
                _ => return Err(RunnerError::internal(span, "unknown scrinium field")),
            };
            fields.insert(symbol, value);
        }
        Ok(Value::Struct(frame, Rc::new(RefCell::new(fields))))
    }

    /// `SermoOpen`: resolve the static handler and start a zero-opener handler.
    pub(crate) fn sermo_open(&mut self, route: String, span: Span) -> Result<Value, RunnerError> {
        let handler = match self.context.ad_routes.get(&route) {
            Some(def) => Some(self.resolve_definition(*def, span)?),
            None => None,
        };
        self.next_conversation_id += 1;
        self.conversation_slots.take();
        let sermo = Rc::new(RefCell::new(SermoValue {
            view: SermoView::Whole,
            conversation: Rc::new(RefCell::new(Conversation {
                id: self.next_conversation_id,
                route,
                opener: Value::Nil,
                handler,
                tier: None,
                queue: VecDeque::new(),
                producer_terminal: None,
                task: None,
                incoming_terminal: None,
                meus_closed: false,
                slots: Rc::clone(&self.conversation_slots),
                released: false,
                closed: false,
                close_error: None,
            })),
        }));
        // WHY: a tier-1 handler is a task that starts when the conversation
        // opens (Q2). A handler without an opener has everything it needs now;
        // one with an opener starts at `SermoSetOpener`.
        if let Some(handler) = handler
            && self.function_by_id(handler, span)?.params.is_empty()
        {
            self.sermo_start(&sermo, span)?;
        }
        Ok(Value::Sermo(sermo))
    }

    /// `SermoSetOpener`: record the opener, then start a tier-1 handler.
    pub(crate) fn sermo_set_opener(
        &mut self,
        sermo: &Rc<RefCell<SermoValue>>,
        opener: Value,
        span: Span,
    ) -> Result<(), RunnerError> {
        let conversation = Rc::clone(&sermo.borrow().conversation);
        let handler = {
            let mut conversation = conversation.borrow_mut();
            conversation.opener = opener;
            conversation.handler
        };
        if handler.is_some() {
            self.sermo_start(sermo, span)?;
        }
        Ok(())
    }

    /// Answer the conversation once: static table, builtins, host, fail closed.
    fn sermo_start(
        &mut self,
        sermo: &Rc<RefCell<SermoValue>>,
        span: Span,
    ) -> Result<(), RunnerError> {
        let conversation = Rc::clone(&sermo.borrow().conversation);
        let (route, opener, handler) = {
            let conversation = conversation.borrow();
            if conversation.tier.is_some() {
                return Ok(());
            }
            (
                conversation.route.clone(),
                conversation.opener.deep_copy(),
                conversation.handler,
            )
        };
        // The `ad` effect boundary is a delivery point for deferred device work.
        self.flush_dispatcher()?;
        let (tier, items, terminal, task) = if let Some(handler) = handler {
            let (items, terminal, task) = self.start_static_handler(handler, opener, span)?;
            (SermoTier::Static, items, terminal, task)
        } else if route == ECHO_ROUTE {
            (
                SermoTier::Builtin,
                vec![opener],
                Some(SermoTerminal::Done),
                None,
            )
        } else if let Some(reply) = self.host.sermo_route(&route, &opener, span) {
            match reply {
                Ok(items) => (SermoTier::Host, items, Some(SermoTerminal::Done), None),
                Err(data) => (
                    SermoTier::Host,
                    Vec::new(),
                    Some(SermoTerminal::Error(data)),
                    None,
                ),
            }
        } else {
            (
                SermoTier::Unrouted,
                Vec::new(),
                Some(SermoTerminal::Error(Value::Str(format!(
                    "unsupported ad route {route}"
                )))),
                None,
            )
        };
        let mut conversation = conversation.borrow_mut();
        conversation.tier = Some(tier);
        conversation.queue.extend(items);
        conversation.producer_terminal = terminal;
        conversation.task = task;
        conversation.release_if_complete();
        Ok(())
    }

    /// Run a tier-1 handler: a plain handler to completion, a cursor handler
    /// as a suspended generator. Returns queued items, the producer terminal
    /// (when already known) and the live task.
    #[allow(clippy::type_complexity)]
    fn start_static_handler(
        &mut self,
        handler: MirFunctionId,
        opener: Value,
        span: Span,
    ) -> Result<(Vec<Value>, Option<SermoTerminal>, Option<GeneratorCursor>), RunnerError> {
        self.refuse_kernel_execution(handler, span)?;
        let dispatched = self.dispatches_kernel(handler, span)?;
        let function = self.function_by_id(handler, span)?;
        let is_generator = function.is_generator;
        let params = function
            .params
            .iter()
            .map(|param| (param.local, param.ty.semantic_id()))
            .collect::<Vec<_>>();
        let mut locals = Vec::with_capacity(params.len());
        if let [(local, ty)] = params.as_slice() {
            match self.extract_dynamic_value_for_type(
                opener,
                *ty,
                &[],
                span,
                MissingFieldPolicy::Valor,
            ) {
                Ok(argument) => locals.push((*local, argument)),
                Err(error) => {
                    return Ok((
                        Vec::new(),
                        Some(SermoTerminal::Error(Value::Str(error.message))),
                        None,
                    ));
                }
            }
        } else if !params.is_empty() {
            return Err(RunnerError::internal(
                span,
                "ad handler takes at most one opener",
            ));
        }

        if dispatched {
            let mut args = locals
                .into_iter()
                .map(|(_, value)| value)
                .collect::<Vec<_>>();
            return match self.dispatch_kernel_call(handler, &mut args, span)? {
                Value::Unit => Ok((Vec::new(), Some(SermoTerminal::Done), None)),
                reply => Ok((vec![reply.deep_copy()], Some(SermoTerminal::Done), None)),
            };
        }
        if is_generator {
            let mut frame = self.new_frame(handler, MirBlockId(0));
            frame.is_generator = true;
            for (local, value) in locals {
                write_slot(&mut frame.locals, local.0 as usize, value, span, "local")?;
            }
            return Ok((Vec::new(), None, Some(GeneratorCursor::new(frame))));
        }
        match self.call_with_locals(handler, locals)? {
            CallOutcome::Ok(Value::Unit) => Ok((Vec::new(), Some(SermoTerminal::Done), None)),
            CallOutcome::Ok(reply) => {
                Ok((vec![reply.deep_copy()], Some(SermoTerminal::Done), None))
            }
            CallOutcome::Err(error) => Ok((
                Vec::new(),
                Some(SermoTerminal::Error(error.deep_copy())),
                None,
            )),
        }
    }

    /// Call `function` with its parameter locals already bound.
    fn call_with_locals(
        &mut self,
        function: MirFunctionId,
        locals: Vec<(radix_mir::MirLocalId, Value)>,
    ) -> Result<CallOutcome, RunnerError> {
        let depth = self.frames.len();
        self.return_value = None;
        self.return_error = None;
        let mut frame = self.new_frame(function, MirBlockId(0));
        for (local, value) in locals {
            write_slot(
                &mut frame.locals,
                local.0 as usize,
                value,
                Span::default(),
                "local",
            )?;
        }
        self.resume_frame(frame);
        self.run_until_depth(depth);
        let error = self.errors.pop();
        while self.frames.len() > depth {
            self.pop_frame();
        }
        if let Some(error) = error {
            return Err(error);
        }
        if let Some(error) = self.return_error.take() {
            return Ok(CallOutcome::Err(error));
        }
        Ok(CallOutcome::Ok(
            self.return_value.take().unwrap_or(Value::Unit),
        ))
    }

    /// Next inbound content item, or `None` once the inbound terminal is
    /// reached. Resumes a live cursor handler only when the queue is empty.
    pub(crate) fn sermo_next(
        &mut self,
        sermo: &Rc<RefCell<SermoValue>>,
        span: Span,
    ) -> Result<Option<Value>, RunnerError> {
        self.sermo_start(sermo, span)?;
        let conversation = Rc::clone(&sermo.borrow().conversation);
        loop {
            let task = {
                let mut state = conversation.borrow_mut();
                if state.incoming_terminal.is_some() {
                    return Ok(None);
                }
                if let Some(item) = state.queue.pop_front() {
                    return Ok(Some(item));
                }
                if let Some(terminal) = state.producer_terminal.take() {
                    state.finish_inbound(terminal);
                    return Ok(None);
                }
                state.task.clone()
            };
            let Some(task) = task else {
                conversation
                    .borrow_mut()
                    .finish_inbound(SermoTerminal::Done);
                return Ok(None);
            };
            let terminal = match self.generator_resume(&task, span)? {
                GeneratorStep::Yield(item) => return Ok(Some(item.deep_copy())),
                GeneratorStep::Return => SermoTerminal::Done,
                GeneratorStep::Raise(error) => SermoTerminal::Error(error.deep_copy()),
            };
            let mut state = conversation.borrow_mut();
            state.task = None;
            state.producer_terminal = Some(terminal);
            state.release_if_complete();
        }
    }

    /// Materialize the whole inbound stream (`↦ T`): every content item until
    /// the terminal. An `error` or `cancel` terminal fails the conversion.
    pub(crate) fn sermo_items(
        &mut self,
        sermo: &Rc<RefCell<SermoValue>>,
        span: Span,
    ) -> Result<Vec<Value>, RunnerError> {
        let mut items = Vec::new();
        while let Some(item) = self.sermo_next(sermo, span)? {
            items.push(item);
        }
        let conversation = Rc::clone(&sermo.borrow().conversation);
        let conversation = conversation.borrow();
        match (&conversation.incoming_terminal, conversation.tier) {
            (Some(SermoTerminal::Done) | None, _) => Ok(items),
            (Some(SermoTerminal::Error(_)), Some(SermoTier::Unrouted)) => {
                Err(RunnerError::unsupported(
                    span,
                    &format!(
                        "sermo route {} before runner provider runtime",
                        conversation.route
                    ),
                ))
            }
            // WHY: the error terminal already carries the producer's own text
            // (a leaf error's route, path and OS message), so that text is the
            // message. The issue identity stays the stable conversion issue:
            // it must not vary with the path or the OS wording.
            (Some(SermoTerminal::Error(data)), _) => {
                let mut error = RunnerError::conversion(span, "sermo reply conversion failed");
                error.message = error_text(data);
                Err(error)
            }
            (Some(SermoTerminal::Cancel), _) => Err(RunnerError::conversion(
                span,
                "sermo reply conversion failed",
            )),
        }
    }

    /// Single-value materialization (`↦ valor` and scalars): the first
    /// content item, or `nihil` when the stream carried none.
    pub(crate) fn sermo_first_item(
        &mut self,
        sermo: &Rc<RefCell<SermoValue>>,
        span: Span,
    ) -> Result<Value, RunnerError> {
        Ok(self
            .sermo_items(sermo, span)?
            .into_iter()
            .next()
            .unwrap_or(Value::Nil))
    }

    /// `fini` on a view. Outbound: close the direction. Inbound: a live
    /// cursor handler is cancelled; otherwise the remaining frames drain to
    /// the producer's terminal.
    pub(crate) fn sermo_finish(
        &mut self,
        sermo: &Rc<RefCell<SermoValue>>,
        span: Span,
    ) -> Result<SermoTerminal, RunnerError> {
        let (view, conversation) = {
            let handle = sermo.borrow();
            (handle.view, Rc::clone(&handle.conversation))
        };
        if view == SermoView::Meus {
            conversation.borrow_mut().meus_closed = true;
            return Ok(SermoTerminal::Done);
        }
        self.sermo_start(sermo, span)?;
        let mut state = conversation.borrow_mut();
        if let Some(terminal) = &state.incoming_terminal {
            return Ok(terminal.clone());
        }
        let terminal = if state.task.as_ref().is_some_and(GeneratorCursor::is_live) {
            SermoTerminal::Cancel
        } else {
            state
                .producer_terminal
                .take()
                .unwrap_or(SermoTerminal::Done)
        };
        state.finish_inbound(terminal.clone());
        Ok(terminal)
    }

    /// `s.close()` (D8.3): close the handle early. A live handler is
    /// cancelled, unread frames are dropped, and the conversation is
    /// released. Returns whether this call closed the handle; an `error`
    /// terminal is kept for [`Self::sermo_take_close_error`].
    #[allow(clippy::unused_self)] // a Runner method like its sermo_* siblings
    pub(crate) fn sermo_close(&mut self, sermo: &Rc<RefCell<SermoValue>>) -> bool {
        let conversation = Rc::clone(&sermo.borrow().conversation);
        let mut state = conversation.borrow_mut();
        state.close_error = None;
        if state.closed {
            return false;
        }
        state.closed = true;
        if state.incoming_terminal.is_none() {
            let producer_live = state.task.as_ref().is_some_and(GeneratorCursor::is_live);
            let terminal = if state.tier.is_none() || producer_live {
                SermoTerminal::Cancel
            } else {
                state
                    .producer_terminal
                    .take()
                    .unwrap_or(SermoTerminal::Done)
            };
            state.finish_inbound(terminal);
        }
        if let Some(SermoTerminal::Error(data)) = &state.incoming_terminal {
            state.close_error = Some(error_text(data));
        }
        state.release();
        true
    }

    /// Whether the last `close()` reported an error.
    pub(crate) fn sermo_close_failed(sermo: &Rc<RefCell<SermoValue>>) -> bool {
        sermo.borrow().conversation.borrow().close_error.is_some()
    }

    /// Take the last `close()` error message.
    pub(crate) fn sermo_take_close_error(sermo: &Rc<RefCell<SermoValue>>) -> String {
        sermo
            .borrow()
            .conversation
            .borrow_mut()
            .close_error
            .take()
            .unwrap_or_default()
    }
}
