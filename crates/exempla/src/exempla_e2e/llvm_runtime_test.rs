use super::*;
use radix::codegen::Target;
use radix::{Compiler, Config, Output};
use std::fs;

/// L9 (cee2f7b7): exit-code parity — a CLI `incipit` with `exitus 1` must make
/// the LLVM host process exit with code 1, matching the Rust oracle (the
/// program entry packs the declared exit code into the single-register exit
/// struct; the runtime main returns it as the process exit code).
#[test]
#[ignore = "slow LLVM host link+run; run: cargo test -p exempla --test e2e_harness llvm_host_exitus_declared_exit_code -- --ignored --nocapture"]
fn llvm_host_exitus_declared_exit_code_matches_rust_oracle() {
    let fab_path = crate::paths::corpus_dir().join("exitus/exitus.fab");
    let source = fs::read_to_string(&fab_path).expect("read exitus.fab");
    let session = radix::driver::Session::new(
        Config::default()
            .with_target(Target::LlvmText)
            .with_dev_stdlib(),
    );
    let mut analysis = radix::driver::analyze_source(&session, "exitus.fab", &source)
        .expect("exitus.fab frontend analysis must succeed");
    let device_roles = radix::mir::device_roles_from_hir(&analysis.hir);
    let exit_code = analysis
        .cli_program
        .as_ref()
        .and_then(|program| program.exit.as_ref())
        .and_then(|exit| match exit {
            radix::cli::CliExit::Fixed(code) => Some(*code),
            _ => None,
        });
    assert_eq!(exit_code, Some(1), "exitus fixture must declare exit code 1");
    let lowered = radix::mir::lower_analyzed_unit_allowing_cli_entry_with_context(&mut analysis)
        .expect("exitus.fab CLI MIR lowering must succeed");
    let llvm = radix::mir::emit_llvm_text_probe_with_device_roles_and_exit(
        &device_roles,
        &lowered.validated,
        &lowered.interner,
        exit_code,
    )
    .expect("exitus.fab LLVM emission must succeed");
    assert!(
        llvm.contains("ret %FaberRtExitV1 %faber.entry.packed"),
        "program entry must return the packed exit struct:\n{llvm}"
    );
    let temp_root = super::super::common::make_temp_root();
    let llvm_file = temp_root.join("exitus.ll");
    fs::write(&llvm_file, &llvm).expect("write exitus LLVM text");

    let probe = run_llvm_exemplum(&llvm_file, &temp_root, "exitus", &fab_path);
    assert_eq!(
        probe.exit_code,
        Some(1),
        "exitus 1 must produce process exit 1: {}",
        probe.reason
    );
    assert!(probe.stdout.is_empty(), "unexpected stdout: {:?}", probe.stdout);
    assert!(probe.stderr.is_empty(), "unexpected stderr: {:?}", probe.stderr);
}

/// L9 (cee2f7b7): nota parity for optional-chain values — `nota` of a
/// present `T ∪ nihil` optional-chain result renders the payload (`100`) and
/// the absent chain renders `nihil`, matching the sibling `.expected`
/// (previously both passed a null opaque handle and dropped the two lines).
#[test]
#[ignore = "slow LLVM host link+run; run: cargo test -p exempla --test e2e_harness llvm_host_membrum_optional_chain_nota -- --ignored --nocapture"]
fn llvm_host_membrum_optional_chain_nota_matches_expected() {
    let fab_path = crate::paths::corpus_dir().join("membrum/membrum.fab");
    let result = Compiler::new(
        Config::default()
            .with_target(Target::LlvmText)
            .with_dev_stdlib(),
    )
    .compile(&fab_path);
    assert!(
        result.success(),
        "membrum.fab LLVM compile failed: {:?}",
        result.diagnostics
    );
    let Some(Output::LlvmText(output)) = result.output else {
        panic!("membrum.fab did not produce LLVM text");
    };
    assert!(
        output.code.contains("__faber_rt_v1_diagnostic_nota_option"),
        "membrum optional-chain nota must use the option carrier:\n{}",
        output.code
    );
    let temp_root = super::super::common::make_temp_root();
    let llvm_file = temp_root.join("membrum.ll");
    fs::write(&llvm_file, output.code).expect("write membrum LLVM text");

    let probe = run_llvm_exemplum(&llvm_file, &temp_root, "membrum", &fab_path);
    assert_eq!(
        probe.bucket,
        LlvmRunBucket::OutputMatched,
        "{}",
        probe.reason
    );
    let expected =
        fs::read(fab_path.with_extension("expected")).expect("read membrum expected bytes");
    assert_eq!(
        probe.stdout.as_bytes(),
        expected,
        "optional-chain nota must render the payload and nihil lines"
    );
    assert_eq!(probe.exit_code, Some(0));
}

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

