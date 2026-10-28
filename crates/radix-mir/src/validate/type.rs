//! Projection, assignability, and type checks.

use super::{FunctionScope, MirValidationError, Validator};
use crate::generic::MirShapeBindings;
use crate::{
    MirAggregateCarrier, MirAggregateFamily, MirBlockId, MirClosureEnvironmentId, MirConstant,
    MirDeviceViewElement, MirGenericBindings, MirKernelBuiltin, MirLayoutKind, MirLocalId,
    MirOperand, MirProjection, MirScalarLayout, MirType,
};
use handroll::FxHashSet;
use radix_hir::DefId;
use radix_lexer::{Span, Symbol};
use radix_types::{IndexExpr, NumericWidth, Primitive, Type, TypeId};

impl Validator<'_, '_> {
    pub(crate) fn project_closure_capture(
        &mut self,
        environment_id: MirClosureEnvironmentId,
        source: DefId,
        span: Span,
    ) -> Option<MirType> {
        self.context
            .closure_environments
            .get(&environment_id)
            .and_then(|environment| {
                environment
                    .captures
                    .iter()
                    .find(|capture| capture.source == source)
                    .map(|capture| capture.ty)
            })
            .or_else(|| {
                self.error(span, "closure capture projection is missing metadata");
                None
            })
    }

    pub(crate) fn project_field(
        &mut self,
        base_ty: MirType,
        field: Symbol,
        span: Span,
    ) -> Option<MirType> {
        if self.is_handler_error_nuntius_projection(base_ty, field) {
            return Some(self.primitive(Primitive::Textus));
        }
        if matches!(self.type_kind(base_ty), Type::Primitive(Primitive::Ignotum)) {
            return Some(self.primitive(Primitive::Ignotum));
        }
        match self.type_kind(base_ty).clone() {
            Type::Struct(def_id) => self
                .context
                .struct_fields
                .get(&def_id)
                .and_then(|fields| fields.get(&field))
                .copied()
                .or_else(|| {
                    self.error(span, "struct field projection is missing field metadata");
                    None
                }),
            Type::Record(fields) => {
                fields
                    .get(&field)
                    .copied()
                    .map(MirType::semantic)
                    .or_else(|| {
                        self.error(span, "record field projection references unknown field");
                        None
                    })
            }
            Type::Applied(base, args) => {
                let Type::Struct(def_id) = self.context.types.get(base) else {
                    self.error(span, "applied field projection base is not a struct");
                    return None;
                };
                let Some(raw_field_ty) = self
                    .context
                    .struct_fields
                    .get(def_id)
                    .and_then(|fields| fields.get(&field))
                    .copied()
                else {
                    self.error(
                        span,
                        "applied struct field projection is missing field metadata",
                    );
                    return None;
                };
                let Some(params) = self.context.struct_type_params.get(def_id) else {
                    self.error(
                        span,
                        "applied struct field projection is missing type parameters",
                    );
                    return None;
                };
                if params.len() != args.len() {
                    self.error(
                        span,
                        "applied struct field projection type argument count mismatch",
                    );
                    return None;
                }
                let type_args = args
                    .into_iter()
                    .map(|arg| match arg {
                        radix_types::AppliedArg::Type(id) => Some(id),
                        radix_types::AppliedArg::Size(_) => None,
                    })
                    .collect::<Option<Vec<_>>>()?;
                let bindings = params
                    .iter()
                    .copied()
                    .zip(type_args.into_iter().map(MirType::semantic))
                    .collect::<MirGenericBindings>();
                self.instantiate_existing(
                    raw_field_ty,
                    &bindings,
                    &MirShapeBindings::default(),
                    span,
                )
                .map_err(|error| self.error(error.span, error.message))
                .ok()
            }
            Type::Map(key, value) | Type::Ledger(key, value) => {
                self.require_assignable(
                    self.primitive(Primitive::Textus),
                    MirType::semantic(key),
                    span,
                    "map field projection key type mismatch",
                );
                Some(MirType::semantic(value))
            }
            Type::Enum(def_id) => self.project_enum_commune_field(def_id, field, span),
            _ => {
                self.error(span, "field projection base is not a struct or record");
                None
            }
        }
    }

    /// `@ commune` fields are present on every variant payload. Member access
    /// `e.message` is a Field projection on the enum type; it is legal only when
    /// every variant carries the same field at the same type.
    fn project_enum_commune_field(
        &mut self,
        enum_def: DefId,
        field: Symbol,
        span: Span,
    ) -> Option<MirType> {
        let Some(variants) = self.context.enum_variants.get(&enum_def).cloned() else {
            self.error(span, "enum field projection is missing variant metadata");
            return None;
        };
        if variants.is_empty() {
            self.error(span, "enum field projection has no variants");
            return None;
        }
        let mut field_ty = None;
        for variant in variants {
            let Some(ty) = self
                .context
                .variant_fields
                .get(&variant)
                .and_then(|fields| fields.get(&field))
                .copied()
            else {
                self.error(
                    span,
                    "enum field projection is not present on every variant",
                );
                return None;
            };
            match field_ty {
                None => field_ty = Some(ty),
                Some(existing) if existing == ty => {}
                Some(_) => {
                    self.error(span, "enum field projection type disagrees across variants");
                    return None;
                }
            }
        }
        field_ty
    }

