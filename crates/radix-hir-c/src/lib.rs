//! C HIR codegen backend (leaf crate) — a novelty target.
//!
//! Spike (no goal doc yet). Emits one C translation unit from checked HIR
//! for an integer-and-text subset: `int` and `string` locals, exact integer
//! arithmetic, comparisons and logic, `if` / `else`, `while`, `break` /
//! `continue`, assignment, and `print`.
//! Everything else fails closed with a named "unsupported on target c"
//! diagnostic; the README lists what is supported and rejected.
//!
//! C is the permissive compiler, so this emitter is the strict one: nothing it
//! emits may reach undefined behavior. Integer arithmetic is never C
//! `int64_t` arithmetic: it runs in exact `faber_i128` helpers inside the
//! runner's representation cap and is narrowed once, at the store.
//!
//! The output needs no runtime library. Headers and helper functions are
//! emitted inline, only when the program uses them. The leaf never invokes a C
//! compiler; building and running the output is the harness's job.
//!
//! `radix-module` re-exports this leaf through its codegen surface and owns
//! target dispatch.

mod closure;
mod conversio;
mod enumeration;
mod error;
mod expr;
mod float;
mod genus;
mod helpers;
mod items;
mod leaf_routes;
mod list;
mod map;
mod names;
mod numeric;
mod option;
mod range;
mod set;
mod stmt;
mod switch;
mod text;
mod textops;
mod ty;

#[cfg(test)]
mod bits_test;
#[cfg(test)]
mod bounded_test;
#[cfg(test)]
mod class_const_test;
#[cfg(test)]
mod closure_test;
#[cfg(test)]
mod control_test;
#[cfg(test)]
mod conversio_test;
#[cfg(test)]
mod copy_test;
#[cfg(test)]
mod emit_test;
#[cfg(test)]
mod enumeration_test;
#[cfg(test)]
mod float32_test;
#[cfg(test)]
mod float_math_test;
#[cfg(test)]
mod genus_list_test;
#[cfg(test)]
mod item_test;
#[cfg(test)]
mod leaf_routes_test;
#[cfg(test)]
mod list_test;
#[cfg(test)]
mod map_test;
#[cfg(test)]
mod names_test;
#[cfg(test)]
mod numeric_test;
#[cfg(test)]
mod option_test;
#[cfg(test)]
mod range_test;
#[cfg(test)]
mod reject_test;
#[cfg(test)]
mod set_test;
#[cfg(test)]
mod sized_test;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod textops_test;

use crate::error::unsupported;
use crate::helpers::Support;
use crate::items::{Function, Returns};
use crate::names::Namer;
use crate::ty::{Elem, Ty};
use handroll::{FxHashMap, FxHashSet};
use radix_codegen_shared::names::{DisplayTokens, NameCatalog};
use radix_codegen_shared::{Codegen, CodegenError};
use radix_hir::{DefId, HirModule};
use radix_lexer::Interner;
use radix_types::TypeTable;
use std::fmt::Write as _;

/// C backend output.
#[derive(Debug, Clone)]
pub struct COutput {
    /// Complete generated C translation unit.
    pub code: String,
}

/// Shared context for C emission.
pub struct CCodegen<'a> {
    names: NameCatalog<'a>,
}

impl<'a> CCodegen<'a> {
    /// Build a codegen context from one analyzed unit.
    #[must_use]
    pub fn new(hir: &'a HirModule, interner: &'a Interner) -> Self {
        Self {
            names: NameCatalog::new(hir, interner),
        }
    }

    /// Install the module's output tokens (`true` / `false` in its locale).
    pub fn set_display_tokens(&mut self, tokens: DisplayTokens) {
        self.names.set_display_tokens(tokens);
    }
}

impl Codegen for CCodegen<'_> {
    type Output = COutput;

    fn generate(
        &self,
        hir: &HirModule,
        types: &TypeTable,
        _interner: &Interner,
    ) -> Result<COutput, CodegenError> {
        radix_codegen_shared::reject_hir_errors(hir)?;
        // A program that never allocates keeps signatures without the arena.
        // Whether it allocates is known only after emission, so a program
        // that does is emitted a second time with the arena threaded through.
        let (code, allocates) = self.emit(hir, types, false)?;
        let code = if allocates {
            self.emit(hir, types, true)?.0
        } else {
            code
        };
        Ok(COutput { code })
    }
}

