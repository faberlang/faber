//! In-process frame conversation types for expression `ad` and directional views.

use crate::{Instans, InstansPraecisio, Valor};
use std::collections::{BTreeMap, VecDeque};
use std::io::{Read, Seek, SeekFrom};
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError};
use std::task::{Context, Poll, Waker};
use std::thread;
use std::time::Duration;
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
    if request.route.starts_with("runtime:") {
        return BuiltinRuntimeDispatch.start(request, responses, cancellation);
    }
    if let Some(dispatch) = override_dispatch {
        if let Err(error) = dispatch.start(request.clone(), responses.clone(), cancellation.clone())
        {
            return builtin_dispatch_fallback(request, responses, cancellation, error);
        }
        return Ok(());
    }
    if let Some(dispatch) = HOST_DISPATCH.get() {
        if let Err(error) = dispatch.start(request.clone(), responses.clone(), cancellation.clone())
        {
            return builtin_dispatch_fallback(request, responses, cancellation, error);
        }
        return Ok(());
    }
    BuiltinRuntimeDispatch.start(request, responses, cancellation)
}

/// When an installed host rejects a route that the builtin runtime covers
/// (plan-time `is_builtin_ad_route` classification), fall back to the builtin
/// dispatch instead of surfacing the host error. The dual-backend contract:
/// builtin-covered routes (e.g. `processus:exi`) must work without any host,
/// so an installed host that does not manifest them must not shadow delivery.
fn builtin_dispatch_fallback(
    request: SermoRequest,
    responses: ResponseSender,
    cancellation: Cancellation,
    error: DispatchError,
) -> Result<(), DispatchError> {
    if is_builtin_route(&request.route) {
        return BuiltinRuntimeDispatch.start(request, responses, cancellation);
    }
    Err(error)
}

fn sermo_request(inner: &SermoInner, target: Option<&'static str>) -> SermoRequest {
    SermoRequest {
        conversation_id: inner.conversation_id.clone(),
        route: inner.route.clone(),
        opener: request_data(inner),
        target,
    }
}

#[derive(Clone, Copy, Debug)]
struct BuiltinRuntimeDispatch;

impl HostDispatch for BuiltinRuntimeDispatch {
    fn start(
        &self,
        request: SermoRequest,
        responses: ResponseSender,
        _cancellation: Cancellation,
    ) -> Result<(), DispatchError> {
        thread::spawn(move || runtime_dispatch_builtin(request, responses));
        Ok(())
    }
}

/// Pure route-key classification for builtin-runtime coverage — the single
/// source of truth for "is this route builtin-covered". Both the dispatch gate
/// (`builtin_route_match`) and the native-host fallback probe
/// (`is_builtin_route`) route through it, so the coverage cannot drift.
///
/// This MUST remain a pure string-key lookup and must never execute a frame
/// builder. Several builtin arms have real side effects that would fire during
/// a probe — `consolum:lege`/`consolum:leget` block reading stdin,
/// `aleator:fractum` draws from the global seeded RNG, and `aleator:semina`
/// rewrites its state — so a classification probe (run while a rejecting host
/// is installed) must never reach them.
fn builtin_route_key(route: &str) -> bool {
    matches!(
        route,
        "runtime:echo"
            | "tempus:nunc"
            | "tempus:monotonicum"
            | "tempus:activum"
            | "tempus:dormiet"
            | "tempus:expectet"
            | "solum:scribe"
            | "solum:scribet"
            | "solum:appone"
            | "solum:apponet"
            | "solum:funde"
            | "solum:dele"
            | "solum:delet"
            | "solum:parens"
            | "solum:nomen"
            | "solum:suffixum"
            | "solum:iunge"
            | "solum:absolve"
            | "solum:temporarium"
            | "solum:domus"
            | "solum:partem"
            | "processus:exsequi"
            | "processus:exsequetur"
            | "processus:captura"
            | "processus:dimitte"
            | "processus:lege"
            | "processus:scribe"
            | "processus:sedes"
            | "processus:muta"
            | "processus:identitas"
            | "processus:argumenta"
            | "processus:exi"
            | "consolum:dic"
            | "consolum:dicet"
            | "consolum:scribe"
            | "consolum:scribet"
            | "consolum:mone"
            | "consolum:monet"
            | "consolum:vide"
            | "consolum:videbit"
            | "consolum:lege"
            | "consolum:leget"
            | "consolum:hauri"
            | "consolum:hauriet"
            | "consolum:funde"
            | "consolum:audit"
            | "consolum:loquitur"
            | "consolum:admonet"
            | "solum:lege"
            | "solum:hauri"
            | "solum:hauriet"
            | "solum:carpe"
            | "solum:carpiet"
            | "solum:mensura"
            | "solum:inveni"
            | "solum:crea"
            | "solum:creabit"
            | "solum:enumera"
            | "solum:enumerabit"
            | "solum:amputa"
            | "solum:amputabit"
            | "solum:exscribe"
            | "solum:exscribet"
            | "solum:renomina"
            | "solum:renominabit"
            | "solum:tange"
            | "solum:tanget"
            | "solum:sequere"
            | "solum:sequetur"
            | "solum:vincula"
            | "solum:modum"
            | "solum:modus"
            | "solum:exstat"
            | "solum:exstabit"
            | "solum:directoriumne"
            | "solum:regularene"
            | "solum:legibilene"
            | "solum:vinculumne"
            | "aleator:fractum"
            | "aleator:sortire"
            | "aleator:octetos"
            | "aleator:uuid"
            | "aleator:semina"
    )
}