/// L7 (cf3cff8f): scalar display/format parity — `octet`/`numerus<u8>` values
/// display their unsigned magnitude like the Rust oracle (`222`, `128`), not
/// the signed i8 rendering the LLVM host used to emit (`-34`, `-128`).
#[test]
#[ignore = "slow LLVM host link+run; run: cargo test -p exempla --test e2e_harness llvm_host_octet_unsigned_display -- --ignored --nocapture"]
fn llvm_host_octet_unsigned_display_matches_rust_output() {
    let fab_path = crate::paths::corpus_dir().join("octet/octet.fab");
    let result = Compiler::new(
        Config::default()
            .with_target(Target::LlvmText)
            .with_dev_stdlib(),
    )
    .compile(&fab_path);
    assert!(
        result.success(),
        "octet/octet.fab LLVM compile failed: {:?}",
        result.diagnostics
    );
    let Some(Output::LlvmText(output)) = result.output else {
        panic!("octet/octet.fab did not produce LLVM text");
    };
    let temp_root = super::super::common::make_temp_root();
    let llvm_file = temp_root.join("octet.ll");
    fs::write(&llvm_file, output.code).expect("write octet LLVM text");

    let probe = run_llvm_exemplum(&llvm_file, &temp_root, "octet", &fab_path);
    assert_eq!(
        probe.stdout,
        "5\n222\n128\n",
        "octet bytes must display unsigned: {}",
        probe.reason
    );
    assert!(
        probe.stderr.is_empty(),
        "unexpected stderr: {:?}",
        probe.stderr
    );
    assert_eq!(probe.exit_code, Some(0));
}

/// L7 (cf3cff8f): scalar display/format parity — `modulus<u32>` values display
/// their unsigned magnitude like the Rust oracle (`4197074466`), not the signed
/// i32 rendering (`-97892830`).
#[test]
#[ignore = "slow LLVM host link+run; run: cargo test -p exempla --test e2e_harness llvm_host_modular_word_unsigned_display -- --ignored --nocapture"]
fn llvm_host_modular_word_unsigned_display_matches_rust_output() {
    let fab_path = crate::paths::corpus_dir().join("operatores/modular-word-sha-round.fab");
    let result = Compiler::new(
        Config::default()
            .with_target(Target::LlvmText)
            .with_dev_stdlib(),
    )
    .compile(&fab_path);
    assert!(
        result.success(),
        "operatores/modular-word-sha-round.fab LLVM compile failed: {:?}",
        result.diagnostics
    );
    let Some(Output::LlvmText(output)) = result.output else {
        panic!("modular-word-sha-round did not produce LLVM text");
    };
    let temp_root = super::super::common::make_temp_root();
    let llvm_file = temp_root.join("modular-word-sha-round.ll");
    fs::write(&llvm_file, output.code).expect("write modular-word-sha-round LLVM text");

    let probe = run_llvm_exemplum(&llvm_file, &temp_root, "modular-word-sha-round", &fab_path);
    assert_eq!(
        probe.stdout,
        "1567288269\n4197074466\n",
        "modulus<u32> values must display unsigned: {}",
        probe.reason
    );
    assert!(
        probe.stderr.is_empty(),
        "unexpected stderr: {:?}",
        probe.stderr
    );
    assert_eq!(probe.exit_code, Some(0));
}

/// L7 (cf3cff8f): scalar display/format parity — `§`-template f64 formatting
/// keeps the integral `.0` decimal marker (`valor: 5.0`) like the Rust oracle,
/// not `value.to_string()`'s `valor: 5`.
#[test]
#[ignore = "slow LLVM host link+run; run: cargo test -p exempla --test e2e_harness llvm_host_f64_format_matches_rust_output -- --ignored --nocapture"]
fn llvm_host_f64_format_matches_rust_output() {
    let fab_path = crate::paths::corpus_dir().join("mori/mori.fab");
    let result = Compiler::new(
        Config::default()
            .with_target(Target::LlvmText)
            .with_dev_stdlib(),
    )
    .compile(&fab_path);
    assert!(
        result.success(),
        "mori/mori.fab LLVM compile failed: {:?}",
        result.diagnostics
    );
    let Some(Output::LlvmText(output)) = result.output else {
        panic!("mori/mori.fab did not produce LLVM text");
    };
    let temp_root = super::super::common::make_temp_root();
    let llvm_file = temp_root.join("mori.ll");
    fs::write(&llvm_file, output.code).expect("write mori LLVM text");

    let probe = run_llvm_exemplum(&llvm_file, &temp_root, "mori", &fab_path);
    assert_eq!(
        probe.stdout,
        "valor: 5.0\nvalor: 2\n",
        "integral f64 must keep the .0 marker: {}",
        probe.reason
    );
    assert!(
        probe.stderr.is_empty(),
        "unexpected stderr: {:?}",
        probe.stderr
    );
    assert_eq!(probe.exit_code, Some(0));
}

/// L8 (acbd2a3d): tensor outcome family — compile a corpus fixture through the
/// dev-stdlib LLVM host path, run the linked binary, and assert the stdout is
/// byte-exact against the sibling `.expected` sidecar (the Rust oracle).
fn assert_tensor_fixture_output(fab_relative: &str, stem: &str) {
    let fab_path = crate::paths::corpus_dir().join(fab_relative);
    let result = Compiler::new(
        Config::default()
            .with_target(Target::LlvmText)
            .with_dev_stdlib(),
    )
    .compile(&fab_path);
    assert!(
        result.success(),
        "{fab_relative} LLVM compile failed: {:?}",
        result.diagnostics
    );
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
    assert!(
        probe.stderr.is_empty(),
        "{fab_relative}: unexpected stderr: {:?}",
        probe.stderr
    );
    assert_eq!(probe.exit_code, Some(0), "{fab_relative}");
}

