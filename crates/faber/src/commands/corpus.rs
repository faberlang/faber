//! Run a source case through the public MIR runner and compare its stdout.

use crate::cli::CorpusArgs;
use faber_cli::package;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub(super) fn cmd_corpus(args: &CorpusArgs) {
    let input = package::absolutize_path(&args.input);
    let expected = match expected_path(&input, args.expected.as_deref()) {
        Ok(path) => path,
        Err(message) => fail(&message),
    };
    let golden = fs::read(&expected).unwrap_or_else(|error| {
        fail(&format!(
            "cannot read golden {}: {error}",
            expected.display()
        ))
    });

    let faber = std::env::current_exe()
        .unwrap_or_else(|error| fail(&format!("cannot locate faber executable: {error}")));
    let output = Command::new(faber)
        .arg("run")
        .arg(&input)
        .output()
        .unwrap_or_else(|error| fail(&format!("cannot run {}: {error}", input.display())));

    if !output.status.success() {
        eprintln!("corpus FAIL: {} exited {}", input.display(), output.status);
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
        std::process::exit(1);
    }
    if output.stdout != golden {
        eprintln!(
            "corpus FAIL: {} differs from {}",
            input.display(),
            expected.display()
        );
        eprintln!("  expected: {:?}", String::from_utf8_lossy(&golden));
        eprintln!("  actual:   {:?}", String::from_utf8_lossy(&output.stdout));
        std::process::exit(1);
    }

    eprintln!("corpus PASS: {} (MIR runner)", input.display());
}

fn expected_path(input: &Path, explicit: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(path) = explicit {
        return Ok(package::absolutize_path(path));
    }
    if input.is_file() && input.extension().is_some_and(|ext| ext == "fab") {
        return Ok(input.with_extension("expected"));
    }
    let package = package::discover_package(input).map_err(|diagnostic| diagnostic.message)?;
    Ok(package.entry.with_extension("expected"))
}

fn fail(message: &str) -> ! {
    eprintln!("corpus FAIL: {message}");
    std::process::exit(1);
}