/// Frame sequence for a builtin route; `None` when the route is not covered by
/// the builtin runtime dispatch. Gated by the pure route-key classification so
/// the coverage set is defined in exactly one place (`builtin_route_key`); the
/// arms below execute only for actual dispatch, never for a coverage probe.
fn builtin_route_match(request: &SermoRequest) -> Option<Vec<(FrameStatus, Valor)>> {
    if !builtin_route_key(request.route.as_str()) {
        return None;
    }
    // Single cloned opener: arms need it by value or by reference.
    let opener = request.opener.clone();
    // Freeze: add no provider-surface route here without an explicit dual-backend decision.
    match request.route.as_str() {
        "runtime:echo" => Some(item_done_frames(opener)),
        "tempus:nunc" => {
            let now = epoch_nanos();
            let instans = Instans::from_nanos(now, InstansPraecisio::Nanosecunda);
            Some(item_done_frames(Valor::Instans(instans.to_rfc3339())))
        }
        "tempus:monotonicum" | "tempus:activum" => {
            Some(item_done_frames(Valor::Numerus(elapsed_nanos())))
        }
        "tempus:dormiet" | "tempus:expectet" => {
            let Some(ms) = valor_numerus(&opener) else {
                return Some(error_frames("tempus:dormiet opener must be numerus"));
            };
            if ms < 0 {
                return Some(error_frames("tempus:dormiet ms must be non-negative"));
            }
            thread::sleep(Duration::from_millis(ms.unsigned_abs()));
            Some(done_frames())
        }
        "solum:scribe" | "solum:scribet" | "solum:appone" | "solum:apponet" => {
            Some(solum_write_text_frames(&request.route, opener))
        }
        "solum:funde" => Some(solum_write_bytes_frames(opener)),
        "solum:dele" | "solum:delet" => Some(solum_delete_frames(&opener)),
        "solum:parens" => Some(solum_parent_frames(&opener)),
        "solum:nomen" => Some(solum_file_name_frames(opener)),
        "solum:suffixum" => Some(solum_extension_frames(opener)),
        "solum:iunge" => Some(solum_join_frames(opener)),
        "solum:absolve" => Some(solum_canonicalize_frames(opener)),
        // Zero-arg path facts (host-providers solum + MIR stepper parity).
        "solum:temporarium" => Some(solum_temporarium_frames()),
        "solum:domus" => Some(solum_domus_frames()),
        "solum:partem" => Some(solum_partem_frames(opener, request.target)),
        "processus:exsequi" | "processus:exsequetur" => {
            Some(processus_exsequi_frames(opener))
        }
        "processus:captura" => Some(processus_captura_frames(opener)),
        "processus:dimitte" => Some(processus_dimitte_frames(opener)),
        "processus:lege" => Some(processus_lege_frames(opener)),
        "processus:scribe" => Some(processus_scribe_frames(opener)),
        "processus:sedes" => Some(processus_sedes_frames()),
        "processus:muta" => Some(processus_muta_frames(opener)),
        "processus:identitas" => Some(processus_identitas_frames()),
        "processus:argumenta" => Some(processus_argumenta_frames()),
        "processus:exi" => Some(processus_exi_frames(opener)),
        // Console I/O — product echo/dic without host=native (host-providers consolum parity).
        "consolum:dic" | "consolum:dicet" => {
            Some(consolum_write_stdout_frames(opener, false))
        }
        "consolum:scribe" | "consolum:scribet" => {
            Some(consolum_write_stdout_frames(opener, true))
        }
        "consolum:mone" | "consolum:monet" | "consolum:vide" | "consolum:videbit" => {
            Some(consolum_write_stderr_line_frames(opener))
        }
        "consolum:lege" | "consolum:leget" => Some(consolum_read_line_frames()),
        "consolum:hauri" | "consolum:hauriet" => Some(consolum_read_stdin_frames(&opener)),
        "consolum:funde" => Some(consolum_write_stdout_bytes_frames(opener)),
        "consolum:audit" => Some(consolum_is_terminal_frames(&ConsolumStream::Stdin)),
        "consolum:loquitur" => Some(consolum_is_terminal_frames(&ConsolumStream::Stdout)),
        "consolum:admonet" => Some(consolum_is_terminal_frames(&ConsolumStream::Stderr)),
        "solum:lege" => Some(solum_lege_frames(&opener, request.target)),
        "solum:hauri" | "solum:hauriet" => Some(solum_hauri_frames(&opener)),
        // Line-oriented read: one Item frame per line (for sermo ↦ lista<textus>).
        "solum:carpe" | "solum:carpiet" => Some(solum_carpe_frames(&opener)),
        "solum:mensura" => Some(solum_mensura_frames(&opener, request.target)),
        "solum:inveni" => Some(solum_inveni_frames(opener, request.target)),
        "solum:crea" | "solum:creabit" => Some(solum_create_dir_frames(&opener)),
        "solum:enumera" | "solum:enumerabit" => Some(solum_list_dir_frames(&opener)),
        "solum:amputa" | "solum:amputabit" => Some(solum_remove_dir_frames(&opener)),
        "solum:exscribe" | "solum:exscribet" => Some(solum_copy_frames(opener)),
        "solum:renomina" | "solum:renominabit" => Some(solum_rename_frames(opener)),
        "solum:tange" | "solum:tanget" => Some(solum_touch_frames(&opener)),
        "solum:sequere" | "solum:sequetur" => Some(solum_follow_symlink_frames(&opener)),
        "solum:vincula" => Some(solum_create_symlink_frames(opener)),
        "solum:modum" => Some(solum_set_mode_frames(opener)),
        "solum:modus" => Some(solum_get_mode_frames(&opener)),
        "solum:exstat"
        | "solum:exstabit"
        | "solum:directoriumne"
        | "solum:regularene"
        | "solum:legibilene"
        | "solum:vinculumne" => {
            Some(solum_path_bool_frames(&request.route, &opener, request.target))
        }
        // Random / seed — product packages without host=native (host-providers aleator parity).
        "aleator:fractum" => Some(aleator_fractum_frames()),
        "aleator:sortire" => Some(aleator_sortire_frames(&opener)),
        "aleator:octetos" => Some(aleator_octetos_frames(opener)),
        "aleator:uuid" => Some(aleator_uuid_frames()),
        "aleator:semina" => Some(aleator_semina_frames(opener)),
        _ => None,
    }
}

#[must_use]
pub fn builtin_route_frames(request: SermoRequest) -> Vec<(FrameStatus, Valor)> {
    builtin_route_match(&request)
        .unwrap_or_else(|| error_frames(format!("unsupported ad route `{}`", request.route)))
}

/// Whether the builtin runtime covers the route (mirror of faber's plan-time
/// `is_builtin_ad_route`). This is the native-host fallback probe
/// (`builtin_dispatch_fallback`) and MUST be a pure route-key classification.
///
/// It must not execute frame builders: the old `Nihil`-opener probe ran every
/// covered arm, which blocked on stdin for `consolum:lege` and consumed seeded
/// RNG state for `aleator:fractum` while a rejecting host was installed.
pub(crate) fn is_builtin_route(route: &str) -> bool {
    builtin_route_key(route)
}

