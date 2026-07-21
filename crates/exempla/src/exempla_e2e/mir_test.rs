use super::{
    count_mir_bucket, count_mir_tier, ledger_path, MirE2eResult, MirOutcomeBucket, MirTier,
};
use std::path::PathBuf;

fn sample(path: &str, tier: MirTier, bucket: MirOutcomeBucket) -> MirE2eResult {
    MirE2eResult {
        path: PathBuf::from(path),
        tier,
        bucket,
        lowering_issue: None,
        reason: "sample".to_owned(),
    }
}

#[test]
fn count_mir_tier_includes_higher_tiers() {
    let results = vec![
        sample(
            "a.fab",
            MirTier::SourceReadable,
            MirOutcomeBucket::FrontendFailed,
        ),
        sample(
            "b.fab",
            MirTier::FrontendAnalyzed,
            MirOutcomeBucket::MirLoweringFailed,
        ),
        sample("c.fab", MirTier::MirLowered, MirOutcomeBucket::MirLowered),
    ];
    assert_eq!(count_mir_tier(&results, MirTier::FrontendAnalyzed), 2);
    assert_eq!(count_mir_tier(&results, MirTier::MirLowered), 1);
}

#[test]
fn count_mir_bucket_matches_exact_bucket() {
    let results = vec![
        sample(
            "a.fab",
            MirTier::SourceReadable,
            MirOutcomeBucket::FrontendFailed,
        ),
        sample(
            "b.fab",
            MirTier::FrontendAnalyzed,
            MirOutcomeBucket::MirLoweringFailed,
        ),
    ];
    assert_eq!(
        count_mir_bucket(&results, MirOutcomeBucket::FrontendFailed),
        1
    );
    assert_eq!(
        count_mir_bucket(&results, MirOutcomeBucket::MirLoweringFailed),
        1
    );
}

#[test]
fn ledger_path_renders_corpus_relative_path() {
    let path = crate::paths::corpus_dir()
        .join("incipit")
        .join("salve-munde.fab");
    assert_eq!(
        ledger_path(&path),
        "examples/corpus/incipit/salve-munde.fab"
    );
}
