use super::{offset_to_line, Diagnostic, DiagnosticPhase, Span};
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
