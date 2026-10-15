//! Genera (goal unit C5): a `class` is a C struct allocated in the arena and
//! always handled by pointer, so every bind aliases (semantics R1).
//!
//! Every genus gets a forward `typedef` before any struct body, so genera may
//! name each other in any order. Members are `m_<name>`: a Faber field may be
//! a C keyword (`default`, `static`). Methods are functions named
//! `g_<Genus>_<method>` that take the receiver after the arena; `self` lowers
//! to a path to the genus's own definition, which the method body binds to
//! `faber_self`.

use crate::Emitter;
use crate::error::unsupported;
use crate::expr::{Operand, Value, c_string};
use crate::helpers::Helper;
use crate::names::Namer;
use crate::ty::{Ty, classify};
use radix_codegen_shared::CodegenError;
use radix_hir::{DefId, HirExpression, HirStruct};
use radix_lexer::Symbol;
use std::fmt::Write as _;

/// A genus the program declares.
pub(crate) struct Genus {
    /// The C type name, `g_<Name>`.
    pub(crate) c_name: String,
    /// The source name, as the runner shows it.
    pub(crate) name: String,
    pub(crate) fields: Vec<Field>,
    /// Method source names and their definitions.
    pub(crate) methods: Vec<(String, DefId)>,
}

/// One instance field.
#[derive(Clone)]
pub(crate) struct Field {
    pub(crate) name: String,
    pub(crate) c_name: String,
    pub(crate) ty: Ty,
    /// The compile-time default (`= value`) a construction falls back to.
    pub(crate) default: Option<HirExpression>,
}

