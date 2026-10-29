//! Value dispatch (`switch` / `elige`, `discerne`) as a statement.
//!
//! The scrutinees are evaluated once, in order, into temporaries; each arm
//! becomes one `if` / `else if` test over them and the first arm that matches
//! runs, as in Faber. One integer subject whose arms are all literal tests
//! becomes C's own `switch` instead, so the compiler can build a jump table
//! for wide dispatches; its `break` closes the case, and no Faber break can
//! be caught by it because a body holding one keeps the if-chain. Text
//! subjects, several subjects, and every other pattern stay on the chain: C's
//! `switch` takes neither text nor several subjects.

use crate::Emitter;
use crate::Local;
use crate::error::unsupported;
use crate::expr::{Operand, Value};
use crate::ty::Ty;
use handroll::FxHashSet;
use radix_codegen_shared::CodegenError;
use radix_codegen_shared::names::prefixed_binding_name;
use radix_hir::visit::{HirVisitor, walk_stmt};
use radix_hir::{
    DefId, HirCasuArm, HirExpression, HirExpressionKind, HirLiteral, HirPattern, HirStatement,
    HirStatementKind,
};
use radix_lexer::Symbol;

/// One scrutinee, held in a temporary.
struct Subject {
    name: String,
    value: Value,
    ty: Ty,
}

/// What matching one arm needs: the tests that must all hold and the names it
/// binds to the subjects.
struct Arm {
    tests: Vec<String>,
    binds: Vec<Bind>,
}

/// One name an arm binds: the C expression it reads and that value's class.
struct Bind {
    def_id: DefId,
    name: Symbol,
    ty: Ty,
    text: String,
}