impl CCodegen<'_> {
    fn emit(
        &self,
        hir: &HirModule,
        types: &TypeTable,
        arena: bool,
    ) -> Result<(String, bool), CodegenError> {
        Emitter {
            names: &self.names,
            types,
            scope: Namer::program_scope(),
            items_scope: Namer::program_scope(),
            locals: FxHashMap::default(),
            functions: FxHashMap::default(),
            called: FxHashSet::default(),
            current: None,
            entry_args: None,
            uses_args: false,
            fac_exits: Vec::new(),
            unbounded_literals: false,
            support: Support::default(),
            depth: 0,
            loops: 0,
            returns: Returns::Main,
            temps: 0,
            arena,
            genera: FxHashMap::default(),
            genus_list_elements: FxHashSet::default(),
            shows: Vec::new(),
            copies: Vec::new(),
            generics: FxHashSet::default(),
            variants: FxHashMap::default(),
            equals: Vec::new(),
            ref_params: FxHashSet::default(),
            closures: Vec::new(),
            text_literals: Vec::new(),
            text_literal_index: FxHashMap::default(),
            closure_seq: 0,
            channels: Vec::new(),
            channel_handlers: FxHashSet::default(),
        }
        .program(hir)
    }
}

pub(crate) struct Emitter<'a> {
    names: &'a NameCatalog<'a>,
    types: &'a TypeTable,
    /// Names claimed in the function being written; reset per function.
    scope: Namer,
    /// File-scope names: functions (`f_`) and constants (`k_`).
    items_scope: Namer,
    /// Locals, parameters and module constants, by definition. Definition
    /// ids are unique across the module, so one table serves every function.
    locals: FxHashMap<DefId, Local>,
    functions: FxHashMap<DefId, Function>,
    /// Functions and methods some written call reaches; the rest are marked
    /// unused so `-Wunused-function` stays quiet.
    called: FxHashSet<DefId>,
    /// The function or method whose body is being written; `None` in `main`.
    current: Option<DefId>,
    /// The name the entry gives the program's arguments (`main args xs`).
    entry_args: Option<radix_lexer::Symbol>,
    /// The entry reads its arguments, so `main` takes `argc` and `argv`.
    uses_args: bool,
    /// Plain `fac` blocks being written: the loop depth they opened at, the
    /// label `rumpe` jumps to, and whether any `rumpe` did.
    fac_exits: Vec<(usize, String, bool)>,
    /// A list literal is being written under `∷ list<T>`: it is growable
    /// whatever length its own type has.
    unbounded_literals: bool,
    support: Support,
    /// Nesting depth of the statement being written (a function body is 1).
    depth: usize,
    /// Enclosing loops of the statement being written.
    loops: usize,
    /// What `return` means in the body being written.
    returns: Returns,
    /// Sequencing temporaries claimed so far (`faber_t<n>`).
    temps: u32,
    /// Every function takes the arena as its hidden first parameter
    /// `faber_a`, and `main` owns the root arena.
    arena: bool,
    /// The genera the program declares.
    genera: FxHashMap<DefId, crate::genus::Genus>,
    /// The genera whose list families are required by a concrete list type.
    genus_list_elements: FxHashSet<DefId>,
    /// Genera whose show function the program asked for, in request order.
    shows: Vec<DefId>,
    /// Genera whose deep-copy function (`copy`, R7) the program asked for.
    copies: Vec<DefId>,
    /// Generic functions, skipped until instantiated; a call to one is
    /// refused by name (a call to one nothing wrote contributes nothing).
    generics: FxHashSet<DefId>,
    /// Each enumeration variant: the enumeration and its position.
    variants: FxHashMap<DefId, (DefId, usize)>,
    /// Enumerations whose structural equality function was asked for.
    equals: Vec<DefId>,
    /// `ref`/`mut` scalar parameters: the C storage is a pointer to the
    /// caller's cell, so a body read and a body store go through it, and a
    /// matching call-site marker passes the caller's address. Definition ids
    /// are unique, so the set never needs clearing.
    ref_params: FxHashSet<DefId>,
    /// Non-capturing closures discovered while writing bodies, each waiting for
    /// its synthesized top-level C function ([`closure::PendingClosure`]).
    closures: Vec<closure::PendingClosure>,
    /// Text literals interned as they are met: the escaped C source, the
    /// byte count, and the code-point count of each.
    text_literals: Vec<(String, i64, i64)>,
    /// First index of each distinct literal source in `text_literals`, so a
    /// repeated literal reuses one static instead of adding another.
    text_literal_index: FxHashMap<String, usize>,
    /// How many synthesized closure names have been claimed.
    closure_seq: u32,
    /// Error channel receiving a failure in the active `do`/`catch` body.
    channels: Vec<String>,
    /// Handler labels referenced by emitted throw/call propagation paths.
    channel_handlers: FxHashSet<String>,
}

