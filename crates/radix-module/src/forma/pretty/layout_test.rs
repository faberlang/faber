//! Leaf-owned tests for the pretty layout core (writer + line index).

use super::{LineIndex, PrettyWriter, INDENT_WIDTH, SOFT_WIDTH};

#[test]
fn constants_are_structural() {
    assert_eq!(INDENT_WIDTH, 4);
    assert_eq!(SOFT_WIDTH, 100);
}

#[test]
fn writer_indents_four_spaces_per_depth() {
    let mut w = PrettyWriter::new();
    w.writeln("a");
    w.indented(|w| {
        w.writeln("b");
        w.indented(|w| w.writeln("c"));
    });
    w.writeln("d");
    assert_eq!(w.finish(), "a\n    b\n        c\nd\n");
}

#[test]
fn writer_tracks_columns_through_indent() {
    let mut w = PrettyWriter::new();
    w.write("fixum");
    assert_eq!(w.column(), 5);
    w.write(" ");
    assert_eq!(w.column(), 6);
    w.newline();
    w.indented(|w| {
        w.write("x");
        assert_eq!(w.column(), INDENT_WIDTH + 1);
    });
}

#[test]
fn raw_write_does_not_indent_embedded_newlines() {
    let mut w = PrettyWriter::new();
    w.indented(|w| {
        w.writeln("a");
        w.write_raw("«multi\nline»");
    });
    assert_eq!(w.finish(), "    a\n«multi\nline»");
}

#[test]
fn inline_trial_fails_on_newline() {
    let mut t = PrettyWriter::trial_at(0);
    t.write("ok");
    assert!(!t.is_failed());
    t.newline();
    assert!(t.is_failed());
}

#[test]
fn inline_trial_measures_width_from_start_column() {
    let mut t = PrettyWriter::trial_at(30);
    t.write("abcd");
    assert_eq!(t.column(), 34);
    assert!(!t.is_failed());
}

#[test]
fn inline_trial_explicit_fail_propagates() {
    let mut t = PrettyWriter::trial_at(0);
    t.fail();
    assert!(t.is_failed());
}

#[test]
fn first_line_trial_stops_at_line_break() {
    let mut t = PrettyWriter::first_line_trial_at(10);
    t.write("head ");
    assert_eq!(t.column(), 15);
    t.newline();
    t.write("ignored rest");
    // Column stays at the first-line width; the trial is not failed.
    assert_eq!(t.column(), 15);
    assert!(!t.is_failed());
}

#[test]
fn line_index_detects_multiline_spans() {
    let source = "functio f() {\n    redde 1\n}\n";
    let index = LineIndex::new(source);
    let single = crate::lexer::Span::new(0, 11); // `functio f()` — one line
    assert!(!index.is_multiline(single));
    let multi = crate::lexer::Span::new(0, 14); // includes the newline
    assert!(index.is_multiline(multi));
    let body = crate::lexer::Span::new(14, 26); // `    redde 1\n` — line plus newline
    assert!(index.is_multiline(body));
}

#[test]
fn line_index_reports_one_based_lines() {
    let source = "a\nbb\nccc\n";
    let index = LineIndex::new(source);
    assert_eq!(index.line_of(0), 1);
    assert_eq!(index.line_of(1), 1);
    assert_eq!(index.line_of(2), 2);
    assert_eq!(index.line_of(4), 2); // the newline after "bb" ends line 2
    assert_eq!(index.line_of(5), 3);
}

#[test]
fn line_index_empty_source_is_single_line() {
    let index = LineIndex::new("");
    assert!(!index.is_multiline(crate::lexer::Span::new(0, 0)));
    assert_eq!(index.line_of(0), 1);
}
