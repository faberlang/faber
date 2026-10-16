//! `@ nondum` free functions and `genus` methods: declaring is legal, every
//! use is refused.
//!
//! The refusal is SEM017 `nondum_function_for_target` and covers a direct
//! call, a call through an imported namespace or an imported name, and the
//! function used as a value. A gate without a target covers every target; a
//! gate with a target covers only that target. A declaration that nothing
//! references compiles, with or without a body.

use std::collections::BTreeMap;

use crate::SemanticErrorKind;
use crate::file_interface::{
    FileExport, FileExportKind, FileInterface, InterfaceCallable, InterfaceMethodExport,
    InterfaceParam, InterfaceStructExport, InterfaceStructField, InterfaceTypeSnapshot,
    InterfaceUnavailable,
};
use crate::passes::visibility_annotation::VisibilityTier;
use crate::test_support::{self, AnalyzedFixture};
use crate::{Primitive, SemanticParamMode};
use radix_target::Target;

const ISSUE: &str = "nondum_function_for_target";

fn arg<'a>(err: &'a crate::SemanticError, name: &str) -> Option<&'a str> {
    err.args
        .iter()
        .find(|arg| arg.name == name)
        .map(|arg| arg.value.as_str())
}

fn refusals(result: &AnalyzedFixture) -> Vec<&crate::SemanticError> {
    result
        .diagnostics
        .iter()
        .filter(|err| err.is_error() && test_support::issue(err) == Some(ISSUE))
        .collect()
}

fn errors(result: &AnalyzedFixture) -> Vec<&crate::SemanticError> {
    result
        .diagnostics
        .iter()
        .filter(|err| err.is_error())
        .collect()
}

fn assert_refused_once(result: &AnalyzedFixture, function: &str, reason: Option<&str>) {
    let found = refusals(result);
    assert_eq!(found.len(), 1, "diagnostics: {:?}", result.diagnostics);
    assert_eq!(found[0].kind, SemanticErrorKind::UnavailableDeclaration);
    assert_eq!(arg(found[0], "function"), Some(function));
    assert_eq!(arg(found[0], "reason"), reason);
    assert!(
        test_support::args_have_no_legacy_message(&found[0].args),
        "structured args only"
    );
}

// ---------------------------------------------------------------------------
// Same-file functions
// ---------------------------------------------------------------------------

const DIRECT_CALL: &str = r#"
@ nondum "pending host dispatch"
functio later(textus s) → textus

incipit {
    nota later("x")
}
"#;

#[test]
fn direct_call_of_a_nondum_function_is_refused() {
    let result = test_support::analyze_source_la(DIRECT_CALL);
    assert!(result.is_err());
    assert_refused_once(&result, "later", Some("pending host dispatch"));
}

#[test]
fn nondum_function_used_as_a_value_is_refused() {
    let result = test_support::analyze_source_la(
        r#"
@ nondum "pending host dispatch"
functio later(textus s) → textus

incipit {
    sit _ f ← later
    tacet
}
"#,
    );
    assert!(result.is_err());
    assert_refused_once(&result, "later", Some("pending host dispatch"));
}

#[test]
fn nondum_function_passed_as_an_argument_is_refused() {
    let result = test_support::analyze_source_la(
        r#"
@ nondum "pending host dispatch"
functio later(textus s) → textus

functio apply((textus) → textus f, textus s) → textus {
    redde f(s)
}

incipit {
    nota apply(later, "x")
}
"#,
    );
    assert!(result.is_err());
    assert_refused_once(&result, "later", Some("pending host dispatch"));
}

#[test]
fn every_use_is_refused_separately() {
    let result = test_support::analyze_source_la(
        r#"
@ nondum "later"
functio later(textus s) → textus

incipit {
    nota later("a")
    nota later("b")
}
"#,
    );
    assert_eq!(refusals(&result).len(), 2, "{:?}", result.diagnostics);
}

