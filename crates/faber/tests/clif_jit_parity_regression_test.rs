//! Regression boundary for native execution: supported programs agree with
//! the runner; unsupported shapes decline before any program output or effects.
//! Corpus exempla and smaller fixtures prove the same boundary independently.

#![cfg(all(feature = "runner", feature = "mir-clif"))]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn run(source: &Path, engine: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_faber"))
        .args(["run", "--target", engine, "--device", "reference"])
        .arg(source)
        .stdin(Stdio::null())
        .output()
        .expect("run faber")
}

fn golden(case: &str) -> Vec<u8> {
    fs::read(root().join("corpus").join(format!("{case}.expected"))).expect("corpus golden")
}

fn failure_line(output: &Output) -> Option<String> {
    String::from_utf8_lossy(&output.stderr)
        .lines()
        .find(|line| line.starts_with("abort:") || line.starts_with("error:"))
        .map(str::to_owned)
}

/// The `CODE` of the first rendered `error[CODE[:issue]]` header, or `None`
/// when stderr carries no coded error diagnostic.
fn first_error_code(stderr: &str) -> Option<&str> {
    stderr.lines().find_map(|line| {
        let rest = line.strip_prefix("error[")?;
        let (identity, _) = rest.split_once(']')?;
        Some(identity.split_once(':').map_or(identity, |(code, _)| code))
    })
}

fn assert_refused(source: &Path, shape: &str) {
    let jit = run(source, "clif-jit");
    let object = Command::new(env!("CARGO_BIN_EXE_faber"))
        .args(["emit", "--target", "clif"])
        .arg(source)
        .stdin(Stdio::null())
        .output()
        .expect("emit clif");
    for output in [&jit, &object] {
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(65), "{source:?}: {stderr}");
        assert!(output.stdout.is_empty(), "refusal must precede execution");
        assert_eq!(
            first_error_code(&stderr),
            Some("CODEGEN001"),
            "refusal is the compile-lane decline: {stderr}"
        );
        assert!(stderr.contains(shape), "expected {shape}: {stderr}");
    }
}

#[test]
fn corpus_lowering_gaps_refuse_by_name_instead_of_running_differently() {
    for (case, shape, status, stdout) in [
        (
            "ad/handler-throw-catch",
            "call handler conversation",
            0,
            "caught bad 4\n",
        ),
        (
            "bits/bits-scalar-conversion",
            "b64 to u64 conversion",
            0,
            "",
        ),
        ("functio/sponte-vel", "optional parameter omission", 0, ""),
        (
            "sponte/call-defaults",
            "optional parameter omission",
            0,
            "Mx A\nDr A\n1\n7\n31\n23\n6\n",
        ),
        (
            "itera/range-runtime-step",
            "inclusive loop with i64-max operand",
            70,
            "8\nstart\n",
        ),
        ("unio/mut-param", "mutable union parameter", 0, "3\n20\n3\n"),
    ] {
        let source = root().join("corpus").join(format!("{case}.fab"));
        let runner = run(&source, "runner");
        assert_eq!(runner.status.code(), Some(status), "{case}: {runner:?}");
        let expected = if stdout.is_empty() {
            golden(case)
        } else {
            stdout.as_bytes().to_vec()
        };
        assert_eq!(runner.stdout, expected, "{case}: runner oracle");
        assert_refused(&source, shape);
    }
    let source = root().join("corpus/tensor/itera-ex-air-rank1.fab");
    let runner = run(&source, "runner");
    assert!(!runner.status.success());
    assert!(String::from_utf8_lossy(&runner.stderr).contains("TARGETLANE001"));
    assert_refused(
        &source,
        "unsupported MIR lowering: path that does not resolve to a local value",
    );
}

#[test]
fn corpus_unwrap_and_inf_boxing_match_the_runner() {
    for (case, status) in [
        ("assertio/nonnull-none-abort", 70),
        ("operatores/inf-est", 0),
        ("operatores/inf-nul", 0),
        ("operatores/inf-valor", 0),
    ] {
        let source = root().join("corpus").join(format!("{case}.fab"));
        let runner = run(&source, "runner");
        let jit = run(&source, "clif-jit");
        assert_eq!(runner.status.code(), Some(status), "{case}: {runner:?}");
        assert_eq!(jit.status.code(), Some(status), "{case}: {jit:?}");
        assert_eq!(runner.stdout, golden(case), "{case}: runner golden");
        assert_eq!(jit.stdout, runner.stdout, "{case}: stdout");
        assert_eq!(failure_line(&jit), failure_line(&runner), "{case}: stderr");
    }
}

