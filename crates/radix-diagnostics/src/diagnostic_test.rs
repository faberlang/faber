use super::{Diagnostic, DiagnosticPhase, Span, get_line_at_offset, offset_to_line};
use crate::diagnostic::MAX_SOURCE_LINE_CHARS;
use std::path::Path;

#[test]
fn io_error_keeps_file_context_without_rendering_path_in_message() {
    let path = Path::new("/tmp/faber/missing/main.fab");
    let error = std::io::Error::from_raw_os_error(2);

    let diagnostic = Diagnostic::io_error(path, &error);

    assert_eq!(diagnostic.phase, DiagnosticPhase::Io);
    assert_eq!(diagnostic.file, path.display().to_string());
    assert_eq!(
        diagnostic
            .args
            .iter()
            .find(|arg| arg.name == "path")
            .map(|arg| arg.value.as_str()),
        Some(path.to_str().expect("UTF-8 path"))
    );
    assert_eq!(
        diagnostic
            .args
            .iter()
            .find(|arg| arg.name == "base")
            .map(|arg| arg.value.as_str()),
        Some("/tmp/faber/missing")
    );
    assert_eq!(
        diagnostic
            .args
            .iter()
            .find(|arg| arg.name == "artifact")
            .map(|arg| arg.value.as_str()),
        Some("source file")
    );
}

#[test]
fn io_error_names_the_artifact_kind_for_each_file_class() {
    let error = std::io::Error::from_raw_os_error(2);
    for (path, expected) in [
        ("/tmp/faber/faber.toml", "manifest"),
        ("/tmp/faber/pack.toml", "locale pack"),
        ("/tmp/faber/suite.proba", "source file"),
        ("/tmp/faber/notes.txt", "file"),
    ] {
        let diagnostic = Diagnostic::io_error(Path::new(path), &error);
        let artifact = diagnostic
            .args
            .iter()
            .find(|arg| arg.name == "artifact")
            .map(|arg| arg.value.as_str());
        assert_eq!(artifact, Some(expected), "artifact for {path}");
    }
}

#[test]
fn offset_to_line_is_one_based_and_advances_after_newline() {
    let source = "ab\ncd\nef";
    assert_eq!(offset_to_line(source, 0), 1);
    assert_eq!(offset_to_line(source, 3), 2);
    assert_eq!(offset_to_line(source, 6), 3);
}

#[test]
fn offset_to_line_clamps_past_eof_to_physical_length() {
    let source = "prima\nsecunda\ntertia\n";
    let physical = u32::try_from(source.lines().count()).expect("fits u32");
    assert_eq!(physical, 3);
    assert_eq!(offset_to_line(source, 1846), physical);
    assert_eq!(offset_to_line("", 0), 1);
    assert_eq!(offset_to_line("\n", 1), 1);
    assert_eq!(offset_to_line("a\n\n", 3), 2);
    assert_eq!(offset_to_line("a\nb", 99), 2);
}

#[test]
fn get_line_at_offset_returns_whole_short_line_unclipped() {
    let source = "prima\nsecunda\ntertia\n";
    assert_eq!(get_line_at_offset(source, 8), "secunda");
}

/// One hostile multi-megabyte line must not drive renderer output volume:
/// the return clips at the documented cap with a trailing ellipsis marker.
#[test]
fn get_line_at_offset_clips_multi_megabyte_line_at_documented_cap() {
    let source = "a".repeat(2_000_000);

    let clipped = get_line_at_offset(&source, 1_999_999);

    assert_eq!(
        clipped.chars().count(),
        MAX_SOURCE_LINE_CHARS + "…".chars().count(),
        "clip cap with ellipsis marker: {clipped:?}"
    );
    assert!(
        clipped.ends_with('…'),
        "ellipsis marker missing: {clipped:?}"
    );
    assert!(
        clipped.starts_with("aaa"),
        "head of the line is kept: {clipped:?}"
    );
}

#[test]
fn source_context_line_never_exceeds_physical_source_length() {
    let mut source = String::new();
    for _ in 0..5 {
        source.push_str("line\n");
    }
    source.push_str("boom token\n");
    let start = u32::try_from(source.find("boom").expect("boom")).expect("fits u32");
    let physical = u32::try_from(source.lines().count()).expect("fits u32");
    assert!(start > physical);

    let diagnostic = Diagnostic::error("boom").with_source_context(
        "main.fab",
        &source,
        Span::new(start, start + 4),
    );
    assert_eq!(diagnostic.line, Some(physical));
    assert_eq!(
        diagnostic.display_location(),
        format!("main.fab:{physical}")
    );
}
