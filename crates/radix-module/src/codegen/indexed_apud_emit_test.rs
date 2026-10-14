//! D3.1–D3.3 emission pins: `itera ex … apud` over a lista/tabula and
//! composite `iuncta` keys on the HIR backends (Rust, Go, TypeScript) and
//! the Faber re-emitter. Behavior is proven by the corpus goldens
//! (`itera/apud-lista`, `itera/apud-tabula`, `tabula/composite-key`); these
//! pins name the carrier each backend must use.

use crate::codegen::Target;
use crate::driver::reemit_faber;
use crate::driver::{Config, ModuleCompileRequest, Session, compile};

const COMPOSITE: &str = r#"
incipit {
    varia tabula<iuncta<textus, numerus>, numerus> sales ← vacua
    fixum textus east ← "east"
    sales[[east, 2024]] ← 5
    sales[[east, 2025]] ← 9
    nota sales[[east, 2024]]
    itera ex sales apud [regio, annus] fixum v {
        nota regio, annus, v
    }
}"#;

const LISTA: &str = r#"
incipit {
    fixum lista<textus> xs ← ["a", "b"]
    itera ex xs apud [i] fixum x {
        nota i, x
    }
}"#;

fn emit(target: Target, source: &str) -> String {
    let session = Session::new(Config::default().with_target(target));
    let result = compile(&session, "apud.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "{:?}", result.diagnostics);
    match result.output {
        Some(crate::Output::Rust(output)) => output.code,
        Some(crate::Output::Go(output)) => output.code,
        Some(crate::Output::TypeScript(output)) => output.code,
        _ => panic!("expected HIR backend output"),
    }
}

#[cfg(feature = "hir-rust")]
#[test]
fn rust_enumerates_lista_and_destructures_composite_keys() {
    let lista = emit(Target::HirRust, LISTA);
    assert!(lista.contains(".iter().enumerate()"), "{lista}");
    let composite = emit(Target::HirRust, COMPOSITE);
    // The path key part clones so `east` survives its next use (E0382).
    assert!(composite.contains("(l_east.clone(), 2024)"), "{composite}");
    assert!(composite.contains("let (l_regio, l_annus) = "), "{composite}");
}

#[cfg(feature = "hir-go")]
#[test]
fn go_keys_composite_maps_by_comparable_array() {
    let lista = emit(Target::HirGo, LISTA);
    assert!(lista.contains("for l_i, l_x := range l_xs"), "{lista}");
    let composite = emit(Target::HirGo, COMPOSITE);
    assert!(
        composite.contains("l_sales[rt.TupleKey([]any{string(l_east), int(2024)})] = 5"),
        "{composite}"
    );
    // T1-G8: the key helpers live in `faber/rt`; no per-file declaration.
    assert!(!composite.contains("func faberTupleKey("), "{composite}");
    assert!(composite.contains("rt.TupleParts("), "{composite}");
}

#[cfg(feature = "hir-ts")]
#[test]
fn ts_builds_value_keyed_tuple_map() {
    let lista = emit(Target::HirTypeScript, LISTA);
    assert!(lista.contains("for (const [l_i, l_x] of (l_xs).entries())"), "{lista}");
    let composite = emit(Target::HirTypeScript, COMPOSITE);
    assert!(
        composite.contains("new tuple.TupleMap<[string, number], number>()"),
        "{composite}"
    );
    assert!(
        composite.contains("for (const [[l_regio, l_annus], l_v] of (l_sales))"),
        "{composite}"
    );
}

#[test]
fn faber_reemit_keeps_the_bracket_list_key() {
    let session = Session::new(Config::default().with_dev_locale_root());
    let pack = crate::locale::latin_locale_pack();
    let result = reemit_faber(&session, &pack, "apud.fab", COMPOSITE);
    assert!(result.success(), "{:?}", result.diagnostics);
    let code = result.output.expect("re-emit output").code;
    assert!(code.contains("sales[[east, 2024]]"), "{code}");
    assert!(!code.contains("iuncta<textus, numerus> ["), "{code}");
}
