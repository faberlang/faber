//! In-process frame conversation types for expression `ad` and directional views.
//!
//! # Lifecycle (D8.3)
//!
//! A conversation completes when its producer sends a terminal (the handler
//! returned, failed, or observed a cancel) or the consumer ends it. At that
//! moment the single release point drops the handler task handle, the
//! cancellation lease and any host dispatch, and gives the router slot back;
//! the handle lives on as a finished record whose unread frames still read as
//! its result. [`sermo_close`] closes early and reports an `error` terminal.
//! The last caller-side handle ([`Sermo`], [`Meus`], [`Tuus`], a cursor) is
//! the safety net: `Drop` cancels and releases an abandoned conversation, and
//! any error it carried is lost.

use crate::{Instans, InstansPraecisio, Valor};
use std::collections::VecDeque;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError};
use std::task::{Context, Poll, Waker};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

// ── Contract-authority re-export ────────────────────────────────────────────
// Single canonical definition lives at
// radix-runtime-contract/src/frame.rs (the compiler-side authority); the
// standalone package carries a committed copy under `crate::contract`.
pub use crate::contract::frame::FrameStatus;

/// Opaque frame record carried on a `Sermo` handle.
#[derive(Clone, Debug, PartialEq)]
pub struct Scrinium {
    pub id: String,
    pub parent_id: Option<String>,
    pub call: String,
    pub status: FrameStatus,
    pub data: Valor,
    pub created_ms: i64,
    pub from: Option<String>,
    pub trace: Option<Valor>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimeResponseState {
    Pending,
    Generated,
}

#[derive(Debug)]
struct SermoInner {
    conversation_id: String,
    route: String,
    outgoing: Vec<Scrinium>,
    incoming: VecDeque<Scrinium>,
    runtime_response_state: RuntimeResponseState,
    incoming_drained: bool,
    /// Terminal `status` observed on the inbound direction (`done`, `error`, or `cancel`).
    incoming_terminal: Option<FrameStatus>,
    incoming_wake_epoch: u64,
    incoming_waiters: Vec<Waker>,
    runtime_cancellation: Option<Cancellation>,
    host_dispatch: Option<DispatchOverride>,
    /// Per-conversation tier-1 table (embedder/test seam); `None` reads the
    /// process-global table installed by [`install_static_routes`].
    static_routes: Option<&'static [StaticRoute]>,
    /// Conversation record (§2.7): the tier that answered, once dispatch ran.
    answering_tier: Option<AnsweringTier>,
    /// Conversation record (§2.7): the tier-1 handler task, held until the
    /// release point.
    handler_task: Option<thread::JoinHandle<()>>,
    detached: bool,
    meus_closed: bool,
    /// The producer's terminal once it sent one (D8.3 completion), with the
    /// text of an `error` terminal, which `close()` reports.
    producer_terminal: Option<(FrameStatus, Option<String>)>,
    lifecycle: Lifecycle,
}

/// Where a conversation is in its D8.3 lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lifecycle {
    /// Holding its router slot.
    Open,
    /// Released: the task handle, lease and slot are gone.
    Released,
    /// Released by `close()`; a second `close()` reports `false`.
    Closed,
}

/// The router tier that answered one conversation (conversation record).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnsweringTier {
    /// Tier 1: a Faber `@ ad` handler from the static table.
    Static,
    /// Tier 2: an in-process runtime builtin (`runtime:echo`, `processus:exi`).
    Builtin,
    /// Tier 3: the installed (or per-conversation) host dispatch.
    Host,
    /// No tier answered; the conversation failed closed.
    None,
}

/// One tier-1 route: a Faber `@ ad` handler registered by generated code.
///
/// `start` runs synchronously on the conversation's own handler task (a
/// thread the router spawns), converts the opener, runs the handler, and
/// answers through `responses`. `takes_opener` is false for a zero-parameter
/// handler, which then starts when the conversation opens instead of when its
/// opener is set.
#[derive(Debug, Clone, Copy)]
pub struct StaticRoute {
    pub route: &'static str,
    pub takes_opener: bool,
    pub start: fn(SermoRequest, ResponseSender, Cancellation),
}

#[derive(Clone)]
struct DispatchOverride(Arc<dyn HostDispatch>);

impl std::fmt::Debug for DispatchOverride {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("DispatchOverride").finish()
    }
}

#[derive(Debug)]
struct SermoShared {
    state: Mutex<SermoInner>,
    incoming_changed: Condvar,
}

impl SermoShared {
    fn new(state: SermoInner) -> Self {
        Self {
            state: Mutex::new(state),
            incoming_changed: Condvar::new(),
        }
    }
}

/// In-flight `ad` conversation handle.
#[derive(Clone, Debug)]
pub struct Sermo {
    inner: Arc<SermoShared>,
    caller: Arc<CallerHandle>,
}

/// The caller's side of a conversation, shared by every caller handle
/// ([`Sermo`] clones, views, cursors). The producer holds the conversation
/// through its `ResponseSender`, not through this, so dropping the last
/// caller handle is the conversation's last reference (D8.3 safety net).
#[derive(Debug)]
struct CallerHandle {
    shared: Arc<SermoShared>,
}

impl Drop for CallerHandle {
    fn drop(&mut self) {
        let mut inner = lock_sermo(&self.shared);
        if !inner.incoming_drained {
            if inner.producer_terminal.is_some() {
                inner.incoming_terminal = Some(FrameStatus::Done);
                inner.incoming_drained = true;
            } else {
                record_incoming_terminal(&mut inner, FrameStatus::Cancel);
            }
        }
        inner.incoming.clear();
        release_conversation(&mut inner);
    }
}

/// Caller-to-gateway live outbound half-stream view.
pub struct Meus<T> {
    inner: Arc<SermoShared>,
    _caller: Arc<CallerHandle>,
    _marker: PhantomData<T>,
}

