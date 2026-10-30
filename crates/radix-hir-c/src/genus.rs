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
use crate::expr::{Operand, Value};
use crate::helpers::{Helper, ListOp};
use crate::names::Namer;
use crate::ty::{Elem, Inner, Ty, classify, returns};
use handroll::FxHashSet;
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
    /// Type-level constants (`const T X = e`), by source name. They belong to
    /// the type and are read through it (`Color.RUBER`), never from an
    /// instance; `c_name` is the file-scope C name.
    pub(crate) consts: Vec<(String, ClassConst)>,
    /// Method source names and their definitions.
    pub(crate) methods: Vec<(String, DefId)>,
    pub(crate) shape: Shape,
}

/// One type-level constant of a genus.
#[derive(Clone)]
pub(crate) struct ClassConst {
    pub(crate) c_name: String,
    pub(crate) ty: Ty,
    pub(crate) value: HirExpression,
}

/// What kind of declaration a [`Genus`] record stands for.
#[derive(Clone)]
pub(crate) enum Shape {
    /// A `class`: fields and methods.
    Class,
    /// An `ordo` (`ordo`) or a tagged union: variants with a tag each.
    Enum { variants: Vec<Variant> },
}

/// One variant of an enumeration.
#[derive(Clone)]
pub(crate) struct Variant {
    pub(crate) def_id: DefId,
    pub(crate) name: String,
    /// The variant's position in its declaration.
    pub(crate) tag: i64,
    /// The static object a payload-less value points at.
    pub(crate) static_name: String,
    pub(crate) fields: Vec<Field>,
}

/// One instance field.
#[derive(Clone)]
pub(crate) struct Field {
    pub(crate) name: String,
    pub(crate) c_name: String,
    pub(crate) ty: Ty,
    /// The compile-time default (`= value`) a construction falls back to.
    pub(crate) default: Option<HirExpression>,
    /// An `optional` field: its class is the optional of the declared type
    /// and a construction that omits it leaves none.
    pub(crate) sponte: bool,
}

