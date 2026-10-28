//! Trap contract: the one stderr line and the one exit status of every
//! Faber-raised failure.
//!
//! ## Two kinds of failure
//!
//! 1. **Native failures** (a real target panic, a segfault, an out-of-memory
//!    kill, a Go runtime crash) keep the target's own stderr and exit status,
//!    passed through untouched: Faber cannot know every target's codes.
//! 2. **Faber-raised failures** are checks Faber overlays on the target (an
//!    index out of bounds, a missing key, a failed conversion, an unhandled
//!    failable call, a store that does not fit). They own their whole surface,
//!    identical on every target that adopts this contract: one stderr line and
//!    one sysexits class for what failed.
//!
//! ## The trap line
//!
//! A Faber-raised failure prints exactly one line on stderr, `error: <text>`
//! (see [`trap_line`]), then exits with its class from [`TRAP_TABLE`]. The
//! explicit program abort (`mori`, a failed `adfirma`, an unhandled failable
//! call in `main`) keeps its own `abort: <text>` line and exits with the class
//! its reason selects (see [`abort_exit_class`]). Output written before the
//! failure is kept.
//!
//! When per-issue codes land the trap line becomes `error[F####]: <text>`.
//! The code travels on stderr and never in the exit status: a POSIX exit
//! status is 8 bits, so a code such as 912 would arrive as 144 and collide
//! with real statuses and with the `128+N` signal statuses.
//!
//! ## The exit class
//!
//! The class is a `sysexits.h` value in 64..=77. Faber never emits 126, 127 or
//! `128+N` (those belong to shells and signals), and `process.exit_process(n)`
//! still exits `n` exactly as written.
//!
//! ## The trap text
//!
//! The words of a Faber-raised failure are defined once, in [`text`]. The
//! runner, the clif lowering and the clif runtime all take their message from
//! there, so the same trap reads the same on every lane.
//!
//! KEEP IN SYNC: `runtime/clif/src/lib.rs` takes its class from this table
//! through `exit_trap`; the runner takes it from `RunnerError::exit_status`.
//! The Rust runtime package's `contract::trap` module is a faithful copy of the two line
//! prefixes and the conversion and program-logic classes (the Rust runtime
//! package is standalone and cannot depend on this crate).

use crate::fault_codes::FAULT_CODES;

/// Prefix of the one stderr line of a Faber-raised failure.
pub const TRAP_LINE_PREFIX: &str = "error: ";

/// Prefix of the one stderr line of an explicit program abort.
pub const ABORT_LINE_PREFIX: &str = "abort: ";

/// The stderr line (without the newline) a Faber-raised failure prints.
#[must_use]
pub fn trap_line(text: &str) -> String {
    format!("{TRAP_LINE_PREFIX}{text}")
}

/// The text of each Faber-raised failure that more than one lane raises.
///
/// A lane never spells one of these inline: the runner, the clif lowering and
/// the clif runtime all call into this module, so the trap line is
/// byte-identical across lanes.
pub mod text {
    /// An exact intermediate or a checked operation left its range.
    pub const NUMERUS_OVERFLOW: &str = "numerus overflow";

    /// An index or window bound into a collection that is negative. It is its
    /// own failure, not the collection's out-of-bounds one, on every lane.
    pub const NEGATIVE_INDEX: &str = "expected non-negative numerus index";

    /// A scalar index read of a text that is past the last character.
    pub const TEXT_INDEX_OUT_OF_BOUNDS: &str = "text index out of bounds";

    /// A scalar index read of a list that is past the last item.
    pub const LIST_INDEX_OUT_OF_BOUNDS: &str = "array index out of bounds";

    /// A keyed read of a map (or ledger) whose key is absent.
    pub const MAP_MISSING_KEY: &str = "tabula missing key";

    /// An index into a byte list (`list<b8>`) that is out of range.
    pub const BYTES_INDEX_TRAP: &str = "list<b8> index trap";

    /// A byte list (`list<b8>`) read as an endian-ordered scalar whose length
    /// is not the scalar's width.
    pub const BYTES_ENDIAN_WIDTH_MISMATCH: &str = "list<b8> endian width mismatch";

    /// An endian-ordered scalar conversion of a byte list at a width the
    /// conversion does not cover.
    pub const BYTES_ENDIAN_WIDTH_UNSUPPORTED: &str = "list<b8> endian width unsupported";

    /// A byte list that is not valid UTF-8 converted to text.
    pub const BYTES_TO_TEXT_FAILED: &str = "list<b8> to textus conversion failed";

    /// A byte list that is not ASCII converted to ascii text.
    pub const BYTES_TO_ASCII_FAILED: &str = "list<b8> to ascii conversion failed";

    /// A byte list longer than the capacity of the bounded byte list it is
    /// converted to.
    pub const BYTES_TO_BOUNDED_FAILED: &str = "list<b8> to bounded list<b8> conversion failed";

    /// A byte list whose length does not fill the tensor it is converted to.
    pub const BYTES_TENSOR_LENGTH_MISMATCH: &str = "list<b8> tensor length mismatch";

    /// The window as slice bounds when it lies fully inside `len` items: the
    /// start is not negative, not past the end, and the end is not past `len`.
    /// A window outside is the failure [`list_window_outside`] or
    /// [`text_window_outside`] words.
    #[must_use]
    pub fn window_inside(start: i128, end: i128, len: usize) -> Option<(usize, usize)> {
        if 0 <= start && start <= end && end <= i128::try_from(len).ok()? {
            Some((usize::try_from(start).ok()?, usize::try_from(end).ok()?))
        } else {
            None
        }
    }

