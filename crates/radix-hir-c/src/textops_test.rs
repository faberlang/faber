//! Text methods and indexing: the call chosen, never a byte-wise shortcut.

use crate::test_support::emitted;

#[test]
fn length_counts_code_points_through_the_runtime() {
    let code = emitted("main {\n    print \"héllo\".length()\n}");
    assert!(
        code.contains("faber_text_length(\"h\\303\\251llo\")"),
        "{code}"
    );
}

#[test]
fn transforming_methods_allocate_in_the_arena() {
    let code = emitted(
        "main {\n    let string s ← \" a b \"\n    print s.trim()\n    print s.split(\" \")\n    print s.replace(\"a\", \"c\")\n    print s.slice(1, 3)\n}",
    );
    for call in [
        "faber_text_trim(faber_a, l_s)",
        "faber_text_split(faber_a, l_s, \" \")",
        "faber_text_replace(faber_a, l_s, \"a\", \"c\")",
        "faber_text_slice(faber_a, l_s, INT64_C(1), INT64_C(3))",
    ] {
        assert!(code.contains(call), "{call:?} missing in {code}");
    }
}

#[test]
fn queries_do_not_allocate() {
    let code = emitted(
        "main {\n    let string s ← \"ab\"\n    print s.contains(\"a\"), s.starts_with(\"a\"), s.is_empty()\n}",
    );
    assert!(code.contains("faber_text_contains(l_s, \"a\")"), "{code}");
    assert!(!code.contains("faber_arena faber_root"), "{code}");
}

#[test]
fn indexing_text_reads_a_code_point() {
    let code = emitted("main {\n    let string s ← \"xyz\"\n    print s[1]\n}");
    assert!(
        code.contains("faber_text_at(faber_a, l_s, INT64_C(1))"),
        "{code}"
    );
}

#[test]
fn an_ascii_literal_is_a_text_literal() {
    let code = emitted("main {\n    let ascii route ← 'pg:query'\n    print route\n}");
    assert!(code.contains("\"pg:query\""), "{code}");
}
