//! A top-level `fixum T X = e` is the same exported constant a `generis`
//! declaration names (st1 R11): `@ publica` exports it through the same file
//! interface entry.

use super::extract_file_interface_with_identity;
use radix_module::driver::{AnalyzeRequest, Config, ModuleImportContract, Session, analyze_source};

fn export_of(source: &str) -> String {
    let session =
        Session::new(Config::default().with_target(radix_module::codegen::Target::HirRust));
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
fn publica_fixum_constant_exports_like_a_publica_generis_constant() {
    let constant = export_of("@ publica\nfixum numerus LIMES = 4096\n");
    let legacy = export_of("@ publica\ngeneris numerus LIMES = 4096\n");
    assert!(constant.contains("Const"), "{constant}");
    assert_eq!(constant, legacy);
}