impl Emitter<'_> {
    /// The C spelling of a class.
    pub(crate) fn c_type(&self, ty: Ty) -> Result<String, CodegenError> {
        if let Some(builtin) = ty.builtin_c_type() {
            return Ok(builtin.to_owned());
        }
        let Ty::Genus(def_id) = ty else {
            return Err(unsupported("type without a C spelling"));
        };
        match self.genera.get(&def_id) {
            Some(genus) => Ok(format!("{} *", genus.c_name)),
            None => Err(unsupported("genus outside the program")),
        }
    }

    /// The declaration of `name` with class `ty`. An immutable binding is
    /// `const` at the top: `const int64_t x`, `const char *const s`,
    /// `g_Point *const p` (the pointer is fixed; a genus's fields stay
    /// writable, as Faber's are).
    pub(crate) fn declare(
        &self,
        ty: Ty,
        name: &str,
        mutable: bool,
    ) -> Result<String, CodegenError> {
        let c_type = self.c_type(ty)?;
        Ok(match (c_type.ends_with('*'), mutable) {
            (true, false) => format!("{c_type}const {name}"),
            (true, true) => format!("{c_type}{name}"),
            (false, false) => format!("const {c_type} {name}"),
            (false, true) => format!("{c_type} {name}"),
        })
    }

    /// Record a genus: its C name, its fields, and its method names. Method
    /// signatures are recorded with the functions.
    pub(crate) fn collect_genus(
        &mut self,
        def_id: DefId,
        genus: &HirStruct,
    ) -> Result<(), CodegenError> {
        if !genus.type_params.is_empty() {
            return Err(unsupported("generic genus"));
        }
        if !genus.implements.is_empty() {
            return Err(unsupported("genus implementing a contract"));
        }
        let name = self.names.resolve_symbol(genus.name).to_owned();
        let c_name = self.items_scope.claim(&format!("g_{name}"));
        let mut members = Namer::program_scope();
        let mut fields = Vec::with_capacity(genus.fields.len());
        for field in &genus.fields {
            if field.is_static() {
                return Err(unsupported("const member"));
            }
            if field.sponte {
                return Err(unsupported("optional (sponte) field"));
            }
            if !field.annotations.is_empty() {
                return Err(unsupported("field annotation"));
            }
            let field_name = self.names.resolve_symbol(field.name).to_owned();
            fields.push(Field {
                c_name: members.claim(&format!("m_{field_name}")),
                name: field_name,
                ty: classify(self.types, field.ty)?,
                default: field.init.clone(),
            });
        }
        let mut methods = Vec::with_capacity(genus.methods.len());
        for method in &genus.methods {
            if !method.annotations.is_empty() {
                return Err(unsupported("method annotation"));
            }
            let method_name = self.names.resolve_symbol(method.func.name).to_owned();
            methods.push((method_name, method.def_id));
        }
        self.genera.insert(
            def_id,
            Genus {
                c_name,
                name,
                fields,
                methods,
            },
        );
        Ok(())
    }

    /// The forward `typedef`s and the struct bodies of every genus.
    pub(crate) fn genus_declarations(&self, order: &[DefId]) -> Result<String, CodegenError> {
        let mut out = String::new();
        for def_id in order {
            if let Some(genus) = self.genera.get(def_id) {
                writeln!(out, "typedef struct {0} {0};", genus.c_name).ok();
            }
        }
        if !out.is_empty() {
            out.push('\n');
        }
        for def_id in order {
            let Some(genus) = self.genera.get(def_id) else {
                continue;
            };
            writeln!(out, "struct {} {{", genus.c_name).ok();
            if genus.fields.is_empty() {
                // C forbids an empty struct; the member is never read.
                out.push_str("    char m_unused;\n");
            }
            for field in &genus.fields {
                writeln!(out, "    {};", self.declare(field.ty, &field.c_name, true)?).ok();
            }
            out.push_str("};\n\n");
        }
        Ok(out)
    }

    fn genus(&self, def_id: DefId) -> Result<&Genus, CodegenError> {
        self.genera
            .get(&def_id)
            .ok_or_else(|| unsupported("genus outside the program"))
    }

    /// `{ x = 1, y = 2 }`: one arena allocation, then the given fields in
    /// literal order and every omitted field's default, as statements of one
    /// statement expression, so evaluation is left to right.
    pub(crate) fn construct(
        &mut self,
        def_id: DefId,
        given: &[(Symbol, &HirExpression)],
    ) -> Result<Operand, CodegenError> {
        let (c_name, fields) = {
            let genus = self.genus(def_id)?;
            (genus.c_name.clone(), genus.fields.clone())
        };
        self.support.need(Helper::Arena);
        let target = format!("faber_t{}", self.temps);
        self.temps += 1;
        let mut body = format!("{c_name} *{target} = faber_alloc(faber_a, sizeof *{target}); ");
        let mut effect = false;
        let mut assigned = Vec::with_capacity(fields.len());
        for (symbol, value) in given {
            let name = self.names.resolve_symbol(*symbol).to_owned();
            let value: &HirExpression = value;
            let Some(field) = fields.iter().find(|field| field.name == name) else {
                return Err(unsupported(
                    "construction of a field the genus does not declare",
                ));
            };
            let operand = self.field_value(value, field.ty)?;
            effect |= operand.effect;
            write!(body, "{target}->{} = {}; ", field.c_name, operand.text).ok();
            assigned.push(name);
        }
        for field in &fields {
            if assigned.contains(&field.name) {
                continue;
            }
            let Some(default) = &field.default else {
                return Err(unsupported(
                    "construction that omits a field without a default",
                ));
            };
            let operand = self.field_value(default, field.ty)?;
            write!(body, "{target}->{} = {}; ", field.c_name, operand.text).ok();
        }
        let text = format!("(__extension__ ({{ {body}{target}; }}))");
        Ok(Operand::new(text, Value::Genus(def_id)).with_effect(effect))
    }

    /// An object literal typed as a genus: `{ x = 1, y = 2 }` lowers to a type
    /// ascription carrying its entries.
    pub(crate) fn construct_entries(
        &mut self,
        def_id: DefId,
        entries: &[radix_hir::HirObjectField],
    ) -> Result<Operand, CodegenError> {
        let mut given = Vec::with_capacity(entries.len());
        for entry in entries {
            let radix_hir::HirObjectKey::Ident(symbol) = entry.key else {
                return Err(unsupported("object literal key that is not a field name"));
            };
            let Some(value) = &entry.value else {
                return Err(unsupported("shorthand field in an object literal"));
            };
            given.push((symbol, value));
        }
        self.construct(def_id, &given)
    }

    /// A value stored into a field of class `ty`; an unwrapped exact value
    /// narrows like any unwrapped store.
    fn field_value(&mut self, value: &HirExpression, ty: Ty) -> Result<Operand, CodegenError> {
        let operand = self.expr(value)?;
        let operand = if ty == Ty::Int {
            self.fit(operand)?
        } else {
            operand
        };
        if operand.value != Value::from(ty) {
            return Err(unsupported("value stored without a store check"));
        }
        self.need_class(ty);
        Ok(operand)
    }

    /// The field `name` of the genus `base` holds.
    fn field_of(&self, base: &Operand, name: Symbol) -> Result<Field, CodegenError> {
        let Value::Genus(def_id) = base.value else {
            return Err(unsupported("field access on a value that is not a genus"));
        };
        let name = self.names.resolve_symbol(name);
        self.genus(def_id)?
            .fields
            .iter()
            .find(|field| field.name == name)
            .cloned()
            .ok_or_else(|| unsupported("field the genus does not declare"))
    }

    /// `p.x`.
    pub(crate) fn field_read(
        &mut self,
        base: &HirExpression,
        name: Symbol,
    ) -> Result<Operand, CodegenError> {
        let base = self.expr(base)?;
        let field = self.field_of(&base, name)?;
        let text = format!("({})->{}", base.text, field.c_name);
        // A truth operand carries its own parentheses.
        let text = if field.ty == Ty::Bool {
            format!("({text})")
        } else {
            text
        };
        // A pure read of an `int` field is a 64-bit fast-path leaf (C4).
        let narrow =
            (field.ty == Ty::Int && !base.effect).then(|| crate::expr::Narrow::leaf(&text));
        let mut operand = Operand::new(text, Value::from(field.ty)).with_effect(base.effect);
        operand.narrow = narrow;
        Ok(operand)
    }

    /// `p.x ← value` as a statement line.
    pub(crate) fn field_write(
        &mut self,
        base: &HirExpression,
        name: Symbol,
        value: &HirExpression,
    ) -> Result<String, CodegenError> {
        let base = self.expr(base)?;
        let field = self.field_of(&base, name)?;
        let value = self.field_value(value, field.ty)?;
        // Which side Faber evaluates first is not pinned for this target yet.
        if base.effect && value.effect {
            return Err(unsupported("field assignment with calls on both sides"));
        }
        Ok(format!(
            "({})->{} = {};",
            base.text, field.c_name, value.text
        ))
    }

    /// A method call on a genus value: the method's function with the
    /// receiver after the arena.
    pub(crate) fn genus_method(
        &mut self,
        receiver: Operand,
        method: Symbol,
        args: &[radix_hir::HirCallArg],
    ) -> Result<Operand, CodegenError> {
        let Value::Genus(def_id) = receiver.value else {
            return Err(unsupported("method call on a value that is not a genus"));
        };
        let name = self.names.resolve_symbol(method);
        let Some(method_def) = self
            .genus(def_id)?
            .methods
            .iter()
            .find(|(method_name, _)| method_name == name)
            .map(|(_, def)| *def)
        else {
            return Err(unsupported("method the genus does not declare"));
        };
        self.invoke(method_def, Some(receiver), args)
    }

    /// Ask for the show function of a genus; it is written after the body.
    fn need_show(&mut self, def_id: DefId) {
        if !self.shows.contains(&def_id) {
            self.shows.push(def_id);
        }
    }

    /// How `print` and a template hole show a genus.
    pub(crate) fn show_genus(&mut self, text: &str, def_id: DefId) -> Result<String, CodegenError> {
        self.need_show(def_id);
        Ok(format!(
            "{}_show(faber_a, {text})",
            self.genus(def_id)?.c_name
        ))
    }

    /// The prototypes and definitions of every show function asked for,
    /// including the ones a nested genus field asks for while being written.
    pub(crate) fn show_functions(&mut self) -> Result<(String, String), CodegenError> {
        let mut prototypes = String::new();
        let mut definitions = String::new();
        let mut done = 0;
        while done < self.shows.len() {
            let def_id = self.shows[done];
            done += 1;
            let (c_name, name, fields) = {
                let genus = self.genus(def_id)?;
                (
                    genus.c_name.clone(),
                    genus.name.clone(),
                    genus.fields.clone(),
                )
            };
            let signature = format!(
                "static const char *{c_name}_show(faber_arena *faber_a, const {c_name} *value)"
            );
            prototypes.push_str(&signature);
            prototypes.push_str(";\n");
            // `Point { x = 1, tag = "p" }`, as the runner shows a genus.
            let mut parts = vec![c_string(&format!("{name} {{ "))?];
            for (position, field) in fields.iter().enumerate() {
                let separator = if position == 0 { "" } else { ", " };
                parts.push(c_string(&format!("{separator}{} = ", field.name))?);
                let read = format!("value->{}", field.c_name);
                let shown = self.show_field(&read, field.ty)?;
                parts.push(shown);
            }
            parts.push(c_string(" }")?);
            self.support.need(Helper::Join);
            write!(
                definitions,
                "{signature} {{\n    return faber_join(faber_a, {}, {});\n}}\n\n",
                parts.len(),
                parts.join(", ")
            )
            .ok();
        }
        Ok((prototypes, definitions))
    }

    /// One field as a genus shows it: text quoted, like a list element.
    fn show_field(&mut self, read: &str, ty: Ty) -> Result<String, CodegenError> {
        if ty == Ty::Text {
            self.support.need(Helper::Quoted);
            return Ok(format!("faber_quoted(faber_a, {read})"));
        }
        let read = if ty == Ty::Bool {
            format!("({read})")
        } else {
            read.to_owned()
        };
        let operand = Operand::new(read, Value::from(ty));
        self.show(&operand)
    }
}
