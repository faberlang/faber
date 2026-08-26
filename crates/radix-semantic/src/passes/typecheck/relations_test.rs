//! U1 (type-algebra-error-lifecycle): semantic relations — the first real
//! consumer of `HirStruct.extends`/`implements`.
//!
//! Probes p1–p14 (green flips + regression floor) and the declared negatives
//! live as durable fixtures under `test-fixtures/oop-relations/`; the sources
//! here are the inline equivalents asserted issue-by-issue.
//!
//! E1 (equality-tier-respelling) also pins the scalar equality relation
//! ladder on this configured (en) surface: relaxed `≅`/`≈` admit mixed-width
//! numeric pairs, strict `≡` stays fail-closed — see the law block on
//! `numeric_value_equality_types`.

use crate::locale::LocalePack;
use crate::test_support::{analyze_source_with_config, AnalyzedFixture};
use crate::PassConfig;
use radix_target::Target;

fn en_config() -> PassConfig {
    let mut config = PassConfig::for_target(Target::HirRust);
    config.locale_pack = Some(
        LocalePack::from_toml_path(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../stdlib/locale/en/pack.toml"),
        )
        .expect("load English locale pack"),
    );
    config
}

/// Analyze an English-surface source (probe spellings: `class`, `fn`, `type`).
fn analyze_en(source: &str) -> AnalyzedFixture {
    analyze_source_with_config(source, en_config())
}

fn assert_en_accepts(source: &str) {
    let result = analyze_en(source);
    assert!(
        result.is_ok(),
        "expected green, got: {:?}",
        result.diagnostics
    );
}

fn assert_en_rejects_with_issue(source: &str, expected: &str) {
    let result = analyze_en(source);
    assert!(
        !result.is_ok(),
        "expected failure, got: {:?}",
        result.diagnostics
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|err| err.is_error() && crate::test_support::issue(err) == Some(expected)),
        "expected diagnostic issue {expected:?}, got: {:?}",
        result.diagnostics
    );
}

fn assert_en_rejects(source: &str) {
    let result = analyze_en(source);
    assert!(
        !result.is_ok(),
        "expected failure, got: {:?}",
        result.diagnostics
    );
}

#[test]
fn p1_field_inheritance_flips_green() {
    assert_en_accepts(
        "class Base {
    string message
}

class Child extends Base { }

fn probe() → string {
    const Child c ← Child {message = \"hi\"}
    return c.message
}

main {
    probe()
}
",
    );
}

#[test]
fn p2_method_inheritance_flips_green() {
    assert_en_accepts(
        "class Base {
    string message
    fn message() → string { return self.message }
}

class Child extends Base { }

fn probe() → string {
    const Child c ← Child {message = \"hi\"}
    return c.message()
}

main {
    probe()
}
",
    );
}

#[test]
fn p5_implements_subsumption_at_arguments_flips_green() {
    assert_en_accepts(
        "interface Greets {
    fn greet() → string
}

class Hello implements Greets {
    fn greet() → string { return \"hello\" }
}

fn probe(Greets g) → string {
    return g.greet()
}

main {
    const Hello h ← Hello {}
    probe(h)
}
",
    );
}

#[test]
fn p6_implements_subsumption_at_initializer_flips_green() {
    assert_en_accepts(
        "interface Greets {
    fn greet() → string
}

class Hello implements Greets {
    fn greet() → string { return \"hello\" }
}

fn probe() → string {
    const Greets g ← Hello {}
    return g.greet()
}

main {
    probe()
}
",
    );
}

#[test]
fn p7_extends_subsumption_at_arguments_flips_green() {
    assert_en_accepts(
        "class Base {
    string message
}

class Child extends Base { }

fn probe(Base b) → string {
    return b.message
}

main {
    const Child c ← Child {message = \"hi\"}
    probe(c)
}
",
    );
}

#[test]
fn p8_subsumption_at_returns_flips_green() {
    assert_en_accepts(
        "interface Greets {
    fn greet() → string
}

class Hello implements Greets {
    fn greet() → string { return \"hello\" }
}

fn make() → Greets {
    return Hello {}
}

main {
    make()
}
",
    );
}

#[test]
fn p13c_bodyless_method_under_abstractus_flips_green() {
    assert_en_accepts(
        "abstract class Shape {
    fn area() → float
}

class Square extends Shape {
    float side
    fn area() → float { return self.side * self.side }
}

main {
    const Square s ← Square {side = 2.0}
    print s.area()
}
",
    );
}

#[test]
fn p14_end_to_end_virtual_dispatch_checks_green() {
    assert_en_accepts(
        "interface Greets {
    fn greet() → string
}

class Formal implements Greets {
    fn greet() → string { return \"good evening\" }
}

class Casual implements Greets {
    fn greet() → string { return \"yo\" }
}

fn greet_of(Greets g) → string {
    return g.greet()
}

main {
    const Formal a ← Formal {}
    const Casual b ← Casual {}
    const string out1 ← greet_of(a)
    print out1
    const string out2 ← greet_of(b)
    print out2
}
",
    );
}

// NEGATIVES — the declared red set.

#[test]
fn heritage_cycle_is_rejected() {
    assert_en_rejects_with_issue(
        "class A extends B {
    string x
}

class B extends A {
    string y
}

main {
    print \"cycle\"
}
",
        "heritage_cycle",
    );
}