/// One local, parameter or module constant, by its C identifier.
pub(crate) struct Local {
    pub(crate) name: String,
    pub(crate) ty: Ty,
    /// The value of an immutable integer local with a folded initializer, so a
    /// read of it folds too.
    pub(crate) constant: Option<i128>,
}

impl Emitter<'_> {
    /// The translation unit, and whether it allocates.
    fn program(mut self, hir: &HirModule) -> Result<(String, bool), CodegenError> {
        if hir.entry_is_async || !hir.entry_annotations.is_empty() {
            return Err(unsupported("async entry or entry annotations"));
        }
        self.entry_args = hir.entry_args_name;
        let mut items = self.items(hir)?;
        self.scope = Namer::program_scope();
        self.loops = 0;
        self.returns = Returns::Main;
        self.current = None;
        // Constants built at run time are set before the entry block runs.
        let mut body = items.initializers.clone();
        let entry_exit = hir
            .entry
            .as_ref()
            .and_then(|entry| radix_codegen_shared::entry_exit::entry_exit_expr(entry, self.types));
        if let Some(entry) = &hir.entry {
            if let Some(exit) = entry_exit {
                // The exit expression is the entry block's result, not a
                // statement: run the body first, then return its value to the
                // process as C's `int` status.
                let mut body_entry = entry.clone();
                body_entry.expr = None;
                self.block(&mut body, &body_entry)?;
                let exit_value = self.expr(exit)?;
                let exit_value = self.settle(exit_value, Ty::Int)?;
                if exit_value.value != crate::expr::Value::Int {
                    return Err(unsupported("entry exit value is not an integer"));
                }
                self.depth += 1;
                self.line(&mut body, &format!("return (int)({});", exit_value.text));
                self.depth -= 1;
            } else {
                self.block(&mut body, entry)?;
            }
        }
        // A closure body is written as its own top-level function, so flush
        // the closures discovered by the function bodies and the entry block
        // before the prototypes and functions are assembled.
        let (closure_prototypes, closure_definitions) = self.flush_closures()?;
        // Show functions are asked for while bodies are written, so they are
        // generated last; the support text follows them, since they need
        // helpers too.
        let (show_prototypes, show_definitions) = self.show_functions()?;
        let (equal_prototypes, equal_definitions) = self.equal_functions()?;
        let (copy_prototypes, copy_definitions) = self.copy_functions()?;
        let genus_order = items.genus_order.clone();
        items.genera = self.genus_declarations(&genus_order)?;
        if hir.entry.is_none() {
            self.support.need(crate::helpers::Helper::Trap);
        }
        let mut out = self.support.text();
        // Text literals are interned while the bodies were written; their
        // statics carry the text header, so a literal's value is the `.s`
        // reference with its counts already in place.
        for (index, (source, bytes, cps)) in self.text_literals.iter().enumerate() {
            writeln!(
                out,
                "FABER_TEXT_LITERAL(faber_L{index}, {source}, {bytes}, {cps});"
            )
            .ok();
        }
        out.push_str(&items.genera);
        if !items.constants.is_empty() {
            out.push_str(&items.constants);
            out.push('\n');
        }
        let prototypes = format!(
            "{}{closure_prototypes}{show_prototypes}{equal_prototypes}{copy_prototypes}",
            items.prototypes(&self.called)
        );
        if !prototypes.is_empty() {
            out.push_str(&prototypes);
            out.push('\n');
        }
        out.push_str(&items.definitions(&self.called));
        out.push_str(&closure_definitions);
        out.push_str(&show_definitions);
        out.push_str(&equal_definitions);
        out.push_str(&copy_definitions);
        if self.uses_args {
            out.push_str("int main(int argc, char **argv) {\n");
        } else {
            out.push_str("int main(void) {\n");
        }
        if self.arena {
            // The root arena: `main` owns it and every call chain starts here
            // (operator, 2026-10-14). It lives until the process exits.
            out.push_str("    faber_arena faber_root = {0};\n");
            out.push_str("    faber_arena *const faber_a = &faber_root;\n");
            if !mentions_arena(&body) {
                out.push_str("    (void)faber_a;\n");
            }
        }
        out.push_str(&body);
        if hir.entry.is_none() {
            // A module with no entry block has nothing to run: `faber run`
            // reports it and exits 1, and so does this program.
            out.push_str("    faber_trap(\"no entry function in MIR program\");\n}\n");
        } else {
            out.push_str("    return 0;\n}\n");
        }
        Ok((out, self.support.uses_arena()))
    }

    /// Headers a class needs wherever it is declared.
    pub(crate) fn need_class(&mut self, class: Ty) {
        match class {
            Ty::Int | Ty::Sized(_) | Ty::Modular(_) | Ty::Saturating(_) | Ty::Bits(_) => {
                self.support.need_ints();
            }
            Ty::Opt(inner) => self.need_class(inner.ty()),
            Ty::Bool => self.support.need_bool(),
            Ty::Float | Ty::Float32 => self.support.need(crate::helpers::Helper::Float),
            Ty::Decimal => self.support.need(crate::helpers::Helper::Decimal),
            Ty::Text | Ty::Genus(_) | Ty::None => {}
            Ty::List(elem) | Ty::Set(elem) => {
                self.support.need(crate::helpers::Helper::List(
                    elem,
                    crate::helpers::ListOp::Struct,
                ));
                if let Elem::Genus(def_id) = elem {
                    self.genus_list_elements.insert(def_id);
                }
                if elem == crate::ty::Elem::Bool {
                    self.support.need_bool();
                }
            }
            Ty::Map(k, v) => {
                self.need_class(Ty::List(k));
                self.need_class(Ty::List(v));
            }
            Ty::Func(id) => {
                if let radix_types::Type::Func(sig) = self.types.get(radix_types::TypeId(id)) {
                    for param in &sig.params {
                        if let Ok(class) = crate::ty::classify(self.types, param.ty) {
                            self.need_class(class);
                        }
                    }
                    if let Ok(Some(ret)) = crate::ty::returns(self.types, sig.ret) {
                        self.need_class(ret);
                    }
                }
            }
        }
    }
}

