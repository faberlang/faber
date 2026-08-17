//! Compiler-owned receiver intrinsic registry.
//!
//! Radix promotes a fixed catalog of `lista`, `tabula`, `copia`, `textus`,
//! `numerus`, `fractus`, `tensor`, `matrix`, `vector`, `intervallum`, and
//! `atomic`
//! methods. Semantic analysis, MIR lowering, and target backends consult this
//! module instead of maintaining parallel string tables.

mod registry;

#[cfg(test)]
#[path = "approximata_test.rs"]
mod approximata_tests;

#[cfg(test)]
#[path = "operator_methods_test.rs"]
mod operator_methods_tests;

pub use registry::{
    IntrinsicMirKind, IntrinsicParamKind, IntrinsicReceiver, IntrinsicReturnKind, MethodIntrinsic, INTRINSICS,
};

use crate::semantic::{Primitive, Type};
use radix_types::{MirAtomicOp, MirCollectionOp};

/// Lookup a promoted intrinsic by receiver family, Faber name, and arity.
#[must_use]
pub fn lookup_method(receiver: IntrinsicReceiver, method: &str, arg_count: usize) -> Option<&'static MethodIntrinsic> {
    INTRINSICS
        .iter()
        .find(|entry| entry.receiver == receiver && entry.faber_name == method && entry.arg_count == arg_count)
}

/// Map a normalized semantic receiver type to intrinsic receiver families.
///
/// `fractus` values may satisfy both [`IntrinsicReceiver::Fractus`] and
/// [`IntrinsicReceiver::Numeric`] rows; callers should try fractus-specific rows
/// before shared numeric rows.
#[must_use]
pub fn receivers_for_type(ty: &Type) -> &'static [IntrinsicReceiver] {
    match ty {
        Type::Array(_) => &[IntrinsicReceiver::Lista],
        // WHY: `octeti` is semantic sugar for `lista<numerus<u8>>`. Routing it to
        // the Lista receiver family gives octeti values every `lista` method.
        // The assignability rule in `semantic/types.rs` makes the two types
        // bidirectionally assignable; this arm makes method dispatch agree.
        Type::Primitive(Primitive::Octeti) => &[IntrinsicReceiver::Lista],
        Type::Map(_, _) => &[IntrinsicReceiver::Tabula],
        Type::Set(_) => &[IntrinsicReceiver::Copia],
        Type::Primitive(Primitive::Textus) => &[IntrinsicReceiver::Textus],
        Type::Primitive(Primitive::Regex) => &[IntrinsicReceiver::Regex],
        Type::Primitive(Primitive::Numerus) => &[IntrinsicReceiver::Numeric],
        Type::Primitive(Primitive::Fractus) => &[IntrinsicReceiver::Fractus, IntrinsicReceiver::Numeric],
        Type::SizedNumeric(Primitive::Fractus, _) => &[IntrinsicReceiver::Fractus, IntrinsicReceiver::Numeric],
        Type::SizedNumeric(Primitive::Numerus, _) => &[IntrinsicReceiver::Numeric],
        Type::ModularWord(_) => &[IntrinsicReceiver::Numeric],
        Type::Tensor(_, _) => &[IntrinsicReceiver::Tensor],
        Type::Matrix(_, _) => &[IntrinsicReceiver::Matrix],
        Type::Vector(_, _) => &[IntrinsicReceiver::Vector],
        Type::Sparsa(_, _) => &[IntrinsicReceiver::Sparsa],
        Type::Intervallum(_) => &[IntrinsicReceiver::Intervallum],
        Type::Atomic(_) => &[IntrinsicReceiver::Atomic],
        _ => &[],
    }
}

