//! Need staging: the temporary `RADIX_DEFAULT_CODE_LOCALE=en` switch
//! makes the pack-less single-file check route read `en`. The file and stdin
//! entries of that route must agree in both switch states, and the switch must
//! turn the untagged English source from red to green.
//!
//! The fixture file deliberately carries no `.fab` extension: a `.fab` path is
//! package-shaped and would take the product package route, not the pack-less
//! single-file route this switch governs.

use std::io::Write;
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};

/// Untagged English source: lexes only under the `en` pack.
const EN_SOURCE: &str = "main { print 1 }";

const SWITCH: &str = "RADIX_DEFAULT_CODE_LOCALE";

fn status_code(status: ExitStatus) -> Option<i32> {
    status.code()
}

fn run_check_file(path: &Path, switch: Option<&str>) -> ExitStatus {
    let mut command = Command::new(env!("CARGO_BIN_EXE_faber"));
    command.arg("check").arg(path);
    match switch {
        Some(value) => {
            command.env(SWITCH, value);
        }
        None => {
            command.env_remove(SWITCH);
        }
    }
    command.output().expect("run `faber check <file>`").status
}

fn run_check_stdin(source: &str, switch: Option<&str>) -> ExitStatus {
    let mut command = Command::new(env!("CARGO_BIN_EXE_faber"));
    command
        .arg("check")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    match switch {
        Some(value) => {
            command.env(SWITCH, value);
        }
        None => {
            command.env_remove(SWITCH);
        }
    }
    let mut child = command.spawn().expect("spawn `faber check -`");
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(source.as_bytes())
        .expect("write source to stdin");
    child.wait().expect("wait for `faber check -`")
}

#[test]
fn default_locale_switch_check_file_and_stdin_agree() {
    let temp = tempfile::tempdir().expect("temporary directory");
    let file = temp.path().join("source");
    std::fs::write(&file, EN_SOURCE).expect("write untagged source");

    let file_with_switch = run_check_file(&file, Some("en"));
    let stdin_with_switch = run_check_stdin(EN_SOURCE, Some("en"));
    assert_eq!(
        status_code(file_with_switch),
        Some(0),
        "`faber check <file>` must read the untagged source as `en` under the switch"
    );
    assert_eq!(
        status_code(stdin_with_switch),
        Some(0),
        "`faber check -` must read the untagged source as `en` under the switch"
    );

    let file_without_switch = run_check_file(&file, None);
    let stdin_without_switch = run_check_stdin(EN_SOURCE, None);
    assert_eq!(
        status_code(stdin_without_switch),
        status_code(file_without_switch),
        "with the switch unset the stdin route must behave as the file route does today"
    );
}