    pub(crate) fn project_vector_lane(
        &mut self,
        base_ty: MirType,
        index: u8,
        span: Span,
    ) -> Option<MirType> {
        let Type::Vector(element, width) = self.type_kind(base_ty).clone() else {
            self.error(span, "vector lane projection base is not vector");
            return None;
        };
        match self.context.types.get_index(width) {
            IndexExpr::Literal(width) if u64::from(index) < *width => {
                Some(MirType::semantic(element))
            }
            IndexExpr::Literal(_) => {
                self.error(span, "vector lane projection index is out of bounds");
                None
            }
            _ => {
                self.error(span, "vector lane projection width is not a literal");
                None
            }
        }
    }

    pub(crate) fn project_matrix_cell(
        &mut self,
        base_ty: MirType,
        row: u8,
        column: u8,
        span: Span,
    ) -> Option<MirType> {
        let Type::Matrix(element, shape) = self.type_kind(base_ty).clone() else {
            self.error(span, "matrix cell projection base is not matrix");
            return None;
        };
        match crate::matrix_shape::literal_matrix_shape(shape, self.context.types.as_table()) {
            Some((rows, columns)) if row < rows && column < columns => {
                Some(MirType::semantic(element))
            }
            Some(_) => {
                self.error(span, "matrix cell projection index is out of bounds");
                None
            }
            None => {
                self.error(span, "matrix cell projection shape is not literal");
                None
            }
        }
    }

    pub(crate) fn is_handler_error_nuntius_projection(
        &self,
        base_ty: MirType,
        field: Symbol,
    ) -> bool {
        let Some(interner) = self.context.interner.as_interner() else {
            return false;
        };
        if interner.resolve(field) != crate::HANDLER_MESSAGE_FIELD {
            return false;
        }
        matches!(
            self.type_kind(base_ty),
            Type::Primitive(Primitive::Textus | Primitive::Ignotum)
        )
    }

    pub(crate) fn project_variant_field(
        &mut self,
        base_ty: MirType,
        variant: DefId,
        field: Symbol,
        span: Span,
    ) -> Option<MirType> {
        if let Some(enum_def) = self.enum_def_from_type(base_ty) {
            self.validate_variant_parent(variant, enum_def, span);
        } else {
            self.error(span, "variant field projection base is not enum");
        }
        let raw_field_ty = self
            .context
            .variant_fields
            .get(&variant)
            .and_then(|fields| fields.get(&field))
            .copied()
            .or_else(|| {
                self.error(span, "variant field projection is missing field metadata");
                None
            })?;
        self.instantiate_variant_field_ty(base_ty, raw_field_ty, span)
    }

    pub(crate) fn validate_variant_parent(&mut self, variant: DefId, enum_def: DefId, span: Span) {
        match self.context.variant_parents.get(&variant).copied() {
            Some(parent) if parent == enum_def => {}
            Some(_) => self.error(span, "enum variant does not belong to enum type"),
            None => self.error(span, "enum variant is missing parent metadata"),
        }
    }