/// Resolve a promoted intrinsic from a semantic receiver type.
#[must_use]
pub fn lookup_method_for_type(receiver_ty: &Type, method: &str, arg_count: usize) -> Option<&'static MethodIntrinsic> {
    if matches!(receiver_ty, Type::ModularWord(_)) && !is_modular_word_method(method) {
        return None;
    }
    for family in receivers_for_type(receiver_ty) {
        if let Some(entry) = lookup_method(*family, method, arg_count) {
            return Some(entry);
        }
    }
    None
}

/// Modular words expose only operations with an explicit u32 lowering. Do not
/// let the shared numeric registry admit methods whose Rust implementation
/// would rely on signed/float behavior or an unproven target contract.
fn is_modular_word_method(method: &str) -> bool {
    matches!(
        method,
        "addita"
            | "subtracta"
            | "multiplicata"
            | "divisa"
            | "modulata"
            | "negativa"
            | "adde"
            | "subtrahe"
            | "multiplica"
            | "divida"
            | "modula"
            | "nega"
            | "minor"
            | "maior"
            | "minime"
            | "maxime"
            | "coniuncta"
            | "disiuncta"
            | "alternata"
            | "complementata"
            | "coniunge"
            | "disiunge"
            | "alterna"
            | "complementa"
            | "sinistrata"
            | "dextrata"
            | "sinistra"
            | "dextra"
    )
}

/// Return the MIR collection op for a registry row, when MIR models the method.
#[must_use]
pub fn mir_collection_op(entry: &MethodIntrinsic) -> Option<MirCollectionOp> {
    match entry.mir_kind {
        IntrinsicMirKind::Collection(op) => Some(op),
        IntrinsicMirKind::Atomic(_) => None,
        IntrinsicMirKind::HigherOrderFiltrata
        | IntrinsicMirKind::HigherOrderMap
        | IntrinsicMirKind::HigherOrderReduce
        | IntrinsicMirKind::CodegenOnly => None,
    }
}

/// Return the MIR atomic op for a registry row, when MIR models the method.
#[must_use]
pub fn mir_atomic_op(entry: &MethodIntrinsic) -> Option<MirAtomicOp> {
    match entry.mir_kind {
        IntrinsicMirKind::Atomic(op) => Some(op),
        IntrinsicMirKind::Collection(_)
        | IntrinsicMirKind::HigherOrderFiltrata
        | IntrinsicMirKind::HigherOrderMap
        | IntrinsicMirKind::HigherOrderReduce
        | IntrinsicMirKind::CodegenOnly => None,
    }
}

/// Whether MIR lowers this method through the higher-order lista helpers.
#[must_use]
pub fn mir_is_higher_order_lista(entry: &MethodIntrinsic) -> bool {
    matches!(
        entry.mir_kind,
        IntrinsicMirKind::HigherOrderFiltrata | IntrinsicMirKind::HigherOrderMap | IntrinsicMirKind::HigherOrderReduce
    )
}

/// Imperativus numeric operator methods that mutate a `varia` receiver in place.
#[must_use]
pub fn is_numeric_imperativus_operator(method: &str) -> bool {
    matches!(
        method,
        "adde"
            | "subtrahe"
            | "multiplica"
            | "divida"
            | "modula"
            | "nega"
            | "coniunge"
            | "disiunge"
            | "alterna"
            | "complementa"
            | "dextra"
            | "sinistra"
    )
}

/// Bitwise and shift operator methods (catalog + receiver) — bare `numerus` only in v1.
#[must_use]
pub fn is_numeric_bitwise_operator(method: &str) -> bool {
    matches!(
        method,
        "coniuncta"
            | "coniunge"
            | "disiuncta"
            | "disiunge"
            | "alternata"
            | "alterna"
            | "complementata"
            | "complementa"
            | "dextra"
            | "dextrata"
            | "sinistra"
            | "sinistrata"
    )
}

/// Compiler-owned atomic method names reserved for `atomic<T>` receivers.
#[must_use]
pub fn is_atomic_operator_method(method: &str) -> bool {
    matches!(method, "load" | "store" | "exchange" | "compare_exchange")
}
