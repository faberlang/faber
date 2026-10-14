#![cfg(feature = "hir-fhir")]

//! Round-trip equivalence harness for HIR artifact serialization.
//!
//! UNIT-U4.2: Proves the `fhir` contract — codegen output from an original
//! `analyze_source` unit must equal codegen output from the same unit after
//! serialize → deserialize, byte-for-byte, across all 5 fixtures × 5
//! HIR-backed targets.
//!
//! ## F23 target-matrix narrowing
//!
//! The Swift backend (`radix-hir-swift`) has pre-existing limitations
//! external to this campaign: it does not support import emission or Param
//! type generation.  The following 2 target–fixture pairs are formally
//! excluded under F23.  Exclusion is NOT a silent skip: the pair must fail
//! with the documented stable issue code on both the original and the loaded
//! unit, so all 25 matrix cells are asserted (23 byte-for-byte + 2
//! exclusion-reason round-trips).
//!
//! | Fixture | Target | Reason |
//! |---------|--------|--------|
//! | `library-import.fab` | Swift | import emission not yet supported |
//! | `generics.fab` | Swift | Param type not supported |
//!
//! These exclusions are explicit in the harness, not caught by a generic
//! catch-all.  Unexpected backend errors still panic the test.
//!
//! CTM F2: `hir_artifact_round_trip_library_import` runs first — if the
//! library-import fixture fails, the snapshot contract (ResolverSnapshot,
//! LibraryRegistry) is incomplete and no other fixture would expose it.

use radix_module::codegen::{CodegenError, Target, generate_from_analyzed};
use radix_module::driver::{AnalyzeRequest, Config, Session, analyze_source};
use radix_module::hir::artifact::load_fhir;
use radix_module::locale::{KeywordSurface, latin_locale_pack};

/// Returns the documented F23 exclusion issue code for a failing
/// fixture+target pair, when the failure matches the documented reason.
///
/// Match is by fixture+target AND the stable issue code on the error: Swift
/// fails library-import on import emission and generics on Param type
/// translation. An excluded pair that fails with any other issue (or a
/// non-excluded pair) is a harness regression, not the documented exclusion,
/// and must not be swallowed.
fn f23_exclusion_issue(fixture_stem: &str, target: Target, err: &CodegenError) -> Option<&'static str> {
    let documented = match (fixture_stem, target) {
        ("library-import", Target::HirSwift) => "swift_import_unsupported",
        ("generics", Target::HirSwift) => "swift_type_unsupported",
        _ => return None,
    };
    let issue = err
        .args
        .iter()
        .find(|arg| arg.name == "issue")
        .map(|arg| arg.value.as_str());
    (issue == Some(documented)).then_some(documented)
}

/// Extract the code string from a codegen output for byte-for-byte comparison.
fn extract_code(output: &radix_module::Output) -> String {
    match output {
        radix_module::Output::Rust(o) => o.code.clone(),
        radix_module::Output::TypeScript(o) => o.code.clone(),
        radix_module::Output::Go(o) => o.code.clone(),
        radix_module::Output::Swift(o) => o.code.clone(),
        radix_module::Output::Faber(o) => o.code.clone(),
        _ => panic!("unexpected non-HIR target output variant"),
    }
}

// ── Round-trip runner ─────────────────────────────────────────────────────