/// Gateway-to-caller live inbound half-stream view.
pub struct Tuus<T> {
    inner: Arc<SermoShared>,
    caller: Arc<CallerHandle>,
    _marker: PhantomData<T>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameError {
    pub issue: &'static str,
    pub message: String,
}

impl FrameError {
    fn new(issue: &'static str, message: impl Into<String>) -> Self {
        Self {
            issue,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for FrameError {}

#[derive(Clone, Debug)]
pub struct SermoRequest {
    pub conversation_id: String,
    pub route: String,
    pub opener: Valor,
    pub target: Option<&'static str>,
}

#[derive(Clone, Debug)]
pub struct Cancellation {
    cancelled: Arc<AtomicBool>,
}

impl Cancellation {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

#[derive(Debug)]
pub struct ResponseSender {
    lease: Arc<ResponseLease>,
}

#[derive(Debug)]
struct ResponseLease {
    shared: Arc<SermoShared>,
    live_senders: AtomicUsize,
    terminal_sent: Mutex<bool>,
    cancellation: Cancellation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DispatchError {
    pub issue: &'static str,
    pub message: String,
}

impl DispatchError {
    pub fn new(issue: &'static str, message: impl Into<String>) -> Self {
        Self {
            issue,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for DispatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for DispatchError {}

pub trait HostDispatch: Send + Sync {
    /// Start handling a conversation request.
    ///
    /// # Errors
    ///
    /// Returns `Err(DispatchError)` if the handler cannot be started (e.g.,
    /// the host dispatch is already installed).
    fn start(
        &self,
        request: SermoRequest,
        responses: ResponseSender,
        cancellation: Cancellation,
    ) -> Result<(), DispatchError>;
}

static HOST_DISPATCH: OnceLock<Arc<dyn HostDispatch>> = OnceLock::new();

static STATIC_ROUTES: OnceLock<&'static [StaticRoute]> = OnceLock::new();

/// Install the program's tier-1 route table (generated entry prologue).
///
/// Independent of [`install_host_dispatch`]; the two may be installed in
/// either order.
///
/// # Errors
///
/// Returns `Err` if a static route table is already installed.
pub fn install_static_routes(routes: &'static [StaticRoute]) -> Result<(), DispatchError> {
    STATIC_ROUTES.set(routes).map_err(|_| {
        DispatchError::new(
            "frame_static_routes_already_installed",
            "static route table is already installed",
        )
    })
}

/// Install a global host dispatch handler.
///
/// # Errors
///
/// Returns `Err` if a host dispatch is already installed.
pub fn install_host_dispatch(dispatch: Arc<dyn HostDispatch>) -> Result<(), DispatchError> {
    HOST_DISPATCH.set(dispatch).map_err(|_| {
        DispatchError::new(
            "frame_host_dispatch_already_installed",
            "host dispatch is already installed",
        )
    })
}

impl ResponseSender {
    fn new(shared: Arc<SermoShared>, cancellation: Cancellation) -> Self {
        Self {
            lease: Arc::new(ResponseLease {
                shared,
                live_senders: AtomicUsize::new(1),
                terminal_sent: Mutex::new(false),
                cancellation,
            }),
        }
    }

    #[must_use]
    /// # Errors
    /// Returns an error when the requested operation cannot be completed.
    pub fn is_cancelled(&self) -> bool {
        self.lease.cancellation.is_cancelled()
    }

    /// Enqueue an item frame.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the sender is cancelled or a terminal frame was already sent.
    pub fn item(&self, data: Valor) -> Result<(), FrameError> {
        self.send(FrameStatus::Item, data)
    }

    /// Enqueue a byte frame.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the sender is cancelled or a terminal frame was already sent.
    pub fn byte(&self, bytes: Vec<u8>) -> Result<(), FrameError> {
        self.send(FrameStatus::Byte, Valor::Octeti(bytes))
    }

    /// Enqueue a done (success) terminal frame.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the sender is cancelled or a terminal frame was already sent.
    pub fn done(&self) -> Result<(), FrameError> {
        self.send(FrameStatus::Done, Valor::Nihil)
    }

    /// Enqueue an error terminal frame.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the sender is cancelled or a terminal frame was already sent.
    pub fn error(&self, message: impl Into<String>) -> Result<(), FrameError> {
        self.send(FrameStatus::Error, Valor::Textus(message.into()))
    }

    /// Enqueue a cancel terminal frame.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the sender is cancelled or a terminal frame was already sent.
    pub fn cancel(&self) -> Result<(), FrameError> {
        self.send(FrameStatus::Cancel, Valor::Nihil)
    }

    /// Enqueue a frame with the given status and data.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the sender is cancelled (non-terminal frames are
    /// rejected after cancellation) or a terminal frame was already sent.
    pub fn send(&self, mut status: FrameStatus, mut data: Valor) -> Result<(), FrameError> {
        if self.is_cancelled() && !status.is_terminal() {
            return Err(FrameError::new(
                "frame_response_cancelled",
                "response sender cannot enqueue content after cancellation",
            ));
        }
        if self.is_cancelled() && status == FrameStatus::Done {
            status = FrameStatus::Cancel;
            data = Valor::Nihil;
        }
        let mut terminal_sent = self
            .lease
            .terminal_sent
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if status.is_terminal() {
            if *terminal_sent {
                return Err(FrameError::new(
                    "frame_response_terminal_already_sent",
                    "response sender already sent a terminal frame",
                ));
            }
            *terminal_sent = true;
        } else if *terminal_sent {
            return Err(FrameError::new(
                "frame_response_after_terminal",
                "response sender cannot enqueue content after a terminal frame",
            ));
        }
        push_response_frame(&self.lease.shared, status, data);
        Ok(())
    }

    /// Deliver the terminal rejection for a start error. Cancellation-aware:
    /// once the receiving side's drop has recorded cancellation on the shared
    /// `Cancellation`, the detached rejection must NOT surface an `Error`
    /// terminal — exactly one terminal state is observed, and it is the
    /// recorded `Cancel` (pushed by this sender's `Drop`), never `Error` after
    /// `Cancel`.
    ///
    /// The cancellation check and the terminal push happen under the sermo
    /// lock — the same lock `cancel_runtime_response` holds while recording
    /// cancellation — so a cancel cannot land between the check and the push.
    pub(crate) fn reject_start_error(&self, error: DispatchError) {
        let mut terminal_sent = self
            .lease
            .terminal_sent
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if *terminal_sent {
            return;
        }
        let mut inner = lock_sermo(&self.lease.shared);
        if self.lease.cancellation.is_cancelled() {
            // Cancellation was recorded by the dropped receive future: leave
            // `terminal_sent` untouched and let this sender's `Drop` record the
            // `Cancel` terminal — one atomic terminal, never `Error` after
            // `Cancel`.
            return;
        }
        *terminal_sent = true;
        push_runtime_frame(&mut inner, FrameStatus::Error, Valor::Textus(error.message));
        self.lease.shared.incoming_changed.notify_all();
    }
}

impl Clone for ResponseSender {
    fn clone(&self) -> Self {
        self.lease.live_senders.fetch_add(1, Ordering::SeqCst);
        Self {
            lease: Arc::clone(&self.lease),
        }
    }
}

impl Drop for ResponseSender {
    fn drop(&mut self) {
        if self.lease.live_senders.fetch_sub(1, Ordering::SeqCst) != 1 {
            return;
        }
        let mut terminal_sent = self
            .lease
            .terminal_sent
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if *terminal_sent {
            return;
        }
        *terminal_sent = true;
        if self.lease.cancellation.is_cancelled() {
            push_response_frame(&self.lease.shared, FrameStatus::Cancel, Valor::Nihil);
        } else {
            push_response_frame(
                &self.lease.shared,
                FrameStatus::Error,
                Valor::Textus("response producer dropped before terminal frame".to_owned()),
            );
        }
    }
}

impl<T> std::fmt::Debug for Meus<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Meus")
            .field("conversation_id", &lock_sermo(&self.inner).conversation_id)
            .finish_non_exhaustive()
    }
}

impl<T> std::fmt::Debug for Tuus<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tuus")
            .field("conversation_id", &lock_sermo(&self.inner).conversation_id)
            .finish_non_exhaustive()
    }
}

/// Generated Rust status enums implement this trait instead of emitting shim fns.
pub trait IntoFrameStatus {
    fn into_frame_status(self) -> FrameStatus;
}

impl IntoFrameStatus for FrameStatus {
    fn into_frame_status(self) -> FrameStatus {
        self
    }
}

/// Generated Rust `scrinium` structs implement this trait instead of emitting shim fns.
pub trait IntoScrinium {
    fn into_scrinium(self) -> Scrinium;
}

impl IntoScrinium for Scrinium {
    fn into_scrinium(self) -> Scrinium {
        self
    }
}

pub fn frame_status_from_user<T: IntoFrameStatus>(value: T) -> FrameStatus {
    value.into_frame_status()
}

pub fn scrinium_from_user<T: IntoScrinium>(frame: T) -> Scrinium {
    frame.into_scrinium()
}

pub fn next_frame_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    format!("frame-{}", NEXT.fetch_add(1, Ordering::Relaxed))
}

fn lock_sermo(shared: &SermoShared) -> MutexGuard<'_, SermoInner> {
    shared.state.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Sermo {
    #[must_use]
    pub fn conversation_id(&self) -> String {
        lock_sermo(&self.inner).conversation_id.clone()
    }

    #[must_use]
    pub fn route(&self) -> String {
        lock_sermo(&self.inner).route.clone()
    }

    #[must_use]
    pub fn incoming_drained(&self) -> bool {
        lock_sermo(&self.inner).incoming_drained
    }

    /// The router tier that answered, or `None` before dispatch started.
    #[must_use]
    pub fn answering_tier(&self) -> Option<AnsweringTier> {
        lock_sermo(&self.inner).answering_tier
    }

    /// Whether the conversation still holds its tier-1 handler task (it is
    /// dropped at the release point).
    #[must_use]
    pub fn handler_task_held(&self) -> bool {
        lock_sermo(&self.inner).handler_task.is_some()
    }

    /// Whether the conversation's release point has run (D8.3).
    #[must_use]
    pub fn released(&self) -> bool {
        lock_sermo(&self.inner).lifecycle != Lifecycle::Open
    }

    pub fn push_incoming(&mut self, frame: Scrinium) {
        let mut inner = lock_sermo(&self.inner);
        inner.runtime_response_state = RuntimeResponseState::Generated;
        if frame.status.is_terminal() {
            note_producer_terminal(&mut inner, frame.status, &frame.data);
        }
        inner.incoming.push_back(frame);
        wake_incoming(&mut inner);
        self.inner.incoming_changed.notify_all();
    }

    #[must_use]
    pub fn first_outgoing(&self) -> Option<Scrinium> {
        lock_sermo(&self.inner).outgoing.first().cloned()
    }
}

/// Set the opener on the request frame. A tier-1 route starts its handler
/// task here: the opener is final once set.
pub fn sermo_set_opener(sermo: &mut Sermo, data: Valor) {
    let mut inner = lock_sermo(&sermo.inner);
    if let Some(request) = inner.outgoing.first_mut()
        && request.status == FrameStatus::Request
    {
        request.data = data;
    }
    if static_route(&inner).is_some() {
        start_runtime_response(&sermo.inner, &mut inner, None);
    }
}

#[must_use]
pub fn sermo_open(route: &str) -> Sermo {
    let sermo = new_sermo(route);
    start_zero_opener_static_route(&sermo);
    sermo
}

/// A zero-parameter tier-1 handler has no opener to wait for: it starts when
/// the conversation opens.
fn start_zero_opener_static_route(sermo: &Sermo) {
    let mut inner = lock_sermo(&sermo.inner);
    if static_route(&inner).is_some_and(|route| !route.takes_opener) {
        start_runtime_response(&sermo.inner, &mut inner, None);
    }
}

fn new_sermo(route: &str) -> Sermo {
    let conversation_id = next_frame_id();
    LIVE_CONVERSATIONS.fetch_add(1, Ordering::SeqCst);
    let shared = Arc::new(SermoShared::new(SermoInner {
        conversation_id: conversation_id.clone(),
        route: route.to_owned(),
        outgoing: vec![Scrinium {
            id: conversation_id,
            parent_id: None,
            call: route.to_owned(),
            status: FrameStatus::Request,
            data: Valor::Nihil,
            created_ms: now_millis(),
            from: None,
            trace: None,
        }],
        incoming: VecDeque::new(),
        runtime_response_state: RuntimeResponseState::Pending,
        incoming_drained: false,
        incoming_terminal: None,
        incoming_wake_epoch: 0,
        incoming_waiters: Vec::new(),
        runtime_cancellation: None,
        host_dispatch: None,
        static_routes: None,
        answering_tier: None,
        handler_task: None,
        detached: false,
        meus_closed: false,
        producer_terminal: None,
        lifecycle: Lifecycle::Open,
    }));
    Sermo {
        caller: Arc::new(CallerHandle {
            shared: Arc::clone(&shared),
        }),
        inner: shared,
    }
}

/// Router slots held by conversations in this process that have not been
/// released (D8.3). Process-wide, so concurrent conversations all count.
static LIVE_CONVERSATIONS: AtomicUsize = AtomicUsize::new(0);

/// Conversations opened in this process and not yet released.
#[must_use]
pub fn live_conversations() -> usize {
    LIVE_CONVERSATIONS.load(Ordering::SeqCst)
}

/// Open a conversation against an explicit tier-1 table and optional host,
/// without touching the process-global installations (embedder/test seam).
#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
#[must_use]
pub fn sermo_open_with_static_routes(
    route: &str,
    routes: &'static [StaticRoute],
    dispatch: Option<Arc<dyn HostDispatch>>,
) -> Sermo {
    let sermo = new_sermo(route);
    {
        let mut inner = lock_sermo(&sermo.inner);
        inner.static_routes = Some(routes);
        inner.host_dispatch = dispatch.map(DispatchOverride);
    }
    start_zero_opener_static_route(&sermo);
    sermo
}

/// Open a conversation with an explicit host dispatcher.
///
/// This constructor is intended for embedders and tests that need independent
/// hosts in one process. It does not mutate the process-global installation and
/// therefore avoids test races and cross-embedder coupling.
pub fn sermo_open_with_dispatch(route: &str, dispatch: Arc<dyn HostDispatch>) -> Sermo {
    let sermo = new_sermo(route);
    lock_sermo(&sermo.inner).host_dispatch = Some(DispatchOverride(dispatch));
    start_zero_opener_static_route(&sermo);
    sermo
}

#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
#[must_use]
pub fn test_response_sender(route: &str) -> (Sermo, ResponseSender, Cancellation) {
    let sermo = sermo_open(route);
    {
        let mut inner = lock_sermo(&sermo.inner);
        inner.runtime_response_state = RuntimeResponseState::Generated;
    }
    let cancellation = Cancellation {
        cancelled: Arc::new(AtomicBool::new(false)),
    };
    let sender = ResponseSender::new(Arc::clone(&sermo.inner), cancellation.clone());
    (sermo, sender, cancellation)
}

#[must_use]
pub fn sermo_meus<T>(sermo: &Sermo) -> Meus<T> {
    Meus {
        inner: sermo.inner.clone(),
        _caller: Arc::clone(&sermo.caller),
        _marker: PhantomData,
    }
}

#[must_use]
pub fn sermo_tuus<T>(sermo: &Sermo) -> Tuus<T> {
    Tuus {
        inner: sermo.inner.clone(),
        caller: Arc::clone(&sermo.caller),
        _marker: PhantomData,
    }
}

/// Push a frame onto a Meus half-stream.
///
/// # Errors
///
/// Returns `Err` if the Meus half-stream is closed.
pub fn meus_da<T>(meus: &Meus<T>, data: Valor) -> Result<(), FrameError> {
    let mut inner = lock_sermo(&meus.inner);
    if inner.meus_closed {
        return Err(FrameError::new(
            "frame_meus_half_stream_closed",
            "meus half-stream is closed",
        ));
    }
    let conversation_id = inner.conversation_id.clone();
    let route = inner.route.clone();
    inner.outgoing.push(Scrinium {
        id: next_frame_id(),
        parent_id: Some(conversation_id),
        call: route,
        status: FrameStatus::Item,
        data,
        created_ms: now_millis(),
        from: None,
        trace: None,
    });
    Ok(())
}

#[must_use]
pub fn meus_fini<T>(meus: &Meus<T>) -> FrameStatus {
    let mut inner = lock_sermo(&meus.inner);
    if !inner.meus_closed {
        let conversation_id = inner.conversation_id.clone();
        let route = inner.route.clone();
        inner.outgoing.push(Scrinium {
            id: next_frame_id(),
            parent_id: Some(conversation_id),
            call: route,
            status: FrameStatus::Done,
            data: Valor::Nihil,
            created_ms: now_millis(),
            from: None,
            trace: None,
        });
        inner.meus_closed = true;
    }
    FrameStatus::Done
}

#[must_use]
pub fn tuus_accipe<T>(tuus: &Tuus<T>) -> Option<Scrinium> {
    recv_content_frame_blocking(&tuus.inner)
}

/// Lazy inbound content-frame iterator; shares the queue with `tuus_accipe`.
pub struct TuusCursor<T> {
    inner: Arc<SermoShared>,
    _caller: Arc<CallerHandle>,
    _marker: PhantomData<T>,
}

impl<T> Iterator for TuusCursor<T> {
    type Item = Scrinium;

    fn next(&mut self) -> Option<Scrinium> {
        recv_content_frame_blocking(&self.inner)
    }
}

#[must_use]
pub fn tuus_cursor<T>(tuus: &Tuus<T>) -> TuusCursor<T> {
    TuusCursor {
        inner: tuus.inner.clone(),
        _caller: Arc::clone(&tuus.caller),
        _marker: PhantomData,
    }
}

#[must_use]
pub fn tuus_fini<T>(tuus: &Tuus<T>) -> FrameStatus {
    let mut inner = lock_sermo(&tuus.inner);
    if inner.incoming_drained {
        return inner.incoming_terminal.unwrap_or(FrameStatus::Done);
    }
    ensure_runtime_response_started(&tuus.inner, &mut inner);
    // WHY: an unread stream whose Faber handler is still producing is
    // cancelled, not drained (the Go/runner choice): the handler may never
    // finish on its own.
    if inner.producer_terminal.is_none() && inner.answering_tier == Some(AnsweringTier::Static) {
        record_incoming_terminal(&mut inner, FrameStatus::Cancel);
        inner.incoming.clear();
        return FrameStatus::Cancel;
    }
    while let Some(frame) = inner.incoming.pop_front() {
        if frame.status.is_terminal() {
            record_incoming_terminal(&mut inner, frame.status);
            return frame.status;
        }
    }
    record_incoming_terminal(&mut inner, FrameStatus::Done);
    FrameStatus::Done
}

#[must_use]
pub fn tuus_as_sermo<T>(tuus: &Tuus<T>) -> Sermo {
    Sermo {
        inner: tuus.inner.clone(),
        caller: Arc::clone(&tuus.caller),
    }
}

fn record_incoming_terminal(inner: &mut SermoInner, status: FrameStatus) {
    inner.incoming_terminal = Some(status);
    inner.incoming_drained = true;
    release_conversation(inner);
}

/// The conversation's single release point (§2.7, D8.3). It runs when the
/// conversation completes: the producer sent its terminal, the consumer
/// reached or cancelled to a terminal, `close()` ran, or the last caller
/// handle dropped. A producer still running is cancelled first. Release
/// drops the handler task handle (detaching the thread), the cancellation
/// lease and any host dispatch, and gives the router slot back. Idempotent.
fn release_conversation(inner: &mut SermoInner) {
    if inner.lifecycle != Lifecycle::Open {
        return;
    }
    inner.lifecycle = Lifecycle::Released;
    if let Some(cancellation) = inner.runtime_cancellation.take()
        && inner.producer_terminal.is_none()
    {
        cancellation.cancel();
    }
    inner.handler_task = None;
    inner.host_dispatch = None;
    LIVE_CONVERSATIONS.fetch_sub(1, Ordering::SeqCst);
}

/// Record the producer's terminal: the conversation is complete.
fn note_producer_terminal(inner: &mut SermoInner, status: FrameStatus, data: &Valor) {
    if inner.producer_terminal.is_none() {
        let error = (status == FrameStatus::Error).then(|| error_text(data));
        inner.producer_terminal = Some((status, error));
    }
}

/// Text of an `error` terminal's data, as `close()` reports it.
fn error_text(data: &Valor) -> String {
    match data {
        Valor::Textus(text) => text.clone(),
        other => format!("{other:?}"),
    }
}

/// Close a conversation early (D8.3 `close()`). A live handler is cancelled,
/// unread frames are dropped, and the conversation is released. Returns
/// whether this call closed it (`false` when it was already closed).
///
/// # Errors
///
/// Returns the text of the conversation's `error` terminal.
pub fn sermo_close(sermo: &Sermo) -> Result<bool, String> {
    let mut inner = lock_sermo(&sermo.inner);
    if inner.lifecycle == Lifecycle::Closed {
        return Ok(false);
    }
    if !inner.incoming_drained {
        let terminal = inner
            .producer_terminal
            .as_ref()
            .map_or(FrameStatus::Cancel, |(status, _)| *status);
        inner.runtime_response_state = RuntimeResponseState::Generated;
        record_incoming_terminal(&mut inner, terminal);
    }
    inner.incoming.clear();
    release_conversation(&mut inner);
    inner.lifecycle = Lifecycle::Closed;
    sermo.inner.incoming_changed.notify_all();
    if inner.incoming_terminal == Some(FrameStatus::Error) {
        let error = inner
            .producer_terminal
            .as_ref()
            .and_then(|(_, error)| error.clone());
        return Err(error.unwrap_or_default());
    }
    Ok(true)
}

fn wake_incoming(inner: &mut SermoInner) {
    inner.incoming_wake_epoch = inner.incoming_wake_epoch.wrapping_add(1);
    for waiter in inner.incoming_waiters.drain(..) {
        waiter.wake();
    }
}

fn push_response_frame(shared: &SermoShared, status: FrameStatus, data: Valor) {
    let mut inner = lock_sermo(shared);
    push_runtime_frame(&mut inner, status, data);
    shared.incoming_changed.notify_all();
}

/// Receive the next content frame of a live inbound view. A view owns no
/// separate dispatch: the first receive starts the route (tiers 2 and 3 start
/// lazily, like `sermo_recv`), and the call waits until a frame or the
/// producer terminal arrives. Without the start and the wait, `accipe` and the
/// cursor on a builtin route saw an empty queue and ended at once.
fn recv_content_frame_blocking(shared: &Arc<SermoShared>) -> Option<Scrinium> {
    let mut inner = lock_sermo(shared);
    if inner.detached || inner.incoming_drained {
        return None;
    }
    if inner.incoming.is_empty() {
        ensure_runtime_response_started(shared, &mut inner);
    }
    while inner.incoming.is_empty() && !inner.detached && !inner.incoming_drained {
        inner = shared
            .incoming_changed
            .wait(inner)
            .unwrap_or_else(PoisonError::into_inner);
    }
    recv_content_frame(&mut inner)
}

fn recv_content_frame(inner: &mut SermoInner) -> Option<Scrinium> {
    if inner.detached || inner.incoming_drained {
        return None;
    }
    let frame = inner.incoming.pop_front()?;
    if frame.status.is_terminal() {
        record_incoming_terminal(inner, frame.status);
        return None;
    }
    Some(frame)
}

fn drain_incoming_to_terminal(sermo: &mut Sermo) {
    while let Some(frame) = sermo_recv(sermo) {
        if frame.status.is_terminal() {
            break;
        }
    }
    let mut inner = lock_sermo(&sermo.inner);
    if !inner.incoming_drained {
        record_incoming_terminal(&mut inner, FrameStatus::Done);
    }
}

/// Drain inbound content frames into a raw frame list for internal materialization.
#[must_use]
pub fn sermo_tuus_frames(mut sermo: Sermo) -> Vec<Scrinium> {
    let mut frames = Vec::new();
    while let Some(frame) = sermo_recv(&mut sermo) {
        if frame.status.is_terminal() {
            break;
        }
        frames.push(frame);
    }
    let mut inner = lock_sermo(&sermo.inner);
    if inner.incoming_terminal.is_none() {
        record_incoming_terminal(&mut inner, FrameStatus::Done);
    }
    frames
}

pub fn sermo_recv(sermo: &mut Sermo) -> Option<Scrinium> {
    let mut inner = lock_sermo(&sermo.inner);
    if inner.detached {
        return None;
    }
    ensure_runtime_response_started(&sermo.inner, &mut inner);
    while inner.incoming.is_empty() && !inner.detached && !inner.incoming_drained {
        inner = sermo
            .inner
            .incoming_changed
            .wait(inner)
            .unwrap_or_else(PoisonError::into_inner);
    }
    let frame = inner.incoming.pop_front()?;
    if frame.status.is_terminal() {
        record_incoming_terminal(&mut inner, frame.status);
    }
    Some(frame)
}

pub fn sermo_recv_async(sermo: &mut Sermo) -> SermoRecvFuture<'_> {
    SermoRecvFuture {
        sermo,
        completed: false,
    }
}

fn sermo_recv_ready(sermo: &mut Sermo) -> Option<Scrinium> {
    let mut inner = lock_sermo(&sermo.inner);
    if inner.detached {
        return None;
    }
    if inner.incoming.is_empty() {
        ensure_runtime_response_started(&sermo.inner, &mut inner);
    }
    let frame = inner.incoming.pop_front()?;
    if frame.status.is_terminal() {
        record_incoming_terminal(&mut inner, frame.status);
    }
    Some(frame)
}

pub struct SermoRecvFuture<'a> {
    sermo: &'a mut Sermo,
    completed: bool,
}

impl std::future::Future for SermoRecvFuture<'_> {
    type Output = Option<Scrinium>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        if let Some(frame) = sermo_recv_ready(this.sermo) {
            this.completed = true;
            return Poll::Ready(Some(frame));
        }
        let mut inner = lock_sermo(&this.sermo.inner);
        if inner.detached || inner.incoming_drained {
            this.completed = true;
            return Poll::Ready(None);
        }
        if !inner.incoming.is_empty() {
            drop(inner);
            if let Some(frame) = sermo_recv_ready(this.sermo) {
                this.completed = true;
                return Poll::Ready(Some(frame));
            }
            return Poll::Pending;
        }
        if !inner
            .incoming_waiters
            .iter()
            .any(|waiter| waiter.will_wake(cx.waker()))
        {
            inner.incoming_waiters.push(cx.waker().clone());
        }
        Poll::Pending
    }
}