#[test]
fn extends_naming_non_genus_is_rejected() {
    assert_en_rejects_with_issue(
        "interface Greets {
    fn greet() → string
}

class Weird extends Greets {
    fn greet() → string { return \"weird\" }
}

main {
    print \"kind\"
}
",
        "extends_requires_genus",
    );
}

#[test]
fn implet_naming_non_implendum_is_rejected() {
    assert_en_rejects_with_issue(
        "class Base {
    string message
}

class Weird implements Base {
    string note
}

main {
    print \"kind\"
}
",
        "implet_requires_implendum",
    );
}

#[test]
fn missing_impl_body_in_chain_is_rejected() {
    assert_en_rejects_with_issue(
        "abstract class Shape {
    fn area() → float
}

class Circle extends Shape {
    float radius
}

main {
    print \"missing\"
}
",
        "abstract_method_unimplemented",
    );
}

#[test]
fn abstract_construction_is_rejected() {
    assert_en_rejects_with_issue(
        "abstract class Shape {
    fn area() → float { return 0.0 }
}

main {
    const Shape s ← Shape {}
    print s.area()
}
",
        "abstract_construction",
    );
}

#[test]
fn p11_field_redeclaration_flips_red() {
    assert_en_rejects_with_issue(
        "class Base {
    string message
}

class Child extends Base {
    string message
}

fn probe() → string {
    const Child c ← Child {message = \"hi\"}
    return c.message
}

main {
    probe()
}
",
        "field_redeclaration",
    );
}

#[test]
fn incompatible_override_is_rejected() {
    assert_en_rejects_with_issue(
        "class Base {
    fn name() → string { return \"base\" }
}

class Child extends Base {
    fn name() → float { return 1.5 }
}

main {
    print \"override\"
}
",
        "override_signature_incompatible",
    );
}

#[test]
fn invariant_generics_reject_lista_child_into_lista_base() {
    assert_en_rejects_with_issue(
        "class Base {
    string message
}

class Child extends Base { }

fn probe(lista<Base> xs) → void {
    print \"probe\"
}

main {
    const Child c ← Child {message = \"hi\"}
    const lista<Child> cs ← [c]
    probe(cs)
}
",
        "argument_type_mismatch",
    );
}

// FUNNEL COVERAGE — all four sites plus the `⇥` channel.

#[test]
fn channel_subsumption_resolves_iace_value_type_mismatch() {
    assert_en_accepts(
        "interface Greets {
    fn greet() → string
}

class Hello implements Greets {
    fn greet() → string { return \"hello\" }
}

fn failable() → vacuum ⇥ Greets ∪ string {
    const Hello h ← Hello {}
    throw h
}

main {
    do {
        failable()
    } catch err {
        print \"caught\"
    }
}
",
    );
}

#[test]
fn intersection_slot_decomposes_at_the_funnel() {
    assert_en_accepts(
        "interface Readable {
    fn read() → string
}

interface Seekable {
    fn seek() → string
}

type Reversible = Readable ∩ Seekable

class Doc implements Readable ∩ Seekable {
    fn read() → string { return \"r\" }
    fn seek() → string { return \"s\" }
}

fn probe(Reversible r) → void {
    print \"accepted\"
}

main {
    const Doc d ← Doc {}
    probe(d)
}
",
    );
}

#[test]
fn inherited_interface_conformance_flows_through_extends() {
    assert_en_accepts(
        "interface Greets {
    fn greet() → string
}

class Base implements Greets {
    fn greet() → string { return \"base\" }
}

class Child extends Base { }

fn probe(Greets g) → string {
    return g.greet()
}

main {
    const Child c ← Child {}
    print probe(c)
}
",
    );
}

#[test]
fn bodyless_relaxation_is_abstractus_only() {
    // A bodyless method under a CONCRETE genus stays a parse error.
    let result = analyze_en(
        "class Concrete {
    fn bodyless() → string
}

main {
    print \"no\"
}
",
    );
    assert!(
        !result.is_ok(),
        "bodyless method outside abstractus must not parse, got: {:?}",
        result.diagnostics
    );
}

// EQUALITY RELATIONS — the scalar equality relation ladder on the configured
// surface (E1 contract pin): relaxed ≅/≈ admit the mixed-width carrier pairs
// the widening join accepts; strict ≡ rejects the same pairs fail-closed.

#[test]
fn equality_relation_relaxed_admits_mixed_width_pairs() {
    assert_en_accepts(
        "main {
    assert 1 ∷ int<i64> ≅ 1 ∷ int<u64>
    assert 1 ∷ int<i64> ≅ 1.0 ∷ float<f64>
    assert 1 ∷ int<u64> ≅ 1.0 ∷ float<f64>
    assert 1 ∷ int<i64> ≈ 1 ∷ int<u64>
}
",
    );
}

#[test]
fn equality_relation_strict_rejects_mixed_width_pairs() {
    assert_en_rejects_with_issue(
        "main {
    assert 1 ∷ int<i64> ≡ 1 ∷ int<u64>
}
",
        "equality_exact_types_required",
    );
}
