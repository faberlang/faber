use super::*;
use radix::codegen::Target;
use radix::{Compiler, Config, Output};
use std::fs;

#[test]
fn llvm_host_vertical_salve_munde_matches_raw_expected_bytes() {
    let fab_path = crate::paths::corpus_dir().join("incipit/salve-munde.fab");
    let result = Compiler::new(Config::default().with_target(Target::LlvmText)).compile(&fab_path);
    assert!(result.success(), "salve-munde LLVM compile failed");
    let Some(Output::LlvmText(output)) = result.output else {
        panic!("salve-munde did not produce LLVM text");
    };
    let temp_root = super::super::common::make_temp_root();
    let llvm_file = temp_root.join("salve-munde.ll");
    fs::write(&llvm_file, output.code).expect("write salve-munde LLVM text");

    let probe = run_llvm_exemplum(&llvm_file, &temp_root, "salve-munde", &fab_path);

    assert_eq!(
        probe.bucket,
        LlvmRunBucket::OutputMatched,
        "{}",
        probe.reason
    );
    let expected = fs::read(fab_path.with_extension("expected")).expect("read raw expected bytes");
    assert_eq!(expected, b"Salve, Munde!\n");
    assert_eq!(probe.stdout.as_bytes(), expected);
}

#[test]
fn llvm_host_boolean_display_matches_raw_expected_bytes() {
    let fab_path = crate::paths::corpus_dir().join("literalia/boolean.fab");
    let result = Compiler::new(Config::default().with_target(Target::LlvmText)).compile(&fab_path);
    assert!(result.success(), "boolean LLVM compile failed");
    let Some(Output::LlvmText(output)) = result.output else {
        panic!("boolean fixture did not produce LLVM text");
    };
    let temp_root = super::super::common::make_temp_root();
    let llvm_file = temp_root.join("boolean.ll");
    fs::write(&llvm_file, output.code).expect("write boolean LLVM text");

    let probe = run_llvm_exemplum(&llvm_file, &temp_root, "boolean", &fab_path);

    assert_eq!(
        probe.bucket,
        LlvmRunBucket::OutputMatched,
        "{}",
        probe.reason
    );
    let expected =
        fs::read(fab_path.with_extension("expected")).expect("read boolean expected bytes");
    assert_eq!(probe.stdout.as_bytes(), expected);
}

#[test]
fn llvm_host_bivalens_display_fixtures_match_raw_expected_bytes() {
    for (relative_path, stem) in [
        ("conversio/bivalens.fab", "conversio-bivalens"),
        ("falsum/falsum.fab", "falsum"),
        ("verum/verum.fab", "verum"),
    ] {
        let fab_path = crate::paths::corpus_dir().join(relative_path);
        let result =
            Compiler::new(Config::default().with_target(Target::LlvmText)).compile(&fab_path);
        assert!(result.success(), "{relative_path} LLVM compile failed");
        let Some(Output::LlvmText(output)) = result.output else {
            panic!("{relative_path} did not produce LLVM text");
        };
        let temp_root = super::super::common::make_temp_root();
        let llvm_file = temp_root.join(format!("{stem}.ll"));
        fs::write(&llvm_file, output.code).expect("write bivalens LLVM text");

        let probe = run_llvm_exemplum(&llvm_file, &temp_root, stem, &fab_path);

        assert_eq!(
            probe.bucket,
            LlvmRunBucket::OutputMatched,
            "{relative_path}: {}",
            probe.reason
        );
    }
}

