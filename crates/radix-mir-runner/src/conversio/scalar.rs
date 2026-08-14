#![allow(
    clippy::bool_to_int_with_if,
    clippy::cast_lossless,
    clippy::format_push_string,
    clippy::match_bool,
    clippy::match_same_arms,
    clippy::needless_pass_by_value,
    clippy::only_used_in_recursion,
    clippy::map_unwrap_or,
    clippy::unused_self,
    clippy::wildcard_imports
)]
// Semantic numeric conversions between host value kinds during conversio
// lowering: fractus→numerus truncates toward zero per Faber semantics, width
// range-checks compare in i128/u128, and numerus→fractus promotes to f64.
#![expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "semantic numeric conversions between host value kinds in conversio lowering"
)]

use super::*;

impl<H: super::super::Host + ?Sized> Runner<'_, H> {
    pub(crate) fn convert_to_textus(
        &mut self,
        value: Value,
        source_ty: TypeId,
        recovery: Option<&MirOperand>,
        span: Span,
    ) -> Result<Value, RunnerError> {
        if matches!(self.types.get(source_ty), Type::Primitive(Primitive::Json)) {
            // Render the JSON document directly from concrete `Value`, mirroring
            // the prior `value_to_runtime_json_valor` -> `faber::Json::to_wire`
            // composition without the `Valor`/`Json` middleman.
            return match self.check_json_document(&value, span)? {
                true => Ok(Value::Str(self.value_to_json_wire(&value, span)?)),
                false => Err(RunnerError::internal(
                    span,
                    "json to textus conversion failed",
                )),
            };
        }
        Ok(Value::Str(match value {
            Value::Str(text) => text,
            Value::Int(v) => v.to_string(),
            Value::UInt(v) => v.to_string(),
            Value::Float(v) => v.to_string(),
            Value::Bool(v) => v.to_string(),
            Value::Instans(v) => v.to_rfc3339(),
            Value::Sermo(sermo) => {
                return self.materialize_sermo_textus(sermo, recovery, span);
            }
            Value::Octeti(bytes) => {
                return match String::from_utf8(bytes) {
                    Ok(text) => Ok(Value::Str(text)),
                    Err(_) => {
                        self.recover_or_fail(recovery, span, "octeti to textus conversion failed")
                    }
                };
            }
            Value::Array(items) => {
                // `octeti` is semantically bidirectionally assignable with
                // `lista<numerus<u8>>`. The runner keeps those carriers
                // distinct, so a list assigned into an octeti slot must be
                // normalized at this conversion boundary before UTF-8
                // decoding. Backends with a shared byte-vector carrier do
                // not need this representation repair.
                let bytes = items
                    .borrow()
                    .iter()
                    .map(|item| match item {
                        Value::Int(value) => u8::try_from(*value).ok(),
                        Value::UInt(value) => u8::try_from(*value).ok(),
                        _ => None,
                    })
                    .collect::<Option<Vec<_>>>();
                let Some(bytes) = bytes else {
                    return self.recover_or_fail(
                        recovery,
                        span,
                        "octeti to textus conversion failed",
                    );
                };
                return match String::from_utf8(bytes) {
                    Ok(text) => Ok(Value::Str(text)),
                    Err(_) => {
                        self.recover_or_fail(recovery, span, "octeti to textus conversion failed")
                    }
                };
            }
            other => {
                return Err(RunnerError::internal(
                    span,
                    format!("unsupported conversio to textus from {other:?}"),
                ));
            }
        }))
    }

    #[allow(clippy::only_used_in_recursion)]
    pub(crate) fn convert_to_numerus(
        &mut self,
        value: Value,
        params: &[Symbol],
        width: NumericWidth,
        recovery: Option<&MirOperand>,
        span: Span,
    ) -> Result<Value, RunnerError> {
        match value {
            Value::Str(text) => match self.parse_textus_numerus(&text, params) {
                // SA-002: range-check against `W` so `textus ↦ numerus<u8>` of
                // "300" fails here exactly as Rust codegen's `.parse::<u8>()`.
                Ok(v) if numerus_fits_width(v, width) => Ok(Value::Int(v)),
                Ok(_) => self.recover_or_fail(
                    recovery,
                    span,
                    "textus to numerus conversion out of range",
                ),
                Err(_) => {
                    self.recover_or_fail(recovery, span, "textus to numerus conversion failed")
                }
            },
            Value::Float(v) => Ok(Value::Int(v as i64)),
            Value::Bool(v) => Ok(Value::Int(if v { 1 } else { 0 })),
            Value::Int(v) => Ok(Value::Int(v)),
            Value::UInt(v) if width.is_unsigned_int() => {
                let (_, hi) = int_width_range(width);
                if v as u128 <= hi as u128 {
                    Ok(Value::UInt(v))
                } else {
                    self.recover_or_fail(
                        recovery,
                        span,
                        "numerus to numerus conversion out of range",
                    )
                }
            }
            Value::UInt(v) => match i64::try_from(v) {
                Ok(v) => Ok(Value::Int(v)),
                Err(_) => self.recover_or_fail(
                    recovery,
                    span,
                    "numerus to numerus conversion out of range",
                ),
            },
            Value::Sermo(sermo) => match materialize_sermo_value(sermo, span) {
                Ok(value) => self.convert_to_numerus(value, params, width, recovery, span),
                Err(_) if recovery.is_some() => {
                    self.recover_or_fail(recovery, span, "sermo to numerus conversion failed")
                }
                Err(error) => Err(error),
            },
            other => Err(RunnerError::internal(
                span,
                format!("unsupported conversio to numerus from {other:?}"),
            )),
        }
    }

    pub(crate) fn convert_to_modular_word(
        &self,
        value: Value,
        width: NumericWidth,
        span: Span,
    ) -> Result<Value, RunnerError> {
        // Narrowing range-checks against the declared width — never wraps or
        // truncates. The u64 carrier surfaces as `UInt`; narrower widths stay
        // signed `Int` since every value fits i64.
        let raw: u64 = match value {
            Value::Int(value) => u64::try_from(value).map_err(|_| {
                RunnerError::internal(
                    span,
                    format!(
                        "modulus conversion value is outside {} range",
                        width.rust_type()
                    ),
                )
            })?,
            Value::UInt(value) => value,
            _ => {
                return Err(RunnerError::internal(
                    span,
                    "modulus conversion requires an integer value",
                ))
            }
        };
        let mask = modular_word_mask(width);
        if raw > mask {
            return Err(RunnerError::internal(
                span,
                format!(
                    "modulus conversion value is outside {} range",
                    width.rust_type()
                ),
            ));
        }
        Ok(modular_word_result(raw, width))
    }

    pub(crate) fn convert_to_vacuum(
        &mut self,
        value: Value,
        span: Span,
    ) -> Result<Value, RunnerError> {
        let Value::Sermo(sermo) = value else {
            return Err(RunnerError::internal(
                span,
                "unsupported conversio to vacuum",
            ));
        };
        materialize_sermo_value(sermo, span)?;
        Ok(Value::Unit)
    }

    fn parse_textus_numerus(
        &self,
        text: &str,
        params: &[Symbol],
    ) -> Result<i64, std::num::ParseIntError> {
        match self.numerus_radix(params) {
            Some(radix) => i64::from_str_radix(text, radix),
            None => text.parse::<i64>(),
        }
    }

    fn numerus_radix(&self, params: &[Symbol]) -> Option<u32> {
        params
            .iter()
            .find_map(|param| match self.interner.resolve(*param) {
                "Hex" => Some(16),
                "Bin" => Some(2),
                "Oct" => Some(8),
                _ => None,
            })
    }

    pub(crate) fn convert_to_fractus(
        &mut self,
        value: Value,
        recovery: Option<&MirOperand>,
        span: Span,
    ) -> Result<Value, RunnerError> {
        match value {
            Value::Str(text) => match text.parse::<f64>() {
                Ok(v) => Ok(Value::Float(v)),
                Err(_) => {
                    self.recover_or_fail(recovery, span, "textus to fractus conversion failed")
                }
            },
            Value::Int(v) => Ok(Value::Float(v as f64)),
            Value::UInt(v) => Ok(Value::Float(v as f64)),
            Value::Bool(v) => Ok(Value::Float(if v { 1.0 } else { 0.0 })),
            Value::Float(v) => Ok(Value::Float(v)),
            other => Err(RunnerError::internal(
                span,
                format!("unsupported conversio to fractus from {other:?}"),
            )),
        }
    }

    pub(crate) fn convert_to_bivalens(
        &mut self,
        value: Value,
        recovery: Option<&MirOperand>,
        span: Span,
    ) -> Result<Value, RunnerError> {
        match value {
            Value::Str(text) => {
                self.bivalens_from_scalar_truthiness(!text.is_empty(), recovery, span)
            }
            Value::Int(v) => self.bivalens_from_scalar_truthiness(v != 0, recovery, span),
            Value::UInt(v) => self.bivalens_from_scalar_truthiness(v != 0, recovery, span),
            Value::Float(v) => self.bivalens_from_scalar_truthiness(v != 0.0, recovery, span),
            Value::Bool(v) => Ok(Value::Bool(v)),
            other => Err(RunnerError::internal(
                span,
                format!("unsupported conversio to bivalens from {other:?}"),
            )),
        }
    }

    /// Map scalar truthiness to `bivalens`, honoring `⇥` recovery on falsy inputs.
    fn bivalens_from_scalar_truthiness(
        &mut self,
        truthy: bool,
        recovery: Option<&MirOperand>,
        span: Span,
    ) -> Result<Value, RunnerError> {
        if truthy {
            return Ok(Value::Bool(true));
        }
        if let Some(recovery) = recovery {
            return self.eval_operand(recovery, span);
        }
        Ok(Value::Bool(false))
    }

    pub(crate) fn convert_to_octeti(
        &self,
        value: Value,
        span: Span,
    ) -> Result<Value, RunnerError> {
        Ok(match value {
            Value::Str(text) => Value::Octeti(text.into_bytes()),
            Value::Octeti(bytes) => Value::Octeti(bytes),
            Value::Array(items) => {
                let bytes = items
                    .borrow()
                    .iter()
                    .map(|item| match item {
                        Value::Int(value) => u8::try_from(*value).ok(),
                        Value::UInt(value) => u8::try_from(*value).ok(),
                        _ => None,
                    })
                    .collect::<Option<Vec<_>>>()
                    .ok_or_else(|| {
                        RunnerError::internal(
                            span,
                            "lista to octeti conversion requires byte-valued numerus elements",
                        )
                    })?;
                Value::Octeti(bytes)
            }
            other => {
                return Err(RunnerError::internal(
                    span,
                    format!("unsupported conversio to octeti from {other:?}"),
                ));
            }
        })
    }

    pub(crate) fn convert_to_valor(
        &self,
        value: Value,
        source_ty: TypeId,
        span: Span,
    ) -> Result<Value, RunnerError> {
        if let Value::Sermo(sermo) = value {
            return materialize_sermo_value(sermo, span);
        }
        if matches!(
            self.types.get(source_ty),
            Type::Primitive(Primitive::Octeti)
        ) {
            let Value::Octeti(bytes) = value else {
                return Err(RunnerError::internal(
                    span,
                    "octeti conversio carried a non-octeti value",
                ));
            };
            return Ok(Value::Array(Rc::new(RefCell::new(
                bytes
                    .into_iter()
                    .map(|byte| Value::Int(i64::from(byte)))
                    .collect(),
            ))));
        }
        Ok(value)
    }

    pub(crate) fn convert_to_json(
        &mut self,
        value: Value,
        recovery: Option<&MirOperand>,
        span: Span,
    ) -> Result<Value, RunnerError> {
        match self.check_json_document(&value, span)? {
            true => self.materialize_json_value(value, span),
            false => self.recover_or_fail(recovery, span, "valor to json conversion failed"),
        }
    }

    /// Validate `value` as a JSON document, mirroring the prior
    /// `value_to_runtime_json_valor` -> `faber::Json::try_from` composition
    /// without the `Valor`/`Json` middleman.
    ///
    /// Returns `Ok(true)` for a valid object-rooted JSON document, `Ok(false)`
    /// when the shape is sound but is not a JSON document (non-object root or a
    /// non-finite number anywhere in the tree), and `Err` for the structural
    /// hard errors the prior bridge raised (non-textus map key, or a variant
    /// outside the kernel valor bridge).
    fn check_json_document(&self, value: &Value, span: Span) -> Result<bool, RunnerError> {
        let safe = self.json_node_is_safe(value, span)?;
        Ok(safe && self.is_json_object_root(value))
    }

    fn is_json_object_root(&self, value: &Value) -> bool {
        matches!(value, Value::Map(_) | Value::Record(_) | Value::Struct(..))
    }

    fn materialize_json_value(&self, value: Value, span: Span) -> Result<Value, RunnerError> {
        Ok(match value {
            Value::Array(items) => {
                let items = items
                    .borrow()
                    .iter()
                    .cloned()
                    .map(|item| self.materialize_json_value(item, span))
                    .collect::<Result<Vec<_>, _>>()?;
                Value::Array(Rc::new(RefCell::new(items)))
            }
            Value::Tensor(tensor) => {
                let tensor = tensor.borrow();
                let items = tensor
                    .data
                    .iter()
                    .cloned()
                    .map(|item| self.materialize_json_value(item, span))
                    .collect::<Result<Vec<_>, _>>()?;
                Value::Array(Rc::new(RefCell::new(items)))
            }
            Value::Map(items) => {
                let items = items.borrow();
                let mut out = FxHashMap::default();
                for (key, item) in items.iter() {
                    let MapKey::Str(text) = key else {
                        return Err(RunnerError::internal(span, "map key is not textus valor"));
                    };
                    out.insert(
                        MapKey::Str(text.clone()),
                        self.materialize_json_value(item.clone(), span)?,
                    );
                }
                Value::Map(Rc::new(RefCell::new(out)))
            }
            Value::Record(fields) => {
                let fields = fields.borrow();
                let mut out = FxHashMap::default();
                for (field, item) in fields.iter() {
                    out.insert(
                        MapKey::Str(self.interner.resolve(*field).to_owned()),
                        self.materialize_json_value(item.clone(), span)?,
                    );
                }
                Value::Map(Rc::new(RefCell::new(out)))
            }
            Value::Struct(def_id, fields) => {
                let fields = fields.borrow();
                let mut out = FxHashMap::default();
                for (field, item) in fields.iter() {
                    out.insert(
                        MapKey::Str(self.struct_wire_field_key(def_id, *field)),
                        self.materialize_json_value(item.clone(), span)?,
                    );
                }
                Value::Map(Rc::new(RefCell::new(out)))
            }
            other => other,
        })
    }

    #[allow(clippy::self_only_used_in_recursion)]
    fn json_node_is_safe(&self, value: &Value, span: Span) -> Result<bool, RunnerError> {
        match value {
            Value::Nil
            | Value::Unit
            | Value::Bool(_)
            | Value::Int(_)
            | Value::UInt(_)
            | Value::Str(_)
            | Value::Instans(_)
            | Value::Octeti(_) => Ok(true),
            Value::Float(n) => Ok(n.is_finite()),
            Value::Array(items) => items.borrow().iter().try_fold(true, |acc, item| {
                Ok(acc && self.json_node_is_safe(item, span)?)
            }),
            Value::Tensor(tensor) => tensor.borrow().data.iter().try_fold(true, |acc, item| {
                Ok(acc && self.json_node_is_safe(item, span)?)
            }),
            Value::Map(items) => items.borrow().iter().try_fold(true, |acc, (key, item)| {
                if !matches!(key, MapKey::Str(_)) {
                    return Err(RunnerError::internal(span, "map key is not textus valor"));
                }
                Ok(acc && self.json_node_is_safe(item, span)?)
            }),
            Value::Record(fields) => fields.borrow().iter().try_fold(true, |acc, (_, item)| {
                Ok(acc && self.json_node_is_safe(item, span)?)
            }),
            Value::Struct(_, fields) => fields.borrow().iter().try_fold(true, |acc, (_, item)| {
                Ok(acc && self.json_node_is_safe(item, span)?)
            }),
            Value::Function(_)
            | Value::Closure(_, _, _)
            | Value::ClosureEnvironment(_, _)
            | Value::Regex(_)
            | Value::Sermo(_)
            | Value::Set(_)
            | Value::EnumVariant(_, _)
            | Value::Tuple(_)
            | Value::Vector(_)
            | Value::Matrix(_)
            | Value::GradientHandle(_) => Err(RunnerError::internal(
                span,
                "value variant is outside kernel valor bridge",
            )),
        }
    }

    /// Render `value` as JSON wire text, mirroring the prior
    /// `value_to_runtime_valor` (with `instans_as_textus = true`) ->
    /// `faber::Json::to_wire` composition. Maps, records, and structs emit keys
    /// in sorted order (matching the prior `BTreeMap` carrier). Assumes
    /// [`check_json_document`](Self::check_json_document) already accepted the
    /// value.
    fn value_to_json_wire(&self, value: &Value, span: Span) -> Result<String, RunnerError> {
        Ok(match value {
            Value::Nil | Value::Unit => "null".to_owned(),
            Value::Bool(v) => if *v { "true" } else { "false" }.to_owned(),
            Value::Int(v) => v.to_string(),
            Value::UInt(v) => v.to_string(),
            Value::Float(v) => render_json_fractus(*v),
            Value::Str(v) => render_json_string(v),
            Value::Instans(v) => render_json_string(&v.to_rfc3339()),
            Value::Octeti(bytes) => {
                let body = bytes
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(",");
                format!("[{body}]")
            }
            Value::Array(items) => {
                let body = items
                    .borrow()
                    .iter()
                    .map(|item| self.value_to_json_wire(item, span))
                    .collect::<Result<Vec<_>, _>>()?
                    .join(",");
                format!("[{body}]")
            }
            Value::Tensor(tensor) => {
                let body = tensor
                    .borrow()
                    .data
                    .iter()
                    .map(|item| self.value_to_json_wire(item, span))
                    .collect::<Result<Vec<_>, _>>()?
                    .join(",");
                format!("[{body}]")
            }
            Value::Map(items) => {
                let items = items.borrow();
                let mut entries: Vec<(String, String)> = Vec::with_capacity(items.len());
                for (key, item) in items.iter() {
                    let MapKey::Str(text) = key else {
                        return Err(RunnerError::internal(span, "map key is not textus valor"));
                    };
                    entries.push((text.clone(), self.value_to_json_wire(item, span)?));
                }
                entries.sort_by(|a, b| a.0.cmp(&b.0));
                let body = entries
                    .iter()
                    .map(|(key, value)| format!("{}:{}", render_json_string(key), value))
                    .collect::<Vec<_>>()
                    .join(",");
                format!("{{{body}}}")
            }
            Value::Record(fields) => {
                let fields = fields.borrow();
                let mut entries: Vec<(String, String)> = Vec::with_capacity(fields.len());
                for (field, item) in fields.iter() {
                    let key = self.interner.resolve(*field).to_owned();
                    entries.push((key, self.value_to_json_wire(item, span)?));
                }
                entries.sort_by(|a, b| a.0.cmp(&b.0));
                let body = entries
                    .iter()
                    .map(|(key, value)| format!("{}:{}", render_json_string(key), value))
                    .collect::<Vec<_>>()
                    .join(",");
                format!("{{{body}}}")
            }
            Value::Struct(def_id, fields) => {
                let fields = fields.borrow();
                let mut entries: Vec<(String, String)> = Vec::with_capacity(fields.len());
                for (field, item) in fields.iter() {
                    let key = self.struct_wire_field_key(*def_id, *field);
                    entries.push((key, self.value_to_json_wire(item, span)?));
                }
                entries.sort_by(|a, b| a.0.cmp(&b.0));
                let body = entries
                    .iter()
                    .map(|(key, value)| format!("{}:{}", render_json_string(key), value))
                    .collect::<Vec<_>>()
                    .join(",");
                format!("{{{body}}}")
            }
            // check_json_document rejects these before rendering; defensive guard.
            Value::Function(_)
            | Value::Closure(_, _, _)
            | Value::ClosureEnvironment(_, _)
            | Value::Regex(_)
            | Value::Sermo(_)
            | Value::Set(_)
            | Value::EnumVariant(_, _)
            | Value::Tuple(_)
            | Value::Vector(_)
            | Value::Matrix(_)
            | Value::GradientHandle(_) => {
                return Err(RunnerError::internal(
                    span,
                    "value variant is outside kernel valor bridge",
                ));
            }
        })
    }

    pub(crate) fn require_json_value(&self, value: &Value, span: Span) -> Result<(), RunnerError> {
        match self.check_json_document(value, span)? {
            true => Ok(()),
            false => Err(RunnerError::internal(
                span,
                "valor to json conversion failed",
            )),
        }
    }

    fn materialize_sermo_textus(
        &mut self,
        sermo: Rc<RefCell<SermoValue>>,
        recovery: Option<&MirOperand>,
        span: Span,
    ) -> Result<Value, RunnerError> {
        match materialize_sermo_value(sermo, span)? {
            Value::Str(text) => Ok(Value::Str(text)),
            _ => self.recover_or_fail(recovery, span, "sermo to textus conversion failed"),
        }
    }

    pub(crate) fn convert_to_ascii(
        &mut self,
        value: Value,
        recovery: Option<&MirOperand>,
        span: Span,
    ) -> Result<Value, RunnerError> {
        match value {
            Value::Str(text) => {
                if text.is_ascii() {
                    Ok(Value::Str(text))
                } else {
                    self.recover_or_fail(recovery, span, "textus to ascii conversion failed")
                }
            }
            Value::Octeti(bytes) => {
                if bytes.is_ascii() {
                    String::from_utf8(bytes).map(Value::Str).map_err(|_| {
                        RunnerError::internal(span, "ascii bytes failed UTF-8 decode")
                    })
                } else {
                    self.recover_or_fail(recovery, span, "octeti to ascii conversion failed")
                }
            }
            _ => Err(RunnerError::internal(
                span,
                "ascii conversio requires textus or octeti source",
            )),
        }
    }

    pub(crate) fn convert_to_regex(&self, value: Value, span: Span) -> Result<Value, RunnerError> {
        match value {
            Value::Regex(pattern) | Value::Str(pattern) => {
                Ok(Value::Regex(runtime_regex_pattern(&pattern)))
            }
            other => Err(RunnerError::internal(
                span,
                format!("unsupported conversio to regex from {other:?}"),
            )),
        }
    }

    pub(crate) fn convert_to_instans(
        &mut self,
        value: Value,
        target: &Type,
        recovery: Option<&MirOperand>,
        span: Span,
    ) -> Result<Value, RunnerError> {
        let precision = instans_precision(target);
        match value {
            Value::Instans(value) => Ok(Value::Instans(value.ad_praecisionem(precision))),
            // String sources parse as RFC3339 wire text; recovery honors `⇥`.
            Value::Str(text) => match Instans::from_rfc3339(&text, precision) {
                Some(value) => Ok(Value::Instans(value)),
                None => self.recover_or_fail(recovery, span, "textus to instans conversion failed"),
            },
            // Integer sources are interpreted as epoch units at the target precision.
            Value::Int(epoch) => Ok(Value::Instans(Instans::from_epoch_integer(
                epoch, precision,
            ))),
            Value::UInt(epoch) => match i64::try_from(epoch) {
                Ok(epoch) => Ok(Value::Instans(Instans::from_epoch_integer(
                    epoch, precision,
                ))),
                Err(_) => self.recover_or_fail(
                    recovery,
                    span,
                    "numerus to instans conversion out of range",
                ),
            },
            other => Err(RunnerError::internal(
                span,
                format!("unsupported conversio to instans from {other:?}"),
            )),
        }
    }
}

