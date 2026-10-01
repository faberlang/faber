//! Semantic relations over `HirStruct.implements`: contract (`implet`)
//! subsumption at the assignment funnel and its declared negatives. Class
//! inheritance (`extends`/`abstractus`) was removed (DECISIONS D8.1), so a
//! genus relates to other types only through the contracts it declares.
//!
//! E1 (equality-tier-respelling) also pins the scalar equality relation
//! ladder on this configured (en) surface: relaxed `≅`/`≈` admit mixed-width
//! numeric pairs, strict `≡` stays fail-closed — see the law block on
//! `numeric_value_equality_types`.

use crate::PassConfig;
use crate::locale::LocalePack;
use crate::test_support::{AnalyzedFixture, analyze_source_with_config};
use radix_target::Target;

fn en_config() -> PassConfig {
    let mut config = PassConfig::for_target(Target::HirRust);
    config.locale_pack = Some(
        LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
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
fn implet_naming_non_implendum_is_rejected() {
    assert_en_rejects_with_issue(
        "class Base {
    var string message
}

class Weird implements Base {
    var string note
}

main {
    print \"kind\"
}
",
        "implet_requires_implendum",
    );
}

#[test]
fn invariant_generics_reject_lista_concrete_into_lista_contract() {
    assert_en_rejects_with_issue(
        "interface Greets {
    fn greet() → string
}

class Hello implements Greets {
    fn greet() → string { return \"hello\" }
}

fn probe(lista<Greets> xs) → void {
    print \"probe\"
}

main {
    const Hello h ← Hello {}
    const lista<Hello> hs ← [h]
    probe(hs)
}
",
        "argument_type_mismatch",
    );
}

#[test]
fn genus_does_not_flow_into_an_unrelated_genus() {
    assert_en_rejects_with_issue(
        "class Base {
    var string message
}

class Other {
    var string message
}

fn probe(Base b) → string {
    return b.message
}

main {
    const Other o ← Other {message = \"hi\"}
    probe(o)
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
fn bodyless_genus_method_is_rejected() {
    // Bodyless methods belong to contracts; a genus method needs a body.
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
        "bodyless genus method must not parse, got: {:?}",
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
    assert 1 ∷ i64 ≅ 1 ∷ u64
    assert 1 ∷ i64 ≅ 1.0 ∷ f64
    assert 1 ∷ u64 ≅ 1.0 ∷ f64
    assert 1 ∷ i64 ≈ 1 ∷ u64
}
",
    );
}

#[test]
fn equality_relation_strict_rejects_mixed_width_pairs() {
    assert_en_rejects_with_issue(
        "main {
    assert 1 ∷ i64 ≡ 1 ∷ u64
}
",
        "equality_exact_types_required",
    );
}
