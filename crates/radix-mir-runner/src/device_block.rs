//! Queued kernel-error delivery for handled blocks (U9-5, `fac omnia`).
//!
//! The lowering brackets every handled block with [`MirDeviceBlockOp`]
//! markers. The runner keeps the open blocks of each frame as a stack and
//! uses it for three decisions:
//!
//! - a `⇥` kernel call under an omnia block returns `Ok` at once and the
//!   launch is tagged with the block; under a plain block or no block the call
//!   completes the queue and a delivered error is the call's own `Err`;
//! - at a delivery point (block exit, a deliver marker, the `dum` back-edge) the
//!   first queued error is taken and routed to its owner block's handler;
//! - the owner block need not be the innermost: an outer launch's error
//!   unwinds the inner blocks ("innermost catch around the call").
//!
//! One error is delivered per flush; later ones are dropped with the queue.

use crate::Runner;
use crate::kernel_dispatch::DeviceFault;
use crate::{Host, OpenBlock, RunnerError, StatementOutcome, Value};
use radix_lexer::Span;
use radix_mir::{MirDeviceBlockOp, MirOperand, MirRuntimeCall};

const ISSUE_DEVICE_KERNEL_FAULT: &str = "E_DEVICE_KERNEL_FAULT";

/// The run-ending error for a recoverable kernel error nothing can handle.
pub(crate) fn unhandled_fault_error(fault: &DeviceFault, span: Span) -> RunnerError {
    RunnerError::internal_with_issue(
        span,
        format!("kernel error with no handler: {}", fault.text),
        ISSUE_DEVICE_KERNEL_FAULT,
    )
}

impl<H: Host + ?Sized> Runner<'_, H> {
    /// Execute one device-block marker.
    pub(crate) fn execute_device_block(
        &mut self,
        op: MirDeviceBlockOp,
        call: &MirRuntimeCall,
        span: Span,
    ) -> Result<StatementOutcome, RunnerError> {
        match op {
            MirDeviceBlockOp::Enter { omnia, handler } => {
                let Some(MirOperand::Place(error_place)) = call.args.first() else {
                    return Err(RunnerError::internal(
                        span,
                        "device block enter without an error place",
                    ));
                };
                let id = self.next_block_id;
                self.next_block_id += 1;
                let block = OpenBlock {
                    id,
                    omnia,
                    handler,
                    error_place: error_place.clone(),
                };
                self.current_frame_mut(span)?.blocks.push(block);
                Ok(StatementOutcome::Continue)
            }
            MirDeviceBlockOp::Exit { error_edge } => self.exit_device_block(error_edge, span),
            MirDeviceBlockOp::Deliver { sync } => {
                if self.current_frame_mut(span)?.blocks.is_empty() {
                    return Ok(StatementOutcome::Continue);
                }
                if sync {
                    self.flush_dispatcher()?;
                }
                self.deliver_and_route(None, span)
            }
        }
    }

    fn exit_device_block(
        &mut self,
        error_edge: bool,
        span: Span,
    ) -> Result<StatementOutcome, RunnerError> {
        let Some(top) = self.current_frame_mut(span)?.blocks.last() else {
            return Ok(StatementOutcome::Continue);
        };
        let (id, omnia) = (top.id, top.omnia);
        if error_edge {
            // The handler is running: the block is closed. Complete its
            // launches and discard what they report; an error owned by an
            // outer block still unwinds to that block.
            self.current_frame_mut(span)?.blocks.pop();
            if omnia {
                self.flush_dispatcher()?;
                return self.deliver_and_route(Some(id), span);
            }
            return self.route_carried(Some(id), span);
        }
        if omnia {
            // Block end, `rumpe`, `perge` or `redde`: deliver while the block
            // is still open so its own error reaches its own handler.
            self.flush_dispatcher()?;
            let outcome = self.deliver_and_route(None, span)?;
            if matches!(outcome, StatementOutcome::Continue) {
                self.current_frame_mut(span)?.blocks.pop();
            }
            return Ok(outcome);
        }
        self.current_frame_mut(span)?.blocks.pop();
        self.route_carried(None, span)
    }

    /// Route only an error already carried from an earlier delivery.
    fn route_carried(
        &mut self,
        discard_from: Option<u64>,
        span: Span,
    ) -> Result<StatementOutcome, RunnerError> {
        match self.carried_fault.take() {
            Some(fault) => self.route_fault(fault, discard_from, span),
            None => Ok(StatementOutcome::Continue),
        }
    }

    /// Take the first delivered error (a carried one is older) and route it.
    fn deliver_and_route(
        &mut self,
        discard_from: Option<u64>,
        span: Span,
    ) -> Result<StatementOutcome, RunnerError> {
        let fault = match self.carried_fault.take() {
            Some(fault) => Some(fault),
            None => self.take_dispatcher_fault(span)?,
        };
        match fault {
            Some(fault) => self.route_fault(fault, discard_from, span),
            None => Ok(StatementOutcome::Continue),
        }
    }

    /// Hand a delivered error to its owner block's handler.
    ///
    /// `discard_from`: an error whose owner id is at least this one belongs to
    /// a block that just closed through its error edge, and is dropped.
    fn route_fault(
        &mut self,
        fault: DeviceFault,
        discard_from: Option<u64>,
        span: Span,
    ) -> Result<StatementOutcome, RunnerError> {
        let Some(owner) = fault.owner else {
            return Err(unhandled_fault_error(&fault, span));
        };
        if discard_from.is_some_and(|from| owner >= from) {
            return Ok(StatementOutcome::Continue);
        }
        let frame = self.current_frame_mut(span)?;
        if let Some(index) = frame.blocks.iter().position(|block| block.id == owner) {
            frame.blocks.truncate(index + 1);
            let (handler, error_place) = {
                let block = &frame.blocks[index];
                (block.handler, block.error_place.clone())
            };
            self.write_place(&error_place, Value::Str(fault.text), span)?;
            self.set_active_block(handler, span);
            return Ok(StatementOutcome::FrameReturned);
        }
        let in_caller = self
            .frames
            .iter()
            .any(|frame| frame.blocks.iter().any(|block| block.id == owner));
        if in_caller {
            self.carried_fault = Some(fault);
        }
        // Otherwise the owner already closed: its launches were discarded.
        Ok(StatementOutcome::Continue)
    }

    /// The innermost open block of the current frame, as a launch owner tag.
    pub(crate) fn launch_owner(&self) -> Option<u64> {
        self.frames.last()?.blocks.last().map(|block| block.id)
    }

    /// Whether the current frame's innermost block queues kernel errors.
    pub(crate) fn top_block_is_omnia(&self) -> bool {
        self.frames
            .last()
            .and_then(|frame| frame.blocks.last())
            .is_some_and(|block| block.omnia)
    }

    pub(crate) fn take_dispatcher_fault(
        &mut self,
        span: Span,
    ) -> Result<Option<DeviceFault>, RunnerError> {
        let Some(dispatcher) = self.kernel_dispatcher.as_ref() else {
            return Ok(None);
        };
        dispatcher
            .try_borrow_mut()
            .map_err(|_| {
                RunnerError::internal(span, "kernel dispatcher is busy during a fault delivery")
            })?
            .deliver_faults()
    }

    fn current_frame_mut(&mut self, span: Span) -> Result<&mut crate::Frame, RunnerError> {
        self.frames
            .last_mut()
            .ok_or_else(|| RunnerError::internal(span, "no active frame"))
    }
}
