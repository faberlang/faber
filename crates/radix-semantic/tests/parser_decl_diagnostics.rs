//! Reader-pack rendering proof for declaration-parser token contracts.

use radix_diagnostics::{Diagnostic, DiagnosticArg};
use radix_semantic::locale::LocalePack;
use std::path::PathBuf;

const DECL_ISSUES: [&str; 35] = [
    "awaited_binding_assignment",
    "array_binding_pattern_start",
    "array_binding_pattern_end",
    "object_binding_pattern_start",
    "object_binding_pattern_end",
    "function_declaration_parameters_start",
    "function_declaration_parameters_end",
    "class_body_end",
    "interface_body_start",
    "interface_method_parameters_start",
    "interface_method_parameters_end",
    "interface_body_end",
    "type_alias_value_separator",
    "schema_body_start",
    "schema_body_end",
    "enum_body_start",
    "enum_body_end",
    "union_body_start",
    "union_variant_fields_end",
    "union_body_end",
    "import_record_start",
    "import_field_value_separator",
    "import_record_end",
    "probandum_body_start",
    "probandum_body_end",
    "annotation_fields_start",
    "annotation_field_value_separator",
    "annotation_fields_end",
    "nucleum_annotation_fields_start",
    "nucleum_annotation_field_value_separator",
    "nucleum_annotation_fields_end",
    "block_start",
    "block_end",
    "generic_parameters_end",
    "contract_type_arguments_end",
];

fn load_pack(locale: &str) -> LocalePack {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../locale")
        .join(locale)
        .join("pack.toml");
    LocalePack::from_toml_path(path).expect("validated reference pack")
}

#[test]
fn declaration_issue_rows_render_in_all_locale_packs() {
    let mut checked = 0;
    for locale in ["en", "la", "ar", "hi", "vi", "th-TH", "zh-Hans", "zh-Hant"] {
        let pack = load_pack(locale);
        let rows = &pack.diagnostics["PARSE001"].issues;
        for issue in DECL_ISSUES {
            assert!(
                rows.contains_key(issue),
                "{locale}.{issue} must have an explicit declaration issue row"
            );
            let diagnostic = Diagnostic::error("PARSE001")
                .with_code("PARSE001")
                .with_args(vec![DiagnosticArg::new("issue", issue)]);
            assert_eq!(diagnostic.code, Some("PARSE001"), "{locale}.{issue}");
            assert_eq!(
                diagnostic.args,
                vec![DiagnosticArg::new("issue", issue)],
                "{locale}.{issue}"
            );
            pack.render_diagnostic_text(&diagnostic)
                .expect("render without missing arguments")
                .expect("template");
            checked += 1;
        }
    }
    assert_eq!(checked, 280);
    println!("declaration issue rows rendered: {checked} (35 issues x 8 packs)");
}