fn materialize_sermo_value(
    sermo: Rc<RefCell<SermoValue>>,
    span: Span,
) -> Result<Value, RunnerError> {
    let sermo = sermo.borrow();
    match sermo.route.as_str() {
        "runtime:echo" => Ok(sermo.opener.clone()),
        route => Err(RunnerError::unsupported(
            span,
            &format!("sermo route {route} before runner provider runtime"),
        )),
    }
}

/// `true` when `v` fits the signed range of `width`. Mirrors Rust codegen's
/// `.parse::<W>()` accept/reject boundary for `textus ↦ numerus<W>` (SA-002);
/// uses the runner's existing target-neutral integer width table.
fn numerus_fits_width(v: i64, width: NumericWidth) -> bool {
    let (lo, hi) = int_width_range(width);
    (lo..=hi).contains(&(v as i128))
}

/// Render an f64 as JSON number wire text, ensuring a decimal point
/// (`5` -> `5.0`). Mirrors `faber_runtime::json::render_fractus` byte-for-byte.
fn render_json_fractus(value: f64) -> String {
    let rendered = value.to_string();
    if rendered.contains('.') || rendered.contains('e') || rendered.contains('E') {
        rendered
    } else {
        format!("{rendered}.0")
    }
}

/// Render a string as a JSON string literal (quoted, with JSON escapes).
/// Mirrors `faber_runtime::json::render_string` byte-for-byte.
fn render_json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}