/// L8 (acbd2a3d): tensor outcome family — bracket access (`accipe`/`ponde`
/// with a `u32` index vector) previously read zeros because the LLVM host
/// rejected non-i64 index arrays. The runtime now widens any i64-fit integer
/// index vector, and the fixture matches the Rust oracle (`3.0 3.0 4.0 4.0 4.0`).
#[test]
#[ignore = "slow LLVM host link+run; run: cargo test -p exempla --test e2e_harness llvm_host_tensor_bracket_access -- --ignored --nocapture"]
fn llvm_host_tensor_bracket_access_matches_rust_output() {
    assert_tensor_fixture_output("tensor/bracket-access.fab", "tensor-bracket-access");
}

/// L8 (acbd2a3d): tensor outcome family — `u32` thread-id index vectors and
/// `lista<u32>` origins route through `accipe`/`ponde` and match the oracle
/// (`0.0 9.0 9.0 7.0`).
#[test]
#[ignore = "slow LLVM host link+run; run: cargo test -p exempla --test e2e_harness llvm_host_tensor_index_width -- --ignored --nocapture"]
fn llvm_host_tensor_index_width_matches_rust_output() {
    assert_tensor_fixture_output("tensor/index-width.fab", "tensor-index-width");
}

/// L8 (acbd2a3d): tensor outcome family — element-width `u8`/`u16` tensors
/// convert through the versioned `tensor_convert` bridge and rank correctly
/// (`4 1 0 0 0 0 0`). Previously the host rejected the u8/u16 carrier kinds and
/// the subsequent rank read uninitialized stack memory (nondeterministic).
#[test]
#[ignore = "slow LLVM host link+run; run: cargo test -p exempla --test e2e_harness llvm_host_conversio_tensor_convert -- --ignored --nocapture"]
fn llvm_host_conversio_tensor_convert_matches_rust_output() {
    assert_tensor_fixture_output("conversio/tensor.fab", "conversio-tensor");
}

/// L8 (acbd2a3d): tensor outcome family — `tensor<textus, [N]>` universal
/// container: `strue`/`accipe` route PTR-kind elements through the tensor ABI
/// and the read text matches the oracle (`alpha`).
#[test]
#[ignore = "slow LLVM host link+run; run: cargo test -p exempla --test e2e_harness llvm_host_tensor_textus -- --ignored --nocapture"]
fn llvm_host_tensor_textus_matches_rust_output() {
    assert_tensor_fixture_output("tensor/textus.fab", "tensor-textus");
}

/// L10 (fa1a5d8c): opaque-display family — compile a corpus fixture through the
/// dev-stdlib LLVM host path, run the linked binary, and assert the stdout is
/// byte-exact against the sibling `.expected` sidecar (the Rust oracle).
fn assert_opaque_fixture_output(fab_relative: &str, stem: &str) {
    let fab_path = crate::paths::corpus_dir().join(fab_relative);
    let result = Compiler::new(
        Config::default()
            .with_target(Target::LlvmText)
            .with_dev_stdlib(),
    )
    .compile(&fab_path);
    assert!(
        result.success(),
        "{fab_relative} LLVM compile failed: {:?}",
        result.diagnostics
    );
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
    assert!(
        probe.stderr.is_empty(),
        "{fab_relative}: unexpected stderr: {:?}",
        probe.stderr
    );
    assert_eq!(probe.exit_code, Some(0), "{fab_relative}");
}

/// L10 (fa1a5d8c): opaque-display family — compile a corpus fixture with no
/// `.expected` sidecar through the dev-stdlib LLVM host path and assert the
/// exact Rust-oracle stdout.
fn assert_opaque_fixture_stdout(fab_relative: &str, stem: &str, expected: &str) {
    let fab_path = crate::paths::corpus_dir().join(fab_relative);
    let result = Compiler::new(
        Config::default()
            .with_target(Target::LlvmText)
            .with_dev_stdlib(),
    )
    .compile(&fab_path);
    assert!(
        result.success(),
        "{fab_relative} LLVM compile failed: {:?}",
        result.diagnostics
    );
    let Some(Output::LlvmText(output)) = result.output else {
        panic!("{fab_relative} did not produce LLVM text");
    };
    let temp_root = super::super::common::make_temp_root();
    let llvm_file = temp_root.join(format!("{stem}.ll"));
    fs::write(&llvm_file, output.code).expect("write {fab_relative} LLVM text");
    let probe = run_llvm_exemplum(&llvm_file, &temp_root, stem, &fab_path);
    assert_eq!(
        probe.bucket,
        LlvmRunBucket::Runnable,
        "{fab_relative}: {}",
        probe.reason
    );
    assert_eq!(probe.stdout, expected, "{fab_relative}");
    assert!(
        probe.stderr.is_empty(),
        "{fab_relative}: unexpected stderr: {:?}",
        probe.stderr
    );
    assert_eq!(probe.exit_code, Some(0), "{fab_relative}");
}

/// L10 (fa1a5d8c): opaque-display family — `nota dims` on a `lista<numerus>`
/// shape vector previously passed a null handle and dropped the line. The
/// numeric-lista carrier now renders `[2, 3]` like the Rust oracle.
#[test]
#[ignore = "slow LLVM host link+run; run: cargo test -p exempla --test e2e_harness llvm_host_tensor_shape_dims -- --ignored --nocapture"]
fn llvm_host_tensor_shape_dims_matches_rust_output() {
    assert_opaque_fixture_output("tensor/shape.fab", "tensor-shape");
}

