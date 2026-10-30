//! Conversations over the shared native leaves (`faber-leaf`).
//!
//! A CLIF program is single-threaded and synchronous, so a route is a value:
//! [`Conversation::open`] records the route and the opener args, and
//! `faber_leaf::serve` runs on the first materialization only. The reply, or
//! the fault, is therefore the one the leaf produced once.
//!
//! The C surface is the `__faber_clif_v1_sermo_*` family. A handle is a
//! one-based index into the process-global conversation table; 0 is the
//! fail-closed "no conversation" value. The ABI table in
//! `crates/radix-mir-clif` owns the signatures and export identity, and its
//! `tests/abi_symbols.rs` keeps table and export in agreement.
//!
//! A fault carries the two facts that cross into Faber: the leaf kind (the
//! Norma `IoFault` variant name) and the error path. The platform message
//! never crosses.

use crate::arena::ArenaPointer;
use std::sync::Mutex;

use faber_leaf::{self as leaf, Arg, LeafError, LeafErrorKind, Reply};

/// A run-time opener value.
///
/// The scalar subset a leaf slot reads. A leaf that wants a list, a map or
/// raw bytes is not yet reachable from a CLIF conversation.
#[derive(Debug, Clone, PartialEq)]
pub enum Opener {
    /// `nihil`.
    Nil,
    /// `numerus`.
    Int(i64),
    /// `bivalens`.
    Bool(bool),
    /// `textus`.
    Str(String),
}

/// The leaf argument a scalar opener crosses as.
///
/// The mirror of `runtime/rust`'s `valor_to_arg` over the scalar subset.
fn valor_to_arg(value: &Opener) -> Arg {
    match value {
        Opener::Nil => Arg::Nil,
        Opener::Int(value) => Arg::Int(*value),
        Opener::Bool(value) => Arg::Bool(*value),
        Opener::Str(value) => Arg::Str(value.clone()),
    }
}

/// One failed leaf call, reduced to the two facts that cross into Faber.
///
/// A route no leaf owns fails closed with itself as the path, and a reply of
/// the wrong shape fails closed the same way, so a fault always names the
/// route it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fault {
    kind: LeafErrorKind,
    path: String,
}

impl Fault {
    /// The kind identity.
    #[must_use]
    pub fn kind(&self) -> LeafErrorKind {
        self.kind
    }

    /// The kind as its Norma `IoFault` variant name.
    #[must_use]
    pub fn kind_name(&self) -> &'static str {
        self.kind.variant_name()
    }

    /// The path supplied by the leaf, or the route for a synthesized fault.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// A leaf error as its two crossing facts. The platform text is dropped.
    fn from_leaf(error: &LeafError) -> Self {
        Self {
            kind: error.kind(),
            path: error.path().to_owned(),
        }
    }

    /// A fail-closed fault that names `route` as its path.
    fn closed(kind: LeafErrorKind, route: &str) -> Self {
        Self {
            kind,
            path: route.to_owned(),
        }
    }
}

/// One conversation over a leaf route.
///
/// Opening is lazy: [`Conversation::open`] stores the route and its opener
/// args, and the leaf runs on the first materialization only.
#[derive(Debug, Clone, PartialEq)]
pub struct Conversation {
    route: String,
    opener: Vec<Arg>,
    served: Option<Result<Reply, Fault>>,
}

impl Conversation {
    /// Open the conversation a leaf route names. Nothing is served yet.
    #[must_use]
    pub fn open(route: &str) -> Self {
        Self {
            route: route.to_owned(),
            opener: Vec::new(),
            served: None,
        }
    }

    /// Open with the positional opener values the leaf slot reads.
    #[must_use]
    pub fn with_opener(mut self, opener: &[Opener]) -> Self {
        self.opener = opener.iter().map(valor_to_arg).collect();
        self.served = None;
        self
    }

    /// The route this conversation was opened on.
    #[must_use]
    pub fn route(&self) -> &str {
        &self.route
    }

