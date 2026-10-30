//! Enumerations and tagged unions.
//!
//! Both are the genus representation with a tag: a pointer to an
//! arena-allocated (or, for a variant with no payload, static) struct
//! `{ int64_t tag; <every variant's fields> }`, so a bind aliases like any
//! genus and the show, field and pointer machinery is shared.
//!
//! * A variant's tag is its position; an `ordo` discriminant is a conversion
//!   fact carried by the `↦` route. A unit variant is one static object, so two values of a
//!   payload-less enumeration are equal exactly when their pointers are.
//! * A payload variant's fields are `m_<Variant>_<field>`; they are read only
//!   by a variant pattern that has just tested the tag.
//!
//! The runner shows a value as `Type.Variant` or `Type.Variant { f = v, … }`
//! with text quoted, like a genus field.

use crate::Emitter;
use crate::error::unsupported;
use crate::expr::{Operand, Value, c_int64};
use crate::genus::{Field, Genus, Shape, Variant};
use crate::helpers::Helper;
use crate::names::Namer;
use crate::ty::{Ty, classify};
use radix_codegen_shared::CodegenError;
use radix_hir::{DefId, HirEnum, HirExpression, HirOrdoRouteMember, HirOrdoRow};
use radix_lexer::Symbol;
use std::fmt::Write as _;