impl Emitter<'_> {
    /// The element's C spelling, using the program's collected genus names.
    pub(crate) fn elem_c_type(&self, elem: Elem) -> Result<String, CodegenError> {
        Ok(match elem {
            Elem::Int => "int64_t".to_owned(),
            Elem::Text => "faber_text".to_owned(),
            Elem::Bool => "bool".to_owned(),
            Elem::Float => "double".to_owned(),
            Elem::Float32 => "float".to_owned(),
            Elem::Modular(width) => width.c_type().to_owned(),
            Elem::Bits(width) => crate::ty::int_width_of_bits(width).c_type().to_owned(),
            Elem::Genus(def_id) => {
                let genus = self
                    .genera
                    .get(&def_id)
                    .ok_or_else(|| unsupported("genus list element outside the program"))?;
                format!("faber_elem_{}", genus.c_name)
            }
        })
    }

    /// The family suffix, including the collected C name for a genus.
    pub(crate) fn elem_suffix(&self, elem: Elem) -> Result<String, CodegenError> {
        Ok(match elem {
            Elem::Int => "i64".to_owned(),
            Elem::Text => "text".to_owned(),
            Elem::Bool => "bool".to_owned(),
            Elem::Float => "f64".to_owned(),
            Elem::Float32 => "f32".to_owned(),
            Elem::Modular(width) => width.marker().to_owned(),
            Elem::Bits(width) => width.faber_marker().to_owned(),
            Elem::Genus(def_id) => self
                .genera
                .get(&def_id)
                .map(|genus| genus.c_name.clone())
                .ok_or_else(|| unsupported("genus list element outside the program"))?,
        })
    }

    /// The pointer type of a family whose spelling may depend on a genus.
    pub(crate) fn elem_list_c_pointer(&self, elem: Elem) -> Result<String, CodegenError> {
        Ok(format!("faber_list_{} *", self.elem_suffix(elem)?))
    }

    /// The C spelling of a class.
    pub(crate) fn c_type(&self, ty: Ty) -> Result<String, CodegenError> {
        if let Ty::List(elem) | Ty::Set(elem) = ty {
            return self.elem_list_c_pointer(elem);
        }
        if let Ty::Opt(inner) = ty {
            if let Some(scalar) = inner.scalar_struct() {
                return Ok(scalar.to_owned());
            }
            return self.c_type(inner.ty());
        }
        if let Ty::Func(id) = ty {
            return self.func_c_type(id);
        }
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

    /// `ret (*)(faber_arena *, params…)`: a function value is a C function
    /// pointer. Every generated function takes the hidden arena first when the
    /// program allocates, so the pointer type carries it too.
    fn func_c_type(&self, id: u32) -> Result<String, CodegenError> {
        let radix_types::Type::Func(sig) = self.types.get(radix_types::TypeId(id)) else {
            return Err(unsupported("function value without a signature"));
        };
        let mut slots = Vec::with_capacity(sig.params.len() + 1);
        if self.arena {
            slots.push("faber_arena *".to_owned());
        }
        for param in &sig.params {
            let class = classify(self.types, param.ty)?;
            slots.push(self.c_type(class)?);
        }
        let ret = returns(self.types, sig.ret)?;
        let ret_text = match ret {
            Some(ty) => self.c_type(ty)?,
            None => "void".to_owned(),
        };
        Ok(format!("{ret_text} (*)({})", slots.join(", ")))
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
        // A function pointer names its parameter inside the star: `ret
        // (*name)(…)`.
        if let Ty::Func(_) = ty {
            let full = self.c_type(ty)?;
            return Ok(full.replacen("(*)", &format!("(*{name})"), 1));
        }
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
        let name = self.names.resolve_symbol(genus.name).to_owned();
        let c_name = self.items_scope.claim(&format!("g_{name}"));
        let mut members = Namer::program_scope();
        let mut fields = Vec::with_capacity(genus.fields.len());
        let mut consts = Vec::new();
        for field in &genus.fields {
            if field
                .annotations
                .iter()
                .any(|a| !self.ignorable_annotation(a))
            {
                return Err(unsupported("field annotation"));
            }
            if field.is_static() {
                // A type-level constant: `const T X = e`, read through the
                // type name. It is never an instance member; its value is a
                // folded compile-time fact.
                let Some(init) = &field.init else {
                    return Err(unsupported("class constant without a value"));
                };
                let field_name = self.names.resolve_symbol(field.name).to_owned();
                let ty = classify(self.types, field.ty)?;
                let c_name = self.items_scope.claim(&format!("k_{name}_{field_name}"));
                consts.push((
                    field_name,
                    ClassConst {
                        c_name,
                        ty,
                        value: init.clone(),
                    },
                ));
                continue;
            }
            let field_name = self.names.resolve_symbol(field.name).to_owned();
            let declared = classify(self.types, field.ty)?;
            let ty = if field.sponte {
                match crate::ty::Inner::from_ty(declared) {
                    Some(inner) => Ty::Opt(inner),
                    None => {
                        return Err(unsupported("optional field of a sized integer or optional"));
                    }
                }
            } else {
                declared
            };
            if matches!(
                ty,
                Ty::List(Elem::Genus(_)) | Ty::Opt(Inner::List(Elem::Genus(_)))
            ) {
                self.need_class(ty);
            }
            fields.push(Field {
                c_name: members.claim(&format!("m_{field_name}")),
                name: field_name,
                ty,
                default: field.init.clone(),
                sponte: field.sponte,
            });
        }
        let mut methods = Vec::with_capacity(genus.methods.len());
        for method in &genus.methods {
            if method
                .annotations
                .iter()
                .any(|a| !self.ignorable_annotation(a))
            {
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
                consts,
                methods,
                shape: Shape::Class,
            },
        );
        Ok(())
    }

    /// The forward `typedef`s and the struct bodies of every genus.
    pub(crate) fn genus_declarations(&mut self, order: &[DefId]) -> Result<String, CodegenError> {
        let mut out = String::new();
        for def_id in order {
            if let Some(genus) = self.genera.get(def_id) {
                writeln!(out, "typedef struct {0} {0};", genus.c_name).ok();
            }
        }
        for def_id in order {
            if self.genus_list_elements.contains(def_id)
                && let Some(genus) = self.genera.get(def_id)
            {
                self.support.need(crate::helpers::Helper::List(
                    Elem::Genus(*def_id),
                    ListOp::Struct,
                ));
                writeln!(out, "typedef {0} *faber_elem_{0};", genus.c_name).ok();
                // The element is a pointer, so its typed zero is NULL: a
                // bounded genus list's spare cells hold a null element.
                writeln!(out, "FABER_LIST_FAMILY({0}, faber_elem_{0}, NULL)", genus.c_name)
                    .ok();
            }
        }
        if !out.is_empty() {
            out.push('\n');
        }
        for def_id in order {
            let Some(genus) = self.genera.get(def_id) else {
                continue;
            };
            if matches!(genus.shape, Shape::Enum { .. }) {
                out.push_str(&self.enum_body(genus)?);
                continue;
            }
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
        base: Option<&HirExpression>,
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
        let mut assigned = FxHashSet::default();
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
            assigned.insert(name);
        }
        for field in &fields {
            if assigned.contains(&field.name) {
                continue;
            }
            if base.is_some() {
                // Copy-with-changes: an omitted field is a shallow copy of the
                // base field (pointers alias, scalars copy), so the default is
                // never evaluated.
                continue;
            }
            let operand = match (&field.default, field.sponte) {
                (Some(default), _) => self.field_value(default, field.ty)?,
                (None, true) => self.none_of(field.ty)?,
                (None, false) => {
                    return Err(unsupported(
                        "construction that omits a field without a default",
                    ));
                }
            };
            write!(body, "{target}->{} = {}; ", field.c_name, operand.text).ok();
        }
        if let Some(base) = base {
            let base_operand = self.expr(base)?;
            effect |= base_operand.effect;
            let base_target = format!("faber_t{}", self.temps);
            self.temps += 1;
            write!(body, "{c_name} *{base_target} = {}; ", base_operand.text).ok();
            for field in &fields {
                if assigned.contains(&field.name) {
                    continue;
                }
                write!(
                    body,
                    "{target}->{} = {base_target}->{}; ",
                    field.c_name, field.c_name
                )
                .ok();
            }
        }
        // The `creo` hook runs once on the finished object, after every field
        // is set (the runner's construction does the same).
        let hook = self
            .genus(def_id)?
            .methods
            .iter()
            .find(|(name, _)| name == "creo")
            .map(|(_, def)| *def);
        if let Some(hook) = hook {
            let Some(function) = self.functions.get(&hook) else {
                return Err(unsupported(
                    "construction with a creo hook before its signature is known",
                ));
            };
            let arena = if self.arena { "faber_a, " } else { "" };
            write!(body, "{}({arena}{target}); ", function.name).ok();
            self.called.insert(hook);
            effect = true;
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
        let mut base: Option<&HirExpression> = None;
        for entry in entries {
            match &entry.key {
                radix_hir::HirObjectKey::Ident(symbol) => {
                    let Some(value) = &entry.value else {
                        return Err(unsupported("shorthand field in an object literal"));
                    };
                    given.push((*symbol, value));
                }
                // `Genus { … } from source`: every omitted field is copied.
                radix_hir::HirObjectKey::Spread(source) => base = Some(source),
                _ => return Err(unsupported("object literal key that is not a field name")),
            }
        }
        self.construct(def_id, &given, base)
    }

    /// A value stored into a field of class `ty`; an unwrapped exact value
    /// narrows like any unwrapped store.
    pub(crate) fn field_value(
        &mut self,
        value: &HirExpression,
        ty: Ty,
    ) -> Result<Operand, CodegenError> {
        let operand = self.expr(value)?;
        let operand = self.settle(operand, ty)?;
        if operand.value != Value::from(ty) {
            return Err(unsupported("value stored without a store check"));
        }
        self.need_class(ty);
        Ok(operand)
    }

    /// The field `name` of the genus `base` holds.
    pub(crate) fn field_of(&self, base: &Operand, name: Symbol) -> Result<Field, CodegenError> {
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
        // `Type.NAME` names a type-level constant, not an instance field: the
        // base is the genus path, not a value. The runner reads the folded
        // fact through the type.
        if let radix_hir::HirExpressionKind::Path(def_id) = &base.kind {
            let field_name = self.names.resolve_symbol(name).to_owned();
            if let Some((c_name, ty)) = self.class_const(*def_id, &field_name) {
                return Ok(Self::cell_read(&c_name, ty));
            }
        }
        let base = self.expr(base)?;
        self.field_from(&base, name)
    }

    /// The C name and class of a genus's type-level constant, by source name.
    fn class_const(&self, def_id: DefId, name: &str) -> Option<(String, Ty)> {
        let genus = self.genera.get(&def_id)?;
        genus
            .consts
            .iter()
            .find(|(source, _)| source == name)
            .map(|(_, konst)| (konst.c_name.clone(), konst.ty))
    }

    /// The file-scope declarations of a genus's type-level constants. They
    /// carry `__attribute__((unused))`: a constant no code reads is still a
    /// legal declaration, and an unused `static const` is a warning under the
    /// target's `-Werror`.
    pub(crate) fn class_constants(&mut self, def_id: DefId) -> Result<String, CodegenError> {
        let consts = self
            .genera
            .get(&def_id)
            .map(|genus| genus.consts.clone())
            .unwrap_or_default();
        let mut out = String::new();
        for (_, konst) in consts {
            let value = self.expr(&konst.value)?;
            if value.value != Value::from(konst.ty) {
                return Err(unsupported("class constant without a folded value"));
            }
            self.need_class(konst.ty);
            writeln!(
                out,
                "static {} __attribute__((unused)) = {};",
                self.declare(konst.ty, &konst.c_name, false)?,
                value.text
            )
            .ok();
        }
        Ok(out)
    }

    /// The field `name` of an already evaluated genus value.
    pub(crate) fn field_from(
        &mut self,
        base: &Operand,
        name: Symbol,
    ) -> Result<Operand, CodegenError> {
        let field = self.field_of(base, name)?;
        let text = format!("({})->{}", base.text, field.c_name);
        let mut operand = Self::cell_read(&text, field.ty).with_effect(base.effect);
        // A read behind a call is not a pure 64-bit fast-path leaf (C4).
        if base.effect {
            operand.narrow = None;
        }
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

    /// Access-control annotations (`@ private`, `@ internal`) have no runtime
    /// effect — the runner enforces nothing at run time — so the target
    /// ignores them; every other annotation can change meaning and is
    /// rejected.
    pub(crate) fn ignorable_annotation(&self, annotation: &radix_hir::HirAnnotation) -> bool {
        matches!(
            self.names.resolve_symbol(annotation.family),
            "private" | "internal" | "privata" | "interna"
        )
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
        let genus = self.genus(def_id)?;
        // A payload-less enumeration shows from a name table and takes no
        // arena.
        let plain = matches!(&genus.shape, Shape::Enum { variants }
            if variants.iter().all(|variant| variant.fields.is_empty()));
        let arena = if plain { "" } else { "faber_a, " };
        Ok(format!("{}_show({arena}{text})", genus.c_name))
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
            if matches!(self.genus(def_id)?.shape, Shape::Enum { .. }) {
                let (prototype, definition) = self.enum_show(def_id)?;
                prototypes.push_str(&prototype);
                definitions.push_str(&definition);
                continue;
            }
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
            // A genus with no fields shows as `Name {}`.
            if fields.is_empty() {
                write!(
                    definitions,
                    "{signature} {{\n    (void)value;\n    (void)faber_a;\n    return {};\n}}\n\n",
                    self.text_literal(&format!("{name} {{}}"))?
                )
                .ok();
                continue;
            }
            let mut parts = vec![self.text_literal(&format!("{name} {{ "))?];
            for (position, field) in fields.iter().enumerate() {
                let separator = if position == 0 { "" } else { ", " };
                parts.push(self.text_literal(&format!("{separator}{} = ", field.name))?);
                let read = format!("value->{}", field.c_name);
                let shown = self.show_field(&read, field.ty)?;
                parts.push(shown);
            }
            parts.push(self.text_literal(" }")?);
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

    /// Deep-copy functions for the genera a `copy` asked for (R7). The body
    /// allocates a fresh object and copies every field: a list a fresh array,
    /// a genus a recursive clone, a scalar / text (immutable) / scalar
    /// optional by assignment. Nested clones join the worklist as they are
    /// named, so one call emits the whole closure.
    pub(crate) fn copy_functions(&mut self) -> Result<(String, String), CodegenError> {
        let mut prototypes = String::new();
        let mut definitions = String::new();
        let mut done = 0;
        let mut emitted: Vec<DefId> = Vec::new();
        while done < self.copies.len() {
            let def_id = self.copies[done];
            done += 1;
            if emitted.contains(&def_id) {
                continue;
            }
            emitted.push(def_id);
            let (c_name, fields) = {
                let genus = self.genus(def_id)?;
                (genus.c_name.clone(), genus.fields.clone())
            };
            let signature = format!(
                "static {c_name} *{c_name}_copy(faber_arena *faber_a, const {c_name} *faber_self)"
            );
            prototypes.push_str(&signature);
            prototypes.push_str(";\n");
            self.support.need(Helper::Arena);
            let mut body = String::new();
            if fields.is_empty() {
                body.push_str("    (void)faber_self;\n");
            }
            for field in &fields {
                let read = format!("faber_self->{}", field.c_name);
                let written = self.copy_field(&read, field.ty)?;
                writeln!(body, "    faber_copy->{} = {written};", field.c_name).ok();
            }
            write!(
                definitions,
                "{signature} {{\n    {c_name} *faber_copy = faber_alloc(faber_a, sizeof *faber_copy);\n{body}    return faber_copy;\n}}\n\n"
            )
            .ok();
        }
        Ok((prototypes, definitions))
    }

    /// One field's deep copy, read from `read`.
    fn copy_field(&mut self, read: &str, ty: Ty) -> Result<String, CodegenError> {
        Ok(match ty {
            Ty::List(elem) => {
                if matches!(elem, Elem::Genus(_)) {
                    return Err(unsupported("copy of a list of genus field"));
                }
                self.support.need(Helper::List(elem, ListOp::Of));
                format!(
                    "faber_list_{}_copy(faber_a, {read})",
                    self.elem_suffix(elem)?
                )
            }
            Ty::Genus(inner) => self.genus_copy_call(inner, read)?,
            Ty::Opt(inner) => match inner {
                // Text is immutable, so sharing the pointer is a copy; a
                // scalar optional is a value struct.
                Inner::Text
                | Inner::Int
                | Inner::Bool
                | Inner::Float
                | Inner::Float32
                | Inner::Modular(_)
                | Inner::Bits(_) => read.to_owned(),
                Inner::List(elem) => {
                    if matches!(elem, Elem::Genus(_)) {
                        return Err(unsupported("copy of an optional list of genus field"));
                    }
                    self.support.need(Helper::List(elem, ListOp::Of));
                    format!(
                        "({read} == NULL ? NULL : faber_list_{}_copy(faber_a, {read}))",
                        self.elem_suffix(elem)?
                    )
                }
                Inner::Genus(id) => {
                    let clone = self.genus_copy_call(id, read)?;
                    format!("({read} == NULL ? NULL : {clone})")
                }
                // A deep copy of a map field needs a map copy family this
                // subset does not emit; reject by name rather than alias.
                Inner::Map(..) => return Err(unsupported("copy of an optional map field")),
            },
            _ => read.to_owned(),
        })
    }

    /// `g_<Name>_copy(faber_a, read)`, requesting the clone.
    fn genus_copy_call(&mut self, def_id: DefId, read: &str) -> Result<String, CodegenError> {
        let c_name = self.genus(def_id)?.c_name.clone();
        self.copies.push(def_id);
        Ok(format!("{c_name}_copy(faber_a, {read})"))
    }

    /// One field as a genus shows it: text quoted, like a list element.
    pub(crate) fn show_field(&mut self, read: &str, ty: Ty) -> Result<String, CodegenError> {
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