impl Drop for SermoRecvFuture<'_> {
    fn drop(&mut self) {
        if !self.completed {
            cancel_runtime_response(&self.sermo.inner);
        }
    }
}

fn cancel_runtime_response(shared: &Arc<SermoShared>) {
    let mut inner = lock_sermo(shared);
    if let Some(cancellation) = inner.runtime_cancellation.take() {
        cancellation.cancel();
        wake_incoming(&mut inner);
        shared.incoming_changed.notify_all();
    }
}

fn ensure_runtime_response_started(shared: &Arc<SermoShared>, inner: &mut SermoInner) {
    start_runtime_response(shared, inner, None);
}

fn ensure_runtime_response_started_for_type<T>(sermo: &mut Sermo)
where
    T: crate::FromValor,
{
    ensure_runtime_response_started_for_target(sermo, std::any::type_name::<T>());
}

fn ensure_runtime_response_started_for_target(sermo: &mut Sermo, target: &'static str) {
    let mut inner = lock_sermo(&sermo.inner);
    start_runtime_response(&sermo.inner, &mut inner, Some(target));
}

/// Start dispatch once per conversation. Tier 1 starts eagerly (at open or
/// when the opener is set); tiers 2 and 3 start lazily on the first receive.
fn start_runtime_response(
    shared: &Arc<SermoShared>,
    inner: &mut SermoInner,
    target: Option<&'static str>,
) {
    if inner.runtime_response_state == RuntimeResponseState::Generated {
        return;
    }
    inner.runtime_response_state = RuntimeResponseState::Generated;
    let request = sermo_request(inner, target);
    let cancellation = Cancellation {
        cancelled: Arc::new(AtomicBool::new(false)),
    };
    inner.runtime_cancellation = Some(cancellation.clone());
    let responses = ResponseSender::new(Arc::clone(shared), cancellation.clone());
    if let Err(error) = route_conversation(inner, request, responses.clone(), cancellation) {
        // The caller holds the sermo lock while starting dispatch, and
        // `reject_start_error` enqueues through that same (non-reentrant)
        // lock — deliver the terminal rejection from a separate thread, once
        // the lock is released, instead of deadlocking.
        thread::spawn(move || responses.reject_start_error(error));
    }
}

