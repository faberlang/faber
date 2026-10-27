//! A top-level `fixum T X = e` is an exported constant (st1 R11): `@ publica`
//! exports it through the file interface as a `Const` entry.

use super::extract_file_interface_with_identity;
use radix_module::driver::{AnalyzeRequest, Config, ModuleImportContract, Session, analyze_source};

fn export_of(source: &str) -> String {
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_target(radix_module::codegen::Target::HirRust),
    );
    let analysis = analyze_source(
        &session,
        "consts.fab",
        source,
        AnalyzeRequest {
            cli_program: None,
            contract: ModuleImportContract::default(),
        },
    )
    .expect("analysis must succeed");
    let interface =
        extract_file_interface_with_identity(&analysis, &["LIMES".to_owned()], "consts.fab", None)
            .expect("extract constant interface");
    format!("{interface:?}")
}

#[test]
fn publica_fixum_constant_exports_as_a_const() {
    let constant = export_of("@ publica\nfixum numerus LIMES = 4096\n");
    assert!(constant.contains("Const"), "{constant}");
    assert!(constant.contains("LIMES"), "{constant}");
}

#[test]
fn exported_constant_carries_its_folded_value() {
    let constant = export_of("@ publica\nfixum numerus LIMES = 4 * 1024\n");
    assert!(constant.contains("Some(Int(4096))"), "{constant}");
}

#[test]
fn exported_constant_without_a_scalar_fold_carries_no_value() {
    let constant = export_of("@ publica\nfixum lista<numerus> LIMES = [1, 2]\n");
    assert!(constant.contains("Const"), "{constant}");
    assert!(!constant.contains("Some(Int"), "{constant}");
}
