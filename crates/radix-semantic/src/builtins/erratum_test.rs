//! A2: the builtin generic genus `erratum<T>` and the catch-binding pin.
//!
//! `erratum<T>` resolves as a one-field generic genus (`payload: T`). `catch`
//! still binds the payload `T` of a `⇥ T` channel, never the carrier.

use crate::test_support::{analyze_source, assert_accepts, assert_rejects_with_issue};
use crate::{Primitive, TypeId};
use radix_hir::visit::{HirVisitor, walk_cape, walk_program};

struct CapeBindingCollector {
    tys: Vec<Option<TypeId>>,
}

impl HirVisitor for CapeBindingCollector {
    fn visit_cape(&mut self, cape: &radix_hir::HirCape) {
        self.tys.push(cape.binding_ty);
        walk_cape(self, cape);
    }
}

#[test]
fn erratum_resolves_as_a_generic_genus_with_a_payload_field() {
    assert_accepts("functio lege(erratum<textus> e) → textus {\n    redde e.payload\n}\n");
}

#[test]
fn erratum_payload_carries_the_applied_argument() {
    assert_rejects_with_issue(
        "functio lege(erratum<textus> e) → numerus {\n    redde e.payload\n}\n",
        "return_type_mismatch",
    );
}

#[test]
fn erratum_may_nest_as_a_type_argument() {
    assert_accepts(
        "functio lege(lista<erratum<numerus>> es) → numerus {\n    redde es[0].payload\n}\n",
    );
}

#[test]
fn catch_binds_the_payload_of_a_textus_channel() {
    let source = concat!(
        "functio fallit(numerus x) → numerus ⇥ textus {\n    redde x\n}\n\n",
        "functio f(numerus x) → numerus {\n",
        "    fac {\n        redde fallit(x)\n    } cape err {\n        redde 0\n    }\n}\n",
    );
    let mut unit = analyze_source(source);
    assert!(unit.is_ok(), "expected no errors: {:?}", unit.diagnostics);

    let mut collector = CapeBindingCollector { tys: Vec::new() };
    walk_program(&mut collector, unit.hir.as_ref().expect("hir"));
    let textus = unit.types.primitive(Primitive::Textus);
    assert_eq!(collector.tys.len(), 1, "one catch expected");
    let binding = collector.tys[0].expect("catch binding_ty must be recorded");
    assert!(
        unit.types.equals(binding, textus),
        "catch must bind the payload textus, not a carrier or ignotum"
    );
}

#[test]
fn erratum_identity_has_latin_key() {
    use crate::locale::{KeywordSurface, TypeBindingKind};

    assert_ne!(TypeBindingKind::Erratum, TypeBindingKind::Ratio);
    assert_ne!(
        TypeBindingKind::Erratum,
        TypeBindingKind::Frame(crate::locale::FrameTypeKind::Sermo)
    );
    let pack = crate::locale::latin_locale_pack();
    let surface = KeywordSurface::new(&pack);
    assert_eq!(
        surface.type_surface(TypeBindingKind::Erratum),
        "erratum",
        "the Latin fallback surface must expose the builtin erratum key"
    );
}