/// The tier-1 entry for this conversation's route, if the static table
/// (per-conversation seam first, then the installed table) serves it.
fn static_route(inner: &SermoInner) -> Option<StaticRoute> {
    let routes = inner
        .static_routes
        .or_else(|| STATIC_ROUTES.get().copied())?;
    routes
        .iter()
        .find(|entry| entry.route == inner.route)
        .copied()
}

/// The three-tier router (D6.9): static Faber table → builtin routes →
/// installed host → fail closed. Records the answering tier on the
/// conversation. A host may no longer answer a builtin route itself.
fn route_conversation(
    inner: &mut SermoInner,
    request: SermoRequest,
    responses: ResponseSender,
    cancellation: Cancellation,
) -> Result<(), DispatchError> {
    if let Some(route) = static_route(inner) {
        inner.answering_tier = Some(AnsweringTier::Static);
        inner.handler_task = Some(spawn_static_handler(
            route,
            request,
            responses,
            cancellation,
        ));
        return Ok(());
    }
    if is_builtin_route(&request.route) {
        inner.answering_tier = Some(AnsweringTier::Builtin);
        return BuiltinRuntimeDispatch.start(request, responses, cancellation);
    }
    let host = inner
        .host_dispatch
        .as_ref()
        .map(|override_dispatch| Arc::clone(&override_dispatch.0))
        .or_else(|| HOST_DISPATCH.get().cloned());
    if let Some(dispatch) = host {
        inner.answering_tier = Some(AnsweringTier::Host);
        return dispatch.start(request, responses, cancellation);
    }
    inner.answering_tier = Some(AnsweringTier::None);
    Err(DispatchError::new(
        "host_dispatch_unavailable",
        format!("no host dispatch installed for route `{}`", request.route),
    ))
}