impl Emitter<'_> {
    pub(crate) fn switch(
        &mut self,
        out: &mut String,
        scrutinees: &[HirExpression],
        arms: &[HirCasuArm],
    ) -> Result<(), CodegenError> {
        self.line(out, "{");
        self.depth += 1;
        let mut subjects = Vec::with_capacity(scrutinees.len());
        for scrutinee in scrutinees {
            let mut operand = self.expr(scrutinee)?;
            if operand.value == Value::Exact {
                operand = self.fit(operand)?;
            }
            let ty = match operand.value {
                Value::Int => Ty::Int,
                Value::Text => Ty::Text,
                Value::Truth => Ty::Bool,
                Value::Float => Ty::Float,
                Value::Float32 => Ty::Float32,
                Value::Decimal => Ty::Decimal,
                Value::List(elem) => Ty::List(elem),
                Value::Set(elem) => Ty::Set(elem),
                Value::Map(k, v) => Ty::Map(k, v),
                Value::Genus(def_id) => Ty::Genus(def_id),
                Value::Opt(inner) => Ty::Opt(inner),
                Value::Exact | Value::Void | Value::None => {
                    return Err(unsupported("switch over a value with no class"));
                }
                Value::Func(_) => {
                    return Err(unsupported("switch over a function value"));
                }
            };
            let value = operand.value;
            let declaration = self.bind(&mut operand);
            self.line(out, declaration.trim_end());
            subjects.push(Subject {
                name: operand.text,
                value,
                ty,
            });
        }
        let plans = jump_table_plan(&subjects, arms);
        let mut opened = false;
        if let Some(plans) = &plans {
            self.jump_table(out, &subjects, arms, plans)?;
            opened = true;
        }
        if !opened {
            for arm in arms {
                let plan = self.plan_arm(&subjects, &arm.patterns)?;
                let always = plan.tests.is_empty();
                if always {
                    self.line(out, if opened { "} else {" } else { "{" });
                } else {
                    // Each test carries its own parentheses; a lone one is the
                    // condition as it stands (a second pair trips clang's
                    // `-Wparentheses-equality`), several are joined and wrapped.
                    let test = match plan.tests.as_slice() {
                        [only] => only.clone(),
                        many => format!("({})", many.join(" && ")),
                    };
                    let head = if opened { "} else if" } else { "if" };
                    self.line(out, &format!("{head} {test} {{"));
                }
                opened = true;
                self.depth += 1;
                for bind in &plan.binds {
                    self.bind_subject(out, bind)?;
                }
                self.depth -= 1;
                if let HirExpressionKind::Block(block) = &arm.body.kind {
                    self.block(out, block)?;
                } else {
                    self.depth += 1;
                    let result = self.expr_statement(out, &arm.body);
                    self.depth -= 1;
                    result?;
                }
                if always {
                    break;
                }
            }
        }
        if opened {
            self.line(out, "}");
        }
        self.depth -= 1;
        self.line(out, "}");
        Ok(())
    }

    /// The chosen dispatch as a C `switch`: one `case` label per matched
    /// literal, the arm body under them, and a C `break` closing each case.
    /// The plan has already settled that no body holds a Faber break, so the
    /// break can only mean the case.
    fn jump_table(
        &mut self,
        out: &mut String,
        subjects: &[Subject],
        arms: &[HirCasuArm],
        plans: &[Cases],
    ) -> Result<(), CodegenError> {
        self.line(out, &format!("switch ({}) {{", subjects[0].name));
        self.depth += 1;
        for (arm, plan) in arms.iter().zip(plans) {
            let planned = self.plan_arm(subjects, &arm.patterns)?;
            match plan {
                Cases::Labels(values) => {
                    let labels = values
                        .iter()
                        .map(|value| Ok(self.int(*value)?.text))
                        .collect::<Result<Vec<_>, CodegenError>>()?;
                    if let [only] = labels.as_slice() {
                        self.line(out, &format!("case {only}: {{"));
                    } else {
                        for label in &labels {
                            self.line(out, &format!("case {label}:"));
                        }
                        self.line(out, "{{");
                    }
                }
                Cases::Default => self.line(out, "default: {"),
                Cases::Other => {
                    return Err(unsupported("jump-table plan over an unlabelled arm"));
                }
            }
            self.depth += 1;
            for bind in &planned.binds {
                self.bind_subject(out, bind)?;
            }
            self.depth -= 1;
            if let HirExpressionKind::Block(block) = &arm.body.kind {
                self.block(out, block)?;
            } else {
                self.depth += 1;
                let result = self.expr_statement(out, &arm.body);
                self.depth -= 1;
                result?;
            }
            self.line(out, "} break;");
        }
        self.depth -= 1;
        Ok(())
    }

    fn plan_arm(
        &mut self,
        subjects: &[Subject],
        patterns: &[HirPattern],
    ) -> Result<Arm, CodegenError> {
        if patterns.len() != subjects.len() {
            return Err(unsupported("switch arm with a pattern count that differs"));
        }
        let mut arm = Arm {
            tests: Vec::new(),
            binds: Vec::new(),
        };
        for (subject, pattern) in subjects.iter().zip(patterns) {
            self.pattern(subject, pattern, &mut arm)?;
        }
        Ok(arm)
    }

    fn pattern(
        &mut self,
        subject: &Subject,
        pattern: &HirPattern,
        arm: &mut Arm,
    ) -> Result<(), CodegenError> {
        match pattern {
            HirPattern::Wildcard => Ok(()),
            HirPattern::Binding(def_id, name) => {
                arm.binds.push(Bind {
                    def_id: *def_id,
                    name: *name,
                    ty: subject.ty,
                    text: subject.name.clone(),
                });
                Ok(())
            }
            HirPattern::Alias(def_id, name, inner) => {
                arm.binds.push(Bind {
                    def_id: *def_id,
                    name: *name,
                    ty: subject.ty,
                    text: subject.name.clone(),
                });
                self.pattern(subject, inner, arm)
            }
            HirPattern::Literal(literal) => {
                let test = self.literal_test(subject, literal, false)?;
                arm.tests.push(test);
                Ok(())
            }
            HirPattern::NegatedLiteral(literal) => {
                let test = self.literal_test(subject, literal, true)?;
                arm.tests.push(test);
                Ok(())
            }
            HirPattern::Or(alternatives) => {
                let mut tests = Vec::with_capacity(alternatives.len());
                for alternative in alternatives {
                    let mut inner = Arm {
                        tests: Vec::new(),
                        binds: Vec::new(),
                    };
                    self.pattern(subject, alternative, &mut inner)?;
                    if !inner.binds.is_empty() {
                        return Err(unsupported("alternative pattern that binds a name"));
                    }
                    // An alternative that matches everything makes the whole
                    // pattern match everything.
                    if inner.tests.is_empty() {
                        return Ok(());
                    }
                    tests.push(inner.tests.join(" && "));
                }
                arm.tests.push(format!("({})", tests.join(" || ")));
                Ok(())
            }
            HirPattern::Variant(def_id, parts) => {
                self.variant_pattern(subject, *def_id, parts, arm)
            }
            HirPattern::TypeTest { .. } => Err(unsupported("type-test pattern")),
        }
    }

    /// `subject == literal`, by the class of the subject.
    fn literal_test(
        &mut self,
        subject: &Subject,
        literal: &HirLiteral,
        negated: bool,
    ) -> Result<String, CodegenError> {
        let name = &subject.name;
        match (subject.value, literal) {
            (Value::Int, HirLiteral::Int(value)) => {
                let value = i128::from(*value);
                let operand = self.int(if negated { -value } else { value })?;
                Ok(format!("({name} == {})", operand.text))
            }
            (Value::Text, HirLiteral::String(symbol)) if !negated => {
                let raw = self.names.resolve_symbol(*symbol);
                let decoded = radix_codegen_shared::unescape_faber_text(raw);
                let text = self.text_literal(&decoded)?;
                self.support.need_stdio();
                Ok(format!("(strcmp({name}, {text}) == 0)"))
            }
            (Value::Truth, HirLiteral::Bool(value)) if !negated => Ok(format!(
                "({name} == {})",
                if *value { "(1)" } else { "(0)" }
            )),
            (Value::Float, HirLiteral::Float(value) | HirLiteral::FloatText { value, .. }) => {
                let operand: Operand = self.float_literal(if negated { -value } else { *value });
                Ok(format!("({name} == {})", operand.text))
            }
            (Value::Float32, HirLiteral::Float(value) | HirLiteral::FloatText { value, .. }) => {
                #[expect(clippy::cast_possible_truncation)]
                let operand: Operand = self.float_literal32(if negated {
                    -*value as f32
                } else {
                    *value as f32
                });
                Ok(format!("({name} == {})", operand.text))
            }
            _ => Err(unsupported("pattern literal of a different class")),
        }
    }

    /// `const T l_name = subject;`, registered as a local.
    fn bind_subject(&mut self, out: &mut String, bind: &Bind) -> Result<(), CodegenError> {
        let (def_id, name) = (bind.def_id, bind.name);
        let Some(kind) = self.names.binding_kind(def_id) else {
            return Err(unsupported(
                "pattern binding the name catalog does not classify",
            ));
        };
        let raw = self.names.resolve_symbol(name);
        let c_name = self.scope.claim(&prefixed_binding_name(kind, raw));
        self.need_class(bind.ty);
        let declaration = self.declare(bind.ty, &c_name, false)?;
        self.line(out, &format!("{declaration} = {};", bind.text));
        self.locals.insert(
            def_id,
            Local {
                name: c_name,
                ty: bind.ty,
                constant: None,
            },
        );
        Ok(())
    }

    /// `casu Circ const r`: the tag test, then each sub-pattern binds the
    /// variant's field at its position. Only names and wildcards nest.
    fn variant_pattern(
        &mut self,
        subject: &Subject,
        def_id: DefId,
        parts: &[HirPattern],
        arm: &mut Arm,
    ) -> Result<(), CodegenError> {
        let Some((enum_def, variant)) = self.variant(def_id) else {
            return Err(unsupported(
                "variant pattern of a name that is not a variant",
            ));
        };
        if subject.value != Value::Genus(enum_def) {
            return Err(unsupported("variant pattern over a different value class"));
        }
        let (tag, fields) = (variant.tag, variant.fields.clone());
        arm.tests.push(format!("({}->tag == {tag})", subject.name));
        for (part, field) in parts.iter().zip(&fields) {
            match part {
                HirPattern::Wildcard => {}
                HirPattern::Binding(def_id, name) => arm.binds.push(Bind {
                    def_id: *def_id,
                    name: *name,
                    ty: field.ty,
                    text: format!("{}->{}", subject.name, field.c_name),
                }),
                _ => return Err(unsupported("nested pattern inside a variant pattern")),
            }
        }
        Ok(())
    }
}

