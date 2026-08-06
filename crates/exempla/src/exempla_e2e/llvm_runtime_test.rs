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
    let result = Compiler::new(
        Config::default()
            .with_target(Target::LlvmText)
            .with_dev_stdlib(),
    )
    .compile(&fab_path);
    assert!(
        result.success(),
        "conversio/fallibilis.fab LLVM compile failed: {:?}",
        result.diagnostics
    );
    let Some(Output::LlvmText(output)) = result.output else {
        panic!("conversio/fallibilis.fab did not produce LLVM text");
    };
    let temp_root = super::super::common::make_temp_root();
    let llvm_file = temp_root.join("conversio-fallibilis.ll");
    fs::write(&llvm_file, output.code).expect("write instans failable LLVM text");

    let probe = run_llvm_exemplum(&llvm_file, &temp_root, "conversio-fallibilis", &fab_path);
    assert_eq!(probe.bucket, LlvmRunBucket::Runnable, "{}", probe.reason);
    // L19: raw `nota` of an instans renders the Rust oracle's `Debug` shape
    // (`Instans { nanos: …, praecisio: … }`) through the instans display
    // carrier, and the fac/cape-absorbed conversion failure no longer latches
    // a nonzero exit code (byte-exact handled output exits 0). The multi-arg
    // nota spacing (three values on one oracle line) is a MIR-lowering gap
    // outside L19 scope, so this asserts the per-line shape only.
    let line = "Instans { nanos: 296638320000000000, praecisio: Secunda }\n";
    assert_eq!(probe.stdout, format!("{line}{line}{line}"));
    assert_eq!(probe.exit_code, Some(0));
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
/// D-PA4 (hand-3 d5596b1c, rebuilt on the S8.3 package-to-LLVM builder): the
/// importa two-module LLVM-host link proof runs through the reusable builder.
///
/// The Faber package graph resolves `importa/importa.fab` + sibling
/// `auxilium.fab`; the builder emits one module per unit (D11) — the entry
/// module declares and calls `saluta` through the canonical external symbol
/// `__faber_external_product_importa_module_auxilium_func_saluta`; the sibling
/// module (library mode — no entry) defines the same symbol. Both modules link
/// with the host runtime archive into one executable whose stdout is
/// `Salve, Marcus!` (exit 0), matching `importa.expected` and the Rust oracle.
#[test]
#[ignore = "slow LLVM host link+run; run: cargo test -p exempla --test e2e_harness llvm_host_importa_two_module_link -- --ignored --nocapture"]
fn llvm_host_importa_two_module_link_matches_rust_oracle() {
    const EXTERNAL_SALUTA: &str = "__faber_external_product_importa_module_auxilium_func_saluta";
    let fab_path = crate::paths::corpus_dir().join("importa/importa.fab");
    let temp_root = super::super::common::make_temp_root();
    let config = radix::Config::default().with_target(Target::LlvmText);
    let runtime_archive = llvm_runtime_archive().expect("LLVM host runtime archive");
    let options = faber_cli::package::PackageLlvmOptions::new(temp_root.join("importa.modules"))
        .with_runtime_archive(Some(runtime_archive));
    let build = faber_cli::package::build_package_llvm(&config, &fab_path, &options)
        .expect("importa package LLVM build must succeed");

    assert_eq!(
        build.modules.len(),
        2,
        "importa package graph must resolve two units (entry + sibling)"
    );
    let entry_module = build
        .modules
        .iter()
        .find(|module| module.is_entry)
        .expect("entry module");
    let sibling_module = build
        .modules
        .iter()
        .find(|module| !module.is_entry)
        .expect("sibling module");
    let entry = fs::read_to_string(&entry_module.llvm_path).expect("read entry module");
    let sibling = fs::read_to_string(&sibling_module.llvm_path).expect("read sibling module");
    assert!(
        entry.contains(EXTERNAL_SALUTA),
        "entry module must declare/call the sibling external symbol:\n{entry}"
    );
    assert!(
        sibling.contains(EXTERNAL_SALUTA),
        "sibling module must define the external symbol:\n{sibling}"
    );
    assert!(
        !sibling.contains("__faber_program_entry_v1"),
        "library module must not emit a program entry:\n{sibling}"
    );
    assert!(
        entry.contains("define %FaberRtExitV1 @__faber_program_entry_v1"),
        "entry module must define the host program entry:\n{entry}"
    );
    assert_eq!(
        build.manifest.entry_module, entry_module.llvm_path,
        "manifest must record the exactly-one entry module"
    );
    assert_eq!(
        build.manifest.modules.len(),
        2,
        "manifest must list one module per unit"
    );

    let probe = run_llvm_modules(
        &build.manifest.modules,
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

/// S8.4 (hand-3 2d77a75f): SECOND local-import fixture through the S8.3
/// package-to-LLVM builder — multi-module cross-unit call parity (D11
/// canonical external identities).
///
/// `geminus/geminus.fab` imports sibling `geminus/adiutor.fab` and calls
/// `greeting` under
/// `__faber_external_product_geminus_module_adiutor_func_greeting`; the
/// sibling module (library mode — no entry) defines the same symbol. The
/// builder emits one module per unit and an inspectable link manifest; the
/// manifest drives a single clang link with the host runtime archive, and the
/// executable must print `Salve, Tullia!` (exit 0), matching the fixture's
/// `.expected` oracle.
#[test]
#[ignore = "slow LLVM host link+run; run: cargo test -p exempla --test e2e_harness llvm_host_geminus_two_module_link -- --ignored --nocapture"]
fn llvm_host_geminus_two_module_link_matches_rust_oracle() {
    const EXTERNAL_GREETING: &str =
        "__faber_external_product_geminus_module_adiutor_func_greeting";
    const ENTRY_SOURCE: &str = r#"# geminus — local-import consumer of adiutor.fab
importa ex "./adiutor" privata adiutor

incipit {
    nota adiutor.greeting("Tullia")
}
"#;
    const SIBLING_SOURCE: &str = r#"# adiutor — sibling module for geminus
functio greeting(textus nomen) → textus {
    redde "Salve, §!"(nomen)
}

incipit {
    nota greeting("adiutor")
}
"#;

    let temp_root = super::super::common::make_temp_root();
    let package_dir = temp_root.join("geminus");
    fs::create_dir_all(&package_dir).expect("create geminus fixture dir");
    fs::write(package_dir.join("adiutor.fab"), SIBLING_SOURCE).expect("write adiutor.fab");
    fs::write(package_dir.join("geminus.fab"), ENTRY_SOURCE).expect("write geminus.fab");
    fs::write(package_dir.join("geminus.expected"), "Salve, Tullia!\n").expect("write expected");
    let fab_path = package_dir.join("geminus.fab");

    let config = radix::Config::default().with_target(Target::LlvmText);
    let runtime_archive = llvm_runtime_archive().expect("LLVM host runtime archive");
    let options = faber_cli::package::PackageLlvmOptions::new(temp_root.join("geminus.modules"))
        .with_runtime_archive(Some(runtime_archive));
    let build = faber_cli::package::build_package_llvm(&config, &fab_path, &options)
        .expect("geminus package LLVM build must succeed");

    assert_eq!(build.product, "geminus");
    assert_eq!(
        build.modules.len(),
        2,
        "geminus package graph must resolve two units (entry + sibling)"
    );
    let entry_module = build
        .modules
        .iter()
        .find(|module| module.is_entry)
        .expect("entry module");
    let sibling_module = build
        .modules
        .iter()
        .find(|module| module.module_segments == ["adiutor".to_owned()])
        .expect("adiutor sibling module");
    let entry = fs::read_to_string(&entry_module.llvm_path).expect("read entry module");
    let sibling = fs::read_to_string(&sibling_module.llvm_path).expect("read sibling module");
    assert!(
        entry.contains(EXTERNAL_GREETING),
        "entry module must declare/call the sibling external symbol:\n{entry}"
    );
    assert!(
        sibling.contains(EXTERNAL_GREETING),
        "sibling module must define the external symbol:\n{sibling}"
    );
    assert!(
        !sibling.contains("__faber_program_entry_v1"),
        "library module must not emit a program entry:\n{sibling}"
    );
    assert!(
        entry.contains("define %FaberRtExitV1 @__faber_program_entry_v1"),
        "entry module must define the host program entry:\n{entry}"
    );
    assert_eq!(
        build.manifest.entry_module, entry_module.llvm_path,
        "manifest must record the exactly-one entry module"
    );
    assert_eq!(
        build.manifest.modules,
        build.modules.iter().map(|module| module.llvm_path.clone()).collect::<Vec<_>>(),
        "manifest module list must be the exact deterministic module list"
    );

    let probe = run_llvm_modules(
        &build.manifest.modules,
        &temp_root,
        "geminus-two-module",
        &fab_path,
    );
    assert_eq!(probe.bucket, LlvmRunBucket::OutputMatched, "{}", probe.reason);
    assert_eq!(probe.stdout, "Salve, Tullia!\n");
    assert!(
        probe.stderr.is_empty(),
        "unexpected stderr: {:?}",
        probe.stderr
    );
    assert_eq!(probe.exit_code, Some(0));
}

/// S8.5 (hand-3 db2bbd69): Norma graph proof — a package fixture importing
/// `norma:chorda` resolves the selected flat Norma unit exactly as Rust
/// package compile does (Faber package graph — no new resolver), lowers it
/// through MIR, and emits ONE `.ll` module per unit (D11 one-module-per-unit):
///
/// - the entry module declares and calls the canonical external symbols
///   (`__faber_external_product_norma_module_chorda_func_retorta`,
///   `…_reputat`, `…_nexa`) the Norma module defines;
/// - the `chorda` module (library mode — no entry) defines the same symbols;
/// - all modules + the host runtime archive link in one clang command;
/// - the executable prints `radar`, `2`, `a-b-c` (exit 0) — identical to the
///   Rust package-compile oracle and the sibling `.expected` fixture.
///
/// No source/runtime special case by exemplar path: the call is a normal
/// `norma:chorda` import resolved through the package graph.
#[test]
#[ignore = "slow LLVM host link+run; run: cargo test -p exempla --test e2e_harness llvm_host_norma_chorda_graph_link -- --ignored --nocapture"]
fn llvm_host_norma_chorda_graph_link_matches_rust_oracle() {
    const EXTERNAL_RETORTA: &str = "__faber_external_product_norma_module_chorda_func_retorta";
    const EXTERNAL_REPUTAT: &str = "__faber_external_product_norma_module_chorda_func_reputat";
    const ENTRY_SOURCE: &str = r#"# chorda-consumer — norma-backed fixture
importa ex "norma:chorda" privata chorda

incipit {
    nota chorda.retorta("radar")
    nota chorda.reputat("ababa", "ab")
    nota chorda.nexa(["a", "b", "c"], "-")
}
"#;
    const EXPECTED: &str = "radar\n2\na-b-c\n";

    let temp_root = super::super::common::make_temp_root();
    let package_dir = temp_root.join("chorda-consumer");
    fs::create_dir_all(&package_dir).expect("create chorda-consumer fixture dir");
    fs::write(package_dir.join("chorda-consumer.fab"), ENTRY_SOURCE)
        .expect("write chorda-consumer.fab");
    fs::write(package_dir.join("chorda-consumer.expected"), EXPECTED)
        .expect("write expected");
    let fab_path = package_dir.join("chorda-consumer.fab");

    let config = radix::Config::default().with_target(Target::LlvmText);
    let runtime_archive = llvm_runtime_archive().expect("LLVM host runtime archive");
    let options = faber_cli::package::PackageLlvmOptions::new(temp_root.join("chorda.modules"))
        .with_runtime_archive(Some(runtime_archive));
    let build = faber_cli::package::build_package_llvm(&config, &fab_path, &options)
        .expect("chorda-consumer package LLVM build must succeed");

    // S8.5 selection: the entry unit plus ONE selected flat Norma unit
    // (`norma:chorda` — the transitive used module closure).
    assert_eq!(
        build.modules.len(),
        2,
        "chorda-consumer package graph must resolve entry + selected Norma unit"
    );
    let entry_module = build
        .modules
        .iter()
        .find(|module| module.is_entry)
        .expect("entry module");
    let norma_module = build
        .modules
        .iter()
        .find(|module| module.is_norma)
        .expect("selected Norma unit module");
    assert_eq!(
        norma_module.module_segments,
        vec!["chorda".to_owned()],
        "selected Norma unit must carry its canonical module path"
    );
    assert!(
        norma_module.unit_path.ends_with("chorda.fab"),
        "Norma module must point at its resolved interface source: {}",
        norma_module.unit_path.display()
    );
    let entry = fs::read_to_string(&entry_module.llvm_path).expect("read entry module");
    let norma = fs::read_to_string(&norma_module.llvm_path).expect("read Norma module");
    for external in [EXTERNAL_RETORTA, EXTERNAL_REPUTAT] {
        assert!(
            entry.contains(external),
            "entry module must declare/call the Norma external symbol {external}:\n{entry}"
        );
        assert!(
            norma.contains(external),
            "Norma module must define the external symbol {external}:\n{norma}"
        );
    }
    assert!(
        entry.contains("define %FaberRtExitV1 @__faber_program_entry_v1"),
        "entry module must define the host program entry:\n{entry}"
    );
    assert!(
        !norma.contains("__faber_program_entry_v1"),
        "Norma library module must not emit a program entry:\n{norma}"
    );
    assert_eq!(
        build.manifest.entry_module, entry_module.llvm_path,
        "manifest must record the exactly-one entry module"
    );
    assert_eq!(
        build.manifest.modules,
        build.modules.iter().map(|module| module.llvm_path.clone()).collect::<Vec<_>>(),
        "manifest module list must be the exact deterministic module list"
    );

    let probe = run_llvm_modules(
        &build.manifest.modules,
        &temp_root,
        "chorda-norma-graph",
        &fab_path,
    );
    assert_eq!(probe.bucket, LlvmRunBucket::OutputMatched, "{}", probe.reason);
    assert_eq!(probe.stdout, EXPECTED);
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

/// L15 (6b5f8f8f): close the exit-code cluster re-entered by the L9
/// exit-struct ABI fix. Each fixture's stdout matches its sibling `.expected`
/// byte-exact AND the process must exit 0 — the recovered `⇥` conversions and
/// value-equal textus/ascii/instans `adfirma` assertions must not latch a
/// runtime status into the exit code.
fn assert_llvm_host_fixture_exits_zero(path: &str) {
    let fab_path = crate::paths::corpus_dir().join(path);
    let source = fs::read_to_string(&fab_path).expect("read fixture source");
    let session = radix::driver::Session::new(
        Config::default()
            .with_target(Target::LlvmText)
            .with_dev_stdlib(),
    );
    let mut analysis = radix::driver::analyze_source(&session, path, &source)
        .expect("fixture frontend analysis must succeed");
    let device_roles = radix::mir::device_roles_from_hir(&analysis.hir);
    let lowered = radix::mir::lower_analyzed_unit_with_context(&mut analysis)
        .expect("fixture MIR lowering must succeed");
    let llvm = radix::mir::emit_llvm_text_probe_with_device_roles_and_exit(
        &device_roles,
        &lowered.validated,
        &lowered.interner,
        None,
    )
    .expect("fixture LLVM emission must succeed");
    let temp_root = super::super::common::make_temp_root();
    let stem = Path::new(path)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("fixture");
    let llvm_file = temp_root.join(format!("{stem}.ll"));
    fs::write(&llvm_file, &llvm).expect("write fixture LLVM text");

    let probe = run_llvm_exemplum(&llvm_file, &temp_root, stem, &fab_path);
    assert_eq!(
        probe.exit_code,
        Some(0),
        "{path}: recovered `⇥` conversions and value-equal assertions must exit 0: {}",
        probe.reason
    );
    let expected = fs::read(fab_path.with_extension("expected")).expect("read expected bytes");
    assert_eq!(
        probe.stdout.as_bytes(),
        expected,
        "{path}: LLVM host stdout must match the sibling .expected byte-exact"
    );
}

#[test]
#[ignore = "slow LLVM host link+run; run: cargo test -p exempla --test e2e_harness llvm_host_conversio_instans_exit_zero -- --ignored --nocapture"]
fn llvm_host_conversio_instans_exit_zero() {
    assert_llvm_host_fixture_exits_zero("conversio/instans.fab");
}

#[test]
#[ignore = "slow LLVM host link+run; run: cargo test -p exempla --test e2e_harness llvm_host_conversio_instans_valor_carrier_exit_zero -- --ignored --nocapture"]
fn llvm_host_conversio_instans_valor_carrier_exit_zero() {
    assert_llvm_host_fixture_exits_zero("conversio/instans-valor-carrier.fab");
}

#[test]
#[ignore = "slow LLVM host link+run; run: cargo test -p exempla --test e2e_harness llvm_host_conversio_octeti_exit_zero -- --ignored --nocapture"]
fn llvm_host_conversio_octeti_exit_zero() {
    assert_llvm_host_fixture_exits_zero("conversio/octeti.fab");
}

#[test]
#[ignore = "slow LLVM host link+run; run: cargo test -p exempla --test e2e_harness llvm_host_conversio_valor_scalaria_exit_zero -- --ignored --nocapture"]
fn llvm_host_conversio_valor_scalaria_exit_zero() {
    assert_llvm_host_fixture_exits_zero("conversio/valor-scalaria.fab");
}

#[test]
#[ignore = "slow LLVM host link+run; run: cargo test -p exempla --test e2e_harness llvm_host_conversio_valor_genus_exit_zero -- --ignored --nocapture"]
fn llvm_host_conversio_valor_genus_exit_zero() {
    assert_llvm_host_fixture_exits_zero("conversio/valor-genus.fab");
}

/// L19 (1d49b51e): close the verify pair — `!.` non-null assertion on an
/// optional genus (`assertio/nonnulla.fab`) and the non-null chain
/// (`operatores/nonnull-chain.fab`) emitted an `option_get` payload store
/// into an aggregate-typed place (`store { ptr, ptr } ptr, ptr %t.addr`),
/// which llvm-as rejected. The unwrap/coalesce now materializes the boxed
/// aggregate value before storing; both fixtures verify, link, run, and match
/// their sibling `.expected` byte-exact with exit 0.
#[test]
#[ignore = "slow LLVM host link+run; run: cargo test -p exempla --test e2e_harness llvm_host_nonnull_chain_matches -- --ignored --nocapture"]
fn llvm_host_nonnull_chain_matches() {
    for path in ["assertio/nonnulla.fab", "operatores/nonnull-chain.fab"] {
        let fab_path = crate::paths::corpus_dir().join(path);
        let result = Compiler::new(
            Config::default()
                .with_target(Target::LlvmText)
                .with_dev_stdlib(),
        )
        .compile(&fab_path);
        assert!(
            result.success(),
            "{path} LLVM compile failed: {:?}",
            result.diagnostics
        );
        let Some(Output::LlvmText(output)) = result.output else {
            panic!("{path} did not produce LLVM text");
        };
        let temp_root = super::super::common::make_temp_root();
        let stem = path.replace('/', "-").replace(".fab", "");
        let llvm_file = temp_root.join(format!("{stem}.ll"));
        fs::write(&llvm_file, output.code).expect("write non-null chain LLVM text");
        let probe = run_llvm_exemplum(&llvm_file, &temp_root, &stem, &fab_path);
        assert_eq!(
            probe.bucket,
            LlvmRunBucket::OutputMatched,
            "{path}: {}",
            probe.reason
        );
        let expected =
            fs::read(fab_path.with_extension("expected")).expect("read non-null expected bytes");
        assert_eq!(probe.stdout.as_bytes(), expected, "{path} byte-exact output");
        assert_eq!(probe.exit_code, Some(0), "{path} exit code");
    }
}

/// L19 (1d49b51e): close the tabula-iteration exit-code row — `itera de`
/// over a `tabula` keys snapshot (`de/de.fab`) stored the key array under the
/// raw map key kind (`VALUE_KIND_TEXT`), but the emitter's `array_get` reads
/// pointer-carried elements as `VALUE_KIND_PTR`, so every element read failed
/// (STATUS_INVALID_ARGUMENT): no keys printed and the process exited 1. The
/// runtime now canonicalizes pointer-carried key/value snapshot kinds; the
/// fixture prints keys + lista indices and exits 0 like the Rust oracle.
#[test]
#[ignore = "slow LLVM host link+run; run: cargo test -p exempla --test e2e_harness llvm_host_de_borrowed_iteration -- --ignored --nocapture"]
fn llvm_host_de_borrowed_iteration_matches_rust_output() {
    let fab_path = crate::paths::corpus_dir().join("de/de.fab");
    let result = Compiler::new(
        Config::default()
            .with_target(Target::LlvmText)
            .with_dev_stdlib(),
    )
    .compile(&fab_path);
    assert!(result.success(), "de/de.fab LLVM compile failed");
    let Some(Output::LlvmText(output)) = result.output else {
        panic!("de/de.fab did not produce LLVM text");
    };
    let temp_root = super::super::common::make_temp_root();
    let llvm_file = temp_root.join("de.ll");
    fs::write(&llvm_file, output.code).expect("write de LLVM text");
    let probe = run_llvm_exemplum(&llvm_file, &temp_root, "de", &fab_path);
    assert_eq!(
        probe.stdout,
        "nomen\naetas\n0\n1\n2\n",
        "tabula keys + lista indices must print: {}",
        probe.reason
    );
    assert!(probe.stderr.is_empty(), "unexpected stderr: {:?}", probe.stderr);
    assert_eq!(probe.exit_code, Some(0));
}

/// L19 (1d49b51e): close the octeti cross-assignment exit-code row —
/// `octeti/unify.fab` assigns a `lista<numerus<u8>>` into an `octeti` (type
/// identity `octeti ≡ lista<numerus<u8>>`); the raw array handle crossed the
/// arena unchanged, but the octeti ABI only looked up the octeti list, so
/// `longitudo`/`accipe` failed (garbage length, latched exit). The runtime
/// octeti operations now resolve U8-kind array handles interchangeably.
#[test]
#[ignore = "slow LLVM host link+run; run: cargo test -p exempla --test e2e_harness llvm_host_octeti_unify -- --ignored --nocapture"]
fn llvm_host_octeti_unify_cross_assignment_matches_rust_output() {
    let fab_path = crate::paths::corpus_dir().join("octeti/unify.fab");
    let result = Compiler::new(
        Config::default()
            .with_target(Target::LlvmText)
            .with_dev_stdlib(),
    )
    .compile(&fab_path);
    assert!(result.success(), "octeti/unify.fab LLVM compile failed");
    let Some(Output::LlvmText(output)) = result.output else {
        panic!("octeti/unify.fab did not produce LLVM text");
    };
    let temp_root = super::super::common::make_temp_root();
    let llvm_file = temp_root.join("octeti-unify.ll");
    fs::write(&llvm_file, output.code).expect("write octeti unify LLVM text");
    let probe = run_llvm_exemplum(&llvm_file, &temp_root, "octeti-unify", &fab_path);
    assert_eq!(
        probe.stdout,
        "4\n222\n5\n2\n",
        "octeti methods on the cross-assigned array must match: {}",
        probe.reason
    );
    assert!(probe.stderr.is_empty(), "unexpected stderr: {:?}", probe.stderr);
    assert_eq!(probe.exit_code, Some(0));
}

/// L19 (1d49b51e): close the numerus-overflow runtime-failure row — the Rust
/// oracle's generated `checked_add(…).expect("numerus overflow")` panics on
/// `numerus` overflow, but the LLVM host silently wrapped (`-9223372036854775808`).
/// `numerus` Add/Sub/Mul now lower through `llvm.*.with.overflow` and abort
/// with the oracle's exact message via the runtime overflow helper.
#[test]
#[ignore = "slow LLVM host link+run; run: cargo test -p exempla --test e2e_harness llvm_host_numerus_overflow_panics -- --ignored --nocapture"]
fn llvm_host_numerus_overflow_panics_with_rust_message() {
    let fab_path = crate::paths::corpus_dir().join("operatores/numerus-overflow.fab");
    let result = Compiler::new(
        Config::default()
            .with_target(Target::LlvmText)
            .with_dev_stdlib(),
    )
    .compile(&fab_path);
    assert!(
        result.success(),
        "operatores/numerus-overflow.fab LLVM compile failed"
    );
    let Some(Output::LlvmText(output)) = result.output else {
        panic!("numerus-overflow did not produce LLVM text");
    };
    assert!(
        output
            .code
            .contains("llvm.sadd.with.overflow.i64"),
        "numerus add must use the checked intrinsic:\n{}",
        output.code
    );
    let temp_root = super::super::common::make_temp_root();
    let llvm_file = temp_root.join("numerus-overflow.ll");
    fs::write(&llvm_file, output.code).expect("write numerus-overflow LLVM text");
    let probe = run_llvm_exemplum(&llvm_file, &temp_root, "numerus-overflow", &fab_path);
    assert_ne!(
        probe.exit_code,
        Some(0),
        "overflow must panic, not wrap: {}",
        probe.reason
    );
    assert!(
        probe.stderr.contains("numerus overflow"),
        "overflow panic message: {:?}",
        probe.stderr
    );
}

/// L19 (1d49b51e): close the tensor structa count-mismatch runtime-failure
/// row — the Rust oracle hard-errors with "tensor structa element count does
/// not match shape"; the host now reproduces the message on stderr and exits
/// nonzero through the latched failure status.
#[test]
#[ignore = "slow LLVM host link+run; run: cargo test -p exempla --test e2e_harness llvm_host_tensor_structa_count_mismatch -- --ignored --nocapture"]
fn llvm_host_tensor_structa_count_mismatch_panics_with_rust_message() {
    let fab_path = crate::paths::corpus_dir().join("tensor/method-errors.fab");
    let result = Compiler::new(
        Config::default()
            .with_target(Target::LlvmText)
            .with_dev_stdlib(),
    )
    .compile(&fab_path);
    assert!(
        result.success(),
        "tensor/method-errors.fab LLVM compile failed"
    );
    let Some(Output::LlvmText(output)) = result.output else {
        panic!("method-errors did not produce LLVM text");
    };
    let temp_root = super::super::common::make_temp_root();
    let llvm_file = temp_root.join("tensor-method-errors.ll");
    fs::write(&llvm_file, output.code).expect("write tensor method-errors LLVM text");
    let probe = run_llvm_exemplum(&llvm_file, &temp_root, "tensor-method-errors", &fab_path);
    assert_ne!(
        probe.exit_code,
        Some(0),
        "structa count mismatch must hard-error: {}",
        probe.reason
    );
    assert!(
        probe.stderr.contains("tensor structa element count does not match shape"),
        "structa error message: {:?}",
        probe.stderr
    );
}

/// L19 (1d49b51e): `conversio/fallibilis.fab` fac/cape status absorption —
/// covered by `llvm_host_instans_failable_fixture_matches_rust_output` above.
/// The conversion failure inside a `→ instans ⇥ textus` function (or a
/// fac/cape failable flow) is carried as a value the caller absorbs; latching
/// it made the process exit 1 despite byte-exact handled output. Conversions
/// in an error-carrying context no longer latch.

/// L15 (6b5f8f8f): close the modular-word link re-entry — the emitter lowered
/// `modulus<N> ↦ numerus<N>` conversions to unversioned probe symbols
/// (`__faber_runtime_convert_runtime_*`) and modular-word listas through the
/// legacy `__faber_aggregate_array_*` construct. Both now lower through the
/// versioned host ABI / inline coercion, so each fixture links, runs, and
/// matches its sibling `.expected` byte-exact with exit 0.
#[test]
#[ignore = "slow LLVM host link+run; run: cargo test -p exempla --test e2e_harness llvm_host_modular_word_family_matches -- --ignored --nocapture"]
fn llvm_host_modular_word_family_matches() {
    for path in [
        "operatores/modular-word.fab",
        "operatores/modular-word-u8.fab",
        "operatores/modular-word-u16.fab",
        "operatores/modular-word-u64.fab",
        "operatores/modular-word-u64-sha-round.fab",
    ] {
        assert_llvm_host_fixture_exits_zero(path);
    }
}

/// L23 (d57baa50): the multi-arg nota grouping space-joins EVERY argument
/// (mirroring the HIR-Rust lane) into ONE diagnostic line, so the closed rows
/// match their sibling `.expected` byte-exact with exit 0.
#[test]
#[ignore = "slow LLVM host link+run; run: cargo test -p exempla --test e2e_harness llvm_host_l23_multi_arg_nota_grouping -- --ignored --nocapture"]
fn llvm_host_l23_multi_arg_nota_grouping_matches_expected() {
    for (rel, stem) in [
        ("itera/nidificatus.fab", "itera-nidificatus"),
        ("lista/methodi-accessus.fab", "lista-methodi-accessus"),
        ("tabula/methodi-accessus.fab", "tabula-methodi-accessus"),
        ("sparsa/conversio.fab", "sparsa-conversio"),
        ("sparsa/decl.fab", "sparsa-decl"),
        ("sparsa/sparsa-codegen-smoke.fab", "sparsa-sparsa-codegen-smoke"),
    ] {
        assert_tensor_fixture_output(rel, stem);
    }
}

/// L23 (d57baa50): closed rows without a `.expected` sidecar assert the exact
/// Rust-oracle stdout (space-joined multi-arg notas, option payloads rendered
/// via `display_option` semantics).
#[test]
#[ignore = "slow LLVM host link+run; run: cargo test -p exempla --test e2e_harness llvm_host_l23_multi_arg_nota_stdout -- --ignored --nocapture"]
fn llvm_host_l23_multi_arg_nota_grouping_matches_rust_output() {
    for (rel, stem, expected) in [
        (
            "intrinseca/textus-transformationes.fab",
            "textus-transformationes",
            "Ave  AVE ROMA   ave roma \nAve Roma [\"Ave\", \"Roma\"] Ave Munde\n",
        ),
        (
            "intrinseca/vacua-ascribere.fab",
            "vacua-ascribere",
            "42 2 verum\n95 1 secundus\n",
        ),
        (
            "itera/de.fab",
            "itera-de",
            "nomen\nurbs\nnomen Marcus\nurbs Roma\nindex: 0 valor: 10\nindex: 1 valor: 20\nindex: 2 valor: 30\nalpha\nbeta\n",
        ),
    ] {
        assert_opaque_fixture_stdout(rel, stem, expected);
    }
}