/// L10 (fa1a5d8c): opaque-display family — `nota out` on a `lista<f32>` result
/// renders `[1.0, 4.0, 9.0, 16.0]` (f32 elements keep the `.0` marker).
#[test]
#[ignore = "slow LLVM host link+run; run: cargo test -p exempla --test e2e_harness llvm_host_tensor_arithmetic_elementwise -- --ignored --nocapture"]
fn llvm_host_tensor_arithmetic_elementwise_matches_rust_output() {
    assert_opaque_fixture_output("tensor/arithmetic-elementwise.fab", "tensor-arithmetic-elementwise");
}

/// L10 (fa1a5d8c): opaque-display family — `nota dims` renders `[2, 2]` after
/// the literal-shaped lista-to-tensor conversion.
#[test]
#[ignore = "slow LLVM host link+run; run: cargo test -p exempla --test e2e_harness llvm_host_lista_tensor_shaped -- --ignored --nocapture"]
fn llvm_host_lista_tensor_shaped_matches_rust_output() {
    assert_opaque_fixture_output("conversio/lista-tensor-shaped.fab", "conversio-lista-tensor-shaped");
}

/// L10 (fa1a5d8c): opaque-display family — `nota dims` renders `[3, 3]` after
/// the rectangular lista-literal tensor conversion.
#[test]
#[ignore = "slow LLVM host link+run; run: cargo test -p exempla --test e2e_harness llvm_host_rectangular_lista_tensor -- --ignored --nocapture"]
fn llvm_host_rectangular_lista_tensor_matches_rust_output() {
    assert_opaque_fixture_output(
        "conversio/rectangular-lista-literal-tensor.fab",
        "conversio-rectangular-lista-literal-tensor",
    );
}

/// L10 (fa1a5d8c): opaque-display family — `lista<numerus>` method results
/// (`addita`, `sectio`, `prima`, `ultima`, `omissa`, `inversa`, `ordinata`)
/// render `[1, 2, 3, 4, 5, 6]` … like the Rust oracle.
#[test]
#[ignore = "slow LLVM host link+run; run: cargo test -p exempla --test e2e_harness llvm_host_lista_methodi_copiae -- --ignored --nocapture"]
fn llvm_host_lista_methodi_copiae_matches_rust_output() {
    assert_opaque_fixture_output("lista/methodi-copiae.fab", "lista-methodi-copiae");
}

/// L10 (fa1a5d8c): opaque-display family — filtered/mapped `lista<numerus>`
/// results render `[2, 4]` / `[2, 4, 6, 8, 10]`.
#[test]
#[ignore = "slow LLVM host link+run; run: cargo test -p exempla --test e2e_harness llvm_host_lista_methodi_functionales -- --ignored --nocapture"]
fn llvm_host_lista_methodi_functionales_matches_rust_output() {
    assert_opaque_fixture_output("lista/methodi-functionales.fab", "lista-methodi-functionales");
}

/// L10 (fa1a5d8c): opaque-display family — mutated `lista<numerus>` renders
/// `[1, 2]` (elements removed in place).
#[test]
#[ignore = "slow LLVM host link+run; run: cargo test -p exempla --test e2e_harness llvm_host_lista_methodi_mutatio -- --ignored --nocapture"]
fn llvm_host_lista_methodi_mutatio_matches_rust_output() {
    assert_opaque_fixture_output("lista/methodi-mutatio.fab", "lista-methodi-mutatio");
}

/// L10 (fa1a5d8c): opaque-display family — `typus` aliases resolve before the
/// displayable-opaque check, so `nota sodales` (aliased `lista<textus>`) and
/// `nota puncta` (aliased `lista<numerus>`) render like the Rust oracle.
#[test]
#[ignore = "slow LLVM host link+run; run: cargo test -p exempla --test e2e_harness llvm_host_typus_aliased_lista -- --ignored --nocapture"]
fn llvm_host_typus_aliased_lista_matches_rust_output() {
    assert_opaque_fixture_stdout(
        "typus/typus.fab",
        "typus-typus",
        "42\nMarcus\nverum\n[\"Gaius\", \"Lucius\", \"Titus\"]\n[100, 95, 87]\n",
    );
}

/// L10 (fa1a5d8c): opaque-display family — `valor` nota renders via the
/// oracle's `display_valor` (`42`, `[222, 173]`, `[1, 2]`, `{"alpha": 10}`,
/// `{"x": 3, "y": 4}`, `[5, 6]`).
#[test]
#[ignore = "slow LLVM host link+run; run: cargo test -p exempla --test e2e_harness llvm_host_valor_boxing -- --ignored --nocapture"]
fn llvm_host_valor_boxing_matches_rust_output() {
    assert_opaque_fixture_output("conversio/valor-boxing.fab", "conversio-valor-boxing");
}

/// L10 (fa1a5d8c): opaque-display family — `nota roundtrip` renders the boxed
/// `lista<numerus>` as `[1, 2, 3, 4]`.
#[test]
#[ignore = "slow LLVM host link+run; run: cargo test -p exempla --test e2e_harness llvm_host_valor_tensor -- --ignored --nocapture"]
fn llvm_host_valor_tensor_matches_rust_output() {
    assert_opaque_fixture_output("conversio/valor-tensor.fab", "conversio-valor-tensor");
}