#[test]
fn a_gate_without_a_target_refuses_on_every_target() {
    for target in [
        Target::HirRust,
        Target::HirTypeScript,
        Target::HirGo,
        Target::MirFmir,
    ] {
        let result = test_support::analyze_source_for_target_la(DIRECT_CALL, target);
        assert_eq!(
            refusals(&result).len(),
            1,
            "{target:?}: {:?}",
            result.diagnostics
        );
    }
}

#[test]
fn a_gate_with_a_target_refuses_only_on_that_target() {
    let source = r#"
@ nondum rs "rust only"
functio later(textus s) → textus {
    redde s
}

incipit {
    nota later("x")
}
"#;
    let on_rust = test_support::analyze_source_for_target_la(source, Target::HirRust);
    assert_refused_once(&on_rust, "later", Some("rust only"));

    let on_ts = test_support::analyze_source_for_target_la(source, Target::HirTypeScript);
    assert!(on_ts.is_ok(), "diagnostics: {:?}", on_ts.diagnostics);
}

#[test]
fn a_nondum_function_with_a_body_is_refused_when_used() {
    let result = test_support::analyze_source_la(
        r#"
@ nondum "withdrawn"
functio later(textus s) → textus {
    redde s
}

incipit {
    nota later("x")
}
"#,
    );
    assert_refused_once(&result, "later", Some("withdrawn"));
}

#[test]
fn a_gate_without_a_reason_carries_no_reason_arg() {
    let result = test_support::analyze_source_la(
        r#"
@ nondum
functio later(textus s) → textus

incipit {
    nota later("x")
}
"#,
    );
    assert_refused_once(&result, "later", None);
}

#[test]
fn braced_gate_form_is_refused() {
    let result = test_support::analyze_source_la(
        r#"
@ nondum { ratio = "braced" }
functio later(textus s) → textus

incipit {
    nota later("x")
}
"#,
    );
    assert_refused_once(&result, "later", Some("braced"));
}

#[test]
fn an_unreferenced_nondum_declaration_compiles() {
    for source in [
        "@ nondum \"later\"\nfunctio later(textus s) → textus\n\nincipit { tacet }\n",
        "@ nondum \"later\"\nfunctio later(textus s) → textus { redde s }\n\nincipit { tacet }\n",
        "@ nondum\nfunctio later() → numerus\n",
    ] {
        let result = test_support::analyze_source_la(source);
        assert!(result.is_ok(), "{source}: {:?}", result.diagnostics);
        assert!(
            result.diagnostics.is_empty(),
            "a declaration-only nondum function raises no warning either: {:?}",
            result.diagnostics
        );
    }
}

#[test]
fn an_unmarked_function_beside_a_nondum_one_still_compiles() {
    let result = test_support::analyze_source_la(
        r#"
@ nondum "later"
functio later(textus s) → textus

functio now(textus s) → textus {
    redde s
}

incipit {
    nota now("x")
}
"#,
    );
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
}

// ---------------------------------------------------------------------------
// Imported functions
// ---------------------------------------------------------------------------

fn text_callable() -> InterfaceCallable {
    InterfaceCallable {
        type_params: Vec::new(),
        params: vec![InterfaceParam {
            ty: InterfaceTypeSnapshot::Primitive(Primitive::Textus),
            mode: SemanticParamMode::Owned,
            optional: false,
            sponte: false,
            axis: false,
        }],
        ret: Box::new(InterfaceTypeSnapshot::Primitive(Primitive::Textus)),
        err: None,
        is_async: false,
        is_generator: false,
        tensor_edge_policy_requirements: Vec::new(),
        device: None,
    }
}

/// A provider exporting `later` (gated by `gates`) and the ungated `now`.
fn provider(gates: Vec<InterfaceUnavailable>) -> FileInterface {
    let mut interface = FileInterface::new();
    for name in ["later", "now"] {
        interface.insert(FileExport {
            name: name.to_owned(),
            kind: FileExportKind::Function(text_callable()),
        });
    }
    if !gates.is_empty() {
        interface.unavailable.insert("later".to_owned(), gates);
    }
    interface
}

fn gate(target: Option<&str>, reason: Option<&str>) -> InterfaceUnavailable {
    InterfaceUnavailable {
        target: target.map(str::to_owned),
        reason: reason.map(str::to_owned),
    }
}