/// What one arm contributes to a C `switch` over one integer subject.
enum Cases {
    /// The integer literals the arm matches, one `case` label each.
    Labels(Vec<i128>),
    /// The arm matches everything: the `default` label.
    Default,
    /// A pattern the switch cannot express; the arm keeps the if-chain.
    Other,
}

/// The per-arm plan for a C `switch`, when the statement admits one: exactly
/// one integer subject, every arm either literal labels or a final catch-all,
/// no literal matched twice (Faber keeps the first arm; C rejects the label),
/// and no arm body holding a Faber break, which a C `break` inside the switch
/// would no longer deliver to the loop around it.
fn jump_table_plan(subjects: &[Subject], arms: &[HirCasuArm]) -> Option<Vec<Cases>> {
    let [subject] = subjects else {
        return None;
    };
    if subject.value != Value::Int || arms.is_empty() {
        return None;
    }
    let mut plans = Vec::with_capacity(arms.len());
    let mut seen: FxHashSet<i128> = FxHashSet::default();
    for (index, arm) in arms.iter().enumerate() {
        let mut plan = match arm.patterns.as_slice() {
            [pattern] => case_plan(subject, pattern),
            _ => Cases::Other,
        };
        match &mut plan {
            Cases::Labels(values) => {
                values.sort_unstable();
                values.dedup();
                if values.iter().any(|label| !seen.insert(*label)) {
                    return None;
                }
            }
            // The catch-all swallows every later arm in Faber, so it is only
            // a `default` when it is the last arm; otherwise those arms are
            // dead in Faber and reachable in C.
            Cases::Default if index + 1 == arms.len() && !seen.is_empty() => {}
            Cases::Default | Cases::Other => return None,
        }
        plans.push(plan);
    }
    for arm in arms {
        let mut finder = BreakFinder { found: false };
        finder.visit_expr(&arm.body);
        if finder.found {
            return None;
        }
    }
    Some(plans)
}