/// True when emitted C reads the hidden arena `faber_a` as an identifier,
/// rather than mentioning it in a literal or as part of another identifier.
pub(crate) fn mentions_arena(text: &str) -> bool {
    mentions_name(text, "faber_a")
}

/// Whether `text` contains `name` as a C identifier token. String literals,
/// character literals and comments do not count as reads of the identifier.
pub(crate) fn mentions_name(text: &str, name: &str) -> bool {
    let bytes = text.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            quote @ (b'\'' | b'"') => {
                at += 1;
                while at < bytes.len() {
                    if bytes[at] == b'\\' {
                        at = (at + 2).min(bytes.len());
                    } else {
                        let closes = bytes[at] == quote;
                        at += 1;
                        if closes {
                            break;
                        }
                    }
                }
            }
            b'/' if bytes.get(at + 1) == Some(&b'/') => {
                at += 2;
                while at < bytes.len() && bytes[at] != b'\n' {
                    at += 1;
                }
            }
            b'/' if bytes.get(at + 1) == Some(&b'*') => {
                at += 2;
                while at + 1 < bytes.len() && bytes[at..at + 2] != *b"*/" {
                    at += 1;
                }
                at = (at + 2).min(bytes.len());
            }
            byte if byte == b'_' || byte.is_ascii_alphabetic() => {
                let start = at;
                at += 1;
                while at < bytes.len() && (bytes[at] == b'_' || bytes[at].is_ascii_alphanumeric()) {
                    at += 1;
                }
                if &text[start..at] == name {
                    return true;
                }
            }
            _ => at += 1,
        }
    }
    false
}
