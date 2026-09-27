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
//! Each conversation keeps the §2.7 record A7 closes over: id and route,
//! answering tier, the handler task, inbound and outbound terminals, and one
//! release point that drops the task once the inbound direction terminates.

use super::{CallOutcome, Host, MirBlockId, MirFunctionId, Runner, RunnerError, Value};
use crate::conversio::MissingFieldPolicy;
use crate::cursor::{GeneratorCursor, GeneratorStep};
use radix_hir::DefId;
use radix_lexer::{Span, Symbol};
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

/// Builtin route answered in-process (tier 2).
const ECHO_ROUTE: &str = "runtime:echo";

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
}

impl Conversation {
    /// Record the inbound terminal and run the single release point: the
    /// handler task handle is dropped (A7 hangs completion-close here).
    fn finish_inbound(&mut self, terminal: SermoTerminal) {
        self.incoming_terminal = Some(terminal);
        self.queue.clear();
        self.task = None;
    }
}

/// A `scrinium` frame value carrying `data`.
pub(crate) fn frame_value(frame: DefId, data_field: Symbol, data: Value) -> Value {
    let mut fields = FxHashMap::default();
    fields.insert(data_field, data);
    Value::Struct(frame, Rc::new(RefCell::new(fields)))
}

impl<H: Host + ?Sized> Runner<'_, H> {
    /// `SermoOpen`: resolve the static handler and start a zero-opener handler.
    pub(crate) fn sermo_open(&mut self, route: String, span: Span) -> Result<Value, RunnerError> {
        let handler = match self.context.ad_routes.get(&route) {
            Some(def) => Some(self.resolve_definition(*def, span)?),
            None => None,
        };
        self.next_conversation_id += 1;
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

        if is_generator {
            let mut frame = self.new_frame(handler, MirBlockId(0));
            frame.is_generator = true;
            frame.locals.extend(locals);
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
        frame.locals.extend(locals);
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
            (Some(SermoTerminal::Error(_) | SermoTerminal::Cancel), _) => {
                Err(RunnerError::internal(span, "sermo reply conversion failed"))
            }
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
}
