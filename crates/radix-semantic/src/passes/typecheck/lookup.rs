//! Local lookup and primitive type helpers for HIR typechecking.
//!
//! This module keeps the checker's low-level table access in one place:
//! lexical bindings, declaration lookup helpers, semantic category predicates,
//! primitive constructors, and member-definition extraction. Most helpers first
//! resolve inference substitutions and aliases so callers check the current
//! semantic meaning rather than the raw arena entry.
//!
//! NULLABILITY AND ACCESS
//! ======================
//! Struct and interface extraction intentionally do not look through optional
//! wrappers for ordinary lookup. Optional chaining and non-null access unwrap
//! their receiver before calling these helpers, which keeps `maybe.field` from
//! silently behaving like `maybe?.field`.

#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for typechecking
use super::*;

const ISSUE_ASCII_LITERAL_INVALID_ESCAPE: &str = "ascii_literal_invalid_escape";
const ISSUE_ASCII_LITERAL_NON_ASCII_SCALAR: &str = "ascii_literal_non_ascii_scalar";
const ISSUE_ASCII_LITERAL_UNESCAPED_CONTROL: &str = "ascii_literal_unescaped_control";
const ISSUE_JSON_DUPLICATE_KEY: &str = "json_duplicate_key";
const ISSUE_JSON_MAP_KEY_MUST_BE_TEXTUS: &str = "json_map_key_must_be_textus";
const ISSUE_JSON_NESTED_VALUE_REQUIRES_VALOR: &str = "json_nested_value_requires_valor";
const ISSUE_JSON_VALUE_TYPE_MISMATCH: &str = "json_value_type_mismatch";
const ISSUE_OCTETI_LITERAL_INVALID_CHARACTER: &str = "octeti_literal_invalid_character";
const ISSUE_OCTETI_LITERAL_ODD_DIGIT_COUNT: &str = "octeti_literal_odd_digit_count";

