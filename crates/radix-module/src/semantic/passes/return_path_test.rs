//! Source-level tests for the return-path exhaustiveness pass (DEFER-011).
//!
//! These compile Faber source through the full driver pipeline and assert on
//! the `SEM033` (`MissingReturn`) diagnostic and its structured issue fact. The
//! driver is the most realistic seam: it exercises parse → collect → resolve →
//! lower → typecheck → exhaustive → `return_path`, so a passing test proves the
//! invariant end to end rather than against a hand-built HIR fixture.

use crate::diagnostics::Diagnostic;
use crate::driver::{analyze_source, compile, Config, Session};
use crate::semantic::SemanticErrorKind;

fn session() -> Session {
    Session::new(Config::default().with_target(crate::codegen::Target::HirRust))
}

fn analysis_session() -> Session {
    Session::new(Config::default().with_target(crate::codegen::Target::HirFaber))
}

fn has_return_path_error(result: &crate::CompileResult) -> bool {
    result.diagnostics.iter().any(is_return_path_error)
}

fn is_return_path_error(diag: &Diagnostic) -> bool {
    diag.code == Some("SEM033") && diagnostic_arg_eq(diag, "issue", "control_flow_fallthrough")
}

fn diagnostic_arg_eq(diag: &Diagnostic, name: &str, value: &str) -> bool {
    diag.args
        .iter()
        .any(|arg| arg.name == name && arg.value == value)
}

fn diagnostic_facts(diagnostics: &[Diagnostic]) -> Vec<(Option<&'static str>, Option<&str>)> {
    diagnostics
        .iter()
        .map(|diag| (diag.code, diag.issue()))
        .collect()
}

fn assert_rejects(source: &str) {
    let Err(diagnostics) = analyze_source(&analysis_session(), "return_path.fab", source) else {
        panic!("expected semantic rejection");
    };
    let result = crate::CompileResult { output: None, diagnostics };
    assert!(
        has_return_path_error(&result),
        "expected a return-path diagnostic, got {:?}",
        diagnostic_facts(&result.diagnostics)
    );
}

#[cfg(feature = "hir-rust")]
fn assert_accepts(source: &str) {
    let result = compile(&session(), "return_path.fab", source);
    let other_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|diag| is_return_path_error(diag))
        .collect();
    assert!(
        other_errors.is_empty(),
        "expected no return-path diagnostic, got {other_errors:?}"
    );
    // Also assert the program compiled cleanly so the test does not silently
    // pass because an earlier pass rejected it for an unrelated reason.
    assert!(
        result.success(),
        "expected clean compile, got {:?}",
        diagnostic_facts(&result.diagnostics)
    );
}

#[test]
fn rejects_missing_return_on_fallthrough() {
    // The canonical DEFER-011 program: passes `check` today, fails rustc.
    assert_rejects(
        "functio maybeReturn(bivalens b) → numerus {
            si b ergo redde 1
        }",
    );
}

#[test]
fn rejects_value_return_with_no_redde() {
    // A trailing expression does not satisfy the normal return channel in a
    // named function; every path must `redde`.
    assert_rejects(
        "functio noReturn(numerus x) → numerus {
            nota x
        }",
    );
}