/// L10 (fa1a5d8c): opaque-display family — JSON-literal `tabula` nota renders
/// in the Rust oracle's derived `Json(Tabula({...}))` Debug shape.
#[test]
#[ignore = "slow LLVM host link+run; run: cargo test -p exempla --test e2e_harness llvm_host_destructura_literal_json -- --ignored --nocapture"]
fn llvm_host_destructura_literal_json_matches_rust_output() {
    assert_opaque_fixture_stdout(
        "destructura/literal.fab",
        "destructura-literal",
        "Json(Tabula({}))\nJson(Tabula({\"x\": Numerus(10), \"y\": Numerus(20)}))\nJson(Tabula({\"clavis\": Numerus(42)}))\nJson(Tabula({\"extra\": Tabula({\"medium\": Numerus(1)})}))\n",
    );
}

/// L11 (fc9be27a): union/option null-check family — compile a corpus fixture
/// through the dev-stdlib LLVM host path, run the linked binary, and assert the
/// stdout is byte-exact against the sibling `.expected` sidecar (the Rust
/// oracle). These are the L7 family-3 rows (`unarius`, `operatores/logica`,
/// `literalia/nihil`, `vel/vel`): `est nihil` / `non est nihil` presence tests
/// and `vel` nullish coalescing on literal-built `T ∪ nihil` values.
fn assert_option_fixture_output(fab_relative: &str, stem: &str) {
    let fab_path = crate::paths::corpus_dir().join(fab_relative);
    let result = Compiler::new(
        Config::default()
            .with_target(Target::LlvmText)
            .with_dev_stdlib(),
    )
    .compile(&fab_path);
    assert!(
        result.success(),
        "{fab_relative} LLVM compile failed: {:?}",
        result.diagnostics
    );
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
    assert!(
        probe.stderr.is_empty(),
        "{fab_relative}: unexpected stderr: {:?}",
        probe.stderr
    );
    assert_eq!(probe.exit_code, Some(0), "{fab_relative}");
}

/// L11 (fc9be27a): family 3 — `unarius` null checks. `est nihil` / `non est
/// nihil` on a literal-built `textus ∪ nihil` binding previously failed because
/// `option_is_present` only accepted raw pointer carriers for non-arena options.
#[test]
#[ignore = "slow LLVM host link+run; run: cargo test -p exempla --test e2e_harness llvm_host_unarius_null_checks -- --ignored --nocapture"]
fn llvm_host_unarius_null_checks_matches_expected() {
    assert_option_fixture_output("unarius/unarius.fab", "unarius-unarius");
}

/// L11 (fc9be27a): family 3 — `operatores/logica` negated-est rows. `non est
/// nihil` on present/absent literal `textus ∪ nihil` values renders verum/falsum.
#[test]
#[ignore = "slow LLVM host link+run; run: cargo test -p exempla --test e2e_harness llvm_host_logica_non_est_nihil -- --ignored --nocapture"]
fn llvm_host_logica_non_est_nihil_matches_expected() {
    assert_option_fixture_output("operatores/logica.fab", "operatores-logica");
}

/// L11 (fc9be27a): family 3 — `literalia/nihil`. `est nihil` / `non est nihil`
/// on a literal `numerus ∪ nihil` binding and a nullable function result
/// previously failed because the raw option carrier rejected the scalar kind.
#[test]
#[ignore = "slow LLVM host link+run; run: cargo test -p exempla --test e2e_harness llvm_host_literalia_nihil -- --ignored --nocapture"]
fn llvm_host_literalia_nihil_matches_expected() {
    assert_option_fixture_output("literalia/nihil.fab", "literalia-nihil");
}

/// L11 (fc9be27a): family 3 — `vel/vel`. Nullish coalescing on literal-built
/// `textus ∪ nihil` bindings (including chains) previously failed because
/// `option_get_or` rejected the raw pointer carrier for non-arena options.
#[test]
#[ignore = "slow LLVM host link+run; run: cargo test -p exempla --test e2e_harness llvm_host_vel_coalescing -- --ignored --nocapture"]
fn llvm_host_vel_coalescing_matches_expected() {
    assert_option_fixture_output("vel/vel.fab", "vel-vel");
}

/// L11 (fc9be27a): family 3 — `si/ergo-redde`. `nota` of a nullable function
/// result (`numerus ∪ nihil`) renders the payload or nihil; presence tests on
/// the returned raw option decode the scalar bits per value-kind.
#[test]
#[ignore = "slow LLVM host link+run; run: cargo test -p exempla --test e2e_harness llvm_host_ergo_redde_optional -- --ignored --nocapture"]
fn llvm_host_ergo_redde_optional_matches_expected() {
    assert_option_fixture_output("si/ergo-redde.fab", "si-ergo-redde");
}

/// L12 (f76f674f): genus field-mutation family — compile a corpus fixture
/// through the dev-stdlib LLVM host path, run the linked binary, and assert
/// the stdout is byte-exact against the sibling `.expected` sidecar (the Rust
/// oracle).
fn assert_genus_fixture_output(fab_relative: &str, stem: &str) {
    let fab_path = crate::paths::corpus_dir().join(fab_relative);
    let result = Compiler::new(
        Config::default()
            .with_target(Target::LlvmText)
            .with_dev_stdlib(),
    )
    .compile(&fab_path);
    assert!(
        result.success(),
        "{fab_relative} LLVM compile failed: {:?}",
        result.diagnostics
    );
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
    assert!(
        probe.stderr.is_empty(),
        "{fab_relative}: unexpected stderr: {:?}",
        probe.stderr
    );
    assert_eq!(probe.exit_code, Some(0), "{fab_relative}");
}

