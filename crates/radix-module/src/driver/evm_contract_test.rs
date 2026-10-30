//! Source-level admission, refusal and preservation for EVM contracts.

use crate::driver::{Config, Target};
use crate::{Compiler, Output};

const CONTRACT: &str = r#"+++
locale = "en"
+++
@ contract { target = "evm" }
class Arithmetic {
    @ public
    fn add(int a, int b) → int { return a + b }
    fn double(int a) → int { return a * 2 }
    @ public
    fn twice(int a) → int { return self.double(a) }
    @ public
    fn empty() → void { }
}
"#;

fn compile(source: &str, target: Target) -> crate::CompileResult {
    let pack = crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
        .expect("English reader pack");
    Compiler::new(Config::default().with_target(target).with_locale_pack(pack)).compile_str("contract.fab", source)
}

fn refuses(source: &str, issue: &str, reason: &str) {
    let result = compile(source, Target::HirFaber);
    assert!(!result.success(), "variant must be refused");
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code.as_deref() == Some("SEM018")
                && diagnostic.issue() == Some(issue)
                && diagnostic
                    .args
                    .iter()
                    .any(|arg| arg.name == "reason" && arg.value == reason)),
        "expected {issue}/{reason}: {:?}",
        result.diagnostics
    );
}

#[cfg(feature = "hir-faber")]
#[test]
fn contract_marker_and_public_private_methods_reemit_without_receiver_changes() {
    let result = compile(CONTRACT, Target::HirFaber);
    assert!(result.success(), "{:?}", result.diagnostics);
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.issue() == Some("redundant_member_publica"))
    );
    let Some(Output::Faber(output)) = result.output else {
        panic!("expected Faber output")
    };
    assert!(output.code.contains("@ contract { target = \"evm\" }"));
    assert!(output.code.contains("fn twice(int a)"));
    let again = compile(&output.code, Target::HirFaber);
    assert!(again.success(), "{:?}", again.diagnostics);
}

#[test]
fn executable_target_cannot_ignore_contract_environment() {
    let result = compile(CONTRACT, Target::HirRust);
    assert!(!result.success());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.issue() == Some("contract_target_mismatch")
                && diagnostic
                    .args
                    .iter()
                    .any(|arg| arg.name == "required" && arg.value == "evm")),
        "{:?}",
        result.diagnostics
    );
}

#[test]
fn contract_marker_payload_is_exact_and_class_only() {
    for (from, to, reason) in [
        ("{ target = \"evm\" }", "", "target_payload_required"),
        ("target = \"evm\"", "target = \"rust\"", "evm_target_payload_required"),
        ("target = \"evm\"", "target = \"evm\", extra = 1", "evm_target_payload_required"),
        (
            "target = \"evm\"",
            "target = \"evm\", target = \"evm\"",
            "evm_target_payload_required",
        ),
    ] {
        refuses(&CONTRACT.replace(from, to), "contract_annotation_invalid", reason);
    }
    refuses(
        "+++\nlocale = \"en\"\n+++\n@ contract { target = \"evm\" }\nfn f() → int { return 1 }",
        "contract_annotation_invalid",
        "class_required",
    );
}

#[test]
fn unsupported_contract_profile_shapes_are_named() {
    for (from, to, reason) in [
        ("class Arithmetic {", "class Arithmetic {\n let int stored", "fields"),
        ("class Arithmetic {", "class Arithmetic<T> {", "generic_class"),
        (
            "fn add(int a, int b) → int",
            "fn add(string a, string b) → string",
            "parameter_shape",
        ),
        ("fn empty() → void", "fn empty<T>() → void", "generic_method"),
    ] {
        refuses(&CONTRACT.replace(from, to), "evm_contract_unsupported", reason);
    }
    refuses(
        &CONTRACT.replace("    @ public\n", ""),
        "evm_contract_unsupported",
        "no_public_entries",
    );
    refuses(
        &format!("{CONTRACT}\nfn construct() → int {{\n let Arithmetic value ← Arithmetic {{ }}\n return 0\n}}"),
        "evm_contract_unsupported",
        "construction",
    );
    refuses(
        &format!(
            "{CONTRACT}\n@ contract {{ target = \"evm\" }}\nclass Another {{ @ public\nfn f() → int {{ return 1 }} }}"
        ),
        "evm_contract_unsupported",
        "multiple_contracts",
    );
}

#[cfg(feature = "hir-faber")]
#[test]
fn unannotated_class_methods_keep_their_normal_source_shape() {
    let source = CONTRACT.replace("@ contract { target = \"evm\" }\n", "");
    let result = compile(&source, Target::HirFaber);
    assert!(result.success(), "{:?}", result.diagnostics);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.issue() == Some("redundant_member_publica"))
    );
    let Some(Output::Faber(output)) = result.output else {
        panic!("expected Faber output")
    };
    assert!(output.code.contains("self.double(a)"));
}

#[cfg(feature = "mir-evm")]
#[test]
fn contract_entries_lower_through_resolved_methods() {
    let result = compile(CONTRACT, Target::MirEvm);
    assert!(result.success(), "{:?}", result.diagnostics);
    assert!(matches!(result.output, Some(Output::Evm(_))));
}