#[test]
fn llvm_host_diagnostic_text_fixtures_match_raw_expected_bytes() {
    for (relative_path, stem) in [("vide/vide.fab", "vide"), ("mone/mone.fab", "mone")] {
        let fab_path = crate::paths::corpus_dir().join(relative_path);
        let result =
            Compiler::new(Config::default().with_target(Target::LlvmText)).compile(&fab_path);
        assert!(result.success(), "{relative_path} LLVM compile failed");
        let Some(Output::LlvmText(output)) = result.output else {
            panic!("{relative_path} did not produce LLVM text");
        };
        let temp_root = super::super::common::make_temp_root();
        let llvm_file = temp_root.join(format!("{stem}.ll"));
        fs::write(&llvm_file, output.code).expect("write diagnostic LLVM text");

        let probe = run_llvm_exemplum(&llvm_file, &temp_root, stem, &fab_path);
        assert_eq!(
            probe.bucket,
            LlvmRunBucket::OutputMatched,
            "{relative_path}: {}",
            probe.reason
        );
        if relative_path == "vide/vide.fab" {
            let expected =
                fs::read(fab_path.with_extension("expected")).expect("read vide expected bytes");
            assert_eq!(probe.stdout.as_bytes(), expected);
            assert!(
                probe.stderr.is_empty(),
                "unexpected vide stderr: {:?}",
                probe.stderr
            );
        } else if relative_path == "mone/mone.fab" {
            assert!(
                probe.stdout.is_empty(),
                "unexpected mone stdout: {:?}",
                probe.stdout
            );
            assert_eq!(probe.stderr, "cave\n");
        }
    }
}

#[test]
fn llvm_host_format_text_fixtures_match_raw_expected_bytes() {
    for (relative_path, stem) in [
        ("literalia/ascii.fab", "literalia-ascii"),
        ("literalia/block-string.fab", "literalia-block-string"),
        ("literalia/forma.fab", "literalia-forma"),
        ("literalia/textus.fab", "literalia-textus"),
        ("scriptum/scriptum.fab", "scriptum"),
    ] {
        let fab_path = crate::paths::corpus_dir().join(relative_path);
        let result =
            Compiler::new(Config::default().with_target(Target::LlvmText)).compile(&fab_path);
        assert!(result.success(), "{relative_path} LLVM compile failed");
        let Some(Output::LlvmText(output)) = result.output else {
            panic!("{relative_path} did not produce LLVM text");
        };
        let temp_root = super::super::common::make_temp_root();
        let llvm_file = temp_root.join(format!("{stem}.ll"));
        fs::write(&llvm_file, output.code).expect("write format/text LLVM text");

        let probe = run_llvm_exemplum(&llvm_file, &temp_root, stem, &fab_path);
        assert_eq!(
            probe.bucket,
            LlvmRunBucket::OutputMatched,
            "{relative_path}: {}",
            probe.reason
        );
    }
}

#[test]
fn llvm_host_textus_query_smoke_matches_declared_contract() {
    let fab_path = crate::paths::corpus_dir().join("intrinseca/textus-quaestiones.fab");
    let result = Compiler::new(Config::default().with_target(Target::LlvmText)).compile(&fab_path);
    assert!(result.success(), "textus-quaestiones LLVM compile failed");
    let Some(Output::LlvmText(output)) = result.output else {
        panic!("textus-quaestiones did not produce LLVM text");
    };
    let temp_root = super::super::common::make_temp_root();
    let llvm_file = temp_root.join("textus-quaestiones.ll");
    fs::write(&llvm_file, output.code).expect("write textus query LLVM text");

    let probe = run_llvm_exemplum(&llvm_file, &temp_root, "textus-quaestiones", &fab_path);
    assert_eq!(probe.bucket, LlvmRunBucket::Runnable, "{}", probe.reason);
    assert_eq!(probe.stdout, "10 verum verum verum\n");
    assert!(
        probe.stderr.is_empty(),
        "unexpected stderr: {:?}",
        probe.stderr
    );
    assert_eq!(probe.exit_code, Some(0));
}