    pub(crate) fn project_index(
        &mut self,
        base_ty: MirType,
        index: Option<(&MirOperand, MirType)>,
        span: Span,
    ) -> Option<MirType> {
        match self.type_kind(base_ty).clone() {
            Type::Array(inner)
            | Type::BoundedArray { element: inner, .. }
            | Type::RangedArray { element: inner, .. }
                if index.is_some_and(|(_, index_ty)| {
                    matches!(self.type_kind(index_ty), Type::Intervallum(..))
                }) =>
            {
                // A range index is a window: a new `lista<T>` of the range
                // (fully in bounds or a failure; never short, never padded).
                let (_, index_ty) = index?;
                if let Some(bound) = self.intervallum_bound(index_ty) {
                    self.require_assignable(
                        MirType::semantic(bound),
                        self.primitive(Primitive::Numerus),
                        span,
                        "list window range bounds are not numerus",
                    );
                }
                let window = self.context.types.as_table().find_array(inner);
                if window.is_none() {
                    self.error(
                        span,
                        "list window result type is missing from the type table",
                    );
                }
                window.map(MirType::semantic)
            }
            Type::Array(inner) => {
                self.require_numerus_index(index, span, "array index is not numerus");
                Some(MirType::semantic(inner))
            }
            // Queue/stack iteration reads the array-backed runtime carrier
            // through `Collection(Index)`, while the semantic types remain
            // opaque handles for their method surfaces.
            Type::Queue(inner)
            | Type::BoundedQueue { element: inner, .. }
            | Type::Stack(inner)
            | Type::BoundedStack { element: inner, .. } => {
                self.require_numerus_index(index, span, "queue or stack index is not numerus");
                Some(MirType::semantic(inner))
            }
            // Route B (AEU-4): bounded lista element gathers index the live
            // prefix lanes only (the lowerer emits constants below len).
            Type::BoundedArray { element, .. } | Type::RangedArray { element, .. } => {
                self.require_numerus_index(index, span, "bounded array index is not numerus");
                Some(MirType::semantic(element))
            }
            // Route B (AEU-7): scalar bounded text buffer element gathers —
            // the `ascii<N>[i]` / `textus<N>[i]` → `ascii<1>` rows (D14 /
            // D17). Like the lista arm, the lowerer emits only constant lanes
            // below len, so the non-ASCII trap row can never fire from a dead
            // slot. `octeti<N>` is not here: a range index over it is the same
            // `octeti` window as the unbounded carrier (see below), and a
            // scalar read routes through `project_octeti_index` with the
            // element-type tables.
            Type::BoundedAscii { .. } | Type::BoundedTextus { .. } => {
                if let Some((_, index_ty)) = index
                    && let Some(bound) = self.intervallum_bound(index_ty)
                {
                    self.require_assignable(
                        MirType::semantic(bound),
                        self.primitive(Primitive::Numerus),
                        span,
                        "bounded cord index range bounds are not numerus",
                    );
                    return Some(self.primitive(Primitive::Textus));
                }
                self.require_numerus_index(index, span, "bounded buffer index is not numerus");
                match crate::ty::index_projection_result_ty(base_ty, self.context.types.as_table())
                {
                    Ok(ty) => Some(ty),
                    Err(error) => {
                        self.error(span, error.message);
                        None
                    }
                }
            }
            // A run-time lane select on a vector register value: lane `i` when
            // `0 <= i < N`, the checked-index failure otherwise (the constant
            // lane is `VectorLane`).
            Type::Vector(element, _) => {
                self.require_numerus_index(index, span, "vector lane index is not numerus");
                Some(MirType::semantic(element))
            }
            Type::Tensor(inner, _) => {
                if let Some((_, index_ty)) = index {
                    self.require_device_view_index(index_ty, span);
                }
                Some(MirType::semantic(inner))
            }
            Type::Map(key, value) | Type::Ledger(key, value) => {
                if let Some((index_op, index_ty)) = index {
                    self.require_assignable_or_fitting_constant(
                        index_op,
                        index_ty,
                        MirType::semantic(key),
                        span,
                        "map index key type mismatch",
                    );
                }
                Some(MirType::semantic(value))
            }
            Type::Primitive(Primitive::Textus) => self.project_textus_index(index, span),
            Type::Primitive(Primitive::Octeti) | Type::BoundedOcteti { .. } => {
                self.project_octeti_index(base_ty, index, span)
            }
            Type::Tuple(elements) => self.project_index_tuple(&elements, index, span),
            _ => {
                self.error(span, "index projection base is not indexable");
                None
            }
        }
    }

    /// The bound type of an `intervallum` used as a range index, if the index
    /// is one.
    fn intervallum_bound(&self, ty: MirType) -> Option<TypeId> {
        match self.type_kind(ty) {
            Type::Intervallum(bound, _) => Some(*bound),
            _ => None,
        }
    }

    /// Range-bound check shared by the scalar collection index arms that read
    /// the live prefix lanes (lista, bounded lista, queue/stack, vector).
    fn require_numerus_index(
        &mut self,
        index: Option<(&MirOperand, MirType)>,
        span: Span,
        message: &str,
    ) {
        if let Some((_, index_ty)) = index {
            self.require_assignable(index_ty, self.primitive(Primitive::Numerus), span, message);
        }
    }

    fn project_textus_index(
        &mut self,
        index: Option<(&MirOperand, MirType)>,
        span: Span,
    ) -> Option<MirType> {
        if let Some((_, index_ty)) = index {
            // A range index is a window read, not a scalar numerus key
            // (mirrors the octeti arm above): bounds must still be
            // integer-valued, and the window stays a textus substring.
            if let Some(range_bound) = self.intervallum_bound(index_ty) {
                self.require_assignable(
                    MirType::semantic(range_bound),
                    self.primitive(Primitive::Numerus),
                    span,
                    "textus index range bounds are not numerus",
                );
                return Some(self.primitive(Primitive::Textus));
            }
            self.require_assignable(
                index_ty,
                self.primitive(Primitive::Numerus),
                span,
                "textus index is not numerus",
            );
        }
        Some(self.primitive(Primitive::Textus))
    }

    fn project_octeti_index(
        &mut self,
        base_ty: MirType,
        index: Option<(&MirOperand, MirType)>,
        span: Span,
    ) -> Option<MirType> {
        if let Some((_, index_ty)) = index {
            // A range index is a window read, not a scalar numerus key.
            // Its bounds must still be integer-valued, and the window
            // preserves the octeti carrier for a following conversion.
            if let Some(range_bound) = self.intervallum_bound(index_ty) {
                self.require_assignable(
                    MirType::semantic(range_bound),
                    self.primitive(Primitive::Numerus),
                    span,
                    "octeti index range bounds are not numerus",
                );
                return Some(self.primitive(Primitive::Octeti));
            }
            self.require_assignable(
                index_ty,
                self.primitive(Primitive::Numerus),
                span,
                "octeti index is not numerus",
            );
        }
        match crate::ty::index_projection_result_ty(base_ty, self.context.types.as_table()) {
            Ok(ty) => Some(ty),
            Err(error) => {
                self.error(span, error.message);
                None
            }
        }
    }