    /// The reply, serving the leaf on the first call.
    ///
    /// # Errors
    ///
    /// Returns the leaf fault, or a fail-closed fault naming the route when no
    /// leaf owns it.
    pub fn materialize(&mut self) -> Result<&Reply, Fault> {
        if self.served.is_none() {
            let opened = self.open_reply();
            self.served = Some(opened);
        }
        match self.served.as_ref() {
            Some(Ok(reply)) => Ok(reply),
            Some(Err(fault)) => Err(fault.clone()),
            None => Err(Fault::closed(LeafErrorKind::Unavailable, &self.route)),
        }
    }

    /// A unit reply.
    ///
    /// # Errors
    ///
    /// Returns the leaf fault, or a fail-closed fault naming the route when
    /// the reply is not `nihil`.
    pub fn unit(&mut self) -> Result<(), Fault> {
        match self.materialize()? {
            Reply::Unit => Ok(()),
            _ => Err(Fault::closed(LeafErrorKind::InvalidArgument, &self.route)),
        }
    }

    /// A `bivalens` reply.
    ///
    /// # Errors
    ///
    /// Returns the leaf fault, or a fail-closed fault naming the route when
    /// the reply is not `bivalens`.
    pub fn bool(&mut self) -> Result<bool, Fault> {
        match self.materialize()? {
            Reply::Bool(value) => Ok(*value),
            _ => Err(Fault::closed(LeafErrorKind::InvalidArgument, &self.route)),
        }
    }

    /// A `numerus` reply.
    ///
    /// # Errors
    ///
    /// Returns the leaf fault, or a fail-closed fault naming the route when
    /// the reply is not `numerus`.
    pub fn int(&mut self) -> Result<i64, Fault> {
        match self.materialize()? {
            Reply::Int(value) => Ok(*value),
            _ => Err(Fault::closed(LeafErrorKind::InvalidArgument, &self.route)),
        }
    }

    /// A `fractus` reply.
    ///
    /// # Errors
    ///
    /// Returns the leaf fault, or a fail-closed fault naming the route when
    /// the reply is not `fractus`.
    pub fn float(&mut self) -> Result<f64, Fault> {
        match self.materialize()? {
            Reply::Float(value) => Ok(*value),
            _ => Err(Fault::closed(LeafErrorKind::InvalidArgument, &self.route)),
        }
    }

    /// A `textus` reply.
    ///
    /// # Errors
    ///
    /// Returns the leaf fault, or a fail-closed fault naming the route when
    /// the reply is not `textus`.
    pub fn text(&mut self) -> Result<String, Fault> {
        match self.materialize()? {
            Reply::Str(value) => Ok(value.clone()),
            _ => Err(Fault::closed(LeafErrorKind::InvalidArgument, &self.route)),
        }
    }

    /// Serve the route once.
    fn open_reply(&self) -> Result<Reply, Fault> {
        match leaf::serve(&self.route, &self.opener) {
            Some(Ok(reply)) => Ok(reply),
            Some(Err(error)) => Err(Fault::from_leaf(&error)),
            None => Err(Fault::closed(LeafErrorKind::Unavailable, &self.route)),
        }
    }
}

/// One open conversation and the fault it last produced.
//
// The fields are read by the accessor exports of the next unit; the tests use
// them today.
#[allow(dead_code)]
struct Slot {
    conversation: Conversation,
    /// The materialization fault. A later unit records leaf faults here.
    fault: Option<Fault>,
}

/// The process-global conversation table, indexed by its one-based handle.
//
// The table records arena-owned slot addresses, never runtime objects. It is
// cleared when a new program starts so stale handles cannot cross root arenas.
static CONVERSATIONS: Mutex<Vec<usize>> = Mutex::new(Vec::new());

pub(crate) fn reset_conversations() {
    CONVERSATIONS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();
}