pub fn dispatch_builtin_route(request: SermoRequest, responses: ResponseSender) {
    send_response_frames(&responses, builtin_route_frames(request));
}

fn runtime_dispatch_builtin(request: SermoRequest, responses: ResponseSender) {
    dispatch_builtin_route(request, responses);
}

fn send_response_frames(responses: &ResponseSender, frames: Vec<(FrameStatus, Valor)>) {
    for (status, data) in frames {
        let _ = responses.send(status, data);
    }
}

fn item_done_frames(data: Valor) -> Vec<(FrameStatus, Valor)> {
    vec![(FrameStatus::Item, data), (FrameStatus::Done, Valor::Nihil)]
}

fn bytes_done_frames(bytes: Vec<u8>) -> Vec<(FrameStatus, Valor)> {
    vec![
        (FrameStatus::Byte, Valor::Octeti(bytes)),
        (FrameStatus::Done, Valor::Nihil),
    ]
}

fn done_frames() -> Vec<(FrameStatus, Valor)> {
    vec![(FrameStatus::Done, Valor::Nihil)]
}

fn error_frames(message: impl Into<String>) -> Vec<(FrameStatus, Valor)> {
    vec![(FrameStatus::Error, Valor::Textus(message.into()))]
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

fn valor_text(data: &Valor) -> Option<String> {
    match data {
        Valor::Textus(text) => Some(text.clone()),
        _ => None,
    }
}

fn valor_text_list(data: &Valor) -> Option<Vec<String>> {
    let Valor::Lista(items) = data else {
        return None;
    };
    items
        .iter()
        .map(|item| match item {
            Valor::Textus(value) => Some(value.clone()),
            _ => None,
        })
        .collect()
}

fn valor_numerus(data: &Valor) -> Option<i64> {
    match data {
        Valor::Numerus(n) => Some(*n),
        _ => None,
    }
}

fn valor_text_pair(data: Valor) -> Result<(String, String), String> {
    let Valor::Lista(items) = data else {
        return Err("route opener must be [textus, textus]".to_owned());
    };
    let mut iter = items.into_iter();
    let (Some(Valor::Textus(path)), Some(Valor::Textus(value)), None) =
        (iter.next(), iter.next(), iter.next())
    else {
        return Err("route opener must be [textus, textus]".to_owned());
    };
    Ok((path, value))
}

fn valor_text_range(data: Valor) -> Result<(String, i64, i64), String> {
    let Valor::Lista(items) = data else {
        return Err("route opener must be [textus, numerus, numerus]".to_owned());
    };
    let mut iter = items.into_iter();
    let (
        Some(Valor::Textus(path)),
        Some(Valor::Numerus(start)),
        Some(Valor::Numerus(length)),
        None,
    ) = (iter.next(), iter.next(), iter.next(), iter.next())
    else {
        return Err("route opener must be [textus, numerus, numerus]".to_owned());
    };
    Ok((path, start, length))
}

fn valor_text_pattern_range(data: Valor) -> Result<(String, String, i64, i64), String> {
    let Valor::Lista(items) = data else {
        return Err("route opener must be [textus, textus, numerus, numerus]".to_owned());
    };
    let mut iter = items.into_iter();
    let (
        Some(Valor::Textus(path)),
        Some(Valor::Textus(pattern)),
        Some(Valor::Numerus(start)),
        Some(Valor::Numerus(length)),
        None,
    ) = (
        iter.next(),
        iter.next(),
        iter.next(),
        iter.next(),
        iter.next(),
    )
    else {
        return Err("route opener must be [textus, textus, numerus, numerus]".to_owned());
    };
    Ok((path, pattern, start, length))
}

fn solum_write_text_frames(route: &str, data: Valor) -> Vec<(FrameStatus, Valor)> {
    let result = valor_text_pair(data).and_then(|(path, value)| {
        if matches!(route, "solum:appone" | "solum:apponet") {
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .map_err(|err| format!("failed to open file for append: {err}"))?;
            file.write_all(value.as_bytes())
                .map_err(|err| format!("failed to append to file: {err}"))
        } else {
            std::fs::write(&path, value).map_err(|err| format!("failed to write file: {err}"))
        }
    });

    match result {
        Ok(()) => done_frames(),
        Err(message) => error_frames(message),
    }
}

fn solum_parent_frames(data: &Valor) -> Vec<(FrameStatus, Valor)> {
    let Some(path) = valor_text(data) else {
        return error_frames("solum:parens opener must be textus");
    };
    let parent = std::path::Path::new(&path)
        .parent()
        .map(|parent| parent.to_string_lossy().into_owned())
        .unwrap_or_default();
    item_done_frames(Valor::Textus(parent))
}

/// Platform temporary directory as one textus item (`norma:solum.temporarium`).
fn solum_temporarium_frames() -> Vec<(FrameStatus, Valor)> {
    item_done_frames(Valor::Textus(
        std::env::temp_dir().to_string_lossy().into_owned(),
    ))
}

/// User home directory as one textus item (`norma:solum.domus`).
fn solum_domus_frames() -> Vec<(FrameStatus, Valor)> {
    match solum_home_value(
        std::env::var("HOME").ok(),
        std::env::var("USERPROFILE").ok(),
    ) {
        Ok(home) => item_done_frames(Valor::Textus(home)),
        Err(message) => error_frames(format!("solum:domus failed: {message}")),
    }
}

fn solum_home_value(
    home: Option<String>,
    userprofile: Option<String>,
) -> Result<String, &'static str> {
    home.or(userprofile)
        .ok_or("no home directory environment variable")
}

fn solum_delete_frames(data: &Valor) -> Vec<(FrameStatus, Valor)> {
    let Some(path) = valor_text(data) else {
        return error_frames("solum:dele opener must be textus");
    };
    match std::fs::remove_file(&path) {
        Ok(()) => done_frames(),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => done_frames(),
        Err(err) => error_frames(format!("solum.dele failed for {path}: {err}")),
    }
}

fn solum_hauri_frames(data: &Valor) -> Vec<(FrameStatus, Valor)> {
    let Some(path) = valor_text(data) else {
        return error_frames("solum:hauri opener must be textus");
    };
    match std::fs::read(&path) {
        Ok(bytes) => bytes_done_frames(bytes),
        Err(err) => error_frames(format!("solum:hauri failed for {path}: {err}")),
    }
}