    fn project_index_tuple(
        &mut self,
        elements: &[TypeId],
        index: Option<(&MirOperand, MirType)>,
        span: Span,
    ) -> Option<MirType> {
        match index {
            Some((MirOperand::Constant(MirConstant::Int(n)), _)) => {
                // Negative or non-fitting indices are out of bounds; usize::MAX is the
                // out-of-range sentinel (matches the old `as` wrap for negatives on 64-bit).
                let idx = usize::try_from(*n).unwrap_or(usize::MAX);
                if idx >= elements.len() {
                    self.error(
                        span,
                        format!("tuple index {n} out of bounds for arity {}", elements.len()),
                    );
                    None
                } else {
                    Some(MirType::semantic(elements[idx]))
                }
            }
            Some((MirOperand::Constant(MirConstant::UInt(n)), _)) => {
                // Non-fitting indices are out of bounds; usize::MAX is the out-of-range sentinel.
                let idx = usize::try_from(*n).unwrap_or(usize::MAX);
                if idx >= elements.len() {
                    self.error(
                        span,
                        format!("tuple index {n} out of bounds for arity {}", elements.len()),
                    );
                    None
                } else {
                    Some(MirType::semantic(elements[idx]))
                }
            }
            Some(_) => {
                self.error(span, "tuple subscript requires compile-time constant index");
                None
            }
            None => None,
        }
    }