/// Run one tier-1 handler as its own task (D6.13). A panicking handler
/// becomes an `error` terminal naming the route; a handler that returns
/// without a terminal gets the `ResponseSender` drop net.
fn spawn_static_handler(
    route: StaticRoute,
    request: SermoRequest,
    responses: ResponseSender,
    cancellation: Cancellation,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let handler_responses = responses.clone();
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            (route.start)(request, handler_responses, cancellation);
        }));
        if outcome.is_err() {
            let _ = responses.error(format!("route handler `{}` panicked", route.route));
        }
    })
}

/// Minimal S1-U3-stabilized builtin dispatch. `runtime:echo` is restored
/// in-process (the provider split never shipped an echo provider). The
/// terminal builtin `processus:exi` is also in-process (`process::exit`)
/// because the host processus provider must keep that route unmanifested.
/// The full pre-split builtin table is intentionally not resurrected.
#[derive(Clone, Copy, Debug)]
struct BuiltinRuntimeDispatch;

impl HostDispatch for BuiltinRuntimeDispatch {
    fn start(
        &self,
        request: SermoRequest,
        responses: ResponseSender,
        _cancellation: Cancellation,
    ) -> Result<(), DispatchError> {
        if request.route == "processus:exi" {
            let Some(code) = processus_exi_code(&request.opener) else {
                thread::spawn(move || {
                    let _ = responses.error("processus:exi opener must be numerus");
                });
                return Ok(());
            };
            std::process::exit(code);
        }
        // Echo one `Item` frame containing the opener, then `Done` (matches
        // the MIR stepper / Go / TS inline echo contract).
        thread::spawn(move || {
            let _ = responses.item(request.opener);
            let _ = responses.done();
        });
        Ok(())
    }
}

/// Pure route-key classification for builtin-runtime coverage — the single
/// source of truth for "is this route builtin-covered". Both the hostless
/// dispatch gate and the native-host fallback probe route through it, so the
/// coverage cannot drift. Must stay aligned with the radix-owned plan fact
/// `is_builtin_ad_route`.
pub(crate) fn is_builtin_route(route: &str) -> bool {
    matches!(route, "runtime:echo" | "processus:exi")
}

fn processus_exi_code(opener: &Valor) -> Option<i32> {
    let Valor::Numerus(code) = opener else {
        return None;
    };
    // SAFETY: clamped to 0..=255, safe for i32.
    #[allow(clippy::cast_possible_truncation)]
    Some((*code).clamp(0, i64::from(u8::MAX)) as i32)
}

fn sermo_request(inner: &SermoInner, target: Option<&'static str>) -> SermoRequest {
    SermoRequest {
        conversation_id: inner.conversation_id.clone(),
        route: inner.route.clone(),
        opener: request_data(inner),
        target,
    }
}

fn push_runtime_frame(inner: &mut SermoInner, status: FrameStatus, data: Valor) {
    if status.is_terminal() {
        note_producer_terminal(inner, status, &data);
        // Completion (D8.3): the producer is done, so the conversation
        // releases now, even while its handle and unread frames live on.
        release_conversation(inner);
    }
    inner.incoming.push_back(Scrinium {
        id: next_frame_id(),
        parent_id: Some(inner.conversation_id.clone()),
        call: inner.route.clone(),
        status,
        data,
        created_ms: now_millis(),
        from: Some("faber-runtime".into()),
        trace: None,
    });
    wake_incoming(inner);
}

fn request_data(inner: &SermoInner) -> Valor {
    inner
        .outgoing
        .first()
        .map_or(Valor::Nihil, |request| request.data.clone())
}

fn ensure_scalar_runtime_response<T>(sermo: &mut Sermo)
where
    T: crate::FromValor,
{
    ensure_runtime_response_started_for_type::<T>(sermo);
}

#[must_use]
pub fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
        })
}

// ---- `sermo ↦ T` materializers --------------------------------------------

fn terminal_error(frame: &Scrinium) -> Option<FrameError> {
    match frame.status {
        FrameStatus::Error => Some(FrameError::new(
            "frame_materialization_terminal_error",
            format!("sermo materialization terminal error: {:?}", frame.data),
        )),
        FrameStatus::Cancel => Some(FrameError::new(
            "frame_materialization_cancelled",
            "sermo materialization cancelled",
        )),
        _ => None,
    }
}

fn drain_remaining_then_err<T>(sermo: &mut Sermo, error: FrameError) -> Result<T, FrameError> {
    drain_incoming_to_terminal(sermo);
    Err(error)
}