#[test]
fn llvm_host_nota_grouping_matches_declared_stream_contract() {
    let fab_path = crate::paths::corpus_dir().join("nota/gradus.fab");
    let result = Compiler::new(Config::default().with_target(Target::LlvmText)).compile(&fab_path);
    assert!(result.success(), "nota/gradus.fab LLVM compile failed");
    let Some(Output::LlvmText(output)) = result.output else {
        panic!("nota/gradus.fab did not produce LLVM text");
    };
    let temp_root = super::super::common::make_temp_root();
    let llvm_file = temp_root.join("nota-gradus.ll");
    fs::write(&llvm_file, output.code).expect("write nota grouping LLVM text");

    let probe = run_llvm_exemplum(&llvm_file, &temp_root, "nota-gradus", &fab_path);
    assert_eq!(probe.bucket, LlvmRunBucket::Runnable, "{}", probe.reason);
    assert_eq!(
        probe.stdout,
        "opus initum\ncondicio: currit\nvide: ansa incipit\nvide: numerus ← 42\n"
    );
    assert_eq!(
        probe.stderr,
        "mone: forma vetus usa\nmone: numerus limen superat: 42\n"
    );
    assert_eq!(probe.exit_code, Some(0));
}

#[test]
fn llvm_host_scalar_conversion_and_failable_fixtures_match_raw_expected_bytes() {
    for (relative_path, stem) in [
        ("conversio/conversio.fab", "conversio-conversio"),
        ("conversio/numeric-bool.fab", "conversio-numeric-bool"),
        ("conversio/octeti.fab", "conversio-octeti"),
        ("cape/cape.fab", "cape"),
        ("iace/functio-fallibilis.fab", "iace-functio-fallibilis"),
        ("iace/iace.fab", "iace"),
    ] {
        let fab_path = crate::paths::corpus_dir().join(relative_path);
        let result =
            Compiler::new(Config::default().with_target(Target::LlvmText)).compile(&fab_path);
        assert!(result.success(), "{relative_path} LLVM compile failed");
        let Some(Output::LlvmText(output)) = result.output else {
            panic!("{relative_path} did not produce LLVM text");
        };
        let temp_root = super::super::common::make_temp_root();
        let llvm_file = temp_root.join(format!("{stem}.ll"));
        fs::write(&llvm_file, output.code).expect("write conversion LLVM text");

        let probe = run_llvm_exemplum(&llvm_file, &temp_root, stem, &fab_path);
        assert_eq!(
            probe.bucket,
            LlvmRunBucket::OutputMatched,
            "{relative_path}: {}",
            probe.reason
        );
    }
}

#[test]
fn llvm_host_fac_cape_failable_fixture_matches_rust_output() {
    let fab_path = crate::paths::corpus_dir().join("fac/fac-cape.fab");
    let result = Compiler::new(Config::default().with_target(Target::LlvmText)).compile(&fab_path);
    assert!(result.success(), "fac/fac-cape.fab LLVM compile failed");
    let Some(Output::LlvmText(output)) = result.output else {
        panic!("fac/fac-cape.fab did not produce LLVM text");
    };
    let temp_root = super::super::common::make_temp_root();
    let llvm_file = temp_root.join("fac-cape.ll");
    fs::write(&llvm_file, output.code).expect("write fac/cape LLVM text");

    let probe = run_llvm_exemplum(&llvm_file, &temp_root, "fac-cape", &fab_path);
    assert_eq!(probe.bucket, LlvmRunBucket::Runnable, "{}", probe.reason);
    assert_eq!(probe.stdout, "Block executed successfully\nAttempt 1\n");
    assert!(
        probe.stderr.is_empty(),
        "unexpected stderr: {:?}",
        probe.stderr
    );
}