/// The slot a one-based handle names.
///
/// `None` for handle 0, a negative handle, or a handle past the table end.
// Read by the accessor exports of the next unit; the tests use it today. The
// table stays a `Vec` so the open export can append to it.
#[allow(dead_code, clippy::ptr_arg, clippy::vec_box)]
fn slot(table: &mut Vec<usize>, handle: i64) -> Option<&mut Slot> {
    let index = usize::try_from(handle).ok()?.checked_sub(1)?;
    let slot = *table.get(index)? as *mut Slot;
    // SAFETY: every entry is installed from a `Slot` allocated by the arena.
    Some(unsafe { &mut *slot })
}

/// Reopen a slot on the opener values a leaf slot reads.
///
/// The route is unchanged, the conversation is unserved again, and the fault
/// a previous call recorded is dropped.
fn reopen(slot: &mut Slot, opener: &[Opener]) {
    slot.conversation = Conversation::open(slot.conversation.route()).with_opener(opener);
    slot.fault = None;
}

/// Open a conversation on `route` and return its one-based handle.
///
/// 0 is the fail-closed "no conversation" value: a null pointer, a negative
/// length, non-UTF-8 bytes, or a route no shared leaf owns.
///
/// # Safety
///
/// When it is not null, `route` must point at `route_len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_sermo_open(
    arena: *const super::Arena,
    route: *const u8,
    route_len: i64,
) -> i64 {
    let Ok(length) = usize::try_from(route_len) else {
        return 0;
    };
    if route.is_null() {
        return 0;
    }
    let bytes = unsafe { std::slice::from_raw_parts(route, length) };
    let Ok(route) = std::str::from_utf8(bytes) else {
        return 0;
    };
    if !leaf::is_route(route) {
        return 0;
    }
    let mut table = CONVERSATIONS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let slot = arena.put(Slot {
        conversation: Conversation::open(route),
        fault: None,
    });
    table.push(slot as usize);
    i64::try_from(table.len()).unwrap_or(0)
}

/// Reopen conversation `handle` with `nihil` as its only opener value.
///
/// An unknown handle is a no-op.
#[unsafe(no_mangle)]
pub extern "C" fn __faber_clif_v1_sermo_set_opener_nil(_arena: *const super::Arena, handle: i64) {
    let mut table = CONVERSATIONS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(open) = slot(&mut table, handle) {
        reopen(open, &[Opener::Nil]);
    }
}

/// Reopen conversation `handle` with the `numerus` `value` as its only opener
/// value.
///
/// An unknown handle is a no-op.
#[unsafe(no_mangle)]
pub extern "C" fn __faber_clif_v1_sermo_set_opener_i64(
    _arena: *const super::Arena,
    handle: i64,
    value: i64,
) {
    let mut table = CONVERSATIONS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(open) = slot(&mut table, handle) {
        reopen(open, &[Opener::Int(value)]);
    }
}

/// Reopen conversation `handle` with the `valor` payload as its only opener
/// value. A null or unsupported value becomes `nihil`. An unknown handle is a
/// no-op.
///
/// # Safety
///
/// `boxed` is null or points to a live `Present`; a `TAG_TEXT` payload must
/// point to a well-formed runtime text.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_sermo_set_opener_valor(
    _arena: *const super::Arena,
    handle: i64,
    boxed: *const super::Present,
) {
    let mut table = CONVERSATIONS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(open) = slot(&mut table, handle) else {
        return;
    };
    let opener = if boxed.is_null() {
        Opener::Nil
    } else {
        let present = unsafe { &*boxed };
        match present.tag {
            super::TAG_INT => Opener::Int(present.bits),
            super::TAG_BOOL => Opener::Bool(present.bits != 0),
            super::TAG_TEXT => Opener::Str(
                unsafe { super::text_str(super::text_bytes(present.bits as *const u8)) }
                    .into_owned(),
            ),
            _ => Opener::Nil,
        }
    };
    reopen(open, &[opener]);
}