/// Drain all frames until terminal, discarding content.
///
/// # Panics
///
/// Panics if the stream produces a terminal error frame.
/// # Errors
/// Returns an error when the requested operation cannot be completed.
pub fn sermo_materialize_vacuum(sermo: &mut Sermo) {
    try_sermo_materialize_vacuum(sermo).expect("sermo ↦ vacuum materialization failed");
}

/// Drain all frames until terminal, discarding content.
///
/// # Errors
///
/// Returns `Err` if the stream produces a terminal error frame.
pub fn try_sermo_materialize_vacuum(sermo: &mut Sermo) -> Result<(), FrameError> {
    while let Some(frame) = sermo_recv(sermo) {
        if let Some(message) = terminal_error(&frame) {
            return Err(message);
        }
        if frame.status.is_terminal() {
            break;
        }
    }
    Ok(())
}

/// Drain all frames until terminal, discarding content (async).
///
/// # Panics
///
/// Panics if the stream produces a terminal error frame.
pub async fn sermo_materialize_vacuum_async(sermo: &mut Sermo) {
    try_sermo_materialize_vacuum_async(sermo)
        .await
        .expect("sermo ↦ vacuum async materialization failed");
}

/// Drain all frames until terminal, discarding content (async).
///
/// # Errors
///
/// Returns `Err` if the stream produces a terminal error frame.
pub async fn try_sermo_materialize_vacuum_async(sermo: &mut Sermo) -> Result<(), FrameError> {
    while let Some(frame) = sermo_recv_async(sermo).await {
        if let Some(message) = terminal_error(&frame) {
            return Err(message);
        }
        if frame.status.is_terminal() {
            break;
        }
    }
    Ok(())
}

/// Materialize a `textus` (String) from the stream.
///
/// # Panics
///
/// Panics if the stream produces a terminal error frame or a non-textus
/// content frame.
pub fn sermo_materialize_textus(sermo: &mut Sermo) -> String {
    try_sermo_materialize_textus(sermo).expect("sermo ↦ textus materialization failed")
}

/// Materialize a `textus` (String) from the stream.
///
/// # Errors
///
/// Returns `Err` if the stream produces a terminal error frame or a
/// non-textus content frame.
pub fn try_sermo_materialize_textus(sermo: &mut Sermo) -> Result<String, FrameError> {
    ensure_runtime_response_started_for_target(sermo, std::any::type_name::<String>());
    let mut out = String::new();
    while let Some(frame) = sermo_recv(sermo) {
        if let Some(message) = terminal_error(&frame) {
            return Err(message);
        }
        if frame.status.is_terminal() {
            break;
        }
        let Valor::Textus(s) = &frame.data else {
            return drain_remaining_then_err(
                sermo,
                FrameError::new(
                    "frame_textus_payload_not_textus",
                    "sermo ↦ textus: content frame payload was not textus",
                ),
            );
        };
        out.push_str(s);
    }
    Ok(out)
}

/// Materialize a `textus` (String) from the stream (async).
///
/// # Panics
///
/// Panics if the stream produces a terminal error frame or a non-textus
/// content frame.
pub async fn sermo_materialize_textus_async(sermo: &mut Sermo) -> String {
    try_sermo_materialize_textus_async(sermo)
        .await
        .expect("sermo ↦ textus async materialization failed")
}

/// Materialize a `textus` (String) from the stream (async).
///
/// # Errors
///
/// Returns `Err` if the stream produces a terminal error frame or a
/// non-textus content frame.
pub async fn try_sermo_materialize_textus_async(sermo: &mut Sermo) -> Result<String, FrameError> {
    ensure_runtime_response_started_for_target(sermo, std::any::type_name::<String>());
    let mut out = String::new();
    while let Some(frame) = sermo_recv_async(sermo).await {
        if let Some(message) = terminal_error(&frame) {
            return Err(message);
        }
        if frame.status.is_terminal() {
            break;
        }
        let Valor::Textus(s) = &frame.data else {
            return drain_remaining_then_err_async(
                sermo,
                FrameError::new(
                    "frame_textus_payload_not_textus",
                    "sermo ↦ textus: content frame payload was not textus",
                ),
            )
            .await;
        };
        out.push_str(s);
    }
    Ok(out)
}

/// Materialize octeti (bytes) from the stream.
///
/// # Panics
///
/// Panics if the stream produces a terminal error frame or a content frame
/// with an unsupported payload variant.
pub fn sermo_materialize_octeti(sermo: &mut Sermo) -> Vec<u8> {
    try_sermo_materialize_octeti(sermo).expect("sermo ↦ octeti materialization failed")
}

/// Materialize octeti (bytes) from the stream.
///
/// # Errors
///
/// Returns `Err` if the stream produces a terminal error frame or a content
/// frame with an unsupported payload variant.
pub fn try_sermo_materialize_octeti(sermo: &mut Sermo) -> Result<Vec<u8>, FrameError> {
    ensure_runtime_response_started_for_type::<Vec<u8>>(sermo);
    let mut out = Vec::new();
    while let Some(frame) = sermo_recv(sermo) {
        if let Some(message) = terminal_error(&frame) {
            return Err(message);
        }
        if frame.status.is_terminal() {
            break;
        }
        match &frame.data {
            Valor::Octeti(bytes) => out.extend_from_slice(bytes),
            Valor::Lista(bytes) => {
                for v in bytes {
                    let Valor::Numerus(n) = v else {
                        return drain_remaining_then_err(
                            sermo,
                            FrameError::new(
                                "frame_octeti_byte_not_numerus",
                                "sermo ↦ octeti: byte payload contained a non-numerus value",
                            ),
                        );
                    };
                    let Ok(byte) = u8::try_from(*n) else {
                        return drain_remaining_then_err(
                            sermo,
                            FrameError::new(
                                "frame_octeti_byte_out_of_range",
                                "sermo ↦ octeti: byte payload value was outside 0..255",
                            ),
                        );
                    };
                    out.push(byte);
                }
            }
            _ => {
                return drain_remaining_then_err(
                    sermo,
                    FrameError::new(
                        "frame_octeti_payload_not_bytes",
                        "sermo ↦ octeti: content frame payload was not octeti or byte lista",
                    ),
                );
            }
        }
    }
    Ok(out)
}

/// Materialize octeti (bytes) from the stream (async).
///
/// # Panics
///
/// Panics if the stream produces a terminal error frame or a content frame
/// with an unsupported payload variant.
pub async fn sermo_materialize_octeti_async(sermo: &mut Sermo) -> Vec<u8> {
    try_sermo_materialize_octeti_async(sermo)
        .await
        .expect("sermo ↦ octeti async materialization failed")
}

/// Materialize octeti (bytes) from the stream (async).
///
/// # Errors
///
/// Returns `Err` if the stream produces a terminal error frame or a content
/// frame with an unsupported payload variant.
pub async fn try_sermo_materialize_octeti_async(sermo: &mut Sermo) -> Result<Vec<u8>, FrameError> {
    ensure_runtime_response_started_for_type::<Vec<u8>>(sermo);
    let mut out = Vec::new();
    while let Some(frame) = sermo_recv_async(sermo).await {
        if let Some(message) = terminal_error(&frame) {
            return Err(message);
        }
        if frame.status.is_terminal() {
            break;
        }
        match &frame.data {
            Valor::Octeti(bytes) => out.extend_from_slice(bytes),
            Valor::Lista(bytes) => {
                for v in bytes {
                    let Valor::Numerus(n) = v else {
                        return drain_remaining_then_err_async(
                            sermo,
                            FrameError::new(
                                "frame_octeti_byte_not_numerus",
                                "sermo ↦ octeti: byte payload contained a non-numerus value",
                            ),
                        )
                        .await;
                    };
                    let Ok(byte) = u8::try_from(*n) else {
                        return drain_remaining_then_err_async(
                            sermo,
                            FrameError::new(
                                "frame_octeti_byte_out_of_range",
                                "sermo ↦ octeti: byte payload value was outside 0..255",
                            ),
                        )
                        .await;
                    };
                    out.push(byte);
                }
            }
            _ => {
                return drain_remaining_then_err_async(
                    sermo,
                    FrameError::new(
                        "frame_octeti_payload_not_bytes",
                        "sermo ↦ octeti: content frame payload was not octeti or byte lista",
                    ),
                )
                .await;
            }
        }
    }
    Ok(out)
}

