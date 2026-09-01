//! Focused proof for the output-transfer codemod command.

use super::{DeclineReason, codemod_single_doc};
use clap::Parser;
use std::fs;
use std::path::PathBuf;

fn fixture(name: &str, source: &str) -> (PathBuf, String) {
    let path = std::env::temp_dir().join(format!("faber-output-transfer-codemod-{name}.fab"));
    let _ = fs::remove_file(&path);
    fs::write(&path, source).expect("write codemod fixture");
    (path, source.to_owned())
}

#[test]
fn accepts_latin_multivalue_statement_and_emits_one_transfer_header() {
    let (path, _) = fixture(
        "latin-multi",
        r#"+++
locale = "la"
+++
incipit {
    nota 7, "salve"
}
"#,
    );
    let report = codemod_single_doc(&path, &fs::read_to_string(&path).expect("read fixture"))
        .expect("codemod accepted source");
    let _ = fs::remove_file(&path);

    assert_eq!(report.rewritten_statements, 1);
    assert!(
        report.declines.is_empty(),
        "unexpected declines: {:?}",
        report.declines
    );
    assert_eq!(report.source.matches("norma:console").count(), 1);
    assert!(
        report
            .source
            .contains("importa ex \"norma:console\" fixum dic ut output")
    );
    assert!(report.source.contains("output ⇇ \"§ §\\n\"(7, \"salve\")"));
    assert!(report.source.contains("⇇"));
    assert!(!report.source.contains("nota 7, \"salve\""));
}

#[test]
fn declines_non_normal_output_shapes_with_structured_reasons_and_reemits_them() {
    let source = r#"+++
locale = "la"
+++
incipit {
    mone "warning"
    vide "debug"
}
"#;
    let (path, original) = fixture("latin-declines", source);
    let report = codemod_single_doc(&path, &original).expect("declined source still re-emits");
    let _ = fs::remove_file(&path);

    assert_eq!(report.rewritten_statements, 0);
    assert_eq!(report.declines.len(), 2);
    assert_eq!(report.source, original);
    assert_decline(&report.declines[0], "unsupported_output_kind");
    assert_decline(&report.declines[1], "unsupported_output_kind");
    assert!(report.source.contains("mone \"warning\""));
    assert!(report.source.contains("vide \"debug\""));
}

#[test]
fn declines_unsupported_locale_through_the_structured_header_decline() {
    // The None arm of the surface-header let-else: an explicit unknown reader
    // locale yields no safe header, so the rewrite declines instead of
    // panicking. Proven at the rewrite seam; the command layer surfaces this
    // locale error before the decline print.
    let source = r#"+++
locale = "fr"
+++
incipit {
    nota "salve"
}
"#;
    let (rewritten, count, declines) = super::rewrite_source(source).expect("source lexes");
    assert_eq!(count, 0);
    assert_eq!(declines.len(), 1);
    assert_decline(&declines[0], "unsupported_locale");
    assert_eq!(rewritten, source);
}

#[test]
fn cli_exposes_the_dedicated_output_transfer_codemod_command() {
    let cli = crate::cli::Cli::try_parse_from([
        "faber",
        "output-transfer-codemod",
        "--stdout",
        "main.fab",
    ])
    .expect("parse output-transfer-codemod");
    let Some(crate::cli::Command::OutputTransferCodemod(args)) = cli.command else {
        panic!("expected output-transfer-codemod command");
    };
    assert!(args.stdout);
    assert_eq!(args.paths, vec![PathBuf::from("main.fab")]);
}

fn assert_decline(reason: &DeclineReason, code: &str) {
    assert_eq!(reason.code, code);
    assert!(reason.end >= reason.start);
}

fn rust_emit(name: &str, source: &str) -> String {
    let result =
        radix::services::compile_cli_source(name, source, radix::module::codegen::Target::HirRust);
    assert!(
        result.success(),
        "native rust emit failed for {name}: {:?}",
        result.diagnostics
    );
    radix::services::output_code(result.output.expect("rust output"))
}

fn rust_format_literal(code: &str, macro_name: &str) -> String {
    let needle = format!("{macro_name}!(\"");
    let start = code
        .find(&needle)
        .unwrap_or_else(|| panic!("missing {macro_name}! format string in:\n{code}"))
        + needle.len();
    let bytes = code.as_bytes();
    let mut end = start;
    while end < bytes.len() {
        if bytes[end] == b'"' && (end == start || bytes[end - 1] != b'\\') {
            break;
        }
        end += 1;
    }
    code[start..end].to_owned()
}

fn decode_format_source(source: &str) -> String {
    let mut out = String::new();
    let mut chars = source.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('\\') => out.push('\\'),
            Some('"') => out.push('"'),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

#[test]
fn eot5b_representative_pre_post_native_output_is_byte_identical() {
    // Representative pre/post through the EOT-5A command. Native identity is
    // the shared hir-rust format table (rust_scribe_format /
    // rust_format_template / generate_format_arg_expr): scribe println! plus
    // macro newline vs write_partial of a constructor with embedded `\n`.
    // Compared as bytes. Not a corpus-wide migration claim.
    let pre = r#"+++
locale = "la"
+++
incipit {
    nota 7, "salve"
}
"#;
    let (path, _) = fixture("eot5b-pre", pre);
    let report = codemod_single_doc(&path, &fs::read_to_string(&path).expect("read fixture"))
        .expect("codemod accepted representative source");
    let _ = fs::remove_file(&path);

    assert_eq!(report.rewritten_statements, 1);
    assert!(
        report.source.contains("output ⇇ \"§ §\\n\"(7, \"salve\")"),
        "admitted rewrite must keep sequential holes, space separators, and the embedded newline:\n{}",
        report.source
    );

    let pre_rust = rust_emit("eot5b-pre.fab", pre);
    let post_rust = rust_emit("eot5b-post.fab", &report.source);
    let pre_fmt = rust_format_literal(&pre_rust, "println");
    let post_fmt = rust_format_literal(&post_rust, "format");
    assert_eq!(
        pre_fmt.as_bytes(),
        post_fmt.trim_end_matches("\\n").as_bytes(),
        "format-choice table must pick the same slots; diverging {{}}/{{:?}} fails the lemma\npre={pre_fmt:?}\npost={post_fmt:?}"
    );
    assert!(
        post_fmt.ends_with("\\n"),
        "template path must embed the trailing newline: {post_fmt:?}"
    );

    let mut pre_native = decode_format_source(&pre_fmt);
    pre_native.push('\n');
    let post_native = decode_format_source(&post_fmt);
    assert_eq!(
        pre_native.as_bytes(),
        post_native.as_bytes(),
        "EOT-5B compares native output as bytes, not normalized text\npre={pre_native:?}\npost={post_native:?}"
    );
    assert!(
        pre_native.ends_with('\n') && post_native.ends_with('\n'),
        "trailing newline is a native byte, not optional whitespace"
    );
    assert!(
        !pre_rust.contains("format!(") && post_rust.contains("format!("),
        "pre stays on println!; post uses the template path"
    );
}
