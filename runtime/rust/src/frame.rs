//! In-process frame conversation types for expression `ad` and directional views.

use crate::{Instans, InstansPraecisio, Valor};
use std::collections::{BTreeMap, VecDeque};
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

#[derive(Debug)]
struct SermoInner {
    conversation_id: String,
    route: String,
    outgoing: Vec<Scrinium>,
    incoming: VecDeque<Scrinium>,
    runtime_response_generated: bool,
    incoming_drained: bool,
    /// Terminal `status` observed on the inbound direction (`done`, `error`, or `cancel`).
    incoming_terminal: Option<FrameStatus>,
    incoming_wake_epoch: u64,
    incoming_waiters: Vec<Waker>,
    runtime_cancellation: Option<Cancellation>,
    host_dispatch: Option<DispatchOverride>,
    detached: bool,
    meus_closed: bool,
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
}

/// Caller-to-gateway live outbound half-stream view.
pub struct Meus<T> {
    inner: Arc<SermoShared>,
    _marker: PhantomData<T>,
}

/// Gateway-to-caller live inbound half-stream view.
pub struct Tuus<T> {
    inner: Arc<SermoShared>,
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

    pub fn push_incoming(&mut self, frame: Scrinium) {
        let mut inner = lock_sermo(&self.inner);
        inner.runtime_response_generated = true;
        inner.incoming.push_back(frame);
        wake_incoming(&mut inner);
        self.inner.incoming_changed.notify_all();
    }

    #[must_use]
    pub fn first_outgoing(&self) -> Option<Scrinium> {
        lock_sermo(&self.inner).outgoing.first().cloned()
    }
}

pub fn sermo_set_opener(sermo: &mut Sermo, data: Valor) {
    if let Some(request) = lock_sermo(&sermo.inner).outgoing.first_mut() {
        if request.status == FrameStatus::Request {
            request.data = data;
        }
    }
}

#[must_use]
pub fn sermo_open(route: &str) -> Sermo {
    let conversation_id = next_frame_id();
    Sermo {
        inner: Arc::new(SermoShared::new(SermoInner {
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
            runtime_response_generated: false,
            incoming_drained: false,
            incoming_terminal: None,
            incoming_wake_epoch: 0,
            incoming_waiters: Vec::new(),
            runtime_cancellation: None,
            host_dispatch: None,
            detached: false,
            meus_closed: false,
        })),
    }
}

/// Open a conversation with an explicit host dispatcher.
///
/// This constructor is intended for embedders and tests that need independent
/// hosts in one process. It does not mutate the process-global installation and
/// therefore avoids test races and cross-embedder coupling.
pub fn sermo_open_with_dispatch(route: &str, dispatch: Arc<dyn HostDispatch>) -> Sermo {
    let sermo = sermo_open(route);
    lock_sermo(&sermo.inner).host_dispatch = Some(DispatchOverride(dispatch));
    sermo
}

#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
#[must_use]
pub fn test_response_sender(route: &str) -> (Sermo, ResponseSender, Cancellation) {
    let sermo = sermo_open(route);
    {
        let mut inner = lock_sermo(&sermo.inner);
        inner.runtime_response_generated = true;
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
        _marker: PhantomData,
    }
}