/// Classify one pattern against one integer subject, without side effects.
fn case_plan(subject: &Subject, pattern: &HirPattern) -> Cases {
    let label = |magnitude: u64, negated: bool| {
        let value = i128::from(magnitude);
        let value = if negated { -value } else { value };
        // A label must name an `int64_t`; a wider literal keeps the chain,
        // whose test compares the subject against the exact value as before.
        i64::try_from(value).is_ok().then_some(value)
    };
    match (subject.value, pattern) {
        (Value::Int, HirPattern::Literal(HirLiteral::Int(magnitude))) => {
            label(*magnitude, false).map_or(Cases::Other, |value| Cases::Labels(vec![value]))
        }
        (Value::Int, HirPattern::NegatedLiteral(HirLiteral::Int(magnitude))) => {
            label(*magnitude, true).map_or(Cases::Other, |value| Cases::Labels(vec![value]))
        }
        (Value::Int, HirPattern::Or(alternatives)) => {
            let mut values = Vec::with_capacity(alternatives.len());
            for alternative in alternatives {
                match case_plan(subject, alternative) {
                    Cases::Labels(mut inner) => values.append(&mut inner),
                    Cases::Default => return Cases::Default,
                    Cases::Other => return Cases::Other,
                }
            }
            Cases::Labels(values)
        }
        // A binding or a wildcard matches everything; an alias binds and then
        // matches whatever remains.
        (_, HirPattern::Wildcard | HirPattern::Binding(..)) => Cases::Default,
        (_, HirPattern::Alias(_, _, inner)) => case_plan(subject, inner),
        (_, _) => Cases::Other,
    }
}

/// Whether a Faber break (`rumpe`) sits anywhere inside the expression.
struct BreakFinder {
    found: bool,
}

impl HirVisitor for BreakFinder {
    fn visit_stmt(&mut self, stmt: &HirStatement) {
        if matches!(stmt.kind, HirStatementKind::Rumpe) {
            self.found = true;
        }
        walk_stmt(self, stmt);
    }
}
