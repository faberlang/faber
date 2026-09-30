//! Shared wording of the implicit-store trap message.
//!
//! The MIR runner reports `<value> does not fit in `<ty>` (<position>)` with an
//! optional inference note. Backends that emit the same trap format the value
//! at run time and append this static suffix, so every backend words the
//! message like the runner.

use radix_hir::{HirStoreKind, HirStoreSite};
use std::fmt::Write;

/// The store position phrase used when the site names no slot.
#[must_use]
pub fn store_kind_phrase(kind: HirStoreKind) -> &'static str {
    match kind {
        HirStoreKind::Declaration => "declaration",
        HirStoreKind::Assignment => "assignment",
        HirStoreKind::Argument => "argument",
        HirStoreKind::ParameterDefault => "parameter default",
        HirStoreKind::Return => "return",
        HirStoreKind::Yield => "yield",
        HirStoreKind::Throw => "throw",
        HirStoreKind::Field => "field",
        HirStoreKind::Element => "collection element",
        HirStoreKind::TuplePosition => "tuple position",
        HirStoreKind::ConversionDefault => "conversion default",
        HirStoreKind::AwaitBinding => "await-binding",
    }
}

/// The message after the formatted value: ` does not fit in `<ty>` (<position>)`
/// plus the inference note, and (when `negative_unsigned`) the note that a
/// negative value cannot be stored in an unsigned slot.
#[must_use]
pub fn store_trap_suffix(
    site: &HirStoreSite,
    slot: Option<&str>,
    ty: &str,
    negative_unsigned: bool,
) -> String {
    let position = match (site.kind, slot) {
        (HirStoreKind::Declaration, Some(slot)) => format!("declaration of `{slot}`"),
        (HirStoreKind::Assignment, Some(slot)) => format!("assignment to `{slot}`"),
        (HirStoreKind::Argument, Some(slot)) => format!("argument `{slot}`"),
        (HirStoreKind::Field, Some(slot)) => format!("field `{slot}`"),
        (HirStoreKind::AwaitBinding, Some(slot)) => format!("await-binding `{slot}`"),
        (kind, _) => store_kind_phrase(kind).to_owned(),
    };
    let mut message = format!(" does not fit in `{ty}` ({position})");
    if let Some(origin) = site.inferred_origin() {
        let name = slot.unwrap_or("the slot");
        let _ = write!(
            message,
            "; `{name}` was inferred as `{ty}` from {origin}, declare its type"
        );
    }
    if negative_unsigned {
        message.push_str(" (a negative value cannot be stored in an unsigned slot)");
    }
    message
}