fn solum_write_bytes_frames(data: Valor) -> Vec<(FrameStatus, Valor)> {
    let Ok((path, bytes)) = valor_path_bytes(data) else {
        return error_frames("solum:funde opener must be [textus, octeti]");
    };
    match std::fs::write(&path, bytes) {
        Ok(()) => done_frames(),
        Err(err) => error_frames(format!("solum:funde failed for {path}: {err}")),
    }
}

fn solum_create_dir_frames(data: &Valor) -> Vec<(FrameStatus, Valor)> {
    let Some(path) = valor_text(data) else {
        return error_frames("solum:crea opener must be textus");
    };
    match std::fs::create_dir_all(&path) {
        Ok(()) => done_frames(),
        Err(err) => error_frames(format!("solum:crea failed for {path}: {err}")),
    }
}

fn solum_list_dir_frames(data: &Valor) -> Vec<(FrameStatus, Valor)> {
    let Some(path) = valor_text(data) else {
        return error_frames("solum:enumera opener must be textus");
    };
    match std::fs::read_dir(&path) {
        Ok(entries) => {
            let mut names = Vec::new();
            for entry in entries {
                match entry {
                    Ok(entry) => names.push(entry.file_name().to_string_lossy().into_owned()),
                    Err(err) => {
                        return error_frames(format!(
                            "solum:enumera entry failed for {path}: {err}"
                        ));
                    }
                }
            }
            names.sort();
            let mut frames: Vec<(FrameStatus, Valor)> = names
                .into_iter()
                .map(|name| (FrameStatus::Item, Valor::Textus(name)))
                .collect();
            frames.push((FrameStatus::Done, Valor::Nihil));
            frames
        }
        Err(err) => error_frames(format!("solum:enumera failed for {path}: {err}")),
    }
}

fn solum_remove_dir_frames(data: &Valor) -> Vec<(FrameStatus, Valor)> {
    let Some(path) = valor_text(data) else {
        return error_frames("solum:amputa opener must be textus");
    };
    match std::fs::remove_dir_all(&path) {
        Ok(()) => done_frames(),
        Err(err) => error_frames(format!("solum:amputa failed for {path}: {err}")),
    }
}

fn solum_copy_frames(data: Valor) -> Vec<(FrameStatus, Valor)> {
    let Ok((source, destination)) = valor_text_pair(data) else {
        return error_frames("solum:exscribe opener must be [textus, textus]");
    };
    match std::fs::copy(&source, &destination) {
        Ok(_) => done_frames(),
        Err(err) => error_frames(format!("solum:exscribe failed: {err}")),
    }
}

fn solum_rename_frames(data: Valor) -> Vec<(FrameStatus, Valor)> {
    let Ok((source, destination)) = valor_text_pair(data) else {
        return error_frames("solum:renomina opener must be [textus, textus]");
    };
    match std::fs::rename(&source, &destination) {
        Ok(()) => done_frames(),
        Err(err) => error_frames(format!("solum:renomina failed: {err}")),
    }
}

fn solum_touch_frames(data: &Valor) -> Vec<(FrameStatus, Valor)> {
    let Some(path) = valor_text(data) else {
        return error_frames("solum:tange opener must be textus");
    };
    match solum_touch_path(std::path::Path::new(&path)) {
        Ok(()) => done_frames(),
        Err(err) => error_frames(format!("solum:tange failed for {path}: {err}")),
    }
}

fn solum_touch_path(path: &std::path::Path) -> std::io::Result<()> {
    if !path.exists() {
        std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(path)?;
    }
    let handle = std::fs::File::open(path)?;
    let now = std::time::SystemTime::now();
    let times = std::fs::FileTimes::new()
        .set_modified(now)
        .set_accessed(now);
    handle.set_times(times)
}

fn solum_follow_symlink_frames(data: &Valor) -> Vec<(FrameStatus, Valor)> {
    let Some(path) = valor_text(data) else {
        return error_frames("solum:sequere opener must be textus");
    };
    match std::fs::read_link(&path) {
        Ok(target) => item_done_frames(Valor::Textus(target.to_string_lossy().into_owned())),
        Err(err) => error_frames(format!("solum:sequere failed for {path}: {err}")),
    }
}

fn solum_create_symlink_frames(data: Valor) -> Vec<(FrameStatus, Valor)> {
    let Ok((source, destination)) = valor_text_pair(data) else {
        return error_frames("solum:vincula opener must be [textus, textus]");
    };
    match std::os::unix::fs::symlink(&source, &destination) {
        Ok(()) => done_frames(),
        Err(err) => error_frames(format!("solum:vincula failed: {err}")),
    }
}

fn solum_set_mode_frames(data: Valor) -> Vec<(FrameStatus, Valor)> {
    let Ok((path, mode)) = valor_text_numerus_pair(data) else {
        return error_frames("solum:modum opener must be [textus, numerus]");
    };
    if !(0..=0o7777).contains(&mode) {
        return error_frames("solum:modum modus must be between 0 and 0o7777");
    }
    use std::os::unix::fs::PermissionsExt;
    // SAFETY: mode is range-checked to 0..=0o7777 above.
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let mode = mode as u32;
    match std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)) {
        Ok(()) => done_frames(),
        Err(err) => error_frames(format!("solum:modum failed for {path}: {err}")),
    }
}

fn solum_get_mode_frames(data: &Valor) -> Vec<(FrameStatus, Valor)> {
    let Some(path) = valor_text(data) else {
        return error_frames("solum:modus opener must be textus");
    };
    use std::os::unix::fs::PermissionsExt;
    match std::fs::metadata(&path) {
        Ok(meta) => item_done_frames(Valor::Numerus(i64::from(meta.permissions().mode()))),
        Err(err) => error_frames(format!("solum:modus failed for {path}: {err}")),
    }
}

fn valor_text_numerus_pair(data: Valor) -> Result<(String, i64), String> {
    let Valor::Lista(items) = data else {
        return Err("expected lista".to_owned());
    };
    if items.len() != 2 {
        return Err("expected [textus, numerus]".to_owned());
    }
    let path = valor_text(&items[0]).ok_or_else(|| "path must be textus".to_owned())?;
    let mode = valor_numerus(&items[1]).ok_or_else(|| "mode must be numerus".to_owned())?;
    Ok((path, mode))
}