    pub(crate) fn validate_projection_representation(
        &mut self,
        base_ty: MirType,
        result_ty: MirType,
        projection: &MirProjection,
        span: Span,
    ) {
        let Ok(base_layout) = self.context.layouts.layout_for_type(base_ty) else {
            return;
        };
        match base_layout.kind {
            MirLayoutKind::DeviceView(view) => {
                if !matches!(projection, MirProjection::Index(_)) {
                    self.error(span, "device view projection must be an index");
                }
                if !view.row_major || view.offset_elements != 0 {
                    self.error(
                        span,
                        "device view index projection requires materialized row-major storage",
                    );
                }
                // WHY: the index is one integer into row-major storage of any
                // rank; the lowerer linearizes an N-element tensor index.
                let Ok(result_layout) = self.context.layouts.layout_for_type(result_ty) else {
                    self.error(span, "device view projection result has no layout");
                    return;
                };
                let expected = match view.element {
                    MirDeviceViewElement::Scalar(scalar) => Some(MirLayoutKind::Scalar(scalar)),
                    MirDeviceViewElement::Vector(vector) => {
                        Some(MirLayoutKind::DeviceVector(vector))
                    }
                    // WHY: a record element's byte facts come from
                    // `resolve_struct_layout`, not the layout table; the index
                    // projection is admitted by element type identity.
                    MirDeviceViewElement::Record(record_ty) => {
                        if result_ty.semantic_id() != record_ty {
                            self.error(span, "device view projection element layout mismatch");
                        }
                        None
                    }
                };
                // `octeti<N>[lo‥hi]` is an `octeti` window: the static byte
                // buffer projects to the opaque unbounded carrier, not to its
                // `u8` cell. A scalar `octeti<N>[i]` still yields the `u8`
                // element through the same element-type tables.
                let octeti_window = view.length.is_some()
                    && matches!(
                        view.element,
                        MirDeviceViewElement::Scalar(MirScalarLayout::U8)
                    )
                    && matches!(
                        self.type_kind(result_ty),
                        Type::Primitive(Primitive::Octeti)
                    );
                if expected.is_some_and(|expected| result_layout.kind != expected) && !octeti_window
                {
                    self.error(span, "device view projection element layout mismatch");
                }
            }
            // Route B (AEU-4): the bounded-array row is an indexable physical
            // aggregate — `element × capacity` storage cells; the gather reads
            // the live prefix lanes by constant index below len. The AEU-7
            // byte families (`ascii<N>` / `textus<N>`, `u8 × N` plus the
            // runtime length member) ride the same contract.
            MirLayoutKind::Aggregate(layout)
                if layout.carrier == MirAggregateCarrier::PhysicalValue
                    && matches!(
                        layout.family,
                        MirAggregateFamily::BoundedArray
                            | MirAggregateFamily::BoundedAscii
                            | MirAggregateFamily::BoundedTextus
                    ) =>
            {
                if !matches!(projection, MirProjection::Index(_)) {
                    self.error(span, "bounded array projection must be an index");
                }
                let Ok(result_layout) = self.context.layouts.layout_for_type(result_ty) else {
                    self.error(span, "bounded array projection result has no layout");
                    return;
                };
                if let (Some(element), MirLayoutKind::Scalar(result_scalar)) =
                    (layout.element, result_layout.kind)
                    && result_scalar != element
                {
                    self.error(span, "bounded array projection element layout mismatch");
                }
            }
            // A genus (struct) value's field read: the field's byte facts and
            // element come from `resolve_struct_layout`, which the layout table
            // cannot see. The result must be that field's own element — a
            // scalar, a device vector, or the physical fixed-array aggregate
            // (`list<T, N>`, validated at its own index projection below).
            MirLayoutKind::Aggregate(layout)
                if layout.carrier == MirAggregateCarrier::PhysicalValue
                    && layout.family == MirAggregateFamily::Struct =>
            {
                let MirProjection::Field(name) = projection else {
                    self.error(span, "genus value projection must be a field read");
                    return;
                };
                // A refusal (`text field`, `f64 field`, `nested genus
                // field`, …) is the device layout law declining the
                // struct, not an ordinary-execution defect. The device
                // gate surfaces it by name when a kernel actually
                // receives the genus (C4-6b); a plain field read is
                // admitted without the byte facts.
                let Ok(struct_layout) = crate::resolve_struct_layout(base_ty, self.context) else {
                    return;
                };
                let Some(field) = struct_layout
                    .fields
                    .iter()
                    .find(|field| field.name == *name)
                else {
                    self.error(
                        span,
                        "genus field projection references an undeclared field",
                    );
                    return;
                };
                let Ok(result_layout) = self.context.layouts.layout_for_type(result_ty) else {
                    self.error(span, "genus field projection result has no layout");
                    return;
                };
                if matches!(
                    self.type_kind(result_ty),
                    Type::BoundedArray { .. } | Type::RangedArray { .. }
                ) {
                    let physical_array = matches!(
                        result_layout.kind,
                        MirLayoutKind::Aggregate(array)
                            if array.family == MirAggregateFamily::BoundedArray
                                && array.carrier == MirAggregateCarrier::PhysicalValue
                    );
                    if !physical_array {
                        self.error(span, "genus field projection element layout mismatch");
                    }
                    return;
                }
                let expected = match field.element {
                    MirDeviceViewElement::Scalar(scalar) => Some(MirLayoutKind::Scalar(scalar)),
                    MirDeviceViewElement::Vector(vector) => {
                        Some(MirLayoutKind::DeviceVector(vector))
                    }
                    MirDeviceViewElement::Record(_) => None,
                };
                if expected.is_some_and(|expected| result_layout.kind != expected) {
                    self.error(span, "genus field projection element layout mismatch");
                }
            }
            MirLayoutKind::Aggregate(layout)
                if layout.carrier == MirAggregateCarrier::PhysicalValue
                    && layout.family != MirAggregateFamily::Tuple =>
            {
                if matches!(projection, MirProjection::Index(_)) {
                    self.error(
                        span,
                        "physical aggregate representation has no index projection contract",
                    );
                }
            }
            MirLayoutKind::Aggregate(layout)
                if layout.family == MirAggregateFamily::TensorRuntime =>
            {
                if matches!(projection, MirProjection::Index(_)) {
                    self.error(
                        span,
                        "tensor runtime aggregate has no device-view projection contract",
                    );
                }
            }
            MirLayoutKind::DeviceVector(vector) => {
                self.validate_device_vector_projection(vector.element, result_ty, projection, span);
            }
            MirLayoutKind::DeviceMatrix(matrix) => {
                if !matches!(projection, MirProjection::MatrixCell { .. }) {
                    self.error(span, "device matrix projection must be a matrix cell");
                }
                let Ok(result_layout) = self.context.layouts.layout_for_type(result_ty) else {
                    self.error(span, "device matrix projection result has no layout");
                    return;
                };
                if result_layout.kind != MirLayoutKind::Scalar(matrix.element) {
                    self.error(span, "device matrix cell element layout mismatch");
                }
            }
            MirLayoutKind::PackedNumeric(packed) => {
                self.error(span, packed.unsupported_execution_issue());
            }
            MirLayoutKind::Aggregate(_)
            | MirLayoutKind::Scalar(_)
            | MirLayoutKind::Atomic(_)
            | MirLayoutKind::OpaqueHandle
            | MirLayoutKind::Void
            | MirLayoutKind::Never => {}
        }
    }

    fn validate_device_vector_projection(
        &mut self,
        element: MirScalarLayout,
        result_ty: MirType,
        projection: &MirProjection,
        span: Span,
    ) {
        if !matches!(
            projection,
            MirProjection::Field(_) | MirProjection::VectorLane(_) | MirProjection::Index(_)
        ) {
            self.error(
                span,
                "device vector projection must be a swizzle field, lane, or lane select",
            );
        }
        let Ok(result_layout) = self.context.layouts.layout_for_type(result_ty) else {
            self.error(span, "device vector projection result has no layout");
            return;
        };
        if let MirLayoutKind::DeviceVector(result_vector) = result_layout.kind {
            if result_vector.element != element {
                self.error(span, "device vector swizzle element layout mismatch");
            }
        } else if let MirLayoutKind::Scalar(result_scalar) = result_layout.kind {
            if result_scalar != element {
                self.error(span, "device vector lane element layout mismatch");
            }
        } else {
            self.error(
                span,
                "device vector projection result must be a vector or element scalar",
            );
        }
    }

