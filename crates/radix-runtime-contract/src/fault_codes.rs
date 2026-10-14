//! Registry of compiler-managed fault codes (`ERRnnn`).
//!
//! A fault code is a stable, locale-independent identity for a compiler-managed
//! fault variant. Programs match on the variant (or on the union), never on the
//! number, and the code is the same in every locale while the variant word is
//! translated.

/// One registered fault variant and the code that identifies it forever.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FaultCode {
    /// Stable `ERRnnn` code, unique across the whole registry.
    pub code: &'static str,
    /// Fault union the variant belongs to (for example `IoFault`).
    pub family: &'static str,
    /// Declared variant name inside the family.
    pub variant: &'static str,
}

/// The flat registry of compiler-managed fault codes.
///
/// Allocation rule: codes are handed out in creation order from one flat space
/// of `ERRnnn` numbers; the number carries no meaning and is never derived from
/// errno or any platform/target number. A lane takes the next free number. A
/// number is fixed once it reaches `main`: never edit it and never reuse it,
/// even if the variant is removed. Removed variants move their code to
/// [`RETIRED_CODES`]. If two lanes collide on one number, the second merger
/// renumbers its own entry.
#[rustfmt::skip]
pub const FAULT_CODES: &[FaultCode] = &[
    FaultCode { code: "ERR001", family: "IoFault", variant: "NotFound" },
    FaultCode { code: "ERR002", family: "IoFault", variant: "Permission" },
    FaultCode { code: "ERR003", family: "IoFault", variant: "NotADirectory" },
    FaultCode { code: "ERR004", family: "IoFault", variant: "IsADirectory" },
    FaultCode { code: "ERR005", family: "IoFault", variant: "InvalidArgument" },
    FaultCode { code: "ERR006", family: "IoFault", variant: "AlreadyExists" },
    FaultCode { code: "ERR007", family: "IoFault", variant: "TimedOut" },
    FaultCode { code: "ERR008", family: "IoFault", variant: "ConnectionRefused" },
    FaultCode { code: "ERR009", family: "IoFault", variant: "ConnectionReset" },
    FaultCode { code: "ERR010", family: "IoFault", variant: "AddrInUse" },
    FaultCode { code: "ERR011", family: "IoFault", variant: "Io" },
    FaultCode { code: "ERR012", family: "IoFault", variant: "Closed" },
    FaultCode { code: "ERR013", family: "IoFault", variant: "Unset" },
    FaultCode { code: "ERR014", family: "IoFault", variant: "Unavailable" },
    FaultCode { code: "ERR015", family: "IoFault", variant: "Cancelled" },
];

/// Codes whose variant was removed. A retired code is never handed out again.
pub const RETIRED_CODES: &[&str] = &[];

/// The code registered for an exact `(family, variant)` pair, `None` otherwise.
#[must_use]
pub fn fault_code(family: &str, variant: &str) -> Option<&'static str> {
    FAULT_CODES
        .iter()
        .find(|entry| entry.family == family && entry.variant == variant)
        .map(|entry| entry.code)
}

#[cfg(test)]
#[path = "fault_codes_test.rs"]
mod tests;