// ---- aleator (host-providers parity; process-local PRNG + urandom) ----

#[derive(Clone, Copy)]
struct AleatorPrng {
    state: u64,
}

impl AleatorPrng {
    fn next_u64(&mut self) -> u64 {
        if self.state == 0 {
            self.state = aleator_default_seed();
        }
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }
}

static ALEATOR_RNG: Mutex<AleatorPrng> = Mutex::new(AleatorPrng { state: 0 });

fn aleator_rng() -> MutexGuard<'static, AleatorPrng> {
    ALEATOR_RNG.lock().unwrap_or_else(PoisonError::into_inner)
}

fn aleator_default_seed() -> u64 {
    // SAFETY: nanoseconds since epoch fit in u64 for the foreseeable future.
    #[allow(clippy::cast_possible_truncation)]
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos() as u64);
    nanos ^ u64::from(std::process::id())
}

fn aleator_fractum_frames() -> Vec<(FrameStatus, Valor)> {
    let bits = aleator_rng().next_u64() >> 11;
    // SAFETY: deliberate f64 fraction from u64 random bits.
    #[allow(clippy::cast_precision_loss)]
    let frac = (bits as f64) / ((1_u64 << 53) as f64);
    item_done_frames(Valor::Fractus(frac))
}

fn aleator_sortire_frames(data: &Valor) -> Vec<(FrameStatus, Valor)> {
    let (min, max) = match &data {
        Valor::Lista(items) if items.len() >= 2 => {
            let min = valor_numerus(&items[0]);
            let max = valor_numerus(&items[1]);
            match (min, max) {
                (Some(a), Some(b)) => (a, b),
                _ => return error_frames("aleator:sortire opener must be [numerus, numerus]"),
            }
        }
        _ => return error_frames("aleator:sortire opener must be [numerus, numerus]"),
    };
    let (lo, hi) = if min <= max { (min, max) } else { (max, min) };
    // SAFETY: all intermediate casts are range-guarded — hi >= lo,
    // span fits in u128, and the modular offset stays within [lo, hi].
    #[allow(
        clippy::cast_sign_loss,
        clippy::cast_possible_wrap,
        clippy::cast_possible_truncation
    )]
    let span: u128 = (i128::from(hi) - i128::from(lo) + 1) as u128;
    if span == 0 {
        return error_frames("aleator:sortire empty range");
    }
    #[allow(clippy::cast_possible_wrap)]
    let offset = (u128::from(aleator_rng().next_u64()) % span) as i128;
    #[allow(clippy::cast_possible_truncation)]
    let result: i64 = (i128::from(lo) + offset) as i64;
    item_done_frames(Valor::Numerus(result))
}

fn aleator_octetos_frames(data: Valor) -> Vec<(FrameStatus, Valor)> {
    use std::io::Read;
    let Some(n) = valor_numerus(&data) else {
        return error_frames("aleator:octetos opener must be numerus");
    };
    let Ok(len) = usize::try_from(n.max(0)) else {
        return error_frames("aleator:octetos n is too large");
    };
    let mut bytes = vec![0_u8; len];
    if len > 0 {
        if let Err(err) =
            std::fs::File::open("/dev/urandom").and_then(|mut file| file.read_exact(&mut bytes))
        {
            return error_frames(format!("aleator:octetos failed: {err}"));
        }
    }
    bytes_done_frames(bytes)
}

fn aleator_uuid_frames() -> Vec<(FrameStatus, Valor)> {
    use std::io::Read;
    let mut bytes = vec![0_u8; 16];
    if let Err(err) =
        std::fs::File::open("/dev/urandom").and_then(|mut file| file.read_exact(&mut bytes))
    {
        return error_frames(format!("aleator:uuid failed: {err}"));
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let uuid = format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15]
    );
    item_done_frames(Valor::Textus(uuid))
}

fn aleator_semina_frames(data: Valor) -> Vec<(FrameStatus, Valor)> {
    let Some(n) = valor_numerus(&data) else {
        return error_frames("aleator:semina opener must be numerus");
    };
    // SAFETY: guarded by n > 0 check.
    #[allow(clippy::cast_sign_loss)]
    let state = n as u64;
    aleator_rng().state = if n > 0 { state } else { aleator_default_seed() };
    done_frames()
}

/// Test-support: run `f` with the global aleator RNG mutex held, passing an
/// exclusive handle to the RNG state. A seeded-RNG probe-purity test can seed,
/// probe, and re-read the real RNG state deterministically — no parallel test
/// can interleave a draw. The handle operates on the live RNG without
/// re-acquiring the mutex (std `Mutex` is non-reentrant: re-locking inside the
/// held section would deadlock). The probe under test (`is_builtin_route`) is
/// pure and never takes the RNG mutex, so holding it across the probe is safe.
#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
pub fn with_aleator_rng_lock<T>(f: impl FnOnce(&mut AleatorRngHandle<'_>) -> T) -> T {
    let rng = aleator_rng();
    f(&mut AleatorRngHandle(rng))
}

/// Test-support: exclusive handle to the global aleator RNG state while its
/// mutex is held (see `with_aleator_rng_lock`).
#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
pub struct AleatorRngHandle<'a>(MutexGuard<'a, AleatorPrng>);

#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
impl AleatorRngHandle<'_> {
    /// Overwrite the seeded RNG state (lock-free: the mutex is already held).
    pub fn seed(&mut self, state: u64) {
        self.0.state = state;
    }

    /// Snapshot the RNG state (lock-free: the mutex is already held).
    pub fn state(&self) -> u64 {
        self.0.state
    }
}