/// L12 (f76f674f): genus field-mutation family — `genus/methodi`. Mutating
/// genus methods (`ego.numerus ← …`) previously lost the write: the receiver
/// was passed by value and never written back, so the `Numerator` counter
/// stayed at 0 instead of 0, 1, 3. The mutating receiver now rides a by-ref
/// `ptr` and the caller observes the mutations (Rust-oracle `&mut self`
/// parity).
#[test]
#[ignore = "slow LLVM host link+run; run: cargo test -p exempla --test e2e_harness llvm_host_genus_methodi_mutation -- --ignored --nocapture"]
fn llvm_host_genus_methodi_mutation_matches_expected() {
    assert_genus_fixture_output("genus/methodi.fab", "genus-methodi");
}

/// L12 (f76f674f): genus field-mutation family — `genus/creo`. The
/// post-construction `creo` hook (validation/clamping/derived init) was never
/// called by the LLVM emitter (MIR carries no creo call — only the Rust
/// backend inserts it), so `Terminus { valor = 200 }` stayed 200 and
/// `Circulus`'s derived `diameter`/`area` stayed 0. The emitter now
/// re-inserts the `creo` call after physical genus construction, matching the
/// Rust oracle (50, clamped 100, radius 5, diameter 10, area 78.53975).
#[test]
#[ignore = "slow LLVM host link+run; run: cargo test -p exempla --test e2e_harness llvm_host_genus_creo_hook -- --ignored --nocapture"]
fn llvm_host_genus_creo_hook_matches_rust_output() {
    let fab_path = crate::paths::corpus_dir().join("genus/creo.fab");
    let result = Compiler::new(
        Config::default()
            .with_target(Target::LlvmText)
            .with_dev_stdlib(),
    )
    .compile(&fab_path);
    assert!(
        result.success(),
        "genus/creo.fab LLVM compile failed: {:?}",
        result.diagnostics
    );
    let Some(Output::LlvmText(output)) = result.output else {
        panic!("genus/creo.fab did not produce LLVM text");
    };
    assert!(
        output.code.contains("call void @creo"),
        "creo hook must be invoked at construction:\n{}",
        output.code
    );
    let temp_root = super::super::common::make_temp_root();
    let llvm_file = temp_root.join("genus-creo.ll");
    fs::write(&llvm_file, output.code).expect("write creo LLVM text");
    let probe = run_llvm_exemplum(&llvm_file, &temp_root, "genus-creo", &fab_path);
    assert_eq!(
        probe.stdout,
        "50\n100\n5\n10\n78.53975\n",
        "{}",
        probe.reason
    );
    assert!(
        probe.stderr.is_empty(),
        "unexpected stderr: {:?}",
        probe.stderr
    );
    assert_eq!(probe.exit_code, Some(0));
}

/// L12 (f76f674f): genus field-mutation family — `vocatio`. Method receivers
/// passed by value made `pone`/`duplica` no-ops, so the chained builder
/// `alter.pone(5).duplica().duplica().accipe()` printed 0 instead of 20 and
/// `pone`'s `redde ego` returned the pre-mutation copy. The by-ref mutating
/// receiver fixes the chain (10, 20) and the `redde ego` value.
#[test]
#[ignore = "slow LLVM host link+run; run: cargo test -p exempla --test e2e_harness llvm_host_vocatio_method_mutation -- --ignored --nocapture"]
fn llvm_host_vocatio_method_mutation_matches_expected() {
    assert_genus_fixture_output("vocatio/vocatio.fab", "vocatio-vocatio");
}
/// D-PA4 (hand-3 d5596b1c): the importa two-module LLVM-host link proof.
///
/// `importa/importa.fab` imports a sibling user-code module and calls
/// `saluta` through the canonical external symbol
/// `__faber_external_product_importa_module_auxilium_func_saluta`; the sibling
/// module (`importa/auxilium.fab`, library mode — no entry) defines the same
/// symbol. Both modules link with the host runtime archive into one
/// executable whose stdout is `Salve, Marcus!` (exit 0), matching
/// `importa.expected` and the Rust oracle.
#[test]
#[ignore = "slow LLVM host link+run; run: cargo test -p exempla --test e2e_harness llvm_host_importa_two_module_link -- --ignored --nocapture"]
fn llvm_host_importa_two_module_link_matches_rust_oracle() {
    const EXTERNAL_SALUTA: &str = "__faber_external_product_importa_module_auxilium_func_saluta";
    let fab_path = crate::paths::corpus_dir().join("importa/importa.fab");
    let sibling = crate::paths::corpus_dir().join("importa/auxilium.fab");

    let result = radix::tool::compile_cli_path(&fab_path, false, Target::LlvmText);
    assert!(
        result.success(),
        "importa.fab LLVM compile failed: {:?}",
        result.diagnostics
    );
    let Some(Output::LlvmText(entry)) = result.output else {
        panic!("importa.fab did not produce LLVM text");
    };
    assert!(
        entry.code.contains(EXTERNAL_SALUTA),
        "entry module must declare/call the sibling external symbol:\n{}",
        entry.code
    );

    let sibling_source = fs::read_to_string(&sibling).expect("read auxilium.fab");
    let session = radix::driver::Session::new(
        Config::default()
            .with_target(Target::LlvmText)
            .with_dev_stdlib(),
    );
    let mut analysis =
        radix::driver::analyze_source(&session, &sibling.display().to_string(), &sibling_source)
            .expect("auxilium.fab frontend analysis must succeed");
    if let Some(identities) = radix::tool::package_identity_facts_for_path(&sibling) {
        analysis.package_import_identities = Some(identities);
    }
    let lowered = radix::mir::lower_analyzed_unit_with_context(&mut analysis)
        .expect("auxilium.fab MIR lowering must succeed");
    let sibling_llvm = radix::mir::emit_llvm_text_probe_library_module(
        &lowered.validated,
        &lowered.interner,
    )
    .expect("auxilium.fab library-mode LLVM emission must succeed");
    assert!(
        sibling_llvm.contains(EXTERNAL_SALUTA),
        "sibling module must define the external symbol:\n{sibling_llvm}"
    );
    assert!(
        !sibling_llvm.contains("__faber_program_entry_v1"),
        "library module must not emit a program entry:\n{sibling_llvm}"
    );

    let temp_root = super::super::common::make_temp_root();
    let entry_file = temp_root.join("importa.entry.ll");
    let sibling_file = temp_root.join("importa.sibling.ll");
    fs::write(&entry_file, entry.code).expect("write entry LLVM text");
    fs::write(&sibling_file, sibling_llvm).expect("write sibling LLVM text");

    let probe = run_llvm_module_pair(
        &entry_file,
        &sibling_file,
        &temp_root,
        "importa-two-module",
        &fab_path,
    );
    assert_eq!(probe.bucket, LlvmRunBucket::OutputMatched, "{}", probe.reason);
    assert_eq!(probe.stdout, "Salve, Marcus!\n");
    assert!(
        probe.stderr.is_empty(),
        "unexpected stderr: {:?}",
        probe.stderr
    );
    assert_eq!(probe.exit_code, Some(0));
}