/// Materialize conversation `handle` as a `numerus`, or reply `fallback`.
///
/// `fallback` is the bottom-recovery value a caller wants when the leaf
/// cannot produce a number. A successful call clears any recorded fault; a
/// leaf fault is recorded on the slot so the `failed` export reports it. An
/// unknown handle returns `fallback` and records nothing.
#[unsafe(no_mangle)]
pub extern "C" fn __faber_clif_v1_sermo_materialize_i64(
    _arena: *const super::Arena,
    handle: i64,
    fallback: i64,
) -> i64 {
    let mut table = CONVERSATIONS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(open) = slot(&mut table, handle) else {
        return fallback;
    };
    match open.conversation.int() {
        Ok(value) => {
            open.fault = None;
            value
        }
        Err(fault) => {
            open.fault = Some(fault);
            fallback
        }
    }
}

/// Drain conversation `handle` as a raw unit reply. A successful call clears
/// any recorded fault; a leaf fault is recorded on the slot. An unknown handle
/// is a no-op and records nothing.
#[unsafe(no_mangle)]
pub extern "C" fn __faber_clif_v1_sermo_drain(_arena: *const super::Arena, handle: i64) {
    let mut table = CONVERSATIONS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(open) = slot(&mut table, handle) else {
        return;
    };
    match open.conversation.unit() {
        Ok(()) => open.fault = None,
        Err(fault) => open.fault = Some(fault),
    }
}

/// Whether conversation `handle` is in a failed state: 1 when its slot has a
/// recorded fault, or when the handle is unknown (fail closed), else 0.
#[unsafe(no_mangle)]
pub extern "C" fn __faber_clif_v1_sermo_failed(_arena: *const super::Arena, handle: i64) -> i8 {
    let mut table = CONVERSATIONS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match slot(&mut table, handle) {
        Some(open) if open.fault.is_none() => 0,
        _ => 1,
    }
}

/// Fault bytes as a raw (pointer, length) pair.
///
/// The fault strings cross as raw bytes, not as the headered text handle the
/// string exports use: the byte count is written through `out_len` when the
/// caller wants one, and the pointer addresses the slot's own bytes.
fn raw(bytes: &[u8], out_len: Option<&mut i64>) -> *const u8 {
    if let Some(out_len) = out_len {
        *out_len = i64::try_from(bytes.len()).unwrap_or(i64::MAX);
    }
    bytes.as_ptr()
}

/// The recorded fault kind of conversation `handle`, as raw bytes.
///
/// The kind is the Norma `IoFault` variant name. A conversation with no
/// recorded fault, or an unknown handle, reports an empty slice: a non-null
/// pointer with length 0.
///
/// # Safety
///
/// When it is not null, `out_len` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_sermo_fault_kind(
    _arena: *const super::Arena,
    handle: i64,
    out_len: *mut i64,
) -> *const u8 {
    let out_len = unsafe { out_len.as_mut() };
    let mut table = CONVERSATIONS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match slot(&mut table, handle).and_then(|open| open.fault.as_ref()) {
        Some(fault) => raw(fault.kind_name().as_bytes(), out_len),
        None => raw(&[], out_len),
    }
}

/// The recorded fault path of conversation `handle`, as raw bytes.
///
/// A conversation with no recorded fault, or an unknown handle, reports an
/// empty slice: a non-null pointer with length 0.
///
/// # Safety
///
/// When it is not null, `out_len` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __faber_clif_v1_sermo_fault_path(
    _arena: *const super::Arena,
    handle: i64,
    out_len: *mut i64,
) -> *const u8 {
    let out_len = unsafe { out_len.as_mut() };
    let mut table = CONVERSATIONS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match slot(&mut table, handle).and_then(|open| open.fault.as_ref()) {
        Some(fault) => raw(fault.path().as_bytes(), out_len),
        None => raw(&[], out_len),
    }
}

#[cfg(test)]
#[path = "sermo_test.rs"]
mod sermo_test;