#[must_use]
pub fn sermo_tuus<T>(sermo: &Sermo) -> Tuus<T> {
    Tuus {
        inner: sermo.inner.clone(),
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
    let mut inner = lock_sermo(&tuus.inner);
    recv_content_frame(&mut inner)
}

/// Lazy inbound content-frame iterator; shares the queue with `tuus_accipe`.
pub struct TuusCursor<T> {
    inner: Arc<SermoShared>,
    _marker: PhantomData<T>,
}

impl<T> Iterator for TuusCursor<T> {
    type Item = Scrinium;

    fn next(&mut self) -> Option<Scrinium> {
        recv_content_frame(&mut lock_sermo(&self.inner))
    }
}

#[must_use]
pub fn tuus_cursor<T>(tuus: &Tuus<T>) -> TuusCursor<T> {
    TuusCursor {
        inner: tuus.inner.clone(),
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
    }
}

fn record_incoming_terminal(inner: &mut SermoInner, status: FrameStatus) {
    inner.incoming_terminal = Some(status);
    inner.incoming_drained = true;
    inner.runtime_cancellation = None;
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

fn recv_content_frame(inner: &mut SermoInner) -> Option<Scrinium> {
    if inner.detached || inner.incoming_drained {
        return None;
    }
    // Content cursors are nonblocking views. Route dispatch is started by the
    // owning `Sermo` receive/materializer path.
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
    if inner.runtime_response_generated {
        return;
    }
    inner.runtime_response_generated = true;
    let request = sermo_request(inner, None);
    let cancellation = Cancellation {
        cancelled: Arc::new(AtomicBool::new(false)),
    };
    inner.runtime_cancellation = Some(cancellation.clone());
    let responses = ResponseSender::new(Arc::clone(shared), cancellation.clone());
    let dispatch = inner
        .host_dispatch
        .as_ref()
        .map(|override_dispatch| Arc::clone(&override_dispatch.0));
    if let Err(error) = start_host_dispatch(request, responses.clone(), cancellation, dispatch) {
        // The caller holds the sermo lock while starting dispatch, and
        // `reject_start_error` enqueues through that same (non-reentrant)
        // lock — deliver the terminal rejection from a separate thread, once
        // the lock is released, instead of deadlocking.
        thread::spawn(move || responses.reject_start_error(error));
    }
}

fn ensure_runtime_response_started_for_type<T>(sermo: &mut Sermo)
where
    T: crate::FromValor,
{
    ensure_runtime_response_started_for_target(sermo, std::any::type_name::<T>());
}

fn ensure_runtime_response_started_for_target(sermo: &mut Sermo, target: &'static str) {
    let mut inner = lock_sermo(&sermo.inner);
    if inner.runtime_response_generated {
        return;
    }
    inner.runtime_response_generated = true;
    let request = sermo_request(&inner, Some(target));
    let cancellation = Cancellation {
        cancelled: Arc::new(AtomicBool::new(false)),
    };
    inner.runtime_cancellation = Some(cancellation.clone());
    let responses = ResponseSender::new(Arc::clone(&sermo.inner), cancellation.clone());
    let dispatch = inner
        .host_dispatch
        .as_ref()
        .map(|override_dispatch| Arc::clone(&override_dispatch.0));
    if let Err(error) = start_host_dispatch(request, responses.clone(), cancellation, dispatch) {
        // The caller holds the sermo lock while starting dispatch, and
        // `reject_start_error` enqueues through that same (non-reentrant)
        // lock — deliver the terminal rejection from a separate thread, once
        // the lock is released, instead of deadlocking.
        thread::spawn(move || responses.reject_start_error(error));
    }
}

fn start_host_dispatch(
    request: SermoRequest,
    responses: ResponseSender,
    cancellation: Cancellation,
    override_dispatch: Option<Arc<dyn HostDispatch>>,
) -> Result<(), DispatchError> {
    // S1-U3 split: concrete built-in effects live in hosts providers. The
    // faber runtime package owns the HostDispatch contract only; with no host
    // dispatch installed, a runtime route fails closed instead of falling
    // back to an in-process builtin implementation.
    if let Some(dispatch) = override_dispatch {
        return dispatch.start(request, responses, cancellation);
    }
    if let Some(dispatch) = HOST_DISPATCH.get() {
        return dispatch.start(request, responses, cancellation);
    }
    Err(DispatchError::new(
        "host_dispatch_unavailable",
        format!("no host dispatch installed for route `{}`", request.route),
    ))
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
/// Materialize `T` from the stream using automatic dispatch by TypeId.
///
/// # Errors
///
/// Returns `Err` if the underlying materializer fails or the internal TypeId
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
/// Materialize `T` from the stream using automatic dispatch by TypeId (async).
///
/// # Errors
///
/// Returns `Err` if the underlying materializer fails or the internal TypeId
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