/// Run the full serialize→deserialize round-trip for a single fixture
/// across all requested targets.  Steps:
///
/// 1. analyze_source → original unit
/// 2. generate_from_analyzed for each target → original code strings
///    (F23-excluded pairs are skipped with a note; unexpected errors panic)
/// 3. HirArtifact::from_analyzed → postcard → load_fhir → loaded unit
/// 4. generate_from_analyzed for each target → loaded code strings
/// 5. assert byte-for-byte equality for each target pair
fn run_round_trip(fixture_path: &str, session: &Session, surface: &KeywordSurface, targets: &[Target]) {
    let source: &str = match fixture_path {
        "fixtures/hir_artifact/hello-world.fab" => {
            include_str!("fixtures/hir_artifact/hello-world.fab")
        }
        "fixtures/hir_artifact/multi-item.fab" => {
            include_str!("fixtures/hir_artifact/multi-item.fab")
        }
        "fixtures/hir_artifact/library-import.fab" => {
            include_str!("fixtures/hir_artifact/library-import.fab")
        }
        "fixtures/hir_artifact/generics.fab" => {
            include_str!("fixtures/hir_artifact/generics.fab")
        }
        "fixtures/hir_artifact/patterns.fab" => {
            include_str!("fixtures/hir_artifact/patterns.fab")
        }
        _ => panic!("unknown fixture: {fixture_path}"),
    };
    let name = std::path::Path::new(fixture_path)
        .file_stem()
        .unwrap()
        .to_str()
        .unwrap();
    let fixture_stem = name;

    // Step 1: analyze → original outputs
    let unit = analyze_source(session, name, source, AnalyzeRequest::default())
        .unwrap_or_else(|e| panic!("analyze_source failed for {name}: {e:?}"));

    let mut original_codes: Vec<(Target, String)> = Vec::new();
    // Documented F23 exclusions observed on the original unit, keyed by the
    // stable issue code. These cells are asserted too: the documented failure
    // reason must survive serialize→load unchanged.
    let mut original_exclusions: Vec<(Target, &'static str)> = Vec::new();
    for target in targets {
        match generate_from_analyzed(*target, &unit, surface) {
            Ok(output) => {
                original_codes.push((*target, extract_code(&output)));
            }
            Err(e) => {
                if let Some(issue) = f23_exclusion_issue(fixture_stem, *target, &e) {
                    eprintln!("SKIP  (F23) fixture={fixture_stem}.fab target={target:?}: {e}");
                    original_exclusions.push((*target, issue));
                } else {
                    panic!("generate_from_analyzed failed for {fixture_stem}.fab target {target:?}: {e}");
                }
            }
        }
    }

    if original_codes.is_empty() {
        // All targets were F23-excluded — nothing to round-trip.
        // This would only happen if a fixture has no supported target at all.
        eprintln!("NOTE  fixture={fixture_stem}.fab: all targets excluded under F23");
        return;
    }

    // Step 2: serialize → bytes → load
    let artifact = radix_module::hir::serialize::from_analyzed(&unit, "test_hash", "test.faber");
    let bytes = postcard::to_allocvec(&artifact).expect("postcard serialize should succeed");
    let loaded = load_fhir(&bytes).unwrap_or_else(|e| panic!("load_fhir failed for {fixture_stem}.fab: {e}"));

    // Schema-v2 presentation sidecar must round-trip exactly: the loader
    // reconstructs HIR and presentation together (U-4 gate).
    assert_eq!(
        loaded.presentation, artifact.presentation,
        "presentation sidecar lost or mutated across serialize→load for {fixture_stem}.fab"
    );

    // Step 3: loaded → output for each target, compare byte-for-byte
    for (target, original_code) in &original_codes {
        let output = generate_from_analyzed(*target, &loaded, surface).unwrap_or_else(|e| {
            panic!("generate_from_analyzed (loaded) failed for {fixture_stem}.fab target {target:?}: {e}")
        });
        let loaded_code = extract_code(&output);

        // Byte-for-byte comparison with rich diagnostics on mismatch.
        if original_code != &loaded_code {
            let mismatch_pos = original_code
                .chars()
                .zip(loaded_code.chars())
                .position(|(a, b)| a != b);
            if let Some(pos) = mismatch_pos {
                let start = pos.saturating_sub(40);
                let orig_end = (pos + 40).min(original_code.len());
                let load_end = (pos + 40).min(loaded_code.len());
                panic!(
                    "ROUND-TRIP MISMATCH (U4.2): fixture={fixture_stem}.fab, target={target:?}\n\
                     first diff at byte offset {pos}\n\
                     original[{start}..{orig_end}]: {:?}\n\
                     loaded[{start}..{load_end}]:    {:?}",
                    &original_code[start..orig_end],
                    &loaded_code[start..load_end],
                );
            }
            panic!(
                "ROUND-TRIP MISMATCH (U4.2): fixture={fixture_stem}.fab, target={target:?}\n\
                 original length: {}, loaded length: {}",
                original_code.len(),
                loaded_code.len(),
            );
        }
    }

    // Step 4: F23-excluded cells are asserted too. Each excluded pair must
    // fail the SAME documented way on the loaded unit — a drift in the
    // exclusion reason across serialize→load is a round-trip regression, not
    // the documented exclusion, and must not be swallowed by the skip.
    for (target, issue) in &original_exclusions {
        match generate_from_analyzed(*target, &loaded, surface) {
            Ok(_) => panic!("F23 excluded pair generated OK from loaded unit: {fixture_stem}.fab target {target:?}"),
            Err(e) => {
                let loaded_issue = e
                    .args
                    .iter()
                    .find(|arg| arg.name == "issue")
                    .map(|arg| arg.value.as_str());
                assert_eq!(
                    loaded_issue,
                    Some(*issue),
                    "F23 exclusion issue drifted across serialize→load for {fixture_stem}.fab target {target:?}: original={issue}, loaded={loaded_issue:?}"
                );
            }
        }
    }
}

// ── Test functions ────────────────────────────────────────────────────────

/// Runs FIRST per CTM F2. If the library-import fixture fails, the snapshot
/// contract (ResolverSnapshot, LibraryRegistry) is incomplete — no other
/// fixture would expose it.
///
/// F23: Swift excluded from library-import (import emission not yet supported).
#[cfg(all(feature = "hir-rust", feature = "hir-ts", feature = "hir-go", feature = "hir-swift"))]
#[test]
fn hir_artifact_round_trip_library_import() {
    let config = Config::default().with_dev_locale_root();
    let session = Session::new(config);
    let pack = latin_locale_pack();
    let surface = KeywordSurface::new(&pack);
    let targets = [
        Target::HirRust,
        Target::HirTypeScript,
        Target::HirGo,
        Target::HirSwift,
        Target::HirFaber,
    ];
    run_round_trip("fixtures/hir_artifact/library-import.fab", &session, &surface, &targets);
}

/// Runs after library-import succeeds. All remaining 4 fixtures × 5 targets
/// (minus F23 exclusions).
///
/// F23: Swift excluded from generics (Param type not yet supported).
#[cfg(all(feature = "hir-rust", feature = "hir-ts", feature = "hir-go", feature = "hir-swift"))]
#[test]
fn hir_artifact_round_trip_all() {
    let config = Config::default().with_dev_locale_root();
    let session = Session::new(config);
    let pack = latin_locale_pack();
    let surface = KeywordSurface::new(&pack);
    let targets = [
        Target::HirRust,
        Target::HirTypeScript,
        Target::HirGo,
        Target::HirSwift,
        Target::HirFaber,
    ];
    let fixtures = [
        "fixtures/hir_artifact/hello-world.fab",
        "fixtures/hir_artifact/multi-item.fab",
        "fixtures/hir_artifact/generics.fab",
        "fixtures/hir_artifact/patterns.fab",
    ];
    for fixture in &fixtures {
        run_round_trip(fixture, &session, &surface, &targets);
    }
}

/// U-4 end-count parity: every structural end recorded in the presentation
/// `block_ends` list must be carried by a block in the HIR tree
/// (`block_end_for` records one end per authored block boundary). Splicing
/// paths that move statements out of an authored block must preserve the
/// block's `end` on the receiving block.
///
/// Regression: the `fac … dum` loop wrap and `proba`
/// test-body splicing dropped `HirBlock::end` while `block_end_for` still
/// recorded the end, leaving the sidecar with more entries than the tree.
#[test]
fn structural_end_count_parity_across_splicing_paths() {
    use radix_module::hir::visit::{HirVisitor, walk_block};

    #[derive(Default)]
    struct EndCounter {
        ends: usize,
    }
    impl HirVisitor for EndCounter {
        fn visit_block(&mut self, block: &radix_module::hir::HirBlock) {
            if block.end.is_some() {
                self.ends += 1;
            }
            walk_block(self, block);
        }
    }

    let config = Config::default().with_dev_locale_root();
    let session = Session::new(config);

    // Exercises the splicing paths: fac…dum wrap, proba body.
    let source = r#"
functio fac_dum(numerus n) → numerus {
    fac {
        nota n
    } dum n ≻ 0
    redde n
}

probandum "end parity" {
    proba "body end preserved" {
        adfirma verum
    }
}
"#;

    let unit = analyze_source(&session, "end-parity", source, AnalyzeRequest::default())
        .unwrap_or_else(|e| panic!("analyze_source failed for end-parity source: {e:?}"));

    let mut counter = EndCounter::default();
    counter.visit_program(&unit.hir);
    assert_eq!(
        counter.ends,
        unit.presentation.block_ends.len(),
        "presentation block_ends must match blocks carrying an authored end",
    );
}
