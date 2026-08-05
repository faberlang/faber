use super::*;
use radix::codegen::Target;
use radix::{Compiler, Config, Output};
use std::fs;

#[test]
#[ignore = "slow LLVM host link+run; run: cargo test -p exempla --test e2e_harness llvm_host_solum_lege_generic -- --ignored --nocapture"]
fn llvm_host_solum_lege_generic_fixture_matches_rust_output() {
    let fab_path = crate::paths::corpus_dir().join("ad/solum-lege-generic.fab");
    let config = radix::Config::default().with_target(Target::LlvmText);
    let llvm = faber_cli::package::with_lowered_package_mir(&config, &fab_path, |lowered| {
        let interner = lowered
            .validated
            .validation()
            .interner
            .ok_or_else(|| "package MIR validation context has no interner".to_owned())?;
        radix::mir::emit_llvm_text_probe(&lowered.validated, interner)
            .map_err(|error| format!("{}:{}", error.category, error.shape))
    })
    .expect("ad/solum-lege-generic.fab package analysis must succeed")
    .expect("ad/solum-lege-generic.fab LLVM emission must succeed");
    let temp_root = super::super::common::make_temp_root();
    let llvm_file = temp_root.join("solum-lege-generic.ll");
    fs::write(&llvm_file, &llvm).expect("write solum-lege-generic LLVM text");

    // Outcome parity vs the Rust oracle: the fixture scribes
    // /tmp/faber-solum-lege-generic.txt, then reads it back as textus /
    // lista<textus> / octeti. The oracle (generated Rust lane) prints:
    //   body text + nota newline, ["prima", "secunda"], [112, 114, …], "solum
    //   lege generic parata" — exit 0. The LLVM host must match byte-for-byte.
    let probe = run_llvm_exemplum(&llvm_file, &temp_root, "solum-lege-generic", &fab_path);
    assert_eq!(probe.bucket, LlvmRunBucket::Runnable, "{}", probe.reason);
    assert_eq!(
        probe.stdout,
        "prima\nsecunda\n\n[\"prima\", \"secunda\"]\n[112, 114, 105, 109, 97, 10, 115, 101, 99, 117, 110, 100, 97, 10]\nsolum lege generic parata\n"
    );
    assert!(
        probe.stderr.is_empty(),
        "unexpected stderr: {:?}",
        probe.stderr
    );
    assert_eq!(probe.exit_code, Some(0));
}

/// Helper: compile a fab file to LLVM text, write the .ll, run it, and assert OutputMatched.
fn assert_llvm_text_output_matches(fab_relative: &str, stem: &str) {
    let fab_path = crate::paths::corpus_dir().join(fab_relative);
    let result = Compiler::new(Config::default().with_target(Target::LlvmText)).compile(&fab_path);
    assert!(result.success(), "{fab_relative} LLVM compile failed");
    let Some(Output::LlvmText(output)) = result.output else {
        panic!("{fab_relative} did not produce LLVM text");
    };
    let temp_root = super::super::common::make_temp_root();
    let llvm_file = temp_root.join(format!("{stem}.ll"));
    fs::write(&llvm_file, output.code).expect("write {fab_relative} LLVM text");
    let probe = run_llvm_exemplum(&llvm_file, &temp_root, stem, &fab_path);
    assert_eq!(
        probe.bucket,
        LlvmRunBucket::OutputMatched,
        "{fab_relative}: {}",
        probe.reason
    );
}

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
fn llvm_host_conversio_bivalens_matches_raw_expected_bytes() {
    assert_llvm_text_output_matches("conversio/bivalens.fab", "conversio-bivalens");
}

#[test]
fn llvm_host_falsum_matches_raw_expected_bytes() {
    assert_llvm_text_output_matches("falsum/falsum.fab", "falsum");
}

#[test]
fn llvm_host_verum_matches_raw_expected_bytes() {
    assert_llvm_text_output_matches("verum/verum.fab", "verum");
}