fn analyze_with_provider(
    source: &str,
    target: Target,
    gates: Vec<InterfaceUnavailable>,
) -> AnalyzedFixture {
    let publica = VisibilityTier::Publica;
    let mut config = crate::PassConfig::for_target(target);
    let exports = || BTreeMap::from([("later".to_owned(), publica), ("now".to_owned(), publica)]);
    config.namespace_exports = BTreeMap::from([
        ("mod".to_owned(), exports()),
        ("./mod".to_owned(), exports()),
    ]);
    // Namespace imports key the interface by binding name, selective imports
    // by import path.
    config.file_interfaces = BTreeMap::from([
        ("mod".to_owned(), provider(gates.clone())),
        ("./mod".to_owned(), provider(gates)),
    ]);
    test_support::analyze_source_with_config_la(source, config)
}

fn analyze_import(source: &str, gates: Vec<InterfaceUnavailable>) -> AnalyzedFixture {
    analyze_with_provider(source, Target::HirRust, gates)
}

fn host_gate() -> Vec<InterfaceUnavailable> {
    vec![gate(None, Some("pending host dispatch"))]
}

#[test]
fn call_through_an_imported_namespace_is_refused() {
    let result = analyze_import(
        r#"importa ex "./mod" mod
incipit { nota mod.later("x") }"#,
        host_gate(),
    );
    assert!(result.is_err());
    assert_refused_once(&result, "mod.later", Some("pending host dispatch"));
}

#[test]
fn namespace_function_used_as_a_value_is_refused() {
    let result = analyze_import(
        r#"importa ex "./mod" mod
incipit {
    sit _ f ← mod.later
    tacet
}"#,
        host_gate(),
    );
    assert!(result.is_err());
    assert_refused_once(&result, "mod.later", Some("pending host dispatch"));
}

#[test]
fn call_through_an_imported_name_is_refused() {
    let result = analyze_import(
        r#"importa ex "./mod" fixum later
incipit { nota later("x") }"#,
        host_gate(),
    );
    assert!(result.is_err());
    assert_refused_once(&result, "later", Some("pending host dispatch"));
}

#[test]
fn call_through_an_aliased_imported_name_names_the_alias() {
    let result = analyze_import(
        r#"importa ex "./mod" fixum later ut deferred
incipit { nota deferred("x") }"#,
        host_gate(),
    );
    assert_refused_once(&result, "deferred", Some("pending host dispatch"));
}

#[test]
fn imported_name_used_as_a_value_is_refused_on_every_use() {
    let result = analyze_import(
        r#"importa ex "./mod" fixum later
incipit {
    sit _ f ← later
    sit _ g ← later
    tacet
}"#,
        host_gate(),
    );
    assert_eq!(refusals(&result).len(), 2, "{:?}", result.diagnostics);
}

#[test]
fn importing_a_nondum_name_without_using_it_compiles() {
    for source in [
        r#"importa ex "./mod" mod
incipit { nota mod.now("x") }"#,
        r#"importa ex "./mod" fixum now
incipit { nota now("x") }"#,
    ] {
        let result = analyze_import(source, host_gate());
        assert!(
            errors(&result).is_empty(),
            "{source}: {:?}",
            result.diagnostics
        );
    }
}

#[test]
fn an_ungated_sibling_export_is_usable_beside_a_nondum_one() {
    let result = analyze_import(
        r#"importa ex "./mod" mod
incipit {
    nota mod.now("x")
    nota mod.later("y")
}"#,
        host_gate(),
    );
    assert_refused_once(&result, "mod.later", Some("pending host dispatch"));
}

#[test]
fn imported_target_gate_refuses_only_on_its_target() {
    let source = r#"importa ex "./mod" mod
incipit { nota mod.later("x") }"#;
    let gates = || vec![gate(Some("rs"), Some("rust only"))];

    let on_rust = analyze_with_provider(source, Target::HirRust, gates());
    assert_refused_once(&on_rust, "mod.later", Some("rust only"));

    let on_ts = analyze_with_provider(source, Target::HirTypeScript, gates());
    assert!(errors(&on_ts).is_empty(), "{:?}", on_ts.diagnostics);
}

