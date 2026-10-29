// structural: numeric conversions between host value kinds in the runner.
#![allow(
    clippy::cast_lossless,
    clippy::match_same_arms,
    clippy::only_used_in_recursion,
    clippy::too_many_lines,
    clippy::map_unwrap_or,
    clippy::unused_self,
    clippy::wildcard_imports
)]
// Semantic numeric conversions between host value kinds during conversio
// lowering (fractus promotion, lista length to tensor shape dimension).
#![expect(
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    reason = "semantic numeric conversions between host value kinds in conversio lowering"
)]

use super::*;

impl<H: super::super::Host + ?Sized> Runner<'_, H> {
    pub(crate) fn convert_to_array(
        &mut self,
        value: Value,
        source_ty: radix_types::TypeId,
        element: radix_types::TypeId,
        recovery: Option<&MirOperand>,
        struct_defaults: &[MirStructFieldDefaults],
        span: Span,
    ) -> Result<Value, RunnerError> {
        if let Value::Octeti(bytes) = value {
            return self.convert_octeti_to_lista_u8(bytes, element, recovery, span);
        }
        if let Value::Sermo(sermo) = value {
            // `↦ lista<T>`: every content frame, each extracted as `T`.
            let items = self.sermo_items(&sermo, span)?;
            let converted = items
                .into_iter()
                .map(|item| {
                    self.extract_dynamic_value_for_type(
                        item,
                        element,
                        struct_defaults,
                        span,
                        MissingFieldPolicy::Valor,
                    )
                })
                .collect::<Result<Vec<_>, _>>();
            return match converted {
                Ok(items) => Ok(Value::Array(Rc::new(RefCell::new(items)))),
                Err(_) => self.recover_or_fail(recovery, span, "valor to lista conversion failed"),
            };
        }
        match self.convert_to_array_inner(value, source_ty, element, struct_defaults, span) {
            Ok(value) => Ok(value),
            Err(_) => self.recover_or_fail(recovery, span, "valor to lista conversion failed"),
        }
    }

    /// `lista<T> ↦ lista<T, magnitudo N>`: the store-time capacity check. A bounded
    /// list is held as an ordinary `Value::Array`; a source longer than `N`
    /// fails the conversion, and `⊥` recovery supplies the fallback, as for
    /// the other failable conversions.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn convert_to_bounded_array(
        &mut self,
        value: Value,
        source_ty: radix_types::TypeId,
        element: radix_types::TypeId,
        capacity: radix_types::IndexId,
        recovery: Option<&MirOperand>,
        struct_defaults: &[MirStructFieldDefaults],
        span: Span,
    ) -> Result<Value, RunnerError> {
        let radix_types::IndexExpr::Literal(capacity) = self.types.get_index(capacity) else {
            return Err(RunnerError::internal(
                span,
                "bounded lista requires a literal capacity",
            ));
        };
        let capacity = usize::try_from(*capacity).unwrap_or(usize::MAX);
        let Ok(converted) =
            self.convert_to_array_inner(value, source_ty, element, struct_defaults, span)
        else {
            return self.recover_or_fail(
                recovery,
                span,
                "lista to bounded lista conversion failed",
            );
        };
        let fits = matches!(&converted, Value::Array(cell) if cell.borrow().len() <= capacity);
        if fits {
            Ok(converted)
        } else {
            self.recover_or_fail(recovery, span, "lista to bounded lista conversion failed")
        }
    }

    /// `lista<T> ↦ lista<T, magnitudo lo‥hi>`: the copied list is kept only
    /// when its length lies inside `[min, max]`; any other length takes the
    /// recovery value or fails the conversion, like the bounded list's overflow.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn convert_to_ranged_array(
        &mut self,
        value: Value,
        source_ty: radix_types::TypeId,
        element: radix_types::TypeId,
        min: u64,
        max: Option<u64>,
        recovery: Option<&MirOperand>,
        struct_defaults: &[MirStructFieldDefaults],
        span: Span,
    ) -> Result<Value, RunnerError> {
        let Ok(converted) =
            self.convert_to_array_inner(value, source_ty, element, struct_defaults, span)
        else {
            return self.recover_or_fail(recovery, span, "lista to ranged lista conversion failed");
        };
        let fits = matches!(&converted, Value::Array(cell) if {
            let length = cell.borrow().len() as u64;
            length >= min && max.is_none_or(|max| length <= max)
        });
        if fits {
            Ok(converted)
        } else {
            self.recover_or_fail(recovery, span, "lista to ranged lista conversion failed")
        }
    }

    fn convert_octeti_to_lista_u8(
        &mut self,
        bytes: Vec<u8>,
        element: radix_types::TypeId,
        recovery: Option<&MirOperand>,
        span: Span,
    ) -> Result<Value, RunnerError> {
        if !matches!(
            self.types.get(element),
            Type::SizedNumeric(Primitive::Numerus, NumericWidth::U8)
        ) {
            return self.recover_or_fail(
                recovery,
                span,
                "list<b8> to lista conversion requires lista<u8>",
            );
        }
        Ok(Value::Array(Rc::new(RefCell::new(
            bytes
                .into_iter()
                .map(|byte| Value::Int(i64::from(byte)))
                .collect(),
        ))))
    }

    fn convert_to_array_inner(
        &mut self,
        value: Value,
        source_ty: radix_types::TypeId,
        element: radix_types::TypeId,
        struct_defaults: &[MirStructFieldDefaults],
        span: Span,
    ) -> Result<Value, RunnerError> {
        Ok(Value::Array(Rc::new(RefCell::new(match value {
            Value::Array(cell) => self.convert_array_elements(
                &cell.borrow(),
                source_ty,
                element,
                struct_defaults,
                span,
            )?,
            Value::Tensor(cell) => self.tensor_logical_values(&cell, span)?,
            Value::Vector(cell) => self.convert_array_elements(
                &cell.borrow(),
                source_ty,
                element,
                struct_defaults,
                span,
            )?,
            // Register matrices flatten row-major into the same flat list as
            // a tensor; the source element type picks any element conversion.
            Value::Matrix(cell) => {
                let data = cell.borrow().data.clone();
                self.convert_array_elements(&data, source_ty, element, struct_defaults, span)?
            }
            Value::Set(cell) => {
                let mut items: Vec<Value> = cell.borrow().iter().map(MapKey::to_value).collect();
                // D19.2/D19.6: copia order is unspecified; shuffle under the
                // test-only seed so `copia ↦ lista<T>` cannot quietly pin the
                // runner's FxHashSet order.
                crate::shuffle::shuffle_unordered(&mut items);
                items
            }
            Value::Map(cell) => {
                let mut items = self.map_to_array(&cell.borrow(), source_ty, element, span)?;
                crate::shuffle::shuffle_unordered(&mut items);
                items
            }
            other => {
                return Err(RunnerError::internal(
                    span,
                    format!("unsupported conversio to lista from {other:?}"),
                ));
            }
        }))))
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn convert_to_map(
        &mut self,
        value: Value,
        source_ty: TypeId,
        key: TypeId,
        value_ty: TypeId,
        recovery: Option<&MirOperand>,
        struct_defaults: &[MirStructFieldDefaults],
        span: Span,
    ) -> Result<Value, RunnerError> {
        match self.convert_to_map_inner(value, source_ty, key, value_ty, struct_defaults, span) {
            Ok(value) => Ok(value),
            Err(_) => self.recover_or_fail(recovery, span, "valor to tabula conversion failed"),
        }
    }

    fn convert_to_map_inner(
        &mut self,
        value: Value,
        source_ty: TypeId,
        key: TypeId,
        value_ty: TypeId,
        struct_defaults: &[MirStructFieldDefaults],
        span: Span,
    ) -> Result<Value, RunnerError> {
        match value {
            Value::Map(cell) => {
                self.require_textus_map_key(key, span)?;
                let converted = self.convert_map_values(
                    &cell.borrow(),
                    source_ty,
                    value_ty,
                    struct_defaults,
                    span,
                )?;
                Ok(Value::Map(Rc::new(RefCell::new(converted))))
            }
            other => Err(RunnerError::internal(
                span,
                format!("unsupported conversio to tabula from {other:?}"),
            )),
        }
    }

    fn require_textus_map_key(&self, key: TypeId, span: Span) -> Result<(), RunnerError> {
        if matches!(self.types.get(key), Type::Primitive(Primitive::Textus)) {
            return Ok(());
        }
        Err(RunnerError::internal(
            span,
            "valor to tabula extraction requires textus keys",
        ))
    }

    fn convert_array_elements(
        &mut self,
        items: &[Value],
        source_ty: TypeId,
        target_element: TypeId,
        struct_defaults: &[MirStructFieldDefaults],
        span: Span,
    ) -> Result<Vec<Value>, RunnerError> {
        let source_element = match self.types.get(source_ty) {
            Type::Array(element) | Type::Queue(element) | Type::Stack(element) => Some(*element),
            Type::Tensor(element, _) | Type::Vector(element, _) | Type::Matrix(element, _) => {
                Some(*element)
            }
            _ => None,
        };
        let Some(source_element) = source_element else {
            return Ok(items.to_vec());
        };
        if source_element == target_element {
            return Ok(items.to_vec());
        }
        items
            .iter()
            .cloned()
            .map(|item| {
                if self.is_dynamic_source(source_element) {
                    self.extract_dynamic_value_for_type(
                        item,
                        target_element,
                        struct_defaults,
                        span,
                        MissingFieldPolicy::Valor,
                    )
                } else {
                    self.convert_to_target(
                        item,
                        source_element,
                        target_element,
                        &[],
                        None,
                        struct_defaults,
                        span,
                    )
                }
            })
            .collect()
    }

    fn convert_map_values(
        &mut self,
        items: &handroll::FxHashMap<MapKey, Value>,
        source_ty: TypeId,
        target_value: TypeId,
        struct_defaults: &[MirStructFieldDefaults],
        span: Span,
    ) -> Result<handroll::FxHashMap<MapKey, Value>, RunnerError> {
        let source_value = match self.types.get(source_ty) {
            Type::Map(_, value) => Some(*value),
            _ => None,
        };
        let Some(source_value) = source_value else {
            return Ok(items.clone());
        };
        if source_value == target_value {
            return Ok(items.clone());
        }
        let mut out = handroll::FxHashMap::default();
        for (key, item) in items {
            let converted = if self.is_dynamic_source(source_value) {
                self.extract_dynamic_value_for_type(
                    item.clone(),
                    target_value,
                    struct_defaults,
                    span,
                    MissingFieldPolicy::Valor,
                )?
            } else {
                self.convert_to_target(
                    item.clone(),
                    source_value,
                    target_value,
                    &[],
                    None,
                    struct_defaults,
                    span,
                )?
            };
            out.insert(key.clone(), converted);
        }
        Ok(out)
    }

    pub(crate) fn convert_to_set(&self, value: Value, span: Span) -> Result<Value, RunnerError> {
        let mut out = FxHashSet::default();
        match value {
            Value::Array(cell) => {
                for item in cell.borrow().iter() {
                    out.insert(item.to_key(span)?);
                }
            }
            Value::Set(cell) => out.extend(cell.borrow().iter().cloned()),
            other => {
                return Err(RunnerError::internal(
                    span,
                    format!("unsupported conversio to copia from {other:?}"),
                ));
            }
        }
        Ok(Value::Set(Rc::new(RefCell::new(out))))
    }

    /// `lista ↦ queue` and `lista ↦ stack`: all three collections share one
    /// array carrier, so the conversion keeps the carrier order — queue FIFO is
    /// list order, and the stack top is the last element, matching `Append`
    /// pushing at the end and `RemoveLast` popping it. A `copia` source keeps
    /// failing closed; only an array carrier converts.
    pub(crate) fn convert_to_queue_stack(
        &mut self,
        value: Value,
        source_ty: TypeId,
        element: TypeId,
        struct_defaults: &[MirStructFieldDefaults],
        span: Span,
    ) -> Result<Value, RunnerError> {
        match value {
            Value::Array(cell) => {
                let items = self.convert_array_elements(
                    &cell.borrow(),
                    source_ty,
                    element,
                    struct_defaults,
                    span,
                )?;
                Ok(Value::Array(Rc::new(RefCell::new(items))))
            }
            other => Err(RunnerError::unsupported(
                span,
                &format!("runtime conversio to queue/stack from {other:?}"),
            )),
        }
    }

    pub(crate) fn convert_to_eager_sequence(
        &mut self,
        value: Value,
        source_ty: TypeId,
        target_ty: TypeId,
        struct_defaults: &[MirStructFieldDefaults],
        recovery: Option<&MirOperand>,
        span: Span,
    ) -> Result<Value, RunnerError> {
        match self.types.get(target_ty) {
            // Sparse tensors use the same target-neutral in-memory carrier as
            // dense tensors (`Value::Tensor`).  Representation is carried by
            // the declared MIR type; converting between the two therefore
            // preserves the tensor cell and its shape while sparse operations
            // interpret zero-valued elements as absent entries.  Keeping this
            // as a passthrough also mirrors the codegen lanes' `fromTensor`
            // contract without inventing a second runtime carrier.
            Type::Tensor(element, _) | Type::Sparsa(element, _) => match value {
                // BO-M-8: Tensor→Tensor passthrough shares the Rc instead of
                // deep-copying data/shape into a fresh RefCell.
                Value::Tensor(cell) => Ok(Value::Tensor(cell)),
                Value::Array(cell) => {
                    let element = *element;
                    self.convert_lista_to_tensor(&cell, target_ty, element, recovery, span)
                }
                // A register matrix converts like its flat cell list: the
                // row-major data flattened, element-converted when the
                // declared element differs, and built at the target shape.
                Value::Matrix(cell) => {
                    let data = cell.borrow().data.clone();
                    self.convert_register_to_tensor(
                        &data,
                        source_ty,
                        target_ty,
                        *element,
                        struct_defaults,
                        recovery,
                        "matrix to tensor conversion failed",
                        span,
                    )
                }
                // A register vector converts like its flat lane list.
                Value::Vector(cell) => {
                    let lanes = cell.borrow().clone();
                    self.convert_register_to_tensor(
                        &lanes,
                        source_ty,
                        target_ty,
                        *element,
                        struct_defaults,
                        recovery,
                        "vector to tensor conversion failed",
                        span,
                    )
                }
                other => Err(RunnerError::internal(
                    span,
                    format!(
                        "unsupported conversio to {:?} from {other:?}",
                        self.types.get(target_ty)
                    ),
                )),
            },
            Type::Cursor(_) => match value {
                Value::Array(cell) => {
                    Ok(Value::Array(Rc::new(RefCell::new(cell.borrow().clone()))))
                }
                Value::Tensor(cell) => {
                    let values = self.tensor_logical_values(&cell, span)?;
                    Ok(Value::Array(Rc::new(RefCell::new(values))))
                }
                other => Err(RunnerError::internal(
                    span,
                    format!(
                        "unsupported conversio to {:?} from {other:?}",
                        self.types.get(target_ty)
                    ),
                )),
            },
            _ => Err(RunnerError::internal(
                span,
                format!(
                    "unsupported eager sequence target {:?}",
                    self.types.get(target_ty)
                ),
            )),
        }
    }

    /// `lista<E> ↦ tensor<E, [literal shape]>` through the generic Convert.
    ///
    /// WHY: a conversio carrying a `⊥` recovery legitimately arrives here (the
    /// typed no-recovery route lowers through `TensorFromFlat`), so the
    /// element count is checked against the declared extents and a mismatch
    /// takes the recovery instead of building a tensor of the wrong shape. A
    /// shape that is not all-literal keeps the flat inference.
    fn convert_lista_to_tensor(
        &mut self,
        items: &Rc<RefCell<Vec<Value>>>,
        target_ty: TypeId,
        element: TypeId,
        recovery: Option<&MirOperand>,
        span: Span,
    ) -> Result<Value, RunnerError> {
        let data = items.borrow().clone();
        if let Some(ListaToTensorShape::Literal(dims)) =
            resolve_lista_to_tensor_shape(target_ty, self.types)
        {
            let expected = dims.iter().try_fold(1usize, |count, dim| {
                usize::try_from(*dim)
                    .ok()
                    .and_then(|extent| count.checked_mul(extent))
            });
            if expected != Some(data.len()) {
                return self.recover_or_fail(recovery, span, "lista to tensor conversion failed");
            }
        }
        let shape = lista_to_tensor_shape(target_ty, self.types, data.len());
        crate::runtime::tensor_value_typed(self.types, data, Some(element), shape, span)
    }

    /// `matrix<E, [R, C]> ↦ tensor<E2, S>` and `vector<E, size N> ↦ tensor<E2, S>`:
    /// the row-major cells or lanes become the tensor's flat data, with any
    /// declared element conversion applied. A literal target shape must match
    /// the cell count; a mismatch takes the recovery instead of building a
    /// tensor of the wrong shape, as for `lista ↦ tensor`.
    #[allow(clippy::too_many_arguments)]
    fn convert_register_to_tensor(
        &mut self,
        cells: &[Value],
        source_ty: TypeId,
        target_ty: TypeId,
        element: TypeId,
        struct_defaults: &[MirStructFieldDefaults],
        recovery: Option<&MirOperand>,
        mismatch_message: &str,
        span: Span,
    ) -> Result<Value, RunnerError> {
        let data = self.convert_array_elements(cells, source_ty, element, struct_defaults, span)?;
        if let Some(ListaToTensorShape::Literal(dims)) =
            resolve_lista_to_tensor_shape(target_ty, self.types)
        {
            let expected = dims.iter().try_fold(1usize, |count, dim| {
                usize::try_from(*dim)
                    .ok()
                    .and_then(|extent| count.checked_mul(extent))
            });
            if expected != Some(data.len()) {
                return self.recover_or_fail(recovery, span, mismatch_message);
            }
        }
        let shape = lista_to_tensor_shape(target_ty, self.types, data.len());
        crate::runtime::tensor_value_typed(self.types, data, Some(element), shape, span)
    }

    /// `tabula<K, V> ↦ lista<T>`: the target element type picks the
    /// projection (`T` is `K` → keys, `T` is `V` → values), matching the Rust
    /// and LLVM lowerings. When `K` and `V` are the same type the keys win.
    fn map_to_array(
        &self,
        map: &handroll::FxHashMap<MapKey, Value>,
        source_ty: radix_types::TypeId,
        element: radix_types::TypeId,
        span: Span,
    ) -> Result<Vec<Value>, RunnerError> {
        if let Type::Map(key_ty, value_ty) = self.types.get(source_ty) {
            if self.types.equals(*key_ty, element) {
                return Ok(map.keys().map(MapKey::to_value).collect());
            }
            if self.types.equals(*value_ty, element) {
                return Ok(map.values().cloned().collect());
            }
        }
        Err(RunnerError::internal(
            span,
            format!(
                "unsupported tabula to lista element target {:?}",
                self.types.get(element)
            ),
        ))
    }

    pub(crate) fn convert_to_struct(
        &mut self,
        value: Value,
        source_ty: TypeId,
        def_id: radix_hir::DefId,
        recovery: Option<&MirOperand>,
        struct_defaults: &[MirStructFieldDefaults],
        span: Span,
    ) -> Result<Value, RunnerError> {
        let missing_policy =
            if matches!(self.types.get(source_ty), Type::Primitive(Primitive::Json)) {
                MissingFieldPolicy::Json
            } else {
                MissingFieldPolicy::Valor
            };
        match value {
            Value::Struct(source_def, cell) if source_def == def_id => {
                Ok(Value::Struct(source_def, cell))
            }
            Value::Map(cell) => {
                let input = cell.borrow().clone();
                self.extract_struct_from_map(
                    def_id,
                    &input,
                    recovery,
                    struct_defaults,
                    span,
                    missing_policy,
                )
            }
            other => self.recover_or_fail(
                recovery,
                span,
                &format!("unsupported conversio to genus from {other:?} conversion failed"),
            ),
        }
    }

    fn extract_struct_from_map(
        &mut self,
        def_id: radix_hir::DefId,
        input: &handroll::FxHashMap<MapKey, Value>,
        recovery: Option<&MirOperand>,
        struct_defaults: &[MirStructFieldDefaults],
        span: Span,
        missing_policy: MissingFieldPolicy,
    ) -> Result<Value, RunnerError> {
        let Some(fields) = self.context.struct_fields.get(&def_id) else {
            return Err(RunnerError::internal(
                span,
                "genus conversion is missing field metadata",
            ));
        };
        let fields = fields
            .iter()
            .map(|(name, ty)| (*name, ty.semantic_id()))
            .collect::<Vec<_>>();
        let optional = self
            .context
            .optional_struct_fields
            .get(&def_id)
            .cloned()
            .unwrap_or_default();
        let mut out = handroll::FxHashMap::default();
        for (name, ty) in fields {
            let key = MapKey::Str(self.struct_wire_field_key(def_id, name));
            let converted = match input.get(&key).cloned() {
                Some(Value::Nil) if optional.contains(&name) => Value::Nil,
                Some(field_value) => {
                    match self.convert_to_struct_field(field_value, ty, struct_defaults, span) {
                        Ok(value) => value,
                        Err(_) => {
                            return self.recover_or_fail(
                                recovery,
                                span,
                                "valor to genus conversion failed",
                            );
                        }
                    }
                }
                None if optional.contains(&name) => self
                    .eval_struct_default(def_id, name, struct_defaults, span)?
                    .unwrap_or(Value::Nil),
                None => match self.missing_struct_field_default(ty, missing_policy) {
                    Some(default) => self
                        .eval_struct_default(def_id, name, struct_defaults, span)?
                        .unwrap_or(default),
                    None => {
                        return self.recover_or_fail(
                            recovery,
                            span,
                            "valor to genus conversion failed",
                        );
                    }
                },
            };
            out.insert(name, converted);
        }
        Ok(Value::Struct(def_id, Rc::new(RefCell::new(out))))
    }

    fn extract_struct_from_dynamic_map(
        &mut self,
        def_id: radix_hir::DefId,
        input: &handroll::FxHashMap<MapKey, Value>,
        struct_defaults: &[MirStructFieldDefaults],
        span: Span,
        missing_policy: MissingFieldPolicy,
    ) -> Result<Value, RunnerError> {
        let Some(fields) = self.context.struct_fields.get(&def_id) else {
            return Err(RunnerError::internal(
                span,
                "genus conversion is missing field metadata",
            ));
        };
        let fields = fields
            .iter()
            .map(|(name, ty)| (*name, ty.semantic_id()))
            .collect::<Vec<_>>();
        let optional = self
            .context
            .optional_struct_fields
            .get(&def_id)
            .cloned()
            .unwrap_or_default();
        let mut out = FxHashMap::default();
        for (name, ty) in fields {
            let key = self.struct_wire_field_key(def_id, name);
            let key = MapKey::Str(key);
            let converted = match input.get(&key).cloned() {
                Some(Value::Nil) if optional.contains(&name) => Value::Nil,
                Some(field_value) => {
                    match self.extract_dynamic_value_for_type(
                        field_value,
                        ty,
                        struct_defaults,
                        span,
                        missing_policy,
                    ) {
                        Ok(value) => value,
                        Err(_) => {
                            return Err(RunnerError::conversion(
                                span,
                                "valor to genus conversion failed",
                            ));
                        }
                    }
                }
                None if optional.contains(&name) => self
                    .eval_struct_default(def_id, name, struct_defaults, span)?
                    .unwrap_or(Value::Nil),
                None => match self.missing_struct_field_default(ty, missing_policy) {
                    Some(default) => self
                        .eval_struct_default(def_id, name, struct_defaults, span)?
                        .unwrap_or(default),
                    None => {
                        return Err(RunnerError::conversion(
                            span,
                            "valor to genus conversion failed",
                        ));
                    }
                },
            };
            out.insert(name, converted);
        }
        Ok(Value::Struct(def_id, Rc::new(RefCell::new(out))))
    }

    pub(crate) fn struct_wire_field_key(&self, def_id: radix_hir::DefId, name: Symbol) -> String {
        self.context
            .json_struct_field_keys
            .get(&def_id)
            .and_then(|fields| fields.get(&name))
            .cloned()
            .unwrap_or_else(|| self.interner.resolve(name).to_owned())
    }

    fn convert_to_struct_field(
        &mut self,
        value: Value,
        target: TypeId,
        struct_defaults: &[MirStructFieldDefaults],
        span: Span,
    ) -> Result<Value, RunnerError> {
        self.convert_to_target(value, target, target, &[], None, struct_defaults, span)
    }

    pub(crate) fn extract_dynamic_value_for_type(
        &mut self,
        value: Value,
        target: TypeId,
        struct_defaults: &[MirStructFieldDefaults],
        span: Span,
        missing_policy: MissingFieldPolicy,
    ) -> Result<Value, RunnerError> {
        match self.types.get(target) {
            Type::Primitive(Primitive::Nihil) => match value {
                Value::Nil => Ok(Value::Nil),
                _ => Err(RunnerError::conversion(
                    span,
                    "valor to nihil conversion failed",
                )),
            },
            Type::Primitive(Primitive::Bivalens) => match value {
                Value::Bool(value) => Ok(Value::Bool(value)),
                _ => Err(RunnerError::conversion(
                    span,
                    "valor to bivalens conversion failed",
                )),
            },
            // Ruling 40: `valor ↦ inf` takes every integer carrier, including
            // one beyond 64 bits (Rust `Magnus::from_valor`).
            Type::SizedNumeric(Primitive::Numerus, NumericWidth::Inf) => match value {
                Value::Int(_) | Value::UInt(_) | Value::Magnus(_) => Ok(value),
                _ => Err(RunnerError::conversion(
                    span,
                    "valor to numerus conversion failed",
                )),
            },
            Type::Primitive(Primitive::Numerus) => match value {
                Value::Int(value) => Ok(Value::Int(value)),
                _ => Err(RunnerError::conversion(
                    span,
                    "valor to numerus conversion failed",
                )),
            },
            // Need (CTO ruling 2026-10-15): `valor ↦ numerus<W>`
            // applies the same magnitude check as `int ↦ W`, so a payload that
            // does not fit `W` (including a negative into an unsigned width)
            // fails on the `⇥` channel, where `⊥` supplies the default. The
            // `NumericWidth::Inf` arm above is untouched; every `i64` fits its
            // `int_width_range` row.
            Type::SizedNumeric(Primitive::Numerus, width) => match value {
                Value::Int(value) if super::scalar::numerus_fits_width(value, *width) => {
                    Ok(Value::Int(value))
                }
                _ => Err(RunnerError::conversion(
                    span,
                    "valor to numerus conversion failed",
                )),
            },
            Type::Primitive(Primitive::Fractus) | Type::SizedNumeric(Primitive::Fractus, _) => {
                match value {
                    Value::Float(value) => Ok(Value::Float(value)),
                    Value::Int(value) => Ok(Value::Float(value as f64)),
                    _ => Err(RunnerError::conversion(
                        span,
                        "valor to fractus conversion failed",
                    )),
                }
            }
            Type::Primitive(Primitive::Textus) => match value {
                Value::Str(value) => Ok(Value::Str(value)),
                Value::Instans(value) => Ok(Value::Str(value.to_rfc3339())),
                _ => Err(RunnerError::conversion(
                    span,
                    "valor to textus conversion failed",
                )),
            },
            // Authored octeti box as a numeric lista; native hosts may supply
            // opaque octeti. Both carriers must extract without text coercion.
            Type::Primitive(Primitive::Octeti) => match value {
                Value::Octeti(_) | Value::Array(_) => self.convert_to_octeti(value, span),
                _ => Err(RunnerError::conversion(
                    span,
                    "valor to list<b8> conversion failed",
                )),
            },
            Type::Primitive(Primitive::Ascii) => match value {
                Value::Ascii(b) => Ok(Value::Ascii(b)),
                Value::Str(value) if value.is_ascii() => Ok(Value::Str(value)),
                _ => Err(RunnerError::conversion(
                    span,
                    "valor to ascii conversion failed",
                )),
            },
            Type::Primitive(Primitive::Valor) => Ok(value),
            Type::Primitive(Primitive::Json) => {
                self.require_json_value(&value, span)?;
                Ok(value)
            }
            Type::Primitive(Primitive::Instans) | Type::SizedInstans(_) => {
                let precision = instans_precision(self.types.get(target));
                let instans = match &value {
                    Value::Instans(v) => Some(v.ad_praecisionem(precision)),
                    Value::Str(text) => Instans::from_rfc3339(text, precision),
                    Value::Int(epoch) => Some(Instans::from_epoch_integer(*epoch, precision)),
                    other => {
                        return Err(RunnerError::internal(
                            span,
                            format!("unsupported valor to instans source {other:?}"),
                        ));
                    }
                };
                instans.map(Value::Instans).ok_or_else(|| {
                    RunnerError::conversion(span, "valor to instans conversion failed")
                })
            }
            // A dynamic `valor` byte extracts as the unsigned `bN` cell it
            // stands for; a negative or too-wide integer is recoverable
            //.
            Type::Bits(width) => {
                let pattern = match value {
                    Value::UInt(pattern) => pattern,
                    Value::Int(value) if value >= 0 => value.cast_unsigned(),
                    _ => {
                        return Err(RunnerError::conversion(
                            span,
                            "valor to bits conversion failed",
                        ));
                    }
                };
                if !bits_pattern_fits_width(pattern, *width) {
                    return Err(RunnerError::conversion(
                        span,
                        "valor to bits conversion failed",
                    ));
                }
                Ok(Value::UInt(pattern))
            }
            Type::Array(element) => {
                // A host byte value extracts as the `lista<b8>` it stands
                // for.
                if matches!(value, Value::Octeti(_))
                    && matches!(
                        self.types.get(self.resolve_conversion_target(*element)),
                        Type::Bits(radix_types::BitsWidth::B8)
                    )
                {
                    return Ok(super::octeti_into_bits_list(value));
                }
                let Value::Array(items) = value else {
                    return Err(RunnerError::conversion(
                        span,
                        "valor to lista conversion failed",
                    ));
                };
                let converted = items
                    .borrow()
                    .iter()
                    .map(|item| {
                        self.extract_dynamic_value_for_type(
                            item.clone(),
                            *element,
                            struct_defaults,
                            span,
                            missing_policy,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Value::Array(Rc::new(RefCell::new(converted))))
            }
            Type::Map(key, value_ty) => {
                self.require_textus_map_key(*key, span)?;
                let Value::Map(items) = value else {
                    return Err(RunnerError::conversion(
                        span,
                        "valor to tabula conversion failed",
                    ));
                };
                let mut out = FxHashMap::default();
                for (key, item) in items.borrow().iter() {
                    let MapKey::Str(key) = key else {
                        return Err(RunnerError::internal(
                            span,
                            "valor to tabula extraction requires textus keys",
                        ));
                    };
                    out.insert(
                        MapKey::Str(key.clone()),
                        self.extract_dynamic_value_for_type(
                            item.clone(),
                            *value_ty,
                            struct_defaults,
                            span,
                            missing_policy,
                        )?,
                    );
                }
                Ok(Value::Map(Rc::new(RefCell::new(out))))
            }
            Type::Struct(def_id) => match value {
                Value::Struct(source_def, cell) if source_def == *def_id => {
                    Ok(Value::Struct(source_def, cell))
                }
                Value::Map(cell) => {
                    let input = cell.borrow().clone();
                    self.extract_struct_from_dynamic_map(
                        *def_id,
                        &input,
                        struct_defaults,
                        span,
                        missing_policy,
                    )
                }
                _ => Err(RunnerError::conversion(
                    span,
                    "valor to genus conversion failed",
                )),
            },
            Type::Tensor(element, _) => {
                let Value::Array(items) = value else {
                    return Err(RunnerError::conversion(
                        span,
                        "valor to tensor conversion failed",
                    ));
                };
                let data = items
                    .borrow()
                    .iter()
                    .map(|item| {
                        self.extract_dynamic_value_for_type(
                            item.clone(),
                            *element,
                            struct_defaults,
                            span,
                            missing_policy,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let shape = lista_to_tensor_shape(target, self.types, data.len());
                crate::runtime::tensor_value_typed(self.types, data, Some(*element), shape, span)
            }
            other => Err(RunnerError::internal(
                span,
                format!("unsupported valor extraction target {other:?}"),
            )),
        }
    }

    fn eval_struct_default(
        &mut self,
        def_id: radix_hir::DefId,
        field: Symbol,
        struct_defaults: &[MirStructFieldDefaults],
        span: Span,
    ) -> Result<Option<Value>, RunnerError> {
        let Some(defaults) = struct_defaults
            .iter()
            .find(|defaults| defaults.def_id == def_id)
        else {
            return Ok(None);
        };
        let Some(default) = defaults.fields.iter().find(|default| default.name == field) else {
            return Ok(None);
        };
        self.eval_named_default(default, span).map(Some)
    }

    fn eval_named_default(
        &mut self,
        default: &MirNamedOperand,
        span: Span,
    ) -> Result<Value, RunnerError> {
        self.eval_operand(&default.value, span)
    }

    fn missing_struct_field_default(
        &self,
        target: TypeId,
        missing_policy: MissingFieldPolicy,
    ) -> Option<Value> {
        if matches!(missing_policy, MissingFieldPolicy::Json) {
            return match self.types.get(target) {
                Type::Primitive(Primitive::Nihil) | Type::Option(_) => Some(Value::Nil),
                _ => None,
            };
        }
        match self.types.get(target) {
            Type::Primitive(Primitive::Nihil) | Type::Option(_) => Some(Value::Nil),
            Type::Primitive(Primitive::Bivalens) => Some(Value::Bool(false)),
            Type::Primitive(Primitive::Numerus) | Type::SizedNumeric(Primitive::Numerus, _) => {
                Some(Value::Int(0))
            }
            Type::Primitive(Primitive::Fractus) | Type::SizedNumeric(Primitive::Fractus, _) => {
                Some(Value::Float(0.0))
            }
            Type::Primitive(Primitive::Textus | Primitive::Ascii) => {
                Some(Value::Str(String::new()))
            }
            Type::Primitive(Primitive::Octeti) | Type::BoundedOcteti { .. } => {
                Some(Value::Octeti(Vec::new()))
            }
            Type::Array(_) => Some(Value::Array(Rc::new(RefCell::new(Vec::new())))),
            Type::Map(_, _) => Some(Value::Map(Rc::new(RefCell::new(
                handroll::FxHashMap::default(),
            )))),
            Type::Ledger(_, _) => Some(Value::Ledger(Rc::new(RefCell::new(
                crate::value::LedgerValue::default(),
            )))),
            Type::Set(_) => Some(Value::Set(Rc::new(RefCell::new(
                handroll::FxHashSet::default(),
            )))),
            _ => None,
        }
    }

    pub(crate) fn recover_or_fail(
        &mut self,
        recovery: Option<&MirOperand>,
        span: Span,
        message: &str,
    ) -> Result<Value, RunnerError> {
        if let Some(recovery) = recovery {
            self.eval_recovery(recovery, span)
        } else {
            Err(RunnerError::conversion(span, message))
        }
    }
}

/// Whether an unsigned `bN` pattern fits `width` (64 bits holds any `u64`).
fn bits_pattern_fits_width(pattern: u64, width: radix_types::BitsWidth) -> bool {
    width.bits() == 64 || pattern < (1u64 << width.bits())
}

fn lista_to_tensor_shape(target_ty: TypeId, types: &TypeTable, flat_len: usize) -> Vec<i64> {
    match resolve_lista_to_tensor_shape(target_ty, types) {
        Some(ListaToTensorShape::Literal(dims)) => dims,
        Some(ListaToTensorShape::InferOneD) | None => vec![flat_len as i64],
    }
}

#[cfg(test)]
#[path = "aggregate_test.rs"]
mod aggregate_test;