#[test]
fn rejects_when_only_one_branch_returns() {
    assert_rejects(
        "functio half(numerus x) → numerus {
            si x > 0 {
                redde x
            } secus {
                nota x
            }
        }",
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn accepts_both_branches_returning() {
    assert_accepts(
        "functio abs(numerus x) → numerus {
            si x > 0 {
                redde x
            } secus {
                redde 0 - x
            }
        }",
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn accepts_if_without_else_falls_through_to_trailing_redde() {
    assert_accepts(
        "functio clamp(numerus x) → numerus {
            si x > 10 {
                redde 10
            }
            redde x
        }",
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn accepts_exhaustive_enum_discerne_without_catchall() {
    // Coverage is provable for an enum scrutinee with all variants covered,
    // even without a wildcard arm (the `omnia` interaction from DEFER-028).
    assert_accepts(
        "discretio Lumen {
            Lucet,
            Tenebrae,
        }
        functio narra(Lumen l) → numerus {
            discerne l {
                casu Lucet ergo redde 1
                casu Tenebrae ergo redde 0
            }
        }",
    );
}

#[test]
fn rejects_non_exhaustive_value_discerne_without_trailing_return() {
    // `elige` over a textus is not provably exhaustive; the author must add a
    // trailing `redde` (as the `elige/in-functione` exemplum correctly does).
    assert_rejects(
        "functio saluta(textus lingua) → textus {
            elige lingua {
                casu \"latina\" ergo redde \"Salve\"
                casu \"graeca\" ergo redde \"Chaire\"
            }
        }",
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn accepts_value_discerne_with_catchall_returning() {
    assert_accepts(
        "functio saluta(textus lingua) → textus {
            elige lingua {
                casu \"latina\" ergo redde \"Salve\"
                ceterum ergo redde \"Ave\"
            }
        }",
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn accepts_throw_as_terminating_path() {
    // `iace` diverges the path, so a function whose every path returns or
    // throws satisfies the contract.
    assert_accepts(
        "functio divide(numerus a, numerus b) → numerus ⇥ textus {
            si b ≡ 0 ergo iace \"division by zero\"
            redde a / b
        }",
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn accepts_handled_where_body_and_catch_both_return() {
    // The `fac { … } cape err { … }` form terminates when both blocks return.
    assert_accepts(
        "functio divide(numerus a, numerus b) → numerus ⇥ textus {
            si b ≡ 0 ergo iace \"division by zero\"
            redde a / b
        }
        functio tutum(numerus a, numerus b) → numerus {
            fac {
                redde divide(a, b)
            } cape err {
                redde 0
            }
        }",
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn accepts_vacuum_function_without_redde() {
    // Effect-only functions (no `→`) owe no value and must not be flagged.
    assert_accepts(
        "functio greet(textus name) {
            nota \"Salve, §!\"(name)
        }",
    );
}

#[test]
fn rejects_loop_with_break_is_not_diverging() {
    // A `dum` loop is not a proof of divergence: it may run zero times, so the
    // function must still return after it.
    assert_rejects(
        "functio loopy(numerus n) → numerus {
            dum n > 0 {
                n ← n - 1
            }
        }",
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn accepts_diverging_loop_without_rumpe() {
    // A bare infinite `loop` with no `rumpe` provably diverges.
    assert_accepts(
        "functio serve() → numerus {
            dum verum {
                nota \"running\"
            }
        }",
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn accepts_nihil_returning_side_effect_function() {
    // `→ nihil` is the Faber idiom for a side-effect function whose result is
    // implicitly null; the primary backend lowers it to `-> ()`, so it owes no
    // `redde` even when the body only produces side effects.
    assert_accepts(
        "discretio Nuntius {
            Pulsus { numerus x },
            Finis
        }
        functio tracta(Nuntius e) → nihil {
            discerne e {
                casu Pulsus fixum x ergo nota x
                casu Finis ergo nota 0
            }
        }",
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn accepts_cursor_generator_without_redde() {
    // A `@ cursor` generator's `→ T` is its yielded type (produced by `cede`),
    // not a normal return, so it must not be flagged even without `redde`.
    assert_accepts(
        "@ cursor
        functio rangeSync(numerus n) → numerus {
            itera ab 0‥n fixum i {
                cede i
            }
        }
        incipit {
            itera ex rangeSync(3) fixum num {
                nota num
            }
        }",
    );
}

#[test]
fn rejects_statement_bodied_closure_with_missing_return() {
    assert_rejects(
        "incipit {
            fixum _ maybe ← (bivalens b) → numerus ∴ fac {
                si b ergo redde 1
            }
        }",
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn accepts_statement_bodied_closure_when_all_paths_return() {
    assert_accepts(
        "incipit {
            fixum _ abs ← (numerus x) → numerus ∴ fac {
                si x > 0 {
                    redde x
                } secus {
                    redde 0 - x
                }
            }
            nota abs(-3)
        }",
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn accepts_expression_bodied_closure_without_redde() {
    assert_accepts(
        "incipit {
            fixum _ summa ← (numerus a, numerus b) → numerus ∴ a + b
            nota summa(2, 3)
        }",
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn accepts_fac_bodied_closure_with_redde() {
    assert_accepts(
        "incipit {
            fixum _ bump ← numerus n → numerus ∴ fac {
                redde n + 10
            }
            nota bump(5)
        }",
    );
}

#[test]
fn diagnostic_uses_missing_return_kind() {
    // The diagnostic reuses the `MissingReturn` kind / `SEM033` code rather
    // than introducing a new variant, so tooling that matches on the code
    // keeps working.
    let result = compile(
        &session(),
        "return_path.fab",
        "functio f(bivalens b) → numerus { si b ergo redde 1 }",
    );
    let diag = result
        .diagnostics
        .iter()
        .find(|d| d.code == Some("SEM033"))
        .expect("SEM033 diagnostic present");
    assert_eq!(diag.code, Some("SEM033"));
    assert!(diag
        .args
        .iter()
        .any(|arg| arg.name == "issue" && arg.value == "control_flow_fallthrough"));
    // The kind is exercised transitively through the catalog; the code is the
    // stable contract. Keep a direct kind reference for documentation parity.
    let _ = SemanticErrorKind::MissingReturn;
}

// --- edge/sad path tests ----------------------------------------------------

#[test]
fn rejects_nested_si_both_branches_missing_return() {
    assert_rejects(
        "functio deep(bivalens a, bivalens b) → numerus {
            si a {
                si b {
                    nota 1
                }
            }
        }",
    );
}

#[test]
fn rejects_si_secus_with_no_return_in_either_branch() {
    assert_rejects(
        "functio half(bivalens b) → numerus {
            si b {
                nota 1
            } secus {
                nota 0
            }
        }",
    );
}

#[test]
fn rejects_enum_discerne_with_missing_variant_not_covered() {
    // When a variant is left uncovered and there is no ceterum/catchall and no
    // trailing redde, the function must reject.
    assert_rejects(
        "discretio Color { Rubeus, Viridis, Caeruleus }
        functio label(Color c) → textus {
            discerne c {
                casu Rubeus ergo redde \"red\"
                casu Viridis ergo redde \"green\"
            }
        }",
    );
}

#[test]
fn rejects_early_throw_but_later_fallthrough() {
    assert_rejects(
        "functio example(numerus x) → numerus ⇥ textus {
            si x < 0 ergo iace \"negative\"
        }",
    );
}

#[test]
fn rejects_dum_verum_with_rumpe_in_si_cape() {
    // F-053: `contains_rumpe` must walk `then_catch` on the `Si` expression
    // arm. A `rumpe` inside the catch block of a `si` inside a `dum verum`
    // loop means the loop is not provably diverging.
    assert_rejects(
        "functio f() → numerus {
            dum verum {
                si verum {
                    nota 1
                } cape err {
                    rumpe
                }
            }
        }",
    );
}

#[test]
fn rejects_dum_verum_with_rumpe_in_closure_body() {
    // F-078: `contains_rumpe_expr` must walk `Clausura` bodies. A `rumpe`
    // inside a closure nested in a `dum verum` loop means the loop is not
    // provably diverging, so the enclosing function must reject.
    assert_rejects(
        "functio f() → numerus {
            dum verum {
                fixum _ g ← (numerus x) → vacuum ∴ fac { rumpe }
            }
        }",
    );
}