#[test]
fn imported_selective_target_gate_refuses_only_on_its_target() {
    let source = r#"importa ex "./mod" fixum later
incipit { nota later("x") }"#;
    let gates = || vec![gate(Some("ts"), None)];

    let on_ts = analyze_with_provider(source, Target::HirTypeScript, gates());
    assert_refused_once(&on_ts, "later", None);

    let on_rust = analyze_with_provider(source, Target::HirRust, gates());
    assert!(errors(&on_rust).is_empty(), "{:?}", on_rust.diagnostics);
}

#[test]
fn a_provider_without_gates_changes_nothing() {
    let result = analyze_import(
        r#"importa ex "./mod" mod
incipit { nota mod.later("x") }"#,
        Vec::new(),
    );
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
}

// ---------------------------------------------------------------------------
// Genus methods
// ---------------------------------------------------------------------------

const GENUS: &str = r#"
genus Nexus {
    varia textus nomen

    @ nondum "pending host dispatch"
    functio mitte(textus data) → vacuum

    functio lege() → textus {
        redde ego.nomen
    }
}
"#;

fn with_main(main: &str) -> String {
    format!("{GENUS}\nincipit {{\n    varia Nexus n ← Nexus {{ nomen = \"h\" }}\n{main}\n}}")
}

#[test]
fn instance_call_of_a_nondum_method_is_refused() {
    let result = test_support::analyze_source_la(&with_main("    n.mitte(\"x\")"));
    assert!(result.is_err());
    assert_refused_once(&result, "Nexus.mitte", Some("pending host dispatch"));
}

#[test]
fn self_call_of_a_nondum_method_is_refused() {
    let source = r#"
genus Nexus {
    varia textus nomen

    @ nondum "pending host dispatch"
    functio mitte(textus data) → vacuum

    functio itera() → vacuum {
        ego.mitte("x")
    }
}
"#;
    let result = test_support::analyze_source_la(source);
    assert_refused_once(&result, "Nexus.mitte", Some("pending host dispatch"));
}

#[test]
fn a_nondum_method_does_not_block_its_siblings() {
    let result = test_support::analyze_source_la(&with_main("    nota n.lege()"));
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
}

#[test]
fn a_declared_but_uncalled_nondum_method_compiles() {
    let result = test_support::analyze_source_la(&with_main("    nota n.nomen"));
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
}

#[test]
fn a_method_gate_with_a_target_refuses_only_on_that_target() {
    let source = r#"
genus Nexus {
    varia textus nomen

    @ nondum rs "rust only"
    functio mitte(textus data) → vacuum {
        nota data
    }
}

incipit {
    varia Nexus n ← Nexus { nomen = "h" }
    n.mitte("x")
}
"#;
    let on_rust = test_support::analyze_source_for_target_la(source, Target::HirRust);
    assert_refused_once(&on_rust, "Nexus.mitte", Some("rust only"));

    let on_ts = test_support::analyze_source_for_target_la(source, Target::HirTypeScript);
    assert!(on_ts.is_ok(), "diagnostics: {:?}", on_ts.diagnostics);
}

#[test]
fn a_nondum_method_on_one_genus_does_not_gate_a_same_named_method_elsewhere() {
    let source = r#"
genus Nexus {
    varia textus nomen

    @ nondum "pending"
    functio mitte() → vacuum
}

genus Aliud {
    varia textus nomen

    functio mitte() → vacuum {
        nota ego.nomen
    }
}

incipit {
    varia Aliud a ← Aliud { nomen = "h" }
    a.mitte()
}
"#;
    let result = test_support::analyze_source_la(source);
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
}

