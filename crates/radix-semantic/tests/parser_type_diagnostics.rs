//! Reader-pack rendering proof for structured type-parser token contracts.
//!
//! The production locale resolver consumes parser facts without prose recovery.

use radix_diagnostics::{Diagnostic, DiagnosticArg};
use radix_lexer::lex;
use radix_parser::{ParseErrorKind, parse};
use radix_semantic::locale::LocalePack;
use std::path::PathBuf;

const TYPE_ISSUES: [&str; 9] = [
    "type_arguments_end",
    "function_type_start",
    "function_type_parameters_end",
    "function_type_return_arrow",
    "numeric_wrapper_start",
    "tuple_type_label_separator",
    "ratio_type_label_separator",
    "shape_dimensions_end",
    "shape_parenthesis_end",
];

fn load_pack(locale: &str) -> LocalePack {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../locale")
        .join(locale)
        .join("pack.toml");
    LocalePack::from_toml_path(path).expect("validated reference pack")
}

#[test]
fn parser_type_diagnostic_resolves_through_two_locale_templates() {
    let source = "incipit { fixum atomic<numerus value ← 0 }";
    let parsed = parse(lex(source));
    let error = parsed
        .errors
        .iter()
        .find(|error| error.kind == ParseErrorKind::Expected)
        .expect("missing type delimiter");
    assert_eq!(
        error.args,
        vec![DiagnosticArg::new("issue", "type_arguments_end")]
    );
    let diagnostic = Diagnostic::error("PARSE001")
        .with_code("PARSE001")
        .with_args(error.args.clone())
        .with_span(error.span);
    for (locale, expected) in [
        ("en", "expected '>' after type arguments"),
        ("hi", "प्रकार तर्कों के बाद '>' अपेक्षित है"),
    ] {
        let rendered = load_pack(locale)
            .render_diagnostic_text(&diagnostic)
            .expect("render")
            .expect("template");
        println!("{locale}: {}", rendered.message);
        assert_eq!(rendered.message, expected);
    }
}

#[test]
fn type_issue_rows_are_complete_and_renderable_in_all_reference_packs() {
    let mut checked = 0;
    for locale in ["en", "la", "ar", "hi", "vi", "th-TH", "zh-Hans", "zh-Hant"] {
        let pack = load_pack(locale);
        let rows = &pack.diagnostics["PARSE001"].issues;
        for issue in TYPE_ISSUES {
            let row = rows
                .get(issue)
                .expect("explicit issue row, not base fallback");
            let diagnostic = Diagnostic::error("PARSE001")
                .with_code("PARSE001")
                .with_args(vec![DiagnosticArg::new("issue", issue)]);
            let rendered = pack
                .render_diagnostic_text(&diagnostic)
                .expect("render without missing arguments")
                .expect("template");
            assert_eq!(rendered.message, row.message, "{locale}.{issue}");
            checked += 1;
        }
    }
    assert_eq!(checked, 72);
    println!("type issue rows rendered: {checked} (9 issues x 8 packs)");
}