impl Emitter<'_> {
    /// Record an enumeration: its C name, its variants with their tags, and
    /// the flattened payload fields.
    pub(crate) fn collect_enum(
        &mut self,
        def_id: DefId,
        declaration: &HirEnum,
    ) -> Result<(), CodegenError> {
        if !declaration.type_params.is_empty() {
            return Err(unsupported("generic union"));
        }
        if !declaration.commune_fields.is_empty() {
            return Err(unsupported("union with shared (commune) fields"));
        }
        let name = self.names.resolve_symbol(declaration.name).to_owned();
        let c_name = self.items_scope.claim(&format!("g_{name}"));
        let mut members = Namer::program_scope();
        let mut fields = Vec::new();
        let mut variants = Vec::with_capacity(declaration.variants.len());
        // The tag is the position. An `ordo` discriminant is a conversion
        // fact: the `↦` route carries each member's resolved value.
        for (index, variant) in declaration.variants.iter().enumerate() {
            let tag = i64::try_from(index)
                .map_err(|_| unsupported("union with more variants than i64"))?;
            let variant_name = self.names.resolve_symbol(variant.name).to_owned();
            let mut own = Vec::with_capacity(variant.fields.len());
            for field in &variant.fields {
                let field_name = self.names.resolve_symbol(field.name).to_owned();
                own.push(Field {
                    c_name: members.claim(&format!("m_{variant_name}_{field_name}")),
                    name: field_name,
                    ty: classify(self.types, field.ty)?,
                    default: None,
                    sponte: false,
                });
            }
            fields.extend(own.iter().cloned());
            let static_name = self.items_scope.claim(&format!("k_{name}_{variant_name}"));
            variants.push(Variant {
                def_id: variant.def_id,
                name: variant_name,
                tag,
                static_name,
                fields: own,
            });
        }
        for (index, variant) in variants.iter().enumerate() {
            self.variants.insert(variant.def_id, (def_id, index));
        }
        self.support.need_ints();
        self.genera.insert(
            def_id,
            Genus {
                c_name,
                name,
                fields,
                consts: Vec::new(),
                methods: Vec::new(),
                shape: Shape::Enum { variants },
            },
        );
        Ok(())
    }

    /// The enumeration a variant belongs to and the variant itself.
    pub(crate) fn variant(&self, def_id: DefId) -> Option<(DefId, &Variant)> {
        let (enum_def, index) = *self.variants.get(&def_id)?;
        match &self.genera.get(&enum_def)?.shape {
            Shape::Enum { variants } => variants.get(index).map(|variant| (enum_def, variant)),
            Shape::Class => None,
        }
    }

    /// A value of the variant with no payload: the static object, `&k_…`.
    pub(crate) fn unit_variant(&self, def_id: DefId) -> Option<Operand> {
        let (enum_def, variant) = self.variant(def_id)?;
        if !variant.fields.is_empty() {
            return None;
        }
        Some(Operand::new(
            format!("(&{})", variant.static_name),
            Value::Genus(enum_def),
        ))
    }

    /// `variant Circ { r = 3 }`: the variant's static object, or one arena
    /// allocation with the tag and the given fields set in literal order.
    pub(crate) fn construct_variant(
        &mut self,
        def_id: DefId,
        given: &[(Symbol, &HirExpression)],
    ) -> Result<Operand, CodegenError> {
        let named: Vec<(String, &HirExpression)> = given
            .iter()
            .map(|(symbol, value)| (self.names.resolve_symbol(*symbol).to_owned(), *value))
            .collect();
        self.construct_variant_named(def_id, &named)
    }

    fn construct_variant_named(
        &mut self,
        def_id: DefId,
        given: &[(String, &HirExpression)],
    ) -> Result<Operand, CodegenError> {
        let Some((enum_def, variant)) = self.variant(def_id) else {
            return Err(unsupported("construction of a name that is not a variant"));
        };
        if variant.fields.is_empty() && given.is_empty() {
            return self
                .unit_variant(def_id)
                .ok_or_else(|| unsupported("construction of a name that is not a variant"));
        }
        let (tag, declared) = (variant.tag, variant.fields.clone());
        let c_name = self
            .genera
            .get(&enum_def)
            .map(|genus| genus.c_name.clone())
            .ok_or_else(|| unsupported("union outside the program"))?;
        if given.len() != declared.len() {
            return Err(unsupported("variant construction with omitted fields"));
        }
        self.support.need(Helper::Arena);
        let target = format!("faber_t{}", self.temps);
        self.temps += 1;
        let mut body = format!(
            "{c_name} *{target} = faber_alloc(faber_a, sizeof *{target}); {target}->tag = {tag}; "
        );
        let mut effect = false;
        for (name, value) in given {
            let Some(field) = declared.iter().find(|field| field.name == *name) else {
                return Err(unsupported(
                    "construction of a field the variant does not declare",
                ));
            };
            let operand = self.field_value(value, field.ty)?;
            effect |= operand.effect;
            write!(body, "{target}->{} = {}; ", field.c_name, operand.text).ok();
        }
        let text = format!("(__extension__ ({{ {body}{target}; }}))");
        Ok(Operand::new(text, Value::Genus(enum_def)).with_effect(effect))
    }

    /// `variant Circ { r = 3 }` as the call it lowers to: named arguments
    /// give the fields, positional ones follow the declaration.
    pub(crate) fn construct_variant_call(
        &mut self,
        def_id: DefId,
        args: &[radix_hir::HirCallArg],
    ) -> Result<Operand, CodegenError> {
        let declared: Vec<String> = match self.variant(def_id) {
            Some((_, variant)) => variant
                .fields
                .iter()
                .map(|field| field.name.clone())
                .collect(),
            None => return Err(unsupported("construction of a name that is not a variant")),
        };
        let mut given = Vec::with_capacity(args.len());
        for (position, arg) in args.iter().enumerate() {
            if arg.spread || arg.marker.is_some() {
                return Err(unsupported("spread or marked variant argument"));
            }
            let name = match (arg.name, declared.get(position)) {
                (Some(name), _) => self.names.resolve_symbol(name).to_owned(),
                (None, Some(name)) => name.clone(),
                (None, None) => return Err(unsupported("variant construction with extra fields")),
            };
            given.push((name, &arg.expr));
        }
        self.construct_variant_named(def_id, &given)
    }

    /// The statics and the struct body of an enumeration.
    pub(crate) fn enum_body(&self, genus: &Genus) -> Result<String, CodegenError> {
        let Shape::Enum { variants } = &genus.shape else {
            return Err(unsupported("enumeration outside the program"));
        };
        let mut out = format!("struct {} {{\n    int64_t tag;\n", genus.c_name);
        for field in &genus.fields {
            writeln!(out, "    {};", self.declare(field.ty, &field.c_name, true)?).ok();
        }
        out.push_str("};\n\n");
        for variant in variants.iter().filter(|variant| variant.fields.is_empty()) {
            // Static storage zeroes the payload; the tag is the only member
            // a unit variant reads.
            writeln!(
                out,
                "static __attribute__((unused)) {} {} = {{ .tag = {} }};",
                genus.c_name, variant.static_name, variant.tag
            )
            .ok();
        }
        out.push('\n');
        Ok(out)
    }

    /// The show function of an enumeration: `Type.Variant`, or
    /// `Type.Variant { f = v, … }` with the payload shown like genus fields.
    /// Returns the prototype and the definition.
    pub(crate) fn enum_show(&mut self, def_id: DefId) -> Result<(String, String), CodegenError> {
        let (c_name, name, variants) = {
            let genus = self
                .genera
                .get(&def_id)
                .ok_or_else(|| unsupported("union outside the program"))?;
            let Shape::Enum { variants } = &genus.shape else {
                return Err(unsupported("enumeration outside the program"));
            };
            (genus.c_name.clone(), genus.name.clone(), variants.clone())
        };
        let payload = variants.iter().any(|variant| !variant.fields.is_empty());
        let arena = if payload {
            "faber_arena *faber_a, "
        } else {
            ""
        };
        let signature = format!("static const char *{c_name}_show({arena}const {c_name} *value)");
        let mut body = String::new();
        for (position, variant) in variants.iter().enumerate() {
            let last = position + 1 == variants.len();
            let head = self.text_literal(&format!("{name}.{}", variant.name))?;
            let line = if variant.fields.is_empty() {
                format!("return {head};")
            } else {
                let mut parts = vec![self.text_literal(&format!("{name}.{} {{ ", variant.name))?];
                for (at, field) in variant.fields.iter().enumerate() {
                    let separator = if at == 0 { "" } else { ", " };
                    parts.push(self.text_literal(&format!("{separator}{} = ", field.name))?);
                    let read = format!("value->{}", field.c_name);
                    parts.push(self.show_field(&read, field.ty)?);
                }
                parts.push(self.text_literal(" }")?);
                self.support.need(Helper::Join);
                format!(
                    "return faber_join(faber_a, {}, {});",
                    parts.len(),
                    parts.join(", ")
                )
            };
            // The last variant is the fall-through: the tag is always one of
            // the declared ones.
            if last {
                writeln!(body, "    {line}").ok();
            } else {
                writeln!(body, "    if (value->tag == {}) {{ {line} }}", variant.tag).ok();
            }
        }
        let definition = format!("{signature} {{\n{body}}}\n\n");
        Ok((format!("{signature};\n"), definition))
    }

    /// `a ≡ b` over two values of one enumeration. A payload-less
    /// enumeration compares pointers (every value is one of its statics); a
    /// payload variant compares tag and fields.
    pub(crate) fn enum_equal(
        &mut self,
        def_id: DefId,
        left: &str,
        right: &str,
    ) -> Result<String, CodegenError> {
        let (c_name, variants) = {
            let genus = self
                .genera
                .get(&def_id)
                .ok_or_else(|| unsupported("union outside the program"))?;
            let Shape::Enum { variants } = &genus.shape else {
                return Err(unsupported("comparison of genus values"));
            };
            (genus.c_name.clone(), variants.clone())
        };
        if variants.iter().all(|variant| variant.fields.is_empty()) {
            return Ok(format!("({left} == {right})"));
        }
        let name = format!("{c_name}_equal");
        if !self.equals.contains(&def_id) {
            self.equals.push(def_id);
        }
        Ok(format!("{name}({left}, {right})"))
    }

    /// `a ≡ b` / `a ≢ b` over two values of enumeration `def_id`, operands
    /// sequenced like any binary expression.
    pub(crate) fn enum_comparison(
        &mut self,
        op: radix_hir::HirBinOp,
        def_id: DefId,
        l: Operand,
        r: Operand,
    ) -> Result<Operand, CodegenError> {
        let effect = l.effect || r.effect;
        let mut operands = [l, r];
        let prelude = self.sequence(&mut operands)?;
        let [l, r] = operands;
        let equal = self.enum_equal(def_id, &l.text, &r.text)?;
        let negate = matches!(
            op,
            radix_hir::HirBinOp::NotEq | radix_hir::HirBinOp::StrictNotEq
        );
        let body = if negate { format!("(!{equal})") } else { equal };
        let text = crate::expr::wrap(&prelude, &body);
        Ok(Operand::new(text, Value::Truth).with_effect(effect))
    }

    /// The prototypes and definitions of the equality functions asked for.
    pub(crate) fn equal_functions(&mut self) -> Result<(String, String), CodegenError> {
        let mut prototypes = String::new();
        let mut definitions = String::new();
        let mut done = 0;
        while done < self.equals.len() {
            let def_id = self.equals[done];
            done += 1;
            let (c_name, variants) = {
                let genus = self
                    .genera
                    .get(&def_id)
                    .ok_or_else(|| unsupported("union outside the program"))?;
                let Shape::Enum { variants } = &genus.shape else {
                    return Err(unsupported("comparison of genus values"));
                };
                (genus.c_name.clone(), variants.clone())
            };
            let signature =
                format!("static bool {c_name}_equal(const {c_name} *a, const {c_name} *b)");
            let mut body = String::from("    if (a->tag != b->tag) { return false; }\n");
            for variant in variants.iter().filter(|variant| !variant.fields.is_empty()) {
                let mut tests = Vec::new();
                for field in &variant.fields {
                    tests.push(self.field_equal(field)?);
                }
                writeln!(
                    body,
                    "    if (a->tag == {}) {{ return {}; }}",
                    variant.tag,
                    tests.join(" && ")
                )
                .ok();
            }
            body.push_str("    return true;\n");
            writeln!(prototypes, "{signature};").ok();
            write!(definitions, "{signature} {{\n{body}}}\n\n").ok();
        }
        if !prototypes.is_empty() {
            self.support.need_bool();
        }
        Ok((prototypes, definitions))
    }

    fn field_equal(&mut self, field: &Field) -> Result<String, CodegenError> {
        let (a, b) = (
            format!("a->{}", field.c_name),
            format!("b->{}", field.c_name),
        );
        match field.ty {
            Ty::Int | Ty::Sized(_) | Ty::Bool | Ty::None => Ok(format!("({a} == {b})")),
            Ty::Text => {
                self.support.need_stdio();
                Ok(format!("(strcmp({a}, {b}) == 0)"))
            }
            Ty::Genus(def_id) => self.enum_equal(def_id, &a, &b),
            Ty::Float
            | Ty::Float32
            | Ty::Opt(_)
            | Ty::List(_)
            | Ty::Set(_)
            | Ty::Map(..)
            | Ty::Decimal
            | Ty::Modular(_)
            | Ty::Saturating(_)
            | Ty::Bits(_)
            | Ty::Func(_) => Err(unsupported(
                "equality of a union with a payload of this class",
            )),
        }
    }
}