/// A `∪` receiver whose members agree on a method signature but disagree on
/// `@ nondum` availability is reachable: `union_common_method_signature`
/// compares signatures only, so the call resolves and the availability gate
/// (call.rs, union branch) runs per member. Intent pinned here: one gated
/// member refuses the WHOLE call with a single SEM017 naming that member,
/// whichever position it holds in the union.
#[test]
fn a_union_call_is_refused_when_any_member_is_nondum() {
    for union in ["Nexus ∪ Aliud", "Aliud ∪ Nexus"] {
        let source = format!(
            r#"
genus Nexus {{
    varia textus nomen

    @ nondum "pending"
    functio mitte() → vacuum
}}

genus Aliud {{
    varia textus nomen

    functio mitte() → vacuum {{
        nota ego.nomen
    }}
}}

functio f({union} x) {{
    x.mitte()
}}
"#
        );
        let result = test_support::analyze_source_la(&source);
        assert_refused_once(&result, "Nexus.mitte", Some("pending"));
    }
}

// Imported genus methods

fn socket_interface(gates: Vec<InterfaceUnavailable>) -> FileInterface {
    let method = |name: &str, unavailable: Vec<InterfaceUnavailable>| InterfaceMethodExport {
        name: name.to_owned(),
        callable: InterfaceCallable {
            type_params: Vec::new(),
            params: Vec::new(),
            ret: Box::new(InterfaceTypeSnapshot::Primitive(Primitive::Vacuum)),
            err: None,
            is_async: false,
            is_generator: false,
            tensor_edge_policy_requirements: Vec::new(),
            device: None,
        },
        visibility: radix_syntax::MemberVisibility::Publica,
        unavailable,
    };
    let mut interface = FileInterface::new();
    interface.insert(FileExport {
        name: "Nexus".to_owned(),
        kind: FileExportKind::Struct(InterfaceStructExport {
            is_json: false,
            implements: Vec::new(),
            params: Vec::new(),
            name: "Nexus".to_owned(),
            fields: vec![InterfaceStructField {
                name: "id".to_owned(),
                ty: InterfaceTypeSnapshot::Primitive(Primitive::Numerus),
                optional: true,
                required: false,
                default: None,
                modifier: Some(radix_syntax::FieldModifier::Varia),
                visibility: radix_syntax::MemberVisibility::Publica,
            }],
            methods: vec![method("claude", gates), method("lege", Vec::new())],
            annotation_contract: None,
        }),
    });
    interface
}

fn analyze_imported_genus(
    body: &str,
    target: Target,
    gates: Vec<InterfaceUnavailable>,
) -> AnalyzedFixture {
    let mut config = crate::PassConfig::for_target(target);
    config.namespace_exports = BTreeMap::from([(
        "nexus".to_owned(),
        BTreeMap::from([("Nexus".to_owned(), VisibilityTier::Publica)]),
    )]);
    config.file_interfaces = BTreeMap::from([("nexus".to_owned(), socket_interface(gates))]);
    test_support::analyze_source_with_config_la(
        &format!(
            "importa ex \"./nexus\" nexus\n\nincipit {{\n    varia nexus.Nexus n ← nexus.Nexus {{ }}\n{body}\n}}"
        ),
        config,
    )
}

#[test]
fn call_of_an_imported_nondum_method_is_refused() {
    let result = analyze_imported_genus("    n.claude()", Target::HirRust, host_gate());
    assert!(result.is_err());
    assert_refused_once(&result, "Nexus.claude", Some("pending host dispatch"));
}

#[test]
fn an_imported_ungated_method_beside_a_nondum_one_is_usable() {
    let result = analyze_imported_genus("    n.lege()", Target::HirRust, host_gate());
    assert!(errors(&result).is_empty(), "{:?}", result.diagnostics);
}

#[test]
fn an_imported_method_gate_with_a_target_refuses_only_on_that_target() {
    let gates = || vec![gate(Some("ts"), None)];
    let on_ts = analyze_imported_genus("    n.claude()", Target::HirTypeScript, gates());
    assert_refused_once(&on_ts, "Nexus.claude", None);

    let on_rust = analyze_imported_genus("    n.claude()", Target::HirRust, gates());
    assert!(errors(&on_rust).is_empty(), "{:?}", on_rust.diagnostics);
}

#[test]
fn an_imported_method_without_gates_changes_nothing() {
    let result = analyze_imported_genus("    n.claude()", Target::HirRust, Vec::new());
    assert!(errors(&result).is_empty(), "{:?}", result.diagnostics);
}
