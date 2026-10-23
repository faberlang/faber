//! What a program uses from the Faber C runtime (`runtime/c/faber.h`).
//!
//! The runtime is header-only: every function is `static inline`, so a
//! program that includes it carries no warning for what it does not use, and
//! the emitter writes no runtime text of its own. What it still tracks is
//! whether the program needs the header at all, and whether anything it uses
//! allocates, since only then does every function take the arena.

use crate::ty::Elem;
use std::collections::BTreeSet;

/// One runtime facility the emitted program calls or names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Helper {
    Types,
    Trap,
    Text,
    Arena,
    Concat,
    IntText,
    ExactText,
    Join,
    Quoted,
    List(Elem, ListOp),
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    /// Bitwise `∧ ∨ ⊻ ¬` and the shifts, all exact and range-checked.
    Bitwise,
    /// The integer methods `absolutum`, `signum`, `minimus`, `maximus`, `potentia`.
    IntMethod,
    /// `≈` / `≉` tolerance equality (`approx_int`, `approx_f64`).
    Approx,
    FitI64,
    StoreI64,
    /// `faber_store_<w>` / `faber_fit_<w>` for the sized integers.
    SizedStore,
    AddO,
    SubO,
    MulO,
    FdivI64,
    FmodI64,
    /// Float arithmetic, rounding and printing (no allocation).
    Float,
    /// A float's text in the arena.
    FloatText,
    /// Text operations that allocate their result (`faber_text_trim`, …).
    TextAlloc,
    /// Text operations that only read (`faber_text_length`, …).
    TextRead,
    /// `faber_try_*` and `faber_conv_fail` (no allocation).
    Conv,
    /// The exact integer comparison helpers (`faber_int_lt`, …).
    IntCmp,
    /// The uncapped `faber_*_wide` helpers for modular-word arithmetic.
    Wide,
    /// The exact `d64` decimal helpers (`faber_dec_add`, `faber_dec_store`, …),
    /// which do not allocate.
    Decimal,
    /// `faber_dec_fmt`: a decimal's text in the arena.
    DecimalText,
    /// A list elementwise operator (`faber_list_<e>_<op>_lift_pair`, …),
    /// which allocates its result list.
    ListLift,
    /// `faber_try_ascii_pack`: the fixed-width radix digits, which allocate
    /// their result text.
    AsciiPack,
    /// A declaring leaf header (`runtime/c/<prefix>.h`), written once after
    /// `faber.h` for each prefix a route of the program dispatches to.
    LeafHeader(&'static str),
}

/// One operation of a list family (`faber_list_<elem>_<op>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ListOp {
    Struct,
    New,
    Of,
    Push,
    At,
    Set,
    Show,
}

impl Helper {
    /// True when the facility allocates in the arena it is handed.
    fn allocates(self) -> bool {
        match self {
            Helper::Arena
            | Helper::Concat
            | Helper::IntText
            | Helper::ExactText
            | Helper::Join
            | Helper::Quoted
            | Helper::FloatText
            | Helper::DecimalText
            | Helper::ListLift
            | Helper::AsciiPack
            | Helper::TextAlloc
            | Helper::List(_, ListOp::New | ListOp::Of | ListOp::Push | ListOp::Show) => true,
            Helper::Types
            | Helper::Trap
            | Helper::Text
            | Helper::List(_, ListOp::Struct | ListOp::At | ListOp::Set)
            | Helper::LeafHeader(_)
            | Helper::Add
            | Helper::Sub
            | Helper::Mul
            | Helper::Div
            | Helper::Rem
            | Helper::Bitwise
            | Helper::IntMethod
            | Helper::Approx
            | Helper::FitI64
            | Helper::StoreI64
            | Helper::SizedStore
            | Helper::AddO
            | Helper::SubO
            | Helper::MulO
            | Helper::FdivI64
            | Helper::FmodI64
            | Helper::TextRead
            | Helper::Conv
            | Helper::IntCmp
            | Helper::Wide
            | Helper::Decimal
            | Helper::Float => false,
        }
    }
}

/// The runtime facilities the emitted program uses, filled while emitting.
#[derive(Default)]
pub(crate) struct Support {
    /// The program uses something the header declares: a runtime function,
    /// or a standard name it includes (`int64_t`, `printf`, `bool`).
    used: bool,
    helpers: BTreeSet<Helper>,
}

impl Support {
    /// The program declares or prints a 64-bit integer.
    pub(crate) fn need_ints(&mut self) {
        self.used = true;
    }

    /// The program prints through `printf`.
    pub(crate) fn need_stdio(&mut self) {
        self.used = true;
    }

    /// The program declares a `bool`.
    pub(crate) fn need_bool(&mut self) {
        self.used = true;
    }

    /// The program calls `helper`.
    pub(crate) fn need(&mut self, helper: Helper) {
        self.used = true;
        self.helpers.insert(helper);
    }

    /// The program allocates, so every function carries the arena.
    pub(crate) fn uses_arena(&self) -> bool {
        self.helpers.iter().any(|helper| helper.allocates())
    }

    /// The include line, under a note on how to build the program; empty when
    /// the program needs nothing from the runtime.
    pub(crate) fn text(&self) -> String {
        if !self.used {
            return String::new();
        }
        let mut out = "/* Generated by radix; do not edit. Needs the Faber C runtime header:\n   compile with -I <runtime>/c (runtime/c in the Faber tree). */\n#include \"faber.h\"\n".to_owned();
        // A leaf header declares the `faber_route_*` wrappers a route
        // dispatches to, and needs `faber.h` first.
        for helper in &self.helpers {
            if let Helper::LeafHeader(prefix) = helper {
                out.push_str("#include \"");
                out.push_str(&crate::leaf_routes::route_header(prefix));
                out.push_str("\"\n");
            }
        }
        out.push('\n');
        out
    }
}
