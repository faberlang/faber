use super::{Diagnostic, DiagnosticPhase};
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