/// S8.1 (d3de92fa): declaration-only entry completeness — a corpus fixture
/// with no `incipit` and no module-scope statements (`proba/proba.fab`) must
/// still produce a binary with a successful entry: the emitted module carries
/// the REAL `__faber_program_entry_v1` (no missing-incipit workaround), the
/// link succeeds, and the process exits 0 with no output, matching the Rust
/// oracle's `DeclarationOnly` outcome.
#[test]
#[ignore = "slow LLVM host link+run; run: cargo test -p exempla --test e2e_harness llvm_host_declaration_only_entry -- --ignored --nocapture"]
fn llvm_host_declaration_only_entry_matches_rust_oracle() {
    let fab_path = crate::paths::corpus_dir().join("proba/proba.fab");
    let result = Compiler::new(
        Config::default()
            .with_target(Target::LlvmText)
            .with_dev_stdlib(),
    )
    .compile(&fab_path);
    assert!(
        result.success(),
        "proba/proba.fab LLVM compile failed: {:?}",
        result.diagnostics
    );
    let Some(Output::LlvmText(output)) = result.output else {
        panic!("proba/proba.fab did not produce LLVM text");
    };
    assert!(
        output.code.contains("define %FaberRtExitV1 @__faber_program_entry_v1"),
        "declaration-only must still emit the real program entry:\n{}",
        output.code
    );
    let temp_root = super::super::common::make_temp_root();
    let llvm_file = temp_root.join("declaration-only.ll");
    fs::write(&llvm_file, output.code).expect("write declaration-only LLVM text");
    let probe = run_llvm_exemplum(&llvm_file, &temp_root, "declaration-only", &fab_path);
    assert_eq!(probe.exit_code, Some(0), "{}", probe.reason);
    assert!(
        probe.stdout.is_empty(),
        "unexpected stdout: {:?}",
        probe.stdout
    );
    assert!(
        probe.stderr.is_empty(),
        "unexpected stderr: {:?}",
        probe.stderr
    );
}

/// S8.1 (d3de92fa): module-scope statements — executable top-level statements
/// without an explicit `incipit` become an implicit entry that executes
/// exactly once in source order. The three `nota` statements must render in
/// deterministic order (`prima`, `secunda`, `tertia`), exit 0, matching the
/// Rust oracle's implicit-entry behavior.
#[test]
#[ignore = "slow LLVM host link+run; run: cargo test -p exempla --test e2e_harness llvm_host_module_scope_statements -- --ignored --nocapture"]
fn llvm_host_module_scope_statements_execute_once_in_order() {
    let source = "nota \"prima\"\nnota \"secunda\"\nnota \"tertia\"\n";
    let session = radix::driver::Session::new(
        Config::default()
            .with_target(Target::LlvmText)
            .with_dev_stdlib(),
    );
    let mut analysis = radix::driver::analyze_source(&session, "module-scope.fab", source)
        .expect("module-scope source frontend analysis must succeed");
    let device_roles = radix::mir::device_roles_from_hir(&analysis.hir);
    let lowered = radix::mir::lower_analyzed_unit_with_context(&mut analysis)
        .expect("module-scope source MIR lowering must succeed");
    let llvm = radix::mir::emit_llvm_text_probe_with_device_roles_and_exit(
        &device_roles,
        &lowered.validated,
        &lowered.interner,
        None,
    )
    .expect("module-scope source LLVM emission must succeed");
    let temp_root = super::super::common::make_temp_root();
    let llvm_file = temp_root.join("module-scope.ll");
    fs::write(&llvm_file, &llvm).expect("write module-scope LLVM text");
    let fab_path = Path::new("module-scope.fab");
    let probe = run_llvm_exemplum(&llvm_file, &temp_root, "module-scope", fab_path);
    assert_eq!(
        probe.stdout,
        "prima\nsecunda\ntertia\n",
        "module-scope statements must execute once in source order: {}",
        probe.reason
    );
    assert!(
        probe.stderr.is_empty(),
        "unexpected stderr: {:?}",
        probe.stderr
    );
    assert_eq!(probe.exit_code, Some(0));
}

