//! Value dispatch (`switch` / `elige`, `discerne`) as a statement.
//!
//! The scrutinees are evaluated once, in order, into temporaries; each arm
//! becomes one `if` / `else if` test over them and the first arm that matches
//! runs, as in Faber. C's own `switch` is not used: it takes neither text nor
//! several subjects, and a C `break` inside it would no longer leave the loop
//! around it.

use crate::Emitter;
use crate::Local;
use crate::error::unsupported;
use crate::expr::{Operand, Value, c_string};
use crate::ty::Ty;
use radix_codegen_shared::CodegenError;
use radix_codegen_shared::names::prefixed_binding_name;
use radix_hir::{DefId, HirCasuArm, HirExpression, HirExpressionKind, HirLiteral, HirPattern};
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
    binds: Vec<(DefId, Symbol, usize)>,
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
                Value::List(elem) => Ty::List(elem),
                Value::Genus(def_id) => Ty::Genus(def_id),
                Value::Opt(inner) => Ty::Opt(inner),
                Value::Exact | Value::Void => {
                    return Err(unsupported("switch over a value with no class"));
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
        let mut opened = false;
        let mut done = false;
        for arm in arms {
            if done {
                break;
            }
            let plan = self.plan_arm(&subjects, &arm.patterns)?;
            let always = plan.tests.is_empty();
            if always {
                self.line(out, if opened { "} else {" } else { "{" });
                done = true;
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
            for (def_id, name, index) in &plan.binds {
                self.bind_subject(out, *def_id, *name, &subjects[*index])?;
            }
            self.depth -= 1;
            match &arm.body.kind {
                HirExpressionKind::Block(block) => self.block(out, block)?,
                _ => {
                    self.depth += 1;
                    let result = self.expr_statement(out, &arm.body);
                    self.depth -= 1;
                    result?;
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
        for (index, pattern) in patterns.iter().enumerate() {
            self.pattern(&subjects[index], index, pattern, &mut arm)?;
        }
        Ok(arm)
    }

    fn pattern(
        &mut self,
        subject: &Subject,
        index: usize,
        pattern: &HirPattern,
        arm: &mut Arm,
    ) -> Result<(), CodegenError> {
        match pattern {
            HirPattern::Wildcard => Ok(()),
            HirPattern::Binding(def_id, name) => {
                arm.binds.push((*def_id, *name, index));
                Ok(())
            }
            HirPattern::Alias(def_id, name, inner) => {
                arm.binds.push((*def_id, *name, index));
                self.pattern(subject, index, inner, arm)
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
                    self.pattern(subject, index, alternative, &mut inner)?;
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
            HirPattern::Variant(..) => Err(unsupported("variant pattern")),
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
                let text = c_string(&decoded)?;
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
            _ => Err(unsupported("pattern literal of a different class")),
        }
    }

    /// `const T l_name = subject;`, registered as a local.
    fn bind_subject(
        &mut self,
        out: &mut String,
        def_id: DefId,
        name: Symbol,
        subject: &Subject,
    ) -> Result<(), CodegenError> {
        let Some(kind) = self.names.binding_kind(def_id) else {
            return Err(unsupported(
                "pattern binding the name catalog does not classify",
            ));
        };
        let raw = self.names.resolve_symbol(name);
        let c_name = self.scope.claim(&prefixed_binding_name(kind, raw));
        self.need_class(subject.ty);
        let declaration = self.declare(subject.ty, &c_name, false)?;
        self.line(out, &format!("{declaration} = {};", subject.name));
        self.locals.insert(
            def_id,
            Local {
                name: c_name,
                ty: subject.ty,
                constant: None,
            },
        );
        Ok(())
    }
}