#[test]
fn llvm_host_instans_failable_fixture_matches_rust_output() {
    let fab_path = crate::paths::corpus_dir().join("conversio/fallibilis.fab");
    let result = Compiler::new(Config::default().with_target(Target::LlvmText)).compile(&fab_path);
    assert!(
        result.success(),
        "conversio/fallibilis.fab LLVM compile failed"
    );
    let Some(Output::LlvmText(output)) = result.output else {
        panic!("conversio/fallibilis.fab did not produce LLVM text");
    };
    let temp_root = super::super::common::make_temp_root();
    let llvm_file = temp_root.join("conversio-fallibilis.ll");
    fs::write(&llvm_file, output.code).expect("write instans failable LLVM text");

    let probe = run_llvm_exemplum(&llvm_file, &temp_root, "conversio-fallibilis", &fab_path);
    assert_eq!(probe.bucket, LlvmRunBucket::Runnable, "{}", probe.reason);
    assert_eq!(
        probe.stdout,
        "1979-05-27T07:32:00Z\n1979-05-27T07:32:00Z\n1979-05-27T07:32:00Z\n"
    );
    assert_eq!(
        probe.stderr,
        "valor to instans conversion failed\nvalor to instans conversion failed\n"
    );
}

#[test]
fn llvm_host_instans_conversion_fixtures_match_raw_expected_bytes() {
    for (relative_path, stem) in [
        ("conversio/instans.fab", "conversio-instans"),
        (
            "conversio/instans-valor-carrier.fab",
            "conversio-instans-valor-carrier",
        ),
    ] {
        let fab_path = crate::paths::corpus_dir().join(relative_path);
        let result =
            Compiler::new(Config::default().with_target(Target::LlvmText)).compile(&fab_path);
        assert!(result.success(), "{relative_path} LLVM compile failed");
        let Some(Output::LlvmText(output)) = result.output else {
            panic!("{relative_path} did not produce LLVM text");
        };
        let temp_root = super::super::common::make_temp_root();
        let llvm_file = temp_root.join(format!("{stem}.ll"));
        fs::write(&llvm_file, output.code).expect("write instans conversion LLVM text");

        let probe = run_llvm_exemplum(&llvm_file, &temp_root, stem, &fab_path);
        assert_eq!(
            probe.bucket,
            LlvmRunBucket::OutputMatched,
            "{relative_path}: {}",
            probe.reason
        );
    }
}

#[test]
fn llvm_host_regex_conversion_fixture_matches_raw_expected_bytes() {
    let fab_path = crate::paths::corpus_dir().join("conversio/regex.fab");
    let result = Compiler::new(Config::default().with_target(Target::LlvmText)).compile(&fab_path);
    assert!(result.success(), "conversio/regex.fab LLVM compile failed");
    let Some(Output::LlvmText(output)) = result.output else {
        panic!("conversio/regex.fab did not produce LLVM text");
    };
    let temp_root = super::super::common::make_temp_root();
    let llvm_file = temp_root.join("conversio-regex.ll");
    fs::write(&llvm_file, output.code).expect("write regex conversion LLVM text");

    let probe = run_llvm_exemplum(&llvm_file, &temp_root, "conversio-regex", &fab_path);
    assert_eq!(
        probe.bucket,
        LlvmRunBucket::OutputMatched,
        "{}",
        probe.reason
    );
}

#[test]
fn llvm_host_valor_scalar_conversion_fixture_matches_raw_expected_bytes() {
    let fab_path = crate::paths::corpus_dir().join("conversio/valor-scalaria.fab");
    let result = Compiler::new(Config::default().with_target(Target::LlvmText)).compile(&fab_path);
    assert!(
        result.success(),
        "conversio/valor-scalaria.fab LLVM compile failed"
    );
    let Some(Output::LlvmText(output)) = result.output else {
        panic!("conversio/valor-scalaria.fab did not produce LLVM text");
    };
    let temp_root = super::super::common::make_temp_root();
    let llvm_file = temp_root.join("conversio-valor-scalaria.ll");
    fs::write(&llvm_file, output.code).expect("write Valor scalar conversion LLVM text");

    let probe = run_llvm_exemplum(
        &llvm_file,
        &temp_root,
        "conversio-valor-scalaria",
        &fab_path,
    );
    assert_eq!(
        probe.bucket,
        LlvmRunBucket::OutputMatched,
        "{}",
        probe.reason
    );
}