/// Materialize a single `Valor` from the stream (first content frame).
///
/// # Panics
///
/// Panics if the stream produces a terminal error frame.
/// # Errors
/// Returns an error when the requested operation cannot be completed.
pub fn sermo_materialize_valor(sermo: &mut Sermo) -> Valor {
    try_sermo_materialize_valor(sermo).expect("sermo ↦ valor materialization failed")
}

/// Materialize a single `Valor` from the stream (first content frame).
///
/// # Errors
///
/// Returns `Err` if the stream produces a terminal error frame.
pub fn try_sermo_materialize_valor(sermo: &mut Sermo) -> Result<Valor, FrameError> {
    let mut captured: Option<Valor> = None;
    while let Some(frame) = sermo_recv(sermo) {
        if let Some(message) = terminal_error(&frame) {
            return Err(message);
        }
        if frame.status.is_terminal() {
            break;
        }
        if captured.is_none() {
            captured = Some(frame.data);
        }
    }
    Ok(captured.unwrap_or(Valor::Nihil))
}

/// Materialize a single `Valor` from the stream (async, first content frame).
///
/// # Panics
///
/// Panics if the stream produces a terminal error frame.
pub async fn sermo_materialize_valor_async(sermo: &mut Sermo) -> Valor {
    try_sermo_materialize_valor_async(sermo)
        .await
        .expect("sermo ↦ valor async materialization failed")
}

/// Materialize a single `Valor` from the stream (async, first content frame).
///
/// # Errors
///
/// Returns `Err` if the stream produces a terminal error frame.
pub async fn try_sermo_materialize_valor_async(sermo: &mut Sermo) -> Result<Valor, FrameError> {
    let mut captured: Option<Valor> = None;
    while let Some(frame) = sermo_recv_async(sermo).await {
        if let Some(message) = terminal_error(&frame) {
            return Err(message);
        }
        if frame.status.is_terminal() {
            break;
        }
        if captured.is_none() {
            captured = Some(frame.data);
        }
    }
    Ok(captured.unwrap_or(Valor::Nihil))
}

/// Materialize a `lista<T>` (Vec<T>) from the stream.
///
/// # Panics
///
/// Panics if the stream produces a terminal error frame or a content frame
/// whose payload does not match the element type.
pub fn sermo_materialize_lista<T>(sermo: &mut Sermo) -> Vec<T>
where
    T: crate::FromValor,
{
    try_sermo_materialize_lista(sermo).expect("sermo ↦ lista<T> materialization failed")
}

/// Materialize a `lista<T>` (Vec<T>) from the stream.
///
/// # Errors
///
/// Returns `Err` if the stream produces a terminal error frame or a content
/// frame whose payload does not match the element type.
pub fn try_sermo_materialize_lista<T>(sermo: &mut Sermo) -> Result<Vec<T>, FrameError>
where
    T: crate::FromValor,
{
    if std::any::type_name::<T>() == std::any::type_name::<String>() {
        ensure_runtime_response_started_for_target(sermo, std::any::type_name::<Vec<String>>());
    }
    let mut out = Vec::new();
    while let Some(frame) = sermo_recv(sermo) {
        if let Some(message) = terminal_error(&frame) {
            return Err(message);
        }
        if frame.status.is_terminal() {
            break;
        }
        let Some(v) = T::from_valor(&frame.data) else {
            return drain_remaining_then_err(
                sermo,
                FrameError::new(
                    "frame_lista_payload_element_type_mismatch",
                    "sermo ↦ lista<T>: content frame payload did not match element type",
                ),
            );
        };
        out.push(v);
    }
    Ok(out)
}

/// Materialize a `lista<T>` (Vec<T>) from the stream (async).
///
/// # Panics
///
/// Panics if the stream produces a terminal error frame or a content frame
/// whose payload does not match the element type.
pub async fn sermo_materialize_lista_async<T>(sermo: &mut Sermo) -> Vec<T>
where
    T: crate::FromValor,
{
    try_sermo_materialize_lista_async(sermo)
        .await
        .expect("sermo ↦ lista<T> async materialization failed")
}

/// Materialize a `lista<T>` (Vec<T>) from the stream (async).
///
/// # Errors
///
/// Returns `Err` if the stream produces a terminal error frame or a content
/// frame whose payload does not match the element type.
pub async fn try_sermo_materialize_lista_async<T>(sermo: &mut Sermo) -> Result<Vec<T>, FrameError>
where
    T: crate::FromValor,
{
    if std::any::type_name::<T>() == std::any::type_name::<String>() {
        ensure_runtime_response_started_for_target(sermo, std::any::type_name::<Vec<String>>());
    }
    let mut out = Vec::new();
    while let Some(frame) = sermo_recv_async(sermo).await {
        if let Some(message) = terminal_error(&frame) {
            return Err(message);
        }
        if frame.status.is_terminal() {
            break;
        }
        let Some(v) = T::from_valor(&frame.data) else {
            return drain_remaining_then_err_async(
                sermo,
                FrameError::new(
                    "frame_lista_payload_element_type_mismatch",
                    "sermo ↦ lista<T>: content frame payload did not match element type",
                ),
            )
            .await;
        };
        out.push(v);
    }
    Ok(out)
}

/// Materialize a scalar `T` from the stream.
///
/// # Panics
///
/// Panics if the stream produces a terminal error frame, zero content frames,
/// multiple content frames, or a content frame whose payload does not match
/// the target type.
pub fn sermo_materialize_scalar<T>(sermo: &mut Sermo) -> T
where
    T: crate::FromValor,
{
    try_sermo_materialize_scalar(sermo).expect("sermo ↦ T scalar materialization failed")
}

/// Materialize a scalar `T` from the stream (async).
///
/// # Panics
///
/// Panics if the stream produces a terminal error frame, zero content frames,
/// multiple content frames, or a content frame whose payload does not match
/// the target type.
pub async fn sermo_materialize_scalar_async<T>(sermo: &mut Sermo) -> T
where
    T: crate::FromValor,
{
    try_sermo_materialize_scalar_async(sermo)
        .await
        .expect("sermo ↦ T scalar async materialization failed")
}

/// Materialize an `Instans` from the stream.
///
/// # Panics
///
/// Panics if the stream produces a terminal error frame, zero content frames,
/// multiple content frames, or a content frame whose payload does not match
/// the target type or precision.
pub fn sermo_materialize_instans(sermo: &mut Sermo, precision: InstansPraecisio) -> Instans {
    try_sermo_materialize_instans(sermo, precision).expect("sermo ↦ instans materialization failed")
}

/// Materialize an `Instans` from the stream.
///
/// # Errors
///
/// Returns `Err` if the stream produces a terminal error frame, zero content
/// frames, multiple content frames, or a content frame whose payload does not
/// match the target type or precision.
pub fn try_sermo_materialize_instans(
    sermo: &mut Sermo,
    precision: InstansPraecisio,
) -> Result<Instans, FrameError> {
    {
        let mut inner = lock_sermo(&sermo.inner);
        ensure_runtime_response_started(&sermo.inner, &mut inner);
    }
    let mut extracted: Option<Instans> = None;
    let mut content_count = 0u32;
    while let Some(frame) = sermo_recv(sermo) {
        if let Some(message) = terminal_error(&frame) {
            return Err(message);
        }
        if frame.status.is_terminal() {
            break;
        }
        content_count += 1;
        if extracted.is_none() {
            extracted = Instans::try_from_valor(&frame.data, precision);
        }
    }
    if content_count == 0 {
        return Err(FrameError::new(
            "frame_instans_no_content_frame",
            "sermo ↦ instans: no content frame before terminal",
        ));
    }
    if content_count > 1 {
        return Err(FrameError::new(
            "frame_instans_multiple_content_frames",
            format!("sermo ↦ instans: more than one content frame (found {content_count})"),
        ));
    }
    extracted.ok_or_else(|| {
        FrameError::new(
            "frame_instans_payload_target_type_mismatch",
            "sermo ↦ instans: content frame payload did not match target type",
        )
    })
}