    pub(crate) fn require_device_view_index(&mut self, index_ty: MirType, span: Span) {
        match self.type_kind(index_ty) {
            Type::Primitive(Primitive::Numerus) | Type::SizedNumeric(Primitive::Numerus, _) => {}
            _ => self.error(span, "device view index is not integer"),
        }
    }

    pub(crate) fn require_block(
        &mut self,
        scope: &FunctionScope<'_, '_>,
        block: MirBlockId,
        span: Span,
    ) {
        if !scope.blocks.contains(&block) {
            self.error(span, format!("block bb{} does not exist", block.0));
        }
    }

    pub(crate) fn validate_mir_type(&mut self, ty: MirType, span: Span) {
        let mut seen = FxHashSet::default();
        self.validate_type_id(ty.semantic_id(), span, &mut seen);
    }

    pub(crate) fn validate_type_id(
        &mut self,
        ty: TypeId,
        span: Span,
        seen: &mut FxHashSet<TypeId>,
    ) {
        if !seen.insert(ty) {
            return;
        }

        match self.context.types.get(ty).clone() {
            Type::Infer(_) | Type::InferUnion(_) => {
                self.error(span, "MIR type is unresolved inference variable");
            }
            Type::MarkerHole { .. } => self.error(span, "MIR type is unresolved marker-width hole"),
            Type::Error => self.error(span, "MIR type is semantic error recovery type"),
            Type::TensorEdgePolicy(_) => {
                self.error(span, "MIR type is a type-only tensor edge-policy marker");
            }
            Type::Array(inner)
            | Type::Set(inner)
            | Type::Promissum(inner)
            | Type::Cursor(inner)
            | Type::Queue(inner)
            | Type::Stack(inner)
            | Type::Tensor(inner, _)
            | Type::Vector(inner, _)
            | Type::Matrix(inner, _)
            | Type::Sparsa(inner, _)
            | Type::Atomic(inner)
            | Type::Option(inner)
            | Type::Ref(_, inner)
            | Type::Alias(_, inner)
            | Type::BoundedArray { element: inner, .. }
            | Type::RangedArray { element: inner, .. }
            | Type::BoundedQueue { element: inner, .. }
            | Type::BoundedStack { element: inner, .. } => self.validate_type_id(inner, span, seen),
            Type::PromissumFailable(success, alternate) | Type::AsyncCursor(success, alternate) => {
                self.validate_type_id(success, span, seen);
                self.validate_type_id(alternate, span, seen);
            }
            Type::Map(key, value) | Type::Ledger(key, value) => {
                self.validate_type_id(key, span, seen);
                self.validate_type_id(value, span, seen);
            }
            Type::Record(fields) => {
                for field_ty in fields.values() {
                    self.validate_type_id(*field_ty, span, seen);
                }
            }
            Type::Func(sig) => {
                for param in &sig.params {
                    self.validate_type_id(param.ty, span, seen);
                }
                self.validate_type_id(sig.ret, span, seen);
                if let Some(error_ty) = sig.err {
                    self.validate_type_id(error_ty, span, seen);
                }
            }
            Type::Applied(base, args) => {
                self.validate_type_id(base, span, seen);
                for arg in args {
                    if let radix_types::AppliedArg::Type(id) = arg {
                        self.validate_type_id(id, span, seen);
                    }
                }
            }
            Type::Union(members) | Type::Intersection(members) => {
                for member in members {
                    self.validate_type_id(member, span, seen);
                }
            }
            Type::Primitive(_)
            | Type::SizedNumeric(_, _)
            | Type::ModularWord(_)
            | Type::Bits(_)
            | Type::SaturatingWord(_)
            | Type::RangedInt { .. }
            | Type::PolicyFloat(_, _)
            | Type::SizedInstans(_)
            | Type::Intervallum(..)
            | Type::Tuple(_)
            | Type::Struct(_)
            | Type::Enum(_)
            | Type::Interface(_)
            | Type::Param(_)
            | Type::BoundedTextus { .. }
            | Type::BoundedAscii { .. }
            | Type::BoundedOcteti { .. } => {}
        }
    }

    pub(crate) fn require_assignable(
        &mut self,
        from: MirType,
        to: MirType,
        span: Span,
        message: impl Into<String>,
    ) {
        if !self.assignable(from, to) {
            self.error(span, message);
        }
    }

    /// [`TypeTable::is_assignable`] plus contract conformance: a struct whose
    /// `implet` clause names a contract flows into a slot of that contract's
    /// type. Only the lowered unit knows the clause, so the check lives here.
    pub(crate) fn assignable(&self, from: MirType, to: MirType) -> bool {
        let types = self.context.types.as_table();
        if let (Type::Struct(strukt), Type::Interface(contract)) =
            (types.get(from.semantic_id()), types.get(to.semantic_id()))
        {
            return self
                .context
                .struct_contracts
                .get(strukt)
                .is_some_and(|contracts| contracts.contains(contract));
        }
        types.is_assignable(from.semantic_id(), to.semantic_id())
    }

