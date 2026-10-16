//! Source-level tests for the return-path exhaustiveness pass (DEFER-011).
//!
//! These compile Faber source through the full driver pipeline and assert on
//! the `SEM033` (`MissingReturn`) diagnostic and its structured issue fact. The
//! driver is the most realistic seam: it exercises parse → collect → resolve →
//! lower → typecheck → exhaustive → `return_path`, so a passing test proves the
//! invariant end to end rather than against a hand-built HIR fixture.

use crate::SemanticError;
use crate::SemanticErrorKind;

fn is_return_path_error(diag: &SemanticError) -> bool {
    diag.kind == SemanticErrorKind::ReturnAbsent && diag.issue() == Some("control_flow_fallthrough")
}

fn diagnostic_facts(diagnostics: &[SemanticError]) -> Vec<(SemanticErrorKind, Option<&str>)> {
    diagnostics
        .iter()
        .map(|diag| (diag.kind, diag.issue()))
        .collect()
}

fn assert_rejects(source: &str) {
    let result = crate::test_support::analyze_source_la(source);
    assert!(result.is_err(), "expected semantic rejection");
    assert!(
        result.diagnostics.iter().any(is_return_path_error),
        "expected a return-path diagnostic, got {:?}",
        diagnostic_facts(&result.diagnostics)
    );
}

fn assert_accepts(source: &str) {
    let result = crate::test_support::analyze_source_la(source);
    let other_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|diag| is_return_path_error(diag))
        .collect();
    assert!(
        other_errors.is_empty(),
        "expected no return-path diagnostic, got {other_errors:?}"
    );
    assert!(
        result.is_ok(),
        "expected clean analysis, got {:?}",
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
            si x ≻ 0 {
                redde x
            } secus {
                nota x
            }
        }",
    );
}

#[test]
fn accepts_both_branches_returning() {
    assert_accepts(
        "functio abs(numerus x) → numerus {
            si x ≻ 0 {
                redde x
            } secus {
                redde 0 - x
            }
        }",
    );
}

#[test]
fn accepts_if_without_else_falls_through_to_trailing_redde() {
    assert_accepts(
        "functio clamp(numerus x) → numerus {
            si x ≻ 10 {
                redde 10
            }
            redde x
        }",
    );
}

#[test]
fn accepts_exhaustive_enum_discerne_without_catchall() {
    // Coverage is provable for an enum scrutinee with all variants covered,
    // even without a wildcard arm (the `omnia` interaction from DEFER-028).
    assert_accepts(
        "discretio Lumen {
            Lucet,
            Tenebrae
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
            dum n ≻ 0 {
                n ← n - 1
            }
        }",
    );
}

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
                casu Pulsus sit x ergo nota x
                casu Finis ergo nota 0
            }
        }",
    );
}

#[test]
fn accepts_cursor_generator_without_redde() {
    // A `@ cursor` generator's `→ T` is its yielded type (produced by `cede`),
    // not a normal return, so it must not be flagged even without `redde`.
    assert_accepts(
        "@ cursor
        functio rangeSync(numerus n) → numerus {
            itera ab 0‥n sit i {
                cede i
            }
        }
        incipit {
            itera ex rangeSync(3) sit num {
                nota num
            }
        }",
    );
}

#[test]
fn rejects_statement_bodied_closure_with_missing_return() {
    assert_rejects(
        "incipit {
            sit maybe ← (bivalens b) → numerus ∴ fac {
                si b ergo redde 1
            }
        }",
    );
}

#[test]
fn accepts_statement_bodied_closure_when_all_paths_return() {
    assert_accepts(
        "incipit {
            sit abs ← (numerus x) → numerus ∴ fac {
                si x ≻ 0 {
                    redde x
                } secus {
                    redde 0 - x
                }
            }
            nota abs(-3)
        }",
    );
}

#[test]
fn accepts_expression_bodied_closure_without_redde() {
    assert_accepts(
        "incipit {
            sit summa ← (numerus a, numerus b) → numerus ∴ a + b
            nota summa(2, 3)
        }",
    );
}

#[test]
fn accepts_fac_bodied_closure_with_redde() {
    assert_accepts(
        "incipit {
            sit bump ← numerus n → numerus ∴ fac {
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
    let result = crate::test_support::analyze_source_la(
        "functio f(bivalens b) → numerus { si b ergo redde 1 }",
    );
    let diag = result
        .diagnostics
        .iter()
        .find(|d| crate::semantic_spec(d.kind).code == "SEM033")
        .expect("SEM033 diagnostic present");
    assert_eq!(crate::semantic_spec(diag.kind).code, "SEM033");
    assert!(
        diag.args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == "control_flow_fallthrough")
    );
    // The kind is exercised transitively through the catalog; the code is the
    // stable contract. Keep a direct kind reference for documentation parity.
    let _ = SemanticErrorKind::ReturnAbsent;
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
            si x ≺ 0 ergo iace \"negative\"
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
                sit g ← (numerus x) → vacuum ∴ fac { rumpe }
            }
        }",
    );
}

#[test]
fn rejects_genus_method_with_value_return_and_no_redde() {
    // A method owes the same normal-return contract as a free function. Before
    // the pass visited struct methods this passed `check` and only failed late
    // in MIR with "no-value return in value-returning function".
    assert_rejects(
        "genus Counter {
            functio get() → numerus {
                nota \"x\"
            }
        }",
    );
}

#[test]
fn rejects_genus_method_when_only_one_branch_returns() {
    assert_rejects(
        "genus Counter {
            functio half(numerus x) → numerus {
                si x ≻ 0 {
                    redde x
                } secus {
                    nota x
                }
            }
        }",
    );
}

#[test]
fn accepts_genus_method_returning_on_every_path() {
    assert_accepts(
        "genus Counter {
            functio abs(numerus x) → numerus {
                si x ≻ 0 {
                    redde x
                } secus {
                    redde 0 - x
                }
            }
        }",
    );
}

#[test]
fn accepts_genus_method_without_a_value_result() {
    assert_accepts(
        "genus Counter {
            functio noop() → vacuum {
                nota \"x\"
            }
        }",
    );
}

#[test]
fn accepts_genus_generator_method_without_redde() {
    // A generator's `→ T` is the yielded type, so a method owes no `redde`
    // either; the exemption is the same one free functions get.
    assert_accepts(
        "genus Streamer {
            @ cursor
            functio rangeSync(numerus n) → numerus {
                itera ab 0‥n sit i {
                    cede i
                }
            }
        }",
    );
}

#[test]
fn accepts_fac_dum_verum_tail_return() {
    // `fac { … } dum verum` is the do-while spelling of an infinite loop; the
    // body's `redde` is reachable and the synthesized break test is dead, so
    // the function terminates exactly like `dum verum { … }`.
    assert_accepts(
        "functio f() → numerus {
            varia numerus i ← 0
            fac {
                i ← i + 1
                si i ≡ 4 ergo redde i * 10
            } dum verum
        }",
    );
}

#[test]
fn rejects_fac_dum_conditional_tail_return() {
    // A non-constant do-while condition can exit before the body returns.
    assert_rejects(
        "functio f(numerus n) → numerus {
            fac {
                si n ≻ 0 ergo redde 1
            } dum n ≻ 0
        }",
    );
}

#[test]
fn rejects_fac_dum_verum_with_rumpe_in_body() {
    // A user-written `rumpe` still escapes the loop, so the function can reach
    // its end without returning.
    assert_rejects(
        "functio f() → numerus {
            fac {
                rumpe
            } dum verum
        }",
    );
}