fn solum_file_name_frames(data: Valor) -> Vec<(FrameStatus, Valor)> {
    let Some(path) = valor_text(&data) else {
        return error_frames("solum:nomen opener must be textus");
    };
    let name = std::path::Path::new(&path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    item_done_frames(Valor::Textus(name))
}

fn solum_extension_frames(data: Valor) -> Vec<(FrameStatus, Valor)> {
    let Some(path) = valor_text(&data) else {
        return error_frames("solum:suffixum opener must be textus");
    };
    let ext = std::path::Path::new(&path)
        .extension()
        .map(|extension| format!(".{}", extension.to_string_lossy()))
        .unwrap_or_default();
    item_done_frames(Valor::Textus(ext))
}

fn solum_join_frames(data: Valor) -> Vec<(FrameStatus, Valor)> {
    let Some(parts) = valor_text_list(&data) else {
        return error_frames("solum:iunge opener must be lista<textus>");
    };
    item_done_frames(Valor::Textus(parts.join("/")))
}

fn solum_canonicalize_frames(data: Valor) -> Vec<(FrameStatus, Valor)> {
    let Some(path) = valor_text(&data) else {
        return error_frames("solum:absolve opener must be textus");
    };
    match std::fs::canonicalize(&path) {
        Ok(resolved) => item_done_frames(Valor::Textus(resolved.to_string_lossy().into_owned())),
        Err(err) => error_frames(format!("solum:absolve failed for {path}: {err}")),
    }
}

fn valor_path_bytes(data: Valor) -> Result<(String, Vec<u8>), String> {
    let Valor::Lista(items) = data else {
        return Err("expected lista".to_owned());
    };
    if items.len() != 2 {
        return Err("expected [path, bytes]".to_owned());
    }
    let path = valor_text(&items[0]).ok_or_else(|| "path must be textus".to_owned())?;
    let bytes = match &items[1] {
        Valor::Octeti(bytes) => bytes.clone(),
        Valor::Lista(values) => {
            let mut out = Vec::with_capacity(values.len());
            for value in values {
                let Valor::Numerus(byte) = value else {
                    return Err("byte list must be numerus".to_owned());
                };
                if !(0..=255).contains(byte) {
                    return Err("byte out of range".to_owned());
                }
                // SAFETY: guarded by range check above.
                #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
                out.push(*byte as u8);
            }
            out
        }
        _ => return Err("bytes must be octeti or lista<numerus>".to_owned()),
    };
    Ok((path, bytes))
}

fn processus_exsequi_frames(data: Valor) -> Vec<(FrameStatus, Valor)> {
    let Some(command) = valor_text(&data) else {
        return error_frames("processus:exsequi opener must be textus");
    };
    match std::process::Command::new("sh")
        .arg("-c")
        .arg(&command)
        .output()
    {
        Ok(output) if output.status.success() => item_done_frames(Valor::Textus(
            String::from_utf8_lossy(&output.stdout).into_owned(),
        )),
        Ok(output) => {
            let status = output.status.code().map_or_else(
                || "terminated without exit code".to_owned(),
                |code| format!("exit status {code}"),
            );
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.is_empty() {
                error_frames(format!("processus:exsequi failed with {status}"))
            } else {
                error_frames(format!(
                    "processus:exsequi failed with {status}: stderr: {stderr}"
                ))
            }
        }
        Err(err) => error_frames(format!("processus.exsequi failed: {err}")),
    }
}

fn processus_captura_frames(data: Valor) -> Vec<(FrameStatus, Valor)> {
    let Some(args) = valor_text_list(&data) else {
        return error_frames("processus:captura opener must be lista<textus>");
    };
    let Some((program, program_args)) = args.split_first() else {
        return error_frames("processus:captura requires a non-empty args list");
    };
    match std::process::Command::new(program)
        .args(program_args)
        .output()
    {
        Ok(output) => {
            let mut fields = BTreeMap::new();
            fields.insert(
                "status".to_owned(),
                Valor::Numerus(i64::from(output.status.code().unwrap_or(-1))),
            );
            fields.insert(
                "stdout".to_owned(),
                Valor::Textus(String::from_utf8_lossy(&output.stdout).into_owned()),
            );
            fields.insert(
                "stderr".to_owned(),
                Valor::Textus(String::from_utf8_lossy(&output.stderr).into_owned()),
            );
            item_done_frames(Valor::Tabula(fields))
        }
        Err(err) => error_frames(format!("processus.captura failed: {err}")),
    }
}

/// Detached spawn → pid (host-providers processus:dimitte parity).
fn processus_dimitte_frames(data: Valor) -> Vec<(FrameStatus, Valor)> {
    let Some(args) = valor_text_list(&data) else {
        return error_frames("processus:dimitte opener must be lista<textus>");
    };
    let Some((program, program_args)) = args.split_first() else {
        return error_frames("processus:dimitte requires a non-empty args list");
    };
    match std::process::Command::new(program)
        .args(program_args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(child) => item_done_frames(Valor::Numerus(i64::from(child.id()))),
        Err(err) => error_frames(format!("processus:dimitte failed: {err}")),
    }
}

fn processus_lege_frames(data: Valor) -> Vec<(FrameStatus, Valor)> {
    let Some(name) = valor_text(&data) else {
        return error_frames("processus:lege opener must be textus");
    };
    match std::env::var(&name) {
        Ok(value) => item_done_frames(Valor::Textus(value)),
        Err(_) => error_frames(format!(
            "processus:lege: environment variable `{name}` is not set"
        )),
    }
}

fn processus_scribe_frames(data: Valor) -> Vec<(FrameStatus, Valor)> {
    let Ok((name, value)) = valor_text_pair(data) else {
        return error_frames("processus:scribe opener must be [textus, textus]");
    };
    // SAFETY: set_env is process-local; same contract as host-providers processus.
    unsafe {
        std::env::set_var(&name, &value);
    }
    done_frames()
}

fn processus_sedes_frames() -> Vec<(FrameStatus, Valor)> {
    match std::env::current_dir() {
        Ok(path) => item_done_frames(Valor::Textus(path.to_string_lossy().into_owned())),
        Err(err) => error_frames(format!("processus:sedes failed: {err}")),
    }
}

fn processus_muta_frames(data: Valor) -> Vec<(FrameStatus, Valor)> {
    let Some(path) = valor_text(&data) else {
        return error_frames("processus:muta opener must be textus");
    };
    match std::env::set_current_dir(&path) {
        Ok(()) => done_frames(),
        Err(err) => error_frames(format!("processus:muta failed: {err}")),
    }
}

fn processus_identitas_frames() -> Vec<(FrameStatus, Valor)> {
    item_done_frames(Valor::Numerus(i64::from(std::process::id())))
}

fn processus_argumenta_frames() -> Vec<(FrameStatus, Valor)> {
    let mut frames: Vec<(FrameStatus, Valor)> = std::env::args()
        .skip(1)
        .map(|arg| (FrameStatus::Item, Valor::Textus(arg)))
        .collect();
    frames.push((FrameStatus::Done, Valor::Nihil));
    frames
}

/// Exit process — never returns on success (host-providers processus:exi parity).
fn processus_exi_frames(data: Valor) -> Vec<(FrameStatus, Valor)> {
    let Some(code) = valor_numerus(&data) else {
        return error_frames("processus:exi opener must be numerus");
    };
    // SAFETY: clamped to 0..=255, safe for i32.
    #[allow(clippy::cast_possible_truncation)]
    let code = code.clamp(0, i64::from(u8::MAX)) as i32;
    std::process::exit(code);
}

#[derive(Clone, Copy)]
enum ConsolumStream {
    Stdin,
    Stdout,
    Stderr,
}

/// Write text to stdout; `with_newline` selects scribe vs dic.
fn consolum_write_stdout_frames(data: Valor, with_newline: bool) -> Vec<(FrameStatus, Valor)> {
    let Some(message) = valor_text(&data) else {
        return error_frames("consolum write opener must be textus");
    };
    use std::io::Write;
    let mut stdout = std::io::stdout().lock();
    let result = if with_newline {
        writeln!(stdout, "{message}").and_then(|()| stdout.flush())
    } else {
        write!(stdout, "{message}").and_then(|()| stdout.flush())
    };
    match result {
        Ok(()) => done_frames(),
        Err(err) => error_frames(format!("consolum stdout write failed: {err}")),
    }
}

fn consolum_write_stderr_line_frames(data: Valor) -> Vec<(FrameStatus, Valor)> {
    let Some(message) = valor_text(&data) else {
        return error_frames("consolum stderr opener must be textus");
    };
    use std::io::Write;
    let mut stderr = std::io::stderr().lock();
    match writeln!(stderr, "{message}").and_then(|()| stderr.flush()) {
        Ok(()) => done_frames(),
        Err(err) => error_frames(format!("consolum stderr write failed: {err}")),
    }
}

fn consolum_write_stdout_bytes_frames(data: Valor) -> Vec<(FrameStatus, Valor)> {
    let bytes = match &data {
        Valor::Octeti(bytes) => bytes.clone(),
        Valor::Lista(values) => {
            let mut out = Vec::with_capacity(values.len());
            for value in values {
                let Valor::Numerus(byte) = value else {
                    return error_frames("consolum:funde opener must be octeti");
                };
                if !(0..=255).contains(byte) {
                    return error_frames("consolum:funde byte out of range");
                }
                // SAFETY: guarded by range check above.
                #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
                out.push(*byte as u8);
            }
            out
        }
        _ => return error_frames("consolum:funde opener must be octeti"),
    };
    use std::io::Write;
    let mut stdout = std::io::stdout().lock();
    match stdout.write_all(&bytes).and_then(|()| stdout.flush()) {
        Ok(()) => done_frames(),
        Err(err) => error_frames(format!("consolum:funde failed: {err}")),
    }
}

fn consolum_read_line_frames() -> Vec<(FrameStatus, Valor)> {
    use std::io::BufRead;
    let mut line = String::new();
    match std::io::stdin().lock().read_line(&mut line) {
        Ok(_) => {
            if line.ends_with("\r\n") {
                line.truncate(line.len() - 2);
            } else if line.ends_with('\n') {
                line.truncate(line.len() - 1);
            }
            item_done_frames(Valor::Textus(line))
        }
        Err(err) => error_frames(format!("consolum:lege failed: {err}")),
    }
}

fn consolum_read_stdin_frames(data: &Valor) -> Vec<(FrameStatus, Valor)> {
    use std::io::Read;
    let Some(magnitude) = valor_numerus(data) else {
        return error_frames("consolum:hauri opener must be numerus");
    };
    // SAFETY: magnitude is clamped to >= 0.
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let magnitude = magnitude.max(0) as usize;
    let mut buffer = vec![0_u8; magnitude];
    match std::io::stdin().lock().read(&mut buffer) {
        Ok(n) => {
            buffer.truncate(n);
            bytes_done_frames(buffer)
        }
        Err(err) => error_frames(format!("consolum:hauri failed: {err}")),
    }
}

fn consolum_is_terminal_frames(stream: &ConsolumStream) -> Vec<(FrameStatus, Valor)> {
    use std::io::IsTerminal;
    let is_tty = match stream {
        ConsolumStream::Stdin => std::io::stdin().is_terminal(),
        ConsolumStream::Stdout => std::io::stdout().is_terminal(),
        ConsolumStream::Stderr => std::io::stderr().is_terminal(),
    };
    item_done_frames(Valor::Bivalens(is_tty))
}

/// One Item per line + Done — frame shape for `try_sermo_materialize_lista::<String>`.
/// Shared by `solum:carpe` and `solum:lege` → `lista<textus>` (not a single Lista item).
fn solum_line_item_frames(path: &str, read_err_prefix: &str) -> Vec<(FrameStatus, Valor)> {
    match std::fs::read_to_string(path) {
        Ok(text) => {
            let mut frames: Vec<(FrameStatus, Valor)> = text
                .lines()
                .map(|line| (FrameStatus::Item, Valor::Textus(line.to_owned())))
                .collect();
            frames.push((FrameStatus::Done, Valor::Nihil));
            frames
        }
        Err(err) => error_frames(format!("{read_err_prefix}: {err}")),
    }
}

/// Read a file as text lines for `norma:solum.carpe` / `solum:carpe`.
fn solum_carpe_frames(data: &Valor) -> Vec<(FrameStatus, Valor)> {
    let Some(path) = valor_text(data) else {
        return error_frames("solum:carpe opener must be textus");
    };
    solum_line_item_frames(&path, &format!("solum:carpe failed for {path}"))
}

fn solum_lege_frames(data: &Valor, target: Option<&'static str>) -> Vec<(FrameStatus, Valor)> {
    let Some(path) = valor_text(data) else {
        return error_frames("solum:lege opener must be textus");
    };
    let Some(target) = target else {
        return error_frames("solum:lege requires a materialization target");
    };
    if target == std::any::type_name::<String>() {
        return match std::fs::read_to_string(&path) {
            Ok(text) => item_done_frames(Valor::Textus(text)),
            Err(err) => error_frames(format!("failed to read file: {err}")),
        };
    }
    // Codegen: `try_sermo_materialize_lista` for `↦ lista<textus>` (carpe shape).
    if target == std::any::type_name::<Vec<String>>() {
        return solum_line_item_frames(&path, "failed to read file");
    }
    if target == std::any::type_name::<Vec<u8>>() {
        return match std::fs::read(&path) {
            Ok(bytes) => item_done_frames(Valor::Lista(
                bytes
                    .into_iter()
                    .map(|byte| Valor::Numerus(i64::from(byte)))
                    .collect(),
            )),
            Err(err) => error_frames(format!("failed to read file: {err}")),
        };
    }
    error_frames(format!("solum:lege target `{target}` is not supported"))
}

fn solum_partem_frames(data: Valor, target: Option<&'static str>) -> Vec<(FrameStatus, Valor)> {
    let Ok((path, start, length)) = valor_text_range(data) else {
        return error_frames("solum:partem opener must be [textus, numerus, numerus]");
    };
    if target.is_some_and(|name| name != std::any::type_name::<Vec<u8>>()) {
        return error_frames(format!(
            "solum:partem target `{}` is not supported",
            target.unwrap_or("<unknown>")
        ));
    }
    let Ok(start) = u64::try_from(start) else {
        return error_frames("solum:partem start must be non-negative");
    };
    let Ok(length) = usize::try_from(length) else {
        return error_frames("solum:partem length must be non-negative");
    };
    match std::fs::File::open(&path) {
        Ok(mut file) => {
            if let Err(err) = file.seek(SeekFrom::Start(start)) {
                return error_frames(format!("failed to seek file: {err}"));
            }
            let mut bytes = vec![0_u8; length];
            match file.read(&mut bytes) {
                Ok(count) => {
                    bytes.truncate(count);
                    bytes_done_frames(bytes)
                }
                Err(err) => error_frames(format!("failed to read file: {err}")),
            }
        }
        Err(err) => error_frames(format!("failed to open file: {err}")),
    }
}

fn solum_mensura_frames(data: &Valor, target: Option<&'static str>) -> Vec<(FrameStatus, Valor)> {
    let Some(path) = valor_text(data) else {
        return error_frames("solum:mensura opener must be textus");
    };
    if target != Some(std::any::type_name::<i64>()) {
        return error_frames(format!(
            "solum:mensura target `{}` is not supported",
            target.unwrap_or("<unknown>")
        ));
    }
    match std::fs::metadata(path) {
        Ok(metadata) => match i64::try_from(metadata.len()) {
            Ok(size) => item_done_frames(Valor::Numerus(size)),
            Err(_) => error_frames("file size exceeds numerus range"),
        },
        Err(err) => error_frames(format!("failed to read file metadata: {err}")),
    }
}

fn solum_inveni_frames(data: Valor, target: Option<&'static str>) -> Vec<(FrameStatus, Valor)> {
    let Ok((path, pattern, start, length)) = valor_text_pattern_range(data) else {
        return error_frames("solum:inveni opener must be [textus, textus, numerus, numerus]");
    };
    if target != Some(std::any::type_name::<i64>()) {
        return error_frames(format!(
            "solum:inveni target `{}` is not supported",
            target.unwrap_or("<unknown>")
        ));
    }
    let Ok(start) = u64::try_from(start) else {
        return error_frames("solum:inveni start must be non-negative");
    };
    let Ok(length) = usize::try_from(length) else {
        return error_frames("solum:inveni length must be non-negative");
    };
    let pattern = pattern.into_bytes();
    // Parity with host-providers solum: empty needle is found at `start`.
    if pattern.is_empty() {
        let offset = i64::try_from(start).unwrap_or(i64::MAX);
        return item_done_frames(Valor::Numerus(offset));
    }
    match std::fs::File::open(&path) {
        Ok(mut file) => {
            if let Err(err) = file.seek(SeekFrom::Start(start)) {
                return error_frames(format!("failed to seek file: {err}"));
            }
            let mut bytes = vec![0_u8; length];
            match file.read(&mut bytes) {
                Ok(count) => {
                    bytes.truncate(count);
                    let found = bytes
                        .windows(pattern.len())
                        .position(|window| window == pattern.as_slice())
                        .and_then(|offset| i64::try_from(start.saturating_add(offset as u64)).ok())
                        .unwrap_or(-1);
                    item_done_frames(Valor::Numerus(found))
                }
                Err(err) => error_frames(format!("failed to read file: {err}")),
            }
        }
        Err(err) => error_frames(format!("failed to open file: {err}")),
    }
}

fn solum_path_bool_frames(
    route: &str,
    data: &Valor,
    target: Option<&'static str>,
) -> Vec<(FrameStatus, Valor)> {
    let Some(path) = valor_text(data) else {
        return error_frames(format!("{route} opener must be textus"));
    };
    if target.is_some_and(|name| name != std::any::type_name::<bool>()) {
        return error_frames(format!(
            "{route} target `{}` is not supported",
            target.unwrap_or("<unknown>")
        ));
    }
    let path = std::path::Path::new(&path);
    let result = match route {
        "solum:exstat" | "solum:exstabit" => path.exists(),
        "solum:directoriumne" => path.is_dir(),
        "solum:regularene" => path.is_file(),
        "solum:legibilene" => path.is_file() && std::fs::File::open(path).is_ok(),
        "solum:vinculumne" => std::fs::symlink_metadata(path)
            .map(|metadata| metadata.file_type().is_symlink())
            .unwrap_or(false),
        _ => false,
    };
    item_done_frames(Valor::Bivalens(result))
}

fn ensure_scalar_runtime_response<T>(sermo: &mut Sermo)
where
    T: crate::FromValor,
{
    ensure_runtime_response_started_for_type::<T>(sermo);
}

fn epoch_nanos() -> i64 {
    // SAFETY: clamped to i64::MAX before narrowing cast.
    #[allow(clippy::cast_possible_truncation)]
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            duration.as_nanos().min(i64::MAX as u128) as i64
        });
    nanos
}

fn elapsed_nanos() -> i64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    let start = START.get_or_init(std::time::Instant::now);
    // SAFETY: clamped to i64::MAX before narrowing cast.
    #[allow(clippy::cast_possible_truncation)]
    let nanos = start.elapsed().as_nanos().min(i64::MAX as u128) as i64;
    nanos
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

#[cfg(test)]
#[path = "frame_private_test.rs"]
mod frame_private_tests;