    /// Like [`Self::require_assignable`], but accepts an integer literal
    /// constant that fits the expected integer or modular type. MIR constants
    /// carry plain `numerus` / `numerus<u64>` types, while source literals in
    /// a typed collection were already range-checked by semantic analysis
    /// against the declared element/key type — re-rejecting them here would
    /// break `lista<modulus<u32>> ← [4294967295, 3]`-style programs.
    pub(crate) fn require_assignable_or_fitting_constant(
        &mut self,
        operand: &MirOperand,
        found: MirType,
        expected: MirType,
        span: Span,
        message: &'static str,
    ) {
        if self.assignable(found, expected) {
            return;
        }
        if self.integer_constant_fits(operand, expected)
            || self.littera_constant_fits(operand, expected)
            || self.policy_float_constant_fits(operand, expected)
        {
            return;
        }
        self.error(span, message);
    }

    /// D10.5 `ascii<1> → littera`: an ASCII literal constant (MIR types every
    /// `MirConstant::Ascii` as bare `ascii`) stands for a one-scalar value
    /// where semantic analysis already typed it `littera` — e.g. the `⊥`
    /// default of `n ↦ littera ⊥ '?'` (D10.6).
    fn littera_constant_fits(&self, operand: &MirOperand, expected: MirType) -> bool {
        matches!(operand, MirOperand::Constant(MirConstant::Ascii(_)))
            && matches!(
                self.type_kind(expected),
                Type::Primitive(Primitive::Littera)
            )
    }

    /// N7E-3 (D4): MIR types every float literal `fractus`, while semantic
    /// analysis already typed the literal as the cell it lands in (ruling 20:
    /// exactly representable and finite) — e.g. the `⊥` default of
    /// `x ↦ trapping<f32> ⊥ 0.5`. A float constant stands for a cell value.
    fn policy_float_constant_fits(&self, operand: &MirOperand, expected: MirType) -> bool {
        matches!(operand, MirOperand::Constant(MirConstant::Float(_)))
            && matches!(self.type_kind(expected), Type::PolicyFloat(_, _))
    }

    pub(crate) fn integer_constant_fits(&self, operand: &MirOperand, expected: MirType) -> bool {
        let MirOperand::Constant(constant) = operand else {
            return false;
        };
        let (signed, unsigned) = match constant {
            MirConstant::Int(v) => (Some(*v), None),
            MirConstant::UInt(v) => (None, Some(*v)),
            // An exact bignum constant fits only the unbounded width (ruling
            // 34: any other landing was a compile error before MIR).
            MirConstant::Magnus(_) => {
                return matches!(
                    self.type_kind(expected),
                    Type::SizedNumeric(Primitive::Numerus, NumericWidth::Inf)
                );
            }
            _ => return false,
        };
        match self.type_kind(expected) {
            Type::SizedNumeric(Primitive::Numerus, width)
            | Type::ModularWord(width)
            | Type::SaturatingWord(width) => {
                let (lo, hi) = numerus_width_range(*width);
                match (signed, unsigned) {
                    (Some(v), _) => (lo..=hi).contains(&i128::from(v)),
                    (_, Some(v)) => (lo..=hi).contains(&i128::from(v)),
                    (None, None) => false,
                }
            }
            // A constant landing in a ranged cell is already folded into its
            // range; the cell holds exactly [lo, hi].
            Type::RangedInt { lo, hi, .. } => match (signed, unsigned) {
                (Some(v), _) => (*lo..=*hi).contains(&i128::from(v)),
                (_, Some(v)) => (*lo..=*hi).contains(&i128::from(v)),
                (None, None) => false,
            },
            Type::Primitive(Primitive::Numerus) => {
                signed.is_some() || unsigned.is_some_and(|v| i64::try_from(v).is_ok())
            }
            // A written integer stands for the raw-bits cell it lands in when
            // its pattern fits N bits (need, B3).
            Type::Bits(width) => match (signed, unsigned) {
                (Some(v), _) => {
                    u64::try_from(v).is_ok_and(|v| width.bits() == 64 || v >> width.bits() == 0)
                }
                (_, Some(v)) => width.bits() == 64 || v >> width.bits() == 0,
                (None, None) => false,
            },
            _ => false,
        }
    }

    pub(crate) fn require_exact(
        &mut self,
        found: MirType,
        expected: MirType,
        span: Span,
        message: &'static str,
    ) {
        if found.semantic_id() != expected.semantic_id() {
            self.error(span, message);
        }
    }