#[test]
fn llvm_host_vide_diagnostic_text_matches_raw_expected_bytes() {
    let fab_path = crate::paths::corpus_dir().join("vide/vide.fab");
    let result = Compiler::new(Config::default().with_target(Target::LlvmText)).compile(&fab_path);
    assert!(result.success(), "vide/vide.fab LLVM compile failed");
    let Some(Output::LlvmText(output)) = result.output else {
        panic!("vide/vide.fab did not produce LLVM text");
    };
    let temp_root = super::super::common::make_temp_root();
    let llvm_file = temp_root.join("vide.ll");
    fs::write(&llvm_file, output.code).expect("write vide LLVM text");
    let probe = run_llvm_exemplum(&llvm_file, &temp_root, "vide", &fab_path);
    assert_eq!(
        probe.bucket,
        LlvmRunBucket::OutputMatched,
        "vide/vide.fab: {}",
        probe.reason
    );
    let expected = fs::read(fab_path.with_extension("expected")).expect("read vide expected bytes");
    assert_eq!(probe.stdout.as_bytes(), expected);
    assert!(
        probe.stderr.is_empty(),
        "unexpected vide stderr: {:?}",
        probe.stderr
    );
}

#[test]
fn llvm_host_mone_diagnostic_text_matches_raw_expected_bytes() {
    let fab_path = crate::paths::corpus_dir().join("mone/mone.fab");
    let result = Compiler::new(Config::default().with_target(Target::LlvmText)).compile(&fab_path);
    assert!(result.success(), "mone/mone.fab LLVM compile failed");
    let Some(Output::LlvmText(output)) = result.output else {
        panic!("mone/mone.fab did not produce LLVM text");
    };
    let temp_root = super::super::common::make_temp_root();
    let llvm_file = temp_root.join("mone.ll");
    fs::write(&llvm_file, output.code).expect("write mone LLVM text");
    let probe = run_llvm_exemplum(&llvm_file, &temp_root, "mone", &fab_path);
    assert_eq!(
        probe.bucket,
        LlvmRunBucket::OutputMatched,
        "mone/mone.fab: {}",
        probe.reason
    );
    assert!(
        probe.stdout.is_empty(),
        "unexpected mone stdout: {:?}",
        probe.stdout
    );
    assert_eq!(probe.stderr, "cave\n");
}

#[test]
fn llvm_host_literalia_ascii_matches_raw_expected_bytes() {
    assert_llvm_text_output_matches("literalia/ascii.fab", "literalia-ascii");
}

#[test]
fn llvm_host_literalia_block_string_matches_raw_expected_bytes() {
    assert_llvm_text_output_matches("literalia/block-string.fab", "literalia-block-string");
}

#[test]
fn llvm_host_literalia_forma_matches_raw_expected_bytes() {
    assert_llvm_text_output_matches("literalia/forma.fab", "literalia-forma");
}

#[test]
fn llvm_host_literalia_textus_matches_raw_expected_bytes() {
    assert_llvm_text_output_matches("literalia/textus.fab", "literalia-textus");
}

#[test]
fn llvm_host_scriptum_matches_raw_expected_bytes() {
    assert_llvm_text_output_matches("scriptum/scriptum.fab", "scriptum");
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
fn llvm_host_conversio_conversio_matches_raw_expected_bytes() {
    assert_llvm_text_output_matches("conversio/conversio.fab", "conversio-conversio");
}

#[test]
fn llvm_host_numeric_bool_conversio_matches_raw_expected_bytes() {
    assert_llvm_text_output_matches("conversio/numeric-bool.fab", "conversio-numeric-bool");
}

#[test]
fn llvm_host_octeti_conversio_matches_raw_expected_bytes() {
    assert_llvm_text_output_matches("conversio/octeti.fab", "conversio-octeti");
}

#[test]
fn llvm_host_cape_matches_raw_expected_bytes() {
    assert_llvm_text_output_matches("cape/cape.fab", "cape");
}

#[test]
fn llvm_host_functio_fallibilis_matches_raw_expected_bytes() {
    assert_llvm_text_output_matches("iace/functio-fallibilis.fab", "iace-functio-fallibilis");
}

#[test]
fn llvm_host_iace_matches_raw_expected_bytes() {
    assert_llvm_text_output_matches("iace/iace.fab", "iace");
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
fn llvm_host_instans_conversio_matches_raw_expected_bytes() {
    assert_llvm_text_output_matches("conversio/instans.fab", "conversio-instans");
}

#[test]
fn llvm_host_instans_valor_carrier_matches_raw_expected_bytes() {
    assert_llvm_text_output_matches(
        "conversio/instans-valor-carrier.fab",
        "conversio-instans-valor-carrier",
    );
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