impl TypeChecker<'_> {
    pub fn error_issue(
        &mut self,
        kind: SemanticErrorKind,
        issue: &'static str,
        span: radix_lexer::Span,
    ) {
        self.errors
            .push(SemanticError::structured(kind, span).with_arg("issue", issue));
    }

    /// [`Self::error_issue`] carrying named message arguments.
    pub fn error_issue_with_args(
        &mut self,
        kind: SemanticErrorKind,
        issue: &'static str,
        span: radix_lexer::Span,
        args: &[(&'static str, String)],
    ) {
        let mut error = SemanticError::structured(kind, span).with_arg("issue", issue);
        for (name, value) in args {
            error = error.with_arg(name, value);
        }
        self.errors.push(error);
    }

    /// Reject an illegal own-bind RHS and attach the D-6 `copy` suggestion.
    pub fn reject_own_bind(&mut self, issue: &'static str, span: radix_lexer::Span) {
        self.errors.push(
            SemanticError::structured(SemanticErrorKind::ModeMismatch, span)
                .with_arg("issue", issue)
                .with_arg("suggestion", crate::SUGGESTION_COPY),
        );
    }

    pub fn invalid_operand_types(&mut self, issue: &'static str, span: radix_lexer::Span) {
        self.error_issue(SemanticErrorKind::OperandTypesInvalid, issue, span);
    }

    /// Reject compiler-only policy markers wherever a runtime value type is
    /// required. Explicit intrinsic type arguments are validated separately.
    pub fn reject_tensor_edge_policy_value_type(&mut self, ty: TypeId, span: radix_lexer::Span) {
        self.record_tensor_edge_policy_runtime_type_use(ty);
        if self
            .types
            .contains_tensor_edge_policy(self.resolve_inferred_type(ty))
        {
            self.error_issue(
                SemanticErrorKind::TypeMismatch,
                "tensor_edge_policy_marker_not_runtime_value",
                span,
            );
        }
    }

    pub fn invalid_operand_types_with_args(
        &mut self,
        issue: &'static str,
        span: radix_lexer::Span,
        args: &[(&'static str, String)],
    ) {
        let mut error = SemanticError::structured(SemanticErrorKind::OperandTypesInvalid, span)
            .with_arg("issue", issue);
        for (name, value) in args {
            error = error.with_arg(name, value);
        }
        self.errors.push(error);
    }

    pub fn undefined_member(&mut self, issue: &'static str, span: radix_lexer::Span) {
        self.error_issue(SemanticErrorKind::MemberUndefined, issue, span);
    }

    pub fn undefined_member_with_args(
        &mut self,
        issue: &'static str,
        span: radix_lexer::Span,
        args: &[(&'static str, String)],
    ) {
        let mut error = SemanticError::structured(SemanticErrorKind::MemberUndefined, span)
            .with_arg("issue", issue);
        for (name, value) in args {
            error = error.with_arg(name, value);
        }
        self.errors.push(error);
    }

    pub fn unavailable_declaration(
        &mut self,
        issue: &'static str,
        span: radix_lexer::Span,
        method: impl Into<String>,
        reason: Option<String>,
    ) {
        let mut error = SemanticError::structured(SemanticErrorKind::UnavailableDeclaration, span)
            .with_arg("issue", issue)
            .with_arg("method", method);
        if let Some(reason) = reason {
            error = error.with_arg("reason", reason);
        }
        self.errors.push(error);
    }

    pub fn immutable_assignment(&mut self, issue: &'static str, span: radix_lexer::Span) {
        self.error_issue(SemanticErrorKind::ImmutableAssignment, issue, span);
    }

    pub fn immutable_assignment_with_args(
        &mut self,
        issue: &'static str,
        span: radix_lexer::Span,
        args: &[(&'static str, String)],
    ) {
        let mut error = SemanticError::structured(SemanticErrorKind::ImmutableAssignment, span)
            .with_arg("issue", issue);
        for (name, value) in args {
            error = error.with_arg(name, value);
        }
        self.errors.push(error);
    }

    pub fn missing_type_annotation(&mut self, issue: &'static str, span: radix_lexer::Span) {
        self.error_issue(SemanticErrorKind::TypeAnnotationAbsent, issue, span);
    }

    pub fn invalid_conversion(&mut self, issue: &'static str, span: radix_lexer::Span) {
        self.error_issue(SemanticErrorKind::ConversionInvalid, issue, span);
    }

    pub fn invalid_conversion_with_args(
        &mut self,
        issue: &'static str,
        span: radix_lexer::Span,
        args: &[(&'static str, &'static str)],
    ) {
        let mut error = SemanticError::structured(SemanticErrorKind::ConversionInvalid, span)
            .with_arg("issue", issue);
        for (name, value) in args {
            error = error.with_arg(name, *value);
        }
        self.errors.push(error);
    }

    pub fn not_callable(&mut self, issue: &'static str, span: radix_lexer::Span) {
        self.error_issue(SemanticErrorKind::NonCallable, issue, span);
    }

    pub fn wrong_arity(&mut self, issue: &'static str, span: radix_lexer::Span) {
        self.error_issue(SemanticErrorKind::ArityMismatch, issue, span);
    }

    pub fn wrong_arity_with_arg(
        &mut self,
        issue: &'static str,
        span: radix_lexer::Span,
        name: &'static str,
        value: impl Into<String>,
    ) {
        self.errors.push(
            SemanticError::structured(SemanticErrorKind::ArityMismatch, span)
                .with_arg("issue", issue)
                .with_arg(name, value),
        );
    }

    /// Find the innermost visible binding for a resolved definition.
    pub fn lookup_binding(&self, def_id: DefId) -> Option<BindingInfo> {
        self.binding_shadow.get(&def_id).copied()
    }

    /// Record the source spelling of a HIR-local binding (param/local).
    pub fn record_binding_name(&mut self, def_id: DefId, name: Symbol) {
        self.binding_names.insert(def_id, name);
    }

    /// Insert a binding into the current lexical scope.
    pub fn insert_binding(&mut self, def_id: DefId, ty: TypeId) {
        self.insert_binding_with_scalar(def_id, ty, false, false);
    }

    pub fn insert_binding_with_scalar(
        &mut self,
        def_id: DefId,
        ty: TypeId,
        scalar_text: bool,
        mutable: bool,
    ) {
        if let Some(scope) = self.scopes.last_mut() {
            let info = BindingInfo {
                ty,
                scalar_text,
                mutable,
            };
            scope.insert(def_id, info);
            self.binding_shadow.insert(def_id, info);
        }
    }

    /// Whether a path expression names a mutable local or `in` parameter binding.
    pub fn path_binding_is_mutable(&self, receiver: &HirExpression) -> bool {
        let HirExpressionKind::Path(def_id) = &receiver.kind else {
            return false;
        };
        self.lookup_binding(*def_id)
            .is_some_and(|binding| binding.mutable)
    }

    /// Start a lexical scope for block, branch, loop, or pattern-local bindings.
    pub fn push_scope(&mut self) {
        self.scopes.push(FxHashMap::default());
    }

    /// Leave the innermost lexical scope.
    pub fn pop_scope(&mut self) {
        let Some(popped) = self.scopes.pop() else {
            return;
        };
        for def_id in popped.keys() {
            if let Some(info) = self
                .scopes
                .iter()
                .rev()
                .find_map(|scope| scope.get(def_id).copied())
            {
                self.binding_shadow.insert(*def_id, info);
            } else {
                self.binding_shadow.remove(def_id);
            }
        }
    }

    /// Return whether the resolved type participates in numeric operators.
    pub fn is_numeric(&self, ty: TypeId) -> bool {
        self.is_integer(ty) || self.is_fractus(ty)
    }

    pub fn is_integer(&self, ty: TypeId) -> bool {
        let (_, payload) = self
            .types
            .peel_access_marker(self.resolve_inferred_type(ty));
        matches!(
            self.types.get(payload),
            Type::Primitive(Primitive::Numerus)
                | Type::SizedNumeric(Primitive::Numerus, _)
                | Type::ModularWord(_)
                | Type::SaturatingWord(_)
        )
    }

    pub fn is_fractus(&self, ty: TypeId) -> bool {
        let (_, payload) = self
            .types
            .peel_access_marker(self.resolve_inferred_type(ty));
        matches!(
            self.types.get(payload),
            Type::Primitive(Primitive::Fractus)
                | Type::SizedNumeric(Primitive::Fractus, _)
                | Type::PolicyFloat(_, _)
        )
    }

    pub fn is_textus(&self, ty: TypeId) -> bool {
        let (_, payload) = self
            .types
            .peel_access_marker(self.resolve_inferred_type(ty));
        matches!(self.types.get(payload), Type::Primitive(Primitive::Textus))
    }

    pub fn is_bool(&self, ty: TypeId) -> bool {
        let (_, payload) = self
            .types
            .peel_access_marker(self.resolve_inferred_type(ty));
        matches!(
            self.types.get(payload),
            Type::Primitive(Primitive::Bivalens)
        )
    }

    pub fn is_instans(&self, ty: TypeId) -> bool {
        matches!(
            self.types.get(self.resolve_inferred_type(ty)),
            Type::Primitive(Primitive::Instans) | Type::SizedInstans(_)
        )
    }

    pub fn numerus_type(&mut self) -> TypeId {
        self.types.primitive(Primitive::Numerus)
    }

    pub fn fractus_type(&mut self) -> TypeId {
        self.types.primitive(Primitive::Fractus)
    }

    pub fn textus_type(&mut self) -> TypeId {
        self.types.primitive(Primitive::Textus)
    }

    pub fn ascii_type(&mut self) -> TypeId {
        self.types.primitive(Primitive::Ascii)
    }

    /// Scalar-index / one-byte char type: `ascii<1>` (`BoundedAscii { 1 }`).
    pub fn ascii_one_type(&mut self) -> TypeId {
        let capacity = self.types.intern_index(IndexExpr::Literal(1));
        self.types.bounded_ascii(capacity)
    }

    pub fn octeti_type(&mut self) -> TypeId {
        self.types.octeti_list()
    }

    pub fn json_type(&mut self) -> TypeId {
        self.types.primitive(Primitive::Json)
    }

    /// D10.2 `littera`: one Unicode scalar.
    pub fn littera_type(&mut self) -> TypeId {
        self.types.primitive(Primitive::Littera)
    }

    pub fn is_littera(&self, ty: TypeId) -> bool {
        matches!(
            self.types.get(self.resolve_inferred_type(ty)),
            Type::Primitive(Primitive::Littera)
        )
    }

    pub fn is_ascii(&self, ty: TypeId) -> bool {
        matches!(
            self.types.get(self.resolve_inferred_type(ty)),
            Type::Primitive(Primitive::Ascii)
        )
    }

    pub fn bool_type(&mut self) -> TypeId {
        self.types.primitive(Primitive::Bivalens)
    }

    pub fn regex_type(&mut self) -> TypeId {
        self.types.primitive(Primitive::Regex)
    }

    pub fn nil_type(&mut self) -> TypeId {
        self.types.primitive(Primitive::Nihil)
    }

    pub fn vacuum_type(&mut self) -> TypeId {
        self.types.primitive(Primitive::Vacuum)
    }

    /// Look up a method contract for a struct or interface receiver.
    ///
    /// Receiver type wrappers are normalized by `*_def_from_type`; this lookup
    /// returns only declared contracts. Fallback collection methods and `ignotum`
    /// escape behavior are handled by call checking.
    pub fn lookup_method_signature(&self, receiver_ty: TypeId, name: Symbol) -> Option<FuncSig> {
        if let Some(struct_def) = self.struct_def_from_type(receiver_ty)
            && let Some(info) = self.structs.get(&struct_def)
            && let Some(sig) = info.methods.get(&name)
        {
            return Some(sig.clone());
        }
        if let Some(interface_def) = self.interface_def_from_type(receiver_ty)
            && let Some(methods) = self.interfaces.get(&interface_def)
            && let Some(method) = methods.get(&name)
        {
            return Some(method.sig.clone());
        }
        None
    }

    /// Look up a nominal field without emitting an access diagnostic.
    ///
    /// Method-call checking uses this probe only after declared method lookup
    /// misses, so a callable field can act as the callee while an explicit
    /// method with the same name keeps precedence. Applied generic receivers
    /// project their type parameters before the call checker inspects the
    /// resulting field type.
    pub fn lookup_field_type(&mut self, receiver_ty: TypeId, name: Symbol) -> Option<TypeId> {
        let struct_def = self.struct_def_from_type(receiver_ty)?;
        let field = self.structs.get(&struct_def)?.fields.get(&name).copied()?;
        Some(self.applied_field_type(receiver_ty, struct_def, field.ty))
    }

    /// Members of a union type after alias transparency (U1R).
    ///
    /// `typus R = A ∪ B` and its unfolded union are the same type (BO-S-4
    /// alias transparency), so the member rule, refinement, and dead-test
    /// warning all compare through this peel. A `Type::Enum` is the closed
    /// variant union: each variant is represented as `Type::Struct` carrying
    /// the variant `DefId`, while payload fields remain in the checker tables.
    /// The `Option` rewrite a nihil union lowers to still returns `None` here;
    /// its one implicit null member is handled by the refinement seam.
    pub(crate) fn union_members_of(&mut self, ty: TypeId) -> Option<Vec<TypeId>> {
        let mut current = self.resolve_inferred_type(ty);
        for _ in 0..8 {
            match self.types.get(current).clone() {
                Type::Union(members) => return Some(members),
                Type::Alias(_, inner) => current = self.resolve_inferred_type(inner),
                Type::Option(inner) => {
                    let mut members = match self.union_members_of(inner) {
                        Some(members) => members,
                        None => vec![inner],
                    };
                    members.push(self.types.primitive(Primitive::Nihil));
                    return Some(members);
                }
                Type::Enum(enum_def) => {
                    let mut variants: Vec<DefId> = self
                        .variant_parent
                        .iter()
                        .filter_map(|(variant, parent)| (*parent == enum_def).then_some(*variant))
                        .collect();
                    variants.sort_unstable_by_key(|variant| variant.0);
                    return Some(
                        variants
                            .into_iter()
                            .map(|variant| self.types.intern(Type::Struct(variant)))
                            .collect(),
                    );
                }
                _ => return None,
            }
        }
        None
    }

    #[must_use]
    pub fn lookup_interface_method_info(
        &self,
        receiver_ty: TypeId,
        name: Symbol,
    ) -> Option<&InterfaceMethodInfo> {
        let interface_def = self.interface_def_from_type(receiver_ty)?;
        self.interfaces.get(&interface_def)?.get(&name)
    }

    pub fn nondum_for_target<'b>(
        &self,
        availability: &'b [radix_hir::HirAvailability],
    ) -> Option<&'b radix_hir::HirAvailability> {
        availability.iter().find(|availability| match availability {
            radix_hir::HirAvailability::Nondum { target, .. } => {
                target.is_none_or(|target| self.annotation_target_matches(target))
            }
        })
    }

    fn annotation_target_matches(&self, target: radix_lexer::Symbol) -> bool {
        let Some(interner) = self.interner else {
            return false;
        };
        self.target_name_matches(interner.resolve(target))
    }

    /// Whether an availability gate's target spelling names the backend this
    /// check runs for.
    pub(super) fn target_name_matches(&self, target: &str) -> bool {
        match self.target {
            radix_target::Target::MirStepper => matches!(target, "stepper" | "mir-stepper"),
            radix_target::Target::HirRust => matches!(target, "rs" | "rust"),
            radix_target::Target::HirFaber => matches!(target, "faber" | "fab"),
            radix_target::Target::HirTypeScript => matches!(target, "ts" | "typescript"),
            radix_target::Target::HirGo => matches!(target, "go"),
            radix_target::Target::HirSwift => matches!(target, "swift"),
            radix_target::Target::HirPython => matches!(target, "python"),
            radix_target::Target::HirHaskell => matches!(target, "haskell"),
            radix_target::Target::HirPascal => matches!(target, "pascal" | "pas"),
            radix_target::Target::HirCobol => matches!(target, "cobol" | "cob"),
            radix_target::Target::HirFortran => matches!(target, "fortran" | "f90"),
            radix_target::Target::HirOdin => matches!(target, "odin"),
            radix_target::Target::MirWasm => matches!(target, "wasm" | "wasm-text" | "wat"),
            radix_target::Target::MirLlvm | radix_target::Target::MirLlvmHost => {
                matches!(target, "llvm" | "llvm-host" | "llvm-text")
            }
            radix_target::Target::MirMetal => matches!(target, "metal" | "metal-text"),
            radix_target::Target::MirAmd => matches!(target, "amd"),
            radix_target::Target::MirWgsl => matches!(target, "wgsl" | "wgsl-text"),
            radix_target::Target::MirSexp => matches!(target, "sexp" | "racket" | "lisp" | "rkt"),
            radix_target::Target::MirBasic => matches!(target, "basic" | "bas"),
            radix_target::Target::MirScena => matches!(target, "scena"),
            radix_target::Target::MirFmir => matches!(target, "fmir-text"),
            radix_target::Target::MirFmirBinary => matches!(target, "fmir"),
            radix_target::Target::MirFmirBundle => matches!(target, "fmir-bin"),
            radix_target::Target::HirFhir => matches!(target, "fhir"),
        }
    }

    /// Extract the underlying struct definition from an ordinary receiver type.
    pub fn struct_def_from_type(&self, ty: TypeId) -> Option<DefId> {
        match self.types.get(self.resolve_inferred_type(ty)) {
            Type::Struct(def_id) => Some(*def_id),
            Type::Ref(_, inner) => self.struct_def_from_type(*inner),
            Type::Applied(base, _) => self.struct_def_from_type(*base),
            _ => None,
        }
    }

    /// Extract the underlying enum definition from a scrutinee-like type.
    pub fn enum_def_from_type(&self, ty: TypeId) -> Option<DefId> {
        match self.types.get(self.resolve_inferred_type(ty)) {
            Type::Enum(def_id) => Some(*def_id),
            Type::Option(inner) => self.enum_def_from_type(*inner),
            Type::Applied(base, _) => self.enum_def_from_type(*base),
            _ => None,
        }
    }

    /// Extract the underlying interface definition from an ordinary receiver type.
    pub fn interface_def_from_type(&self, ty: TypeId) -> Option<DefId> {
        match self.types.get(self.resolve_inferred_type(ty)) {
            Type::Interface(def_id) => Some(*def_id),
            Type::Ref(_, inner) => self.interface_def_from_type(*inner),
            Type::Applied(base, _) => self.interface_def_from_type(*base),
            _ => None,
        }
    }

    /// Map a literal node to its semantic type.
    ///
    /// When `expected` is a `tabula<K,V>`, a JSON object literal is validated
    /// entry-by-entry and typed as the annotated map on success; otherwise JSON
    /// literals are `valor`. Float literals also accept an expected fractus width
    /// so declarations such as `fixum lista<f32> xs ← [1.0]` do not need
    /// per-element casts.
    ///
    /// `nihil` stays the primitive nil type here; optional/nullability acceptance
    /// is decided later by unification, coalescing, and `TypeTable::is_assignable`.
    pub fn literal_type(
        &mut self,
        lit: &HirLiteral,
        span: radix_lexer::Span,
        expected: Option<TypeId>,
    ) -> TypeId {
        match lit {
            HirLiteral::Int(value) => {
                if let Some(width) = self.expected_bits_width(expected) {
                    return self.bits_literal_type(Some(*value), width, expected, span);
                }
                if let Some(float_ty) = self.expected_float_slot_type(expected) {
                    return self.integer_constant_in_float_slot(*value, float_ty, span);
                }
                if let Some(decimal_ty) = self.expected_decimal_slot_type(expected) {
                    return self.integer_constant_in_decimal_slot(*value, decimal_ty, span);
                }
                let literal_ty = self
                    .expected_numerus_literal_type(expected)
                    .unwrap_or_else(|| self.numerus_type());
                if let Type::ModularWord(width) = self.types.get(literal_ty) {
                    // Literal range follows the declared width: 0..=2^W-1
                    // (unsigned) or 0..=2^(W-1)-1 (signed; a negative literal
                    // is a unary minus over its magnitude).
                    // (u64 max+1 cannot be spelled under the u64 lexer carrier.)
                    let in_range = width
                        .integer_range()
                        .is_some_and(|(_, hi)| i128::from(*value) <= hi);
                    if !in_range {
                        self.error_issue(
                            SemanticErrorKind::TypeMismatch,
                            MODULAR_WORD_LITERAL_OUT_OF_RANGE,
                            span,
                        );
                        return self.error_type;
                    }
                    literal_ty
                } else if self.literal_exceeds_signed_numerus_domain(literal_ty, *value) {
                    // The lexer now carries a u64 magnitude, so the signed
                    // numerus cap that used to be lexical lives here: magnitudes
                    // above i64::MAX are valid only for `numerus<u64>` targets.
                    self.error_issue(
                        SemanticErrorKind::TypeMismatch,
                        NUMERUS_LITERAL_OUT_OF_RANGE,
                        span,
                    );
                    self.error_type
                } else {
                    literal_ty
                }
            }
            HirLiteral::BigInt(digits) => {
                if let Some(width) = self.expected_bits_width(expected) {
                    // Beyond `u64`: fits no `bN`.
                    return self.bits_literal_type(None, width, expected, span);
                }
                self.big_integer_literal_type(digits, span, expected)
            }
            HirLiteral::Float(_) => self
                .expected_fractus_literal_type(expected)
                .unwrap_or_else(|| self.fractus_type()),
            // decimal-widths D1: annotated decimals carry exact digit text to
            // typing; `numerus<d64>` accept the literal at the
            // declared width while every other expectation keeps fractus law.
            HirLiteral::FloatText { text, value } => {
                if let Some(ty) = self.expected_decimal_literal_type(expected, *text, span) {
                    ty
                } else if let Some(float_ty) = self.expected_fractus_literal_type(expected) {
                    // Ruling 20(2): a written float outside the slot's finite
                    // range is an error, never infinity.
                    if self.check_float_literal_range(float_ty, *value, span) {
                        float_ty
                    } else {
                        self.error_type
                    }
                } else {
                    self.fractus_type()
                }
            }
            HirLiteral::String(_) => self.textus_type(),
            HirLiteral::Ascii(sym) => self.check_ascii_literal(*sym, span, expected),
            HirLiteral::Octeti(sym) => self.check_octeti_literal(*sym, span),
            HirLiteral::JsonValor(value) => self.check_json_valor_literal(value, span, expected),
            HirLiteral::Regex(sym, _) => self.check_regex_literal(*sym, span),
            HirLiteral::Bool(_) => self.bool_type(),
            HirLiteral::Nil => self.nil_type(),
        }
    }

    /// Ruling 34: an integer literal beyond `u64` is an untyped exact
    /// constant that lands only where its exact value fits: an `inf` slot
    /// always, a float slot when exactly representable (ruling 20), nothing
    /// else — a bounded slot, a decimal slot and the lone `int` default are
    /// `numerus_literal_out_of_range`.
    fn big_integer_literal_type(
        &mut self,
        digits: &str,
        span: radix_lexer::Span,
        expected: Option<TypeId>,
    ) -> TypeId {
        if let Some(slot) = self.expected_numerus_literal_type(expected)
            && self.is_inf_type(slot)
        {
            return slot;
        }
        if let Some(float_ty) = self.expected_float_slot_type(expected)
            && let Some(value) = radix_types::big_literal::parse_big_literal(digits)
        {
            return if self.check_float_exact_constant(float_ty, &value, span) {
                float_ty
            } else {
                self.error_type
            };
        }
        if self.expected_decimal_slot_type(expected).is_some() {
            return self.decimal_literal_out_of_range(span);
        }
        self.numerus_literal_out_of_range(span);
        self.error_type
    }

    fn expected_fractus_literal_type(&self, expected: Option<TypeId>) -> Option<TypeId> {
        let expected = self.resolve_inferred_type(expected?);
        match self.types.get(expected) {
            Type::Primitive(Primitive::Fractus)
            | Type::SizedNumeric(Primitive::Fractus, _)
            | Type::PolicyFloat(_, _) => Some(expected),
            _ => None,
        }
    }

    /// The float type of an expected slot, when the expectation is `fractus`.
    pub(super) fn expected_float_slot_type(&self, expected: Option<TypeId>) -> Option<TypeId> {
        self.expected_fractus_literal_type(expected)
    }

    /// d11-6 rulings 16/20: an integer constant stored into a float slot takes
    /// the float type when it is exactly representable there; otherwise the
    /// author must write the float themselves.
    fn integer_constant_in_float_slot(
        &mut self,
        value: u64,
        float_ty: TypeId,
        span: radix_lexer::Span,
    ) -> TypeId {
        if self.check_float_integer_constant(float_ty, i128::from(value), span) {
            float_ty
        } else {
            self.error_type
        }
    }

    /// The decimal type of an expected slot, when the expectation is `d64`.
    pub(super) fn expected_decimal_slot_type(&self, expected: Option<TypeId>) -> Option<TypeId> {
        let expected = self.resolve_inferred_type(expected?);
        match self.types.get(expected) {
            Type::SizedNumeric(Primitive::Numerus, width) if width.is_decimal() => Some(expected),
            _ => None,
        }
    }

    /// F9 ruling 30: an integer constant in a decimal slot is a decimal with no
    /// fraction. It takes the slot's type when its scaled carrier fits the
    /// width; otherwise it is `decimal_literal_out_of_range`.
    fn integer_constant_in_decimal_slot(
        &mut self,
        value: u64,
        decimal_ty: TypeId,
        span: radix_lexer::Span,
    ) -> TypeId {
        let fits = match self.types.get(decimal_ty) {
            Type::SizedNumeric(_, width) => width.decimal_scale().is_some_and(|scale| {
                10_i128
                    .checked_pow(scale)
                    .and_then(|factor| i128::from(value).checked_mul(factor))
                    .is_some_and(|carrier| carrier <= i128::from(i64::MAX))
            }),
            _ => false,
        };
        if fits {
            decimal_ty
        } else {
            self.decimal_literal_out_of_range(span)
        }
    }

    fn expected_numerus_literal_type(&self, expected: Option<TypeId>) -> Option<TypeId> {
        let expected = self.resolve_inferred_type(expected?);
        match self.types.get(expected) {
            Type::Primitive(Primitive::Numerus) => Some(expected),
            Type::SizedNumeric(Primitive::Numerus, width) if !width.is_decimal() => Some(expected),
            Type::ModularWord(_) | Type::SaturatingWord(_) => Some(expected),
            _ => None,
        }
    }

    /// Annotated decimal literal meeting a `numerus<d64>`
    /// expectation (decimal-widths D1). Returns the expected type when the
    /// digit text scales exactly into the width's carrier.
    fn expected_decimal_literal_type(
        &mut self,
        expected: Option<TypeId>,
        text: radix_lexer::Symbol,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let expected = self.resolve_inferred_type(expected?);
        let Type::SizedNumeric(Primitive::Numerus, width) = self.types.get(expected) else {
            return None;
        };
        if !width.is_decimal() {
            return None;
        }
        let interner = self.interner?;
        let digits = interner.resolve(text);
        let Some((mantissa, exp10)) = radix_types::numeric_width::decimal_text_to_scaled(digits)
        else {
            self.error_issue(
                SemanticErrorKind::TypeMismatch,
                DECIMAL_LITERAL_UNPARSEABLE,
                span,
            );
            return Some(self.error_type);
        };
        // The scaled carrier holds mantissa × 10^(exp10 + scale); a negative
        // shift means digits beyond the width's scale (precision loss) and a
        // positive shift that overflows i128 exceeds the carrier domain.
        let scale = i32::try_from(width.decimal_scale()?).ok()?;
        let Some(shift) = exp10.checked_add(scale) else {
            return Some(self.decimal_literal_out_of_range(span));
        };
        if shift < 0 {
            return Some(self.decimal_literal_out_of_range(span));
        }
        let factor = i128::from(10u32).checked_pow(shift.cast_unsigned());
        let carrier_max: i128 = match width {
            NumericWidth::D64 => i128::from(i64::MAX),
            _ => return None,
        };
        if factor
            .and_then(|f| mantissa.checked_mul(f))
            .is_none_or(|carrier| carrier > carrier_max)
        {
            return Some(self.decimal_literal_out_of_range(span));
        }
        Some(expected)
    }

    fn decimal_literal_out_of_range(&mut self, span: radix_lexer::Span) -> TypeId {
        self.error_issue(
            SemanticErrorKind::TypeMismatch,
            DECIMAL_LITERAL_OUT_OF_RANGE,
            span,
        );
        self.error_type
    }

    /// Whether an integer literal magnitude exceeds the domain of its resolved
    /// numerus target. `numerus<u64>` is the only full-range unsigned target;
    /// signed widths and the narrower unsigned widths keep the pre-u64-carrier
    /// lexical cap at `i64::MAX` so no existing acceptance silently widens.
    fn literal_exceeds_signed_numerus_domain(&self, literal_ty: TypeId, value: u64) -> bool {
        let resolved = self.resolve_inferred_type(literal_ty);
        match self.types.get(resolved) {
            Type::Primitive(Primitive::Numerus) => value > i64::MAX as u64,
            // `u64` and the unbounded `inf` hold every magnitude the u64 lexer
            // carrier can spell; literals beyond u64 are `BigInt` (only `inf`).
            Type::SizedNumeric(Primitive::Numerus, width) => {
                !matches!(width, NumericWidth::U64 | NumericWidth::Inf) && value > i64::MAX as u64
            }
            _ => false,
        }
    }

    pub fn interned_literal_payload(&self, sym: Symbol) -> Option<&str> {
        self.interner.map(|interner| interner.resolve(sym))
    }

    fn check_ascii_literal(
        &mut self,
        sym: Symbol,
        span: radix_lexer::Span,
        expected: Option<TypeId>,
    ) -> TypeId {
        let Some(raw) = self.interned_literal_payload(sym) else {
            return self.ascii_type();
        };
        match radix_lexer::ascii_literal::decode_ascii_literal(raw) {
            Ok(decoded) => {
                // A bounded expected type is the literal's real carrier
                // contract. Check concrete capacities here so a literal cannot
                // silently widen to bare `ascii` before the assignment check.
                if let Some(expected) = expected.map(|ty| self.resolve_inferred_type(ty))
                    && let Type::BoundedAscii { capacity } = self.types.get(expected)
                {
                    let capacity = self.resolve_index(*capacity);
                    if let IndexExpr::Literal(capacity) = self.types.get_index(capacity) {
                        if decoded.len() as u64 > *capacity {
                            self.invalid_operand_types_with_args(
                                "ascii_literal_too_long",
                                span,
                                &[("capacity", capacity.to_string())],
                            );
                            return self.error_type;
                        }
                        // Keep the expected concrete carrier so Rust emit
                        // constructs `AsciiN<N>` rather than widening the
                        // literal to bare `Ascii`.
                        return expected;
                    }
                }

                // D10.5: a one-letter literal where a `littera` is expected
                // is that scalar (`ascii<1> → littera`), so backends emit a
                // character literal rather than a one-letter string.
                if decoded.len() == 1
                    && let Some(expected) = expected
                    && self.is_littera(expected)
                {
                    return self.littera_type();
                }
                // Bare ascii is unbounded: `ascii_literal_too_long` is reserved
                // for `ascii<N>`. One-byte payloads stay `ascii<1>`.
                if decoded.len() == 1 {
                    return self.ascii_one_type();
                }
                self.ascii_type()
            }
            Err(radix_lexer::ascii_literal::AsciiLiteralError::NonAsciiScalar(ch)) => {
                self.invalid_operand_types_with_args(
                    ISSUE_ASCII_LITERAL_NON_ASCII_SCALAR,
                    span,
                    &[("character", ch.to_string())],
                );
                self.error_type
            }
            Err(radix_lexer::ascii_literal::AsciiLiteralError::UnescapedControl(ch)) => {
                self.invalid_operand_types_with_args(
                    ISSUE_ASCII_LITERAL_UNESCAPED_CONTROL,
                    span,
                    &[("codepoint", format!("{:04X}", ch as u32))],
                );
                self.error_type
            }
            Err(radix_lexer::ascii_literal::AsciiLiteralError::InvalidEscape) => {
                self.invalid_operand_types(ISSUE_ASCII_LITERAL_INVALID_ESCAPE, span);
                self.error_type
            }
        }
    }

    fn check_octeti_literal(&mut self, sym: Symbol, span: radix_lexer::Span) -> TypeId {
        let Some(raw) = self.interned_literal_payload(sym) else {
            return self.octeti_type();
        };
        match radix_lexer::octeti_literal::decode_octeti_hex(raw) {
            Ok(_) => self.octeti_type(),
            Err(radix_lexer::octeti_literal::OctetiLiteralError::InvalidCharacter(ch)) => {
                self.invalid_operand_types_with_args(
                    ISSUE_OCTETI_LITERAL_INVALID_CHARACTER,
                    span,
                    &[("character", ch.to_string())],
                );
                self.error_type
            }
            Err(radix_lexer::octeti_literal::OctetiLiteralError::OddDigitCount) => {
                self.invalid_operand_types(ISSUE_OCTETI_LITERAL_ODD_DIGIT_COUNT, span);
                self.error_type
            }
        }
    }

    /// Type a compile-time JSON literal as `json`, or as an annotated `tabula<K,V>`
    /// when `expected` names a compatible map.
    ///
    /// Valid map ascribes return the expected `Map` type so directional
    /// `unify_assign` succeeds; incompatible ascribes emit per-entry diagnostics
    /// and fall back to `json`, which `unify_assign` then rejects.
    fn check_json_valor_literal(
        &mut self,
        value: &radix_syntax::JsonValue,
        span: radix_lexer::Span,
        expected: Option<TypeId>,
    ) -> TypeId {
        use radix_syntax::JsonValue;

        self.reject_duplicate_json_keys(value);

        let Some(expected) = expected else {
            return self.json_type();
        };
        let resolved = self.resolve_inferred_type(expected);
        let Type::Map(key_ty, value_ty) = self.types.get(resolved).clone() else {
            return self.json_type();
        };

        let JsonValue::Object(members) = value else {
            return self.json_type();
        };

        let textus = self.textus_type();
        if !self.types.is_assignable(textus, key_ty) {
            let key_span = members.first().map_or(span, |member| member.key_span);
            self.error_issue(
                SemanticErrorKind::TypeMismatch,
                ISSUE_JSON_MAP_KEY_MUST_BE_TEXTUS,
                key_span,
            );
            return self.json_type();
        }

        let accepts_nested = matches!(
            self.types.get(self.resolve_inferred_type(value_ty)),
            Type::Primitive(Primitive::Valor)
        );
        let mut ok = true;
        for member in members {
            let nested = matches!(member.value, JsonValue::Object(_) | JsonValue::Array(_));
            if nested && !accepts_nested {
                self.error_issue(
                    SemanticErrorKind::TypeMismatch,
                    ISSUE_JSON_NESTED_VALUE_REQUIRES_VALOR,
                    member.key_span,
                );
                ok = false;
                continue;
            }

            let inferred = self.json_scalar_value_type(&member.value);
            if !(self.types.is_assignable(inferred, value_ty) || nested && accepts_nested) {
                self.error_issue(
                    SemanticErrorKind::TypeMismatch,
                    ISSUE_JSON_VALUE_TYPE_MISMATCH,
                    member.key_span,
                );
                ok = false;
            }
        }

        if ok { expected } else { self.json_type() }
    }

    /// Infer the compile-time scalar type of a JSON value for map ascribe checks.
    fn json_scalar_value_type(&self, value: &radix_syntax::JsonValue) -> TypeId {
        use radix_syntax::JsonValue;

        match value {
            JsonValue::Numerus(_) => self.types.primitive(Primitive::Numerus),
            JsonValue::Fractus(_) => self.types.primitive(Primitive::Fractus),
            JsonValue::String(_) => self.types.primitive(Primitive::Textus),
            JsonValue::Bool(_) => self.types.primitive(Primitive::Bivalens),
            JsonValue::Null => self.types.primitive(Primitive::Nihil),
            JsonValue::Object(_) | JsonValue::Array(_) => self.types.primitive(Primitive::Valor),
        }
    }

    /// Walk the JSON tree and emit a diagnostic for every duplicate key in any
    /// object, pointing at the second occurrence. Nested objects are checked too.
    fn reject_duplicate_json_keys(&mut self, value: &radix_syntax::JsonValue) {
        match value {
            radix_syntax::JsonValue::Object(members) => {
                let mut seen: rustc_hash::FxHashSet<&str> = rustc_hash::FxHashSet::default();
                let Some(interner) = self.interner else {
                    return;
                };
                for member in members {
                    let key_text = interner.resolve(member.key);
                    if !seen.insert(key_text) {
                        self.invalid_operand_types_with_args(
                            ISSUE_JSON_DUPLICATE_KEY,
                            member.key_span,
                            &[("key", key_text.to_owned())],
                        );
                    }
                }
                for member in members {
                    self.reject_duplicate_json_keys(&member.value);
                }
            }
            radix_syntax::JsonValue::Array(elements) => {
                for element in elements {
                    self.reject_duplicate_json_keys(element);
                }
            }
            radix_syntax::JsonValue::String(_)
            | radix_syntax::JsonValue::Numerus(_)
            | radix_syntax::JsonValue::Fractus(_)
            | radix_syntax::JsonValue::Bool(_)
            | radix_syntax::JsonValue::Null => {}
        }
    }
}

const MODULAR_WORD_LITERAL_OUT_OF_RANGE: &str = "modular_word_literal_out_of_range";
const NUMERUS_LITERAL_OUT_OF_RANGE: &str = "numerus_literal_out_of_range";
const DECIMAL_LITERAL_UNPARSEABLE: &str = "decimal_literal_unparseable";
const DECIMAL_LITERAL_OUT_OF_RANGE: &str = "decimal_literal_out_of_range";