#[test]
fn jit_specific_pins_preserve_correct_display_and_air_support() {
    for case in [
        "bounded/conversio-lista-to-octeti",
        "cape/receptio-air",
        "genus/air-struct-literal",
        "summa/reducta-first-air",
    ] {
        let source = root().join("corpus").join(format!("{case}.fab"));
        let jit = run(&source, "clif-jit");
        assert_eq!(jit.status.code(), Some(0), "{case}: {jit:?}");
        if source.with_extension("expected").is_file() {
            assert_eq!(jit.stdout, golden(case), "{case}: display rule");
        } else {
            assert!(jit.stdout.is_empty(), "{case}: declaration-only");
        }
        let runner = run(&source, "runner");
        if case.starts_with("bounded/") {
            assert_eq!(runner.status.code(), Some(0));
            assert_ne!(runner.stdout, jit.stdout, "runner's known display failure");
        } else {
            assert!(!runner.status.success(), "runner's AIR lane restriction");
            assert!(String::from_utf8_lossy(&runner.stderr).contains("TARGETLANE001"));
        }
        // The `full` suite is a private-tree fixture (it pins every target,
        // device lanes included), so its leg compiles out of the public cut;
        // the `public` suite leg runs everywhere.
        #[cfg(feature = "device")]
        {
            let manifest = root().join(format!(
                "corpus-suites/full/cases/language/{case}.fab/case.toml"
            ));
            let pin: toml::Value =
                toml::from_str(&fs::read_to_string(manifest).expect("case pin")).expect("TOML");
            assert_eq!(pin["targets"]["clif-jit"]["outcome"].as_str(), Some("pass"));
            assert_eq!(
                pin["targets"]["runner"]["outcome"].as_str(),
                Some("known-fail")
            );
        }
        let manifest = root().join(format!(
            "corpus-suites/public/cases/language/{case}.fab/case.toml"
        ));
        let pin: toml::Value =
            toml::from_str(&fs::read_to_string(manifest).expect("case pin")).expect("TOML");
        assert_eq!(pin["targets"]["clif-jit"]["outcome"].as_str(), Some("pass"));
        assert_eq!(
            pin["targets"]["runner"]["outcome"].as_str(),
            Some("known-fail")
        );
    }
}

#[test]
fn small_unsupported_programs_decline_before_even_the_first_print() {
    let dir = tempfile::tempdir().expect("scratch");
    for (name, source, shape) in [
        (
            "handler",
            "@ call 'm:fail'\nfn fail(int n) → int ⇥ string {\n    throw \"bad §\"(n)\n}\nmain {\n    print \"before\"\n    do {\n        let int n ← call 'm:fail' (4) ↦ int\n        print n\n    } catch e {\n        print e\n    }\n}\n",
            "call handler conversation",
        ),
        (
            "bits",
            "main {\n    print \"before\"\n    let b64 b ← 0xffffffffffffffff\n    let u64 n ← b ↦ u64\n    print n\n}\n",
            "b64 to u64 conversion",
        ),
        (
            "omitted",
            "fn pag(int p optional coalesce 1) → int { return p }\nmain {\n    print \"before\"\n    print pag()\n}\n",
            "optional parameter omission",
        ),
        (
            "bare_optional",
            "fn greet(string n, string t optional) → string {\n    return t coalesce \"Mx\" + \" \" + n\n}\nmain {\n    print \"before\"\n    print greet(\"A\")\n}\n",
            "optional parameter omission",
        ),
        (
            "edge",
            "main {\n    print \"before\"\n    mut int n ← 0\n    for range 9223372036854775800…9223372036854775807 let i {\n        n ← n + 1\n    }\n    print n\n}\n",
            "inclusive loop with i64-max operand",
        ),
        (
            "mut_union",
            "class Circ { mut int r = 1 }\nclass Sq { mut int s = 10 }\ntype Figura = Circ ∪ Sq\nfn bump(mut Figura u) → void {}\nmain {\n    print \"before\"\n    mut Circ c ← Circ {}\n    bump(mut c)\n}\n",
            "mutable union parameter",
        ),
    ] {
        let path = dir.path().join(format!("{name}.fab"));
        fs::write(&path, source).expect("fixture");
        assert_refused(&path, shape);
    }
}

#[test]
fn small_unwrap_and_open_integer_programs_preserve_output_and_abort() {
    let dir = tempfile::tempdir().expect("scratch");
    for (name, source, status, expected) in [
        (
            "unwrap",
            "class Persona { mut string nomen }\nfn find() → Persona ∪ none { return null }\nmain {\n    let Persona ∪ none gone ← find()\n    print \"before\"\n    print gone!.nomen\n}\n",
            70,
            "before\n",
        ),
        (
            "inf",
            "fn take(value v) → inf { return v ↦ inf ⊥ -1 }\nmain {\n    let inf small ← 7\n    let inf big ← 18446744073709551616\n    print take(small ↦ value)\n    print take(big ↦ value)\n    print take(3.0 ↦ value)\n}\n",
            0,
            "7\n18446744073709551616\n-1\n",
        ),
    ] {
        let path = dir.path().join(format!("{name}.fab"));
        fs::write(&path, source).expect("fixture");
        let runner = run(&path, "runner");
        let jit = run(&path, "clif-jit");
        assert_eq!(runner.status.code(), Some(status), "{name}: {runner:?}");
        assert_eq!(jit.status.code(), Some(status), "{name}: {jit:?}");
        assert_eq!(runner.stdout, expected.as_bytes());
        assert_eq!(jit.stdout, runner.stdout);
        assert_eq!(failure_line(&jit), failure_line(&runner));
        if status == 70 {
            assert_eq!(
                failure_line(&jit).as_deref(),
                Some("abort: unwrap of nihil")
            );
        }
    }
}