/// Materialize an `Instans` from the stream (async).
///
/// # Panics
///
/// Panics if the stream produces a terminal error frame, zero content frames,
/// multiple content frames, or a content frame whose payload does not match
/// the target type or precision.
pub async fn sermo_materialize_instans_async(
    sermo: &mut Sermo,
    precision: InstansPraecisio,
) -> Instans {
    try_sermo_materialize_instans_async(sermo, precision)
        .await
        .expect("sermo ↦ instans async materialization failed")
}

/// Materialize an `Instans` from the stream (async).
///
/// # Errors
///
/// Returns `Err` if the stream produces a terminal error frame, zero content
/// frames, multiple content frames, or a content frame whose payload does not
/// match the target type or precision.
pub async fn try_sermo_materialize_instans_async(
    sermo: &mut Sermo,
    precision: InstansPraecisio,
) -> Result<Instans, FrameError> {
    {
        let mut inner = lock_sermo(&sermo.inner);
        ensure_runtime_response_started(&sermo.inner, &mut inner);
    }
    let mut extracted: Option<Instans> = None;
    let mut content_count = 0u32;
    while let Some(frame) = sermo_recv_async(sermo).await {
        if let Some(message) = terminal_error(&frame) {
            return Err(message);
        }
        if frame.status.is_terminal() {
            break;
        }
        content_count += 1;
        if extracted.is_none() {
            extracted = Instans::try_from_valor(&frame.data, precision);
        }
    }
    if content_count == 0 {
        return Err(FrameError::new(
            "frame_instans_no_content_frame",
            "sermo ↦ instans: no content frame before terminal",
        ));
    }
    if content_count > 1 {
        return Err(FrameError::new(
            "frame_instans_multiple_content_frames",
            format!("sermo ↦ instans: more than one content frame (found {content_count})"),
        ));
    }
    extracted.ok_or_else(|| {
        FrameError::new(
            "frame_instans_payload_target_type_mismatch",
            "sermo ↦ instans: content frame payload did not match target type",
        )
    })
}

/// Materialize a scalar `T` from the stream.
///
/// # Errors
///
/// Returns `Err` if the stream produces a terminal error frame, zero content
/// frames, multiple content frames, or a content frame whose payload does not
/// match the target type.
pub fn try_sermo_materialize_scalar<T>(sermo: &mut Sermo) -> Result<T, FrameError>
where
    T: crate::FromValor,
{
    ensure_scalar_runtime_response::<T>(sermo);
    let mut extracted: Option<T> = None;
    let mut content_count = 0u32;
    while let Some(frame) = sermo_recv(sermo) {
        if let Some(message) = terminal_error(&frame) {
            return Err(message);
        }
        if frame.status.is_terminal() {
            break;
        }
        content_count += 1;
        if extracted.is_none() {
            extracted = T::from_valor(&frame.data);
        }
    }
    if content_count == 0 {
        return Err(FrameError::new(
            "frame_scalar_no_content_frame",
            "sermo ↦ T scalar: no content frame before terminal",
        ));
    }
    if content_count > 1 {
        return Err(FrameError::new(
            "frame_scalar_multiple_content_frames",
            format!("sermo ↦ T scalar: more than one content frame (found {content_count})"),
        ));
    }
    extracted.ok_or_else(|| {
        FrameError::new(
            "frame_scalar_payload_target_type_mismatch",
            "sermo ↦ T scalar: content frame payload did not match target type",
        )
    })
}

/// Materialize a scalar `T` from the stream (async).
///
/// # Errors
///
/// Returns `Err` if the stream produces a terminal error frame, zero content
/// frames, multiple content frames, or a content frame whose payload does not
/// match the target type.
pub async fn try_sermo_materialize_scalar_async<T>(sermo: &mut Sermo) -> Result<T, FrameError>
where
    T: crate::FromValor,
{
    ensure_scalar_runtime_response::<T>(sermo);
    let mut extracted: Option<T> = None;
    let mut content_count = 0u32;
    while let Some(frame) = sermo_recv_async(sermo).await {
        if let Some(message) = terminal_error(&frame) {
            return Err(message);
        }
        if frame.status.is_terminal() {
            break;
        }
        content_count += 1;
        if extracted.is_none() {
            extracted = T::from_valor(&frame.data);
        }
    }
    if content_count == 0 {
        return Err(FrameError::new(
            "frame_scalar_no_content_frame",
            "sermo ↦ T scalar: no content frame before terminal",
        ));
    }
    if content_count > 1 {
        return Err(FrameError::new(
            "frame_scalar_multiple_content_frames",
            format!("sermo ↦ T scalar: more than one content frame (found {content_count})"),
        ));
    }
    extracted.ok_or_else(|| {
        FrameError::new(
            "frame_scalar_payload_target_type_mismatch",
            "sermo ↦ T scalar: content frame payload did not match target type",
        )
    })
}

/// Materialize `↦ T` for monomorphized generic provider bodies (`lege<T>`, …).
///
/// Codegen cannot pick lista vs scalar vs octeti while `T` is still a type
/// parameter. At monomorphization this dispatches by `TypeId` so
/// `lista<textus>` uses multi-item frames and does not panic on
/// `frame_scalar_multiple_content_frames`.
/// Materialize `T` from the stream using automatic dispatch by `TypeId`.
///
/// # Errors
///
/// Returns `Err` if the underlying materializer fails or the internal `TypeId`
/// cast detects a mismatch.
pub fn try_sermo_materialize_auto<T>(sermo: &mut Sermo) -> Result<T, FrameError>
where
    T: crate::FromValor + 'static,
{
    use std::any::TypeId;
    if TypeId::of::<T>() == TypeId::of::<Vec<String>>() {
        let lines = try_sermo_materialize_lista::<String>(sermo)?;
        return ok_type_id_cast(lines);
    }
    if TypeId::of::<T>() == TypeId::of::<Vec<u8>>() {
        let bytes = try_sermo_materialize_octeti(sermo)?;
        return ok_type_id_cast(bytes);
    }
    if TypeId::of::<T>() == TypeId::of::<String>() {
        let text = try_sermo_materialize_textus(sermo)?;
        return ok_type_id_cast(text);
    }
    try_sermo_materialize_scalar(sermo)
}

/// Async twin of [`try_sermo_materialize_auto`].
/// Materialize `T` from the stream using automatic dispatch by `TypeId` (async).
///
/// # Errors
///
/// Returns `Err` if the underlying materializer fails or the internal `TypeId`
/// cast detects a mismatch.
pub async fn try_sermo_materialize_auto_async<T>(sermo: &mut Sermo) -> Result<T, FrameError>
where
    T: crate::FromValor + 'static,
{
    use std::any::TypeId;
    if TypeId::of::<T>() == TypeId::of::<Vec<String>>() {
        let lines = try_sermo_materialize_lista_async::<String>(sermo).await?;
        return ok_type_id_cast(lines);
    }
    if TypeId::of::<T>() == TypeId::of::<Vec<u8>>() {
        let bytes = try_sermo_materialize_octeti_async(sermo).await?;
        return ok_type_id_cast(bytes);
    }
    if TypeId::of::<T>() == TypeId::of::<String>() {
        let text = try_sermo_materialize_textus_async(sermo).await?;
        return ok_type_id_cast(text);
    }
    try_sermo_materialize_scalar_async(sermo).await
}

fn ok_type_id_cast<T: 'static, U: 'static>(value: U) -> Result<T, FrameError> {
    use std::any::TypeId;
    if TypeId::of::<T>() != TypeId::of::<U>() {
        return Err(FrameError::new(
            "frame_materialize_auto_type_id_mismatch",
            "sermo materialize_auto internal type-id cast mismatch",
        ));
    }
    // SAFETY: TypeId equality above guarantees T and U are the same type.
    let ptr = Box::into_raw(Box::new(value)).cast::<T>();
    Ok(unsafe { *Box::from_raw(ptr) })
}

async fn drain_remaining_then_err_async<T>(
    sermo: &mut Sermo,
    error: FrameError,
) -> Result<T, FrameError> {
    while let Some(frame) = sermo_recv_async(sermo).await {
        if frame.status.is_terminal() {
            break;
        }
    }
    let mut inner = lock_sermo(&sermo.inner);
    if !inner.incoming_drained {
        record_incoming_terminal(&mut inner, FrameStatus::Done);
    }
    Err(error)
}
