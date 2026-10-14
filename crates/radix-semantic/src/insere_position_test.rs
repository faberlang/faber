//! `insere` in every `=` position (st1 U4, DECISIONS D8.10 + D5.10).
//!
//! Contract:
//! - a block-level `=` constant and a genus field default under every
//!   modifier (`fixum`, `varia`, `generis`) fold an embed exactly as a module
//!   constant does: one initializer surface;
//! - the slot types the embed: a `redde insere "p"` inside a `praefixum` body
//!   takes the constant's `textus`; a `_` slot is SEM061
//!   `insere_type_required`;
//! - outside a compile-time initializer an embed is SEM061
//!   `insere_outside_constant`;
//! - driver rejections surface at a block-level embed too;
//! - no `Insere` node survives analysis.

use crate::insere::{InsereFiles, InsereRejection, InsereSource};
use crate::test_support::{analyze_source_with_config_la, issue};
use crate::{PassConfig, SemanticErrorKind};
use radix_hir::visit::{HirVisitor, walk_expr};
use radix_hir::{HirExpression, HirExpressionKind, HirLiteral};
use radix_target::Target;

fn config_with(path: &str, source: InsereSource) -> PassConfig {
    let mut config = PassConfig::for_target(Target::HirRust);
    let mut files = InsereFiles::default();
    files.insert(path, source);
    config.insere_files = files;
    config
}

fn text_config() -> PassConfig {
    config_with("n.txt", InsereSource::Bytes(b"salve".to_vec()))
}

fn rejects_with(source: &str, config: PassConfig, expected: &str) {
    let fixture = analyze_source_with_config_la(source, config);
    assert!(
        fixture.diagnostics.iter().any(|err| {
            err.kind == SemanticErrorKind::InsereInvalid && issue(err) == Some(expected)
        }),
        "expected SEM061 {expected:?}, got: {:?}",
        fixture.diagnostics
    );
}

/// What survived analysis: every `textus` literal, whether an `octeti`
/// literal exists, and whether any `Insere` node is left.
#[derive(Default)]
struct Survivors {
    texts: Vec<String>,
    octeti: bool,
    unfolded: bool,
}

struct Scan<'a> {
    interner: &'a radix_lexer::Interner,
    out: Survivors,
}

impl HirVisitor for Scan<'_> {
    fn visit_expr(&mut self, expr: &HirExpression) {
        match &expr.kind {
            HirExpressionKind::Literal(HirLiteral::String(sym)) => {
                self.out.texts.push(self.interner.resolve(*sym).to_owned());
            }
            HirExpressionKind::Literal(HirLiteral::Octeti(_)) => self.out.octeti = true,
            HirExpressionKind::Insere(_) => self.out.unfolded = true,
            _ => {}
        }
        walk_expr(self, expr);
    }
}

fn survivors(source: &str, config: PassConfig) -> Survivors {
    let fixture = analyze_source_with_config_la(source, config);
    assert!(fixture.is_ok(), "diagnostics: {:?}", fixture.diagnostics);
    let hir = fixture.result.hir.as_ref().expect("hir");
    let mut scan = Scan {
        interner: &fixture.interner,
        out: Survivors::default(),
    };
    scan.visit_program(hir);
    scan.out
}

fn assert_folded_text(found: &Survivors, context: &str) {
    assert!(
        found.texts.iter().any(|text| text == "salve"),
        "{context}: {:?}",
        found.texts
    );
    assert!(!found.unfolded, "{context}: an embed survived analysis");
}

#[test]
fn block_level_constant_embed_folds() {
    let found = survivors(
        "functio f() → textus {\n    fixum textus x = insere \"n.txt\"\n    redde x\n}\n",
        text_config(),
    );
    assert_folded_text(&found, "block-level constant");
}

#[test]
fn block_level_octeti_embed_folds_to_raw_bytes() {
    let config = config_with("b.bin", InsereSource::Bytes(vec![0x01, 0xff]));
    let found = survivors(
        "functio f() → octeti {\n    fixum octeti b = insere \"b.bin\"\n    redde b\n}\n",
        config,
    );
    assert!(found.octeti && !found.unfolded);
}

#[test]
fn every_field_modifier_default_embed_folds() {
    for modifier in ["fixum", "varia", "generis"] {
        let source = format!("genus G {{\n    {modifier} textus s = insere \"n.txt\"\n}}\n");
        let found = survivors(&source, text_config());
        assert_folded_text(&found, modifier);
    }
}

#[test]
fn embed_inside_a_praefixum_body_takes_the_slot_type() {
    let found = survivors(
        "fixum textus S = praefixum {\n    redde insere \"n.txt\"\n}\n",
        text_config(),
    );
    assert_folded_text(&found, "praefixum body");
}

#[test]
fn embed_in_a_hole_slot_requires_a_type() {
    rejects_with(
        "fixum _ S = insere \"n.txt\"\n",
        text_config(),
        "insere_type_required",
    );
}

#[test]
fn embed_into_a_non_file_slot_is_one_error() {
    let fixture =
        analyze_source_with_config_la("fixum numerus N = insere \"n.txt\"\n", text_config());
    let errors: Vec<_> = fixture
        .diagnostics
        .iter()
        .filter(|err| err.is_error())
        .collect();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].kind, SemanticErrorKind::InsereInvalid);
    assert_eq!(issue(errors[0]), Some("insere_type_invalid"));
}

#[test]
fn embed_outside_a_constant_initializer_is_rejected() {
    for body in [
        "nota insere \"n.txt\"",
        "varia textus x ← \"a\"\n    x ← insere \"n.txt\"",
        "fixum textus x ← insere \"n.txt\"",
    ] {
        let source = format!("incipit {{\n    {body}\n}}\n");
        rejects_with(&source, text_config(), "insere_outside_constant");
    }
}

#[test]
fn praefixum_default_of_an_instance_field_is_checked_for_purity() {
    // The one initializer surface includes `praefixum` for `fixum`/`varia`
    // fields, so its body answers to the same purity rule as a static's.
    let fixture = analyze_source_with_config_la(
        "genus G {\n    varia textus v = praefixum {\n        nota \"x\"\n        redde \"y\"\n    }\n}\n",
        PassConfig::for_target(Target::HirRust),
    );
    assert!(
        fixture.diagnostics.iter().any(|err| {
            err.kind == SemanticErrorKind::PraefixumInvalid
                && issue(err) == Some("praefixum_forbidden")
        }),
        "{:?}",
        fixture.diagnostics
    );
}

#[test]
fn block_level_embed_rejections_surface_their_issue() {
    let config = config_with("p.txt", InsereSource::Rejected(InsereRejection::Missing));
    rejects_with(
        "functio f() → textus {\n    fixum textus x = insere \"p.txt\"\n    redde x\n}\n",
        config,
        "insere_file_missing",
    );
}