    /// A list window `xs[lo‥hi]` that does not lie fully inside the list.
    /// The bounds are the half-open window as signed values (an inclusive end
    /// already advanced by one); `len` is the list length.
    #[must_use]
    pub fn list_window_outside(start: i128, end: i128, len: usize) -> String {
        format!("list range {start}‥{end} outside 0‥{len}")
    }

    /// A text window `s[lo‥hi]` that does not lie fully inside the text, in
    /// characters; same bounds convention as [`list_window_outside`].
    #[must_use]
    pub fn text_window_outside(start: i128, end: i128, len: usize) -> String {
        format!("text range {start}‥{end} outside 0‥{len}")
    }
}

/// What failed, at the grain that decides the exit class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TrapKind {
    /// A program's command-line operand failed validation.
    CliOperand,
    /// A value could not be converted from text, json or valor, or a parse
    /// failed.
    Conversion,
    /// A file or directory was not found.
    NotFound,
    /// A service the program needs is unavailable: a device runtime is
    /// missing, a connection was refused.
    Unavailable,
    /// The program broke its own contract: an index out of bounds, a missing
    /// key, a `nonnull` on none, a store that does not fit, a full bounded
    /// collection, a lockstep length mismatch, a bad range step, an unhandled
    /// failable call. Also the class of a failure that fits no other row.
    ProgramLogic,
    /// The operating system refused to run or spawn something.
    OsSpawn,
    /// A file, socket or other resource could not be created.
    CannotCreate,
    /// Any other I/O failure.
    OtherIo,
    /// An operation timed out or failed in a way worth retrying.
    Timeout,
    /// A peer broke a wire protocol.
    Protocol,
    /// The operation was not permitted.
    Permission,
}

/// One row of [`TRAP_TABLE`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrapRow {
    /// The failure kind.
    pub kind: TrapKind,
    /// The `sysexits.h` exit class.
    pub class: u8,
    /// The `sysexits.h` name of the class.
    pub name: &'static str,
}

/// Every Faber-raised failure kind and its sysexits class, defined once.
// One row per kind; without this rustfmt expands each entry to five lines.
#[rustfmt::skip]
pub const TRAP_TABLE: &[TrapRow] = &[
    TrapRow { kind: TrapKind::CliOperand, class: 64, name: "EX_USAGE" },
    TrapRow { kind: TrapKind::Conversion, class: 65, name: "EX_DATAERR" },
    TrapRow { kind: TrapKind::NotFound, class: 66, name: "EX_NOINPUT" },
    TrapRow { kind: TrapKind::Unavailable, class: 69, name: "EX_UNAVAILABLE" },
    TrapRow { kind: TrapKind::ProgramLogic, class: 70, name: "EX_SOFTWARE" },
    TrapRow { kind: TrapKind::OsSpawn, class: 71, name: "EX_OSERR" },
    TrapRow { kind: TrapKind::CannotCreate, class: 73, name: "EX_CANTCREAT" },
    TrapRow { kind: TrapKind::OtherIo, class: 74, name: "EX_IOERR" },
    TrapRow { kind: TrapKind::Timeout, class: 75, name: "EX_TEMPFAIL" },
    TrapRow { kind: TrapKind::Protocol, class: 76, name: "EX_PROTOCOL" },
    TrapRow { kind: TrapKind::Permission, class: 77, name: "EX_NOPERM" },
];

/// The exit class of the failure kind, taken from [`TRAP_TABLE`] (the unit
/// test pins that the table lists every kind, so the fallback is unreachable).
#[must_use]
pub fn exit_class(kind: TrapKind) -> u8 {
    TRAP_TABLE
        .iter()
        .find(|row| row.kind == kind)
        .map_or(70, |row| row.class)
}

/// The exit class of a failure kind as a process exit status.
#[must_use]
pub fn exit_status(kind: TrapKind) -> i32 {
    i32::from(exit_class(kind))
}

/// The failure kind of an `IoFault` variant, `None` for a name that is not
/// one. The classification is by the variant, never by the message text.
#[must_use]
pub fn io_fault_kind(variant: &str) -> Option<TrapKind> {
    Some(match variant {
        "NotFound" => TrapKind::NotFound,
        "Permission" => TrapKind::Permission,
        "NotADirectory" | "IsADirectory" | "ConnectionReset" | "Io" | "Closed" => TrapKind::OtherIo,
        "AlreadyExists" | "AddrInUse" => TrapKind::CannotCreate,
        "TimedOut" | "Cancelled" => TrapKind::Timeout,
        "ConnectionRefused" | "Unavailable" => TrapKind::Unavailable,
        "InvalidArgument" | "Unset" => TrapKind::ProgramLogic,
        _ => return None,
    })
}

/// The failure kind of a registered fault code (`ERRnnn`), `None` for a code
/// that is not an `IoFault` variant.
#[must_use]
pub fn fault_code_kind(code: &str) -> Option<TrapKind> {
    FAULT_CODES
        .iter()
        .find(|entry| entry.code == code && entry.family == "IoFault")
        .and_then(|entry| io_fault_kind(entry.variant))
}

/// The exit class of an explicit program abort from its reason text.
///
/// An abort whose reason opens with a registered fault code (`ERR001 ...`, the
/// shape of an unhandled compiler-managed fault) takes that fault's class; any
/// other reason is the program's own abort and takes [`TrapKind::ProgramLogic`].
#[must_use]
pub fn abort_exit_class(reason: &str) -> u8 {
    let kind = reason
        .split_once(' ')
        .and_then(|(code, _)| fault_code_kind(code))
        .unwrap_or(TrapKind::ProgramLogic);
    exit_class(kind)
}

/// [`abort_exit_class`] as a process exit status.
#[must_use]
pub fn abort_exit_status(reason: &str) -> i32 {
    i32::from(abort_exit_class(reason))
}

#[cfg(test)]
#[path = "trap_test.rs"]
mod tests;