/// S8.1 (d3de92fa): process argumenta — an `incipit argumenta args` binding
/// without `@ cli` lowers to an `array<textus>` local the emitted entry seeds
/// from `__faber_rt_v1_arguments`. The runtime captures argv excluding the
/// host argv[0] program path (Faber semantics; the product `processus`
/// provider and the Rust CLI parser both use `args().skip(1)`), and the
/// harness passes the exact Rust oracle args to the LLVM binary, so `args[0]`
/// is the first user argument.
#[test]
#[ignore = "slow LLVM host link+run; run: cargo test -p exempla --test e2e_harness llvm_host_argumenta_argv -- --ignored --nocapture"]
fn llvm_host_argumenta_binding_reads_process_argv() {
    let source = "incipit argumenta args {\n    nota args[0]\n}\n";
    let session = radix::driver::Session::new(
        Config::default()
            .with_target(Target::LlvmText)
            .with_dev_stdlib(),
    );
    let mut analysis = radix::driver::analyze_source(&session, "argumenta.fab", source)
        .expect("argumenta source frontend analysis must succeed");
    assert!(
        analysis.cli_program.is_none(),
        "raw argumenta fixture must not carry a CLI descriptor"
    );
    let device_roles = radix::mir::device_roles_from_hir(&analysis.hir);
    let lowered = radix::mir::lower_analyzed_unit_with_context(&mut analysis)
        .expect("argumenta source MIR lowering must succeed");
    let llvm = radix::mir::emit_llvm_text_probe_with_device_roles_and_exit(
        &device_roles,
        &lowered.validated,
        &lowered.interner,
        None,
    )
    .expect("argumenta source LLVM emission must succeed");
    assert!(
        llvm.contains("__faber_rt_v1_arguments"),
        "entry must seed the argumenta local from the runtime carrier:\n{llvm}"
    );
    let temp_root = super::super::common::make_temp_root();
    let llvm_file = temp_root.join("argumenta.ll");
    fs::write(&llvm_file, &llvm).expect("write argumenta LLVM text");
    let fab_path = Path::new("argumenta.fab");
    // Exact oracle args are passed to the LLVM host; argv[0] (the binary
    // path) is excluded per Faber semantics, so args[0] is the first user
    // argument. (Named residual: the raw-argumenta Rust codegen at
    // radix-hir-rust/src/module.rs emits `std::env::args().collect()`, which
    // still includes argv[0]; the CLI parser and the product `processus`
    // provider both skip it.)
    let probe = run_llvm_exemplum_with_args(
        &llvm_file,
        &temp_root,
        "argumenta",
        fab_path,
        &["alpha", "beta", "gamma"],
    );
    assert_eq!(probe.stdout, "alpha\n", "{}", probe.reason);
    assert!(
        probe.stderr.is_empty(),
        "unexpected stderr: {:?}",
        probe.stderr
    );
    assert_eq!(probe.exit_code, Some(0));
}

/// S8.1 (d3de92fa): expected runtime-failure propagation — a failed `adfirma`
/// latches `STATUS_PANIC` in the emitted entry and the runtime main returns
/// the latched status as the process exit code (3), proving runtime failures
/// propagate through the entry contract instead of exiting 0.
#[test]
#[ignore = "slow LLVM host link+run; run: cargo test -p exempla --test e2e_harness llvm_host_runtime_failure_propagation -- --ignored --nocapture"]
fn llvm_host_runtime_failure_propagates_status() {
    let source = "incipit {\n    adfirma 1 + 1 ≡ 3\n}\n";
    let session = radix::driver::Session::new(
        Config::default()
            .with_target(Target::LlvmText)
            .with_dev_stdlib(),
    );
    let mut analysis = radix::driver::analyze_source(&session, "runtime-failure.fab", source)
        .expect("runtime-failure source frontend analysis must succeed");
    let device_roles = radix::mir::device_roles_from_hir(&analysis.hir);
    let lowered = radix::mir::lower_analyzed_unit_with_context(&mut analysis)
        .expect("runtime-failure source MIR lowering must succeed");
    let llvm = radix::mir::emit_llvm_text_probe_with_device_roles_and_exit(
        &device_roles,
        &lowered.validated,
        &lowered.interner,
        None,
    )
    .expect("runtime-failure source LLVM emission must succeed");
    assert!(
        llvm.contains("__faber_rt_v1_assert"),
        "failed adfirma must route through the versioned assert carrier:\n{llvm}"
    );
    let temp_root = super::super::common::make_temp_root();
    let llvm_file = temp_root.join("runtime-failure.ll");
    fs::write(&llvm_file, &llvm).expect("write runtime-failure LLVM text");
    let fab_path = Path::new("runtime-failure.fab");
    let probe = run_llvm_exemplum(&llvm_file, &temp_root, "runtime-failure", fab_path);
    assert_eq!(
        probe.exit_code,
        Some(3),
        "failed adfirma must propagate STATUS_PANIC (3) as the process exit: {}",
        probe.reason
    );
    assert!(
        probe.stdout.is_empty(),
        "unexpected stdout: {:?}",
        probe.stdout
    );
}