    pub(crate) fn constant_ty(&self, value: &MirConstant) -> MirType {
        match value {
            MirConstant::Int(_) => self.primitive(Primitive::Numerus),
            // Unsigned constants carry the sized `numerus<u64>` type so typed
            // boundaries (tabula keys, copia members) accept them exactly.
            MirConstant::UInt(_) => self
                .context
                .types
                .find_sized_numeric(Primitive::Numerus, NumericWidth::U64)
                .map_or_else(|| self.primitive(Primitive::Numerus), MirType::semantic),
            // A shape-size scalar is a float-sized compile-time value (the
            // symbolic companion divides by it at runtime).
            MirConstant::Float(_) | MirConstant::ShapeSize { .. } => {
                self.primitive(Primitive::Fractus)
            }
            MirConstant::String(_) => self.primitive(Primitive::Textus),
            MirConstant::Ascii(_) => self.primitive(Primitive::Ascii),
            MirConstant::Bool(_) => self.primitive(Primitive::Bivalens),
            MirConstant::Nil => self.primitive(Primitive::Nihil),
            MirConstant::Unit => self.primitive(Primitive::Vacuum),
            MirConstant::Octeti(_) => self.primitive(Primitive::Octeti),
            MirConstant::Regex { .. } => self.primitive(Primitive::Regex),
            MirConstant::Function(_) => self.primitive(Primitive::Ignotum),
            // An exact bignum constant is the unbounded integer; the sized
            // type is interned whenever an `inf` is declared, and the
            // fallback keeps hand-built programs typing as a plain integer.
            MirConstant::Magnus(_) => self
                .context
                .types
                .find_sized_numeric(Primitive::Numerus, NumericWidth::Inf)
                .map_or_else(|| self.primitive(Primitive::Numerus), MirType::semantic),
        }
    }

    pub(crate) fn primitive(&self, primitive: Primitive) -> MirType {
        MirType::semantic(self.context.types.primitive(primitive))
    }

    pub(crate) fn is_u32_scalar(&self, ty: MirType) -> bool {
        self.context
            .layouts
            .layout_for_type(ty)
            .is_ok_and(|layout| layout.kind == MirLayoutKind::Scalar(MirScalarLayout::U32))
    }

    pub(crate) fn is_u32_vector3(&self, ty: MirType) -> bool {
        let Ok(layout) = self.context.layouts.layout_for_type(ty) else {
            return false;
        };
        let MirLayoutKind::DeviceVector(vector) = layout.kind else {
            return false;
        };
        vector.element == MirScalarLayout::U32 && vector.width == MirKernelBuiltin::VECTOR_WIDTH
    }

    pub(crate) fn is_vacuum(&self, ty: MirType) -> bool {
        ty.semantic_id() == self.context.types.primitive(Primitive::Vacuum)
    }

    pub(crate) fn is_unit_return(&self, ty: MirType) -> bool {
        matches!(
            self.context.types.get(ty.semantic_id()),
            Type::Primitive(Primitive::Vacuum | Primitive::Nihil)
        )
    }

    pub(crate) fn is_numquam(&self, ty: MirType) -> bool {
        ty.semantic_id() == self.context.types.primitive(Primitive::Numquam)
    }

    pub(crate) fn type_kind(&self, ty: MirType) -> &Type {
        self.context.types.get(ty.semantic_id())
    }

    pub(crate) fn enum_def_from_type(&self, ty: MirType) -> Option<DefId> {
        match self.type_kind(ty) {
            Type::Enum(def_id) => Some(*def_id),
            Type::Applied(base, _) => self.enum_def_from_type(MirType::semantic(*base)),
            Type::Alias(_, inner) | Type::Ref(_, inner) => {
                self.enum_def_from_type(MirType::semantic(*inner))
            }
            _ => None,
        }
    }

    pub(crate) fn nullable_inner_ty(&self, ty: MirType) -> Option<MirType> {
        self.context
            .types
            .nullable_inner(ty.semantic_id())
            .map(MirType::semantic)
    }

    pub(crate) fn report_duplicate_local_ids(&mut self, pairs: &[(MirLocalId, Span)]) {
        for (id, span) in pairs {
            self.error(*span, format!("duplicate MIR local id _{}", id.0));
        }
    }

    pub(crate) fn error(&mut self, span: Span, message: impl Into<String>) {
        self.errors.push(MirValidationError::new(span, message));
    }
}

/// Inclusive `[min, max]` of a `numerus<W>` width as i128 so every width —
/// including `u64` — range-checks without host overflow.
fn numerus_width_range(width: NumericWidth) -> (i128, i128) {
    match width {
        NumericWidth::I8 => (i128::from(i8::MIN), i128::from(i8::MAX)),
        NumericWidth::I16 => (i128::from(i16::MIN), i128::from(i16::MAX)),
        NumericWidth::I32 => (i128::from(i32::MIN), i128::from(i32::MAX)),
        NumericWidth::U8 => (0, i128::from(u8::MAX)),
        NumericWidth::U16 => (0, i128::from(u16::MAX)),
        NumericWidth::U32 => (0, i128::from(u32::MAX)),
        NumericWidth::U64 => (0, i128::from(u64::MAX)),
        // `i64`, the float widths, and the decimal-widths D1 scaled-integer
        // carriers all fit the i64 domain.
        NumericWidth::I64
        | NumericWidth::F16
        | NumericWidth::BF16
        | NumericWidth::F32
        | NumericWidth::F64
        | NumericWidth::D64 => (i128::from(i64::MIN), i128::from(i64::MAX)),
        // `inf` has no bound: every `Int`/`UInt` constant range-checks (a
        // `MirConstant::Magnus` is admitted by `integer_constant_fits`
        // directly, never through this i128 window).
        NumericWidth::Inf => (i128::MIN, i128::MAX),
    }
}