impl Emitter<'_> {
    /// `ordo ↦ int`, `ordo ↦ string` and `int ↦ ordo [⊥ default]`: the
    /// `↦` route carries each member's resolved discriminant, in declaration
    /// order, so a conversion is a chain over the tag or the value. A number
    /// no member has ends the program with the runner's own text, unless a
    /// recovery value stands in for it.
    pub(crate) fn ordo_conversion(
        &mut self,
        source: &HirExpression,
        row: HirOrdoRow,
        members: &[HirOrdoRouteMember],
        recovery: Option<&HirExpression>,
    ) -> Result<Operand, CodegenError> {
        let value = self.expr(source)?;
        let effect = value.effect;
        let n = self.temps;
        self.temps += 1;
        let held = format!("faber_v{n}");
        match (row, value.value) {
            (HirOrdoRow::ToNumerus | HirOrdoRow::ToTextus, Value::Genus(def_id)) => {
                let c_name = self
                    .genera
                    .get(&def_id)
                    .map(|genus| genus.c_name.clone())
                    .ok_or_else(|| unsupported("enumeration outside the program"))?;
                let text = matches!(row, HirOrdoRow::ToTextus);
                let mut arms = Vec::with_capacity(members.len());
                for (position, member) in members.iter().enumerate() {
                    let result = if text {
                        let raw = self.names.resolve_symbol(member.name).to_owned();
                        self.text_literal(&raw)?
                    } else {
                        let Some(number) = member.value else {
                            return Err(unsupported("ordo member without a numeric discriminant"));
                        };
                        self.support.need_ints();
                        c_int64(number)
                    };
                    arms.push((position, result));
                }
                let Some((_, last)) = arms.pop() else {
                    return Err(unsupported("conversion of an enumeration with no members"));
                };
                let mut chain = last;
                for (position, result) in arms.into_iter().rev() {
                    chain = format!("({held}->tag == {position} ? {result} : {chain})");
                }
                let form = if text { Value::Text } else { Value::Int };
                let body = format!(
                    "(__extension__ ({{ const {c_name} *{held} = {}; {chain}; }}))",
                    value.text
                );
                Ok(Operand::new(body, form).with_effect(effect))
            }
            (HirOrdoRow::FromNumerus, Value::Int | Value::Exact) => {
                let mut chain = if let Some(recovery) = recovery {
                    let fallback = self.expr(recovery)?;
                    let Value::Genus(_) = fallback.value else {
                        return Err(unsupported("recovery value of a different class"));
                    };
                    fallback.text
                } else {
                    self.support.need(Helper::Trap);
                    "(faber_abort(\"no ordo member has this value\"), NULL)".to_owned()
                };
                let mut def = None;
                for member in members.iter().rev() {
                    let Some(number) = member.value else {
                        continue;
                    };
                    let Some(object) = self.unit_variant(member.variant) else {
                        return Err(unsupported("ordo member that is not a unit variant"));
                    };
                    if let Value::Genus(enum_def) = object.value {
                        def = Some(enum_def);
                    }
                    self.support.need_ints();
                    chain = format!(
                        "({held} == (faber_i128){} ? {} : {chain})",
                        c_int64(number),
                        object.text
                    );
                }
                let Some(def) = def else {
                    return Err(unsupported("conversion to an enumeration with no members"));
                };
                self.support.need(Helper::Types);
                let body = format!(
                    "(__extension__ ({{ const faber_i128 {held} = {}; {chain}; }}))",
                    value.text
                );
                Ok(Operand::new(body, Value::Genus(def)).with_effect(effect))
            }
            _ => Err(unsupported("ordo conversion between these value classes")),
        }
    }
}
