use super::{path_pattern_matches, TestSourceFilter};
use std::path::Path;

#[test]
fn restricting_explicit_proba_replaces_include() {
    let existing = TestSourceFilter {
        include: vec!["decode".to_owned()],
        exclude: vec!["*skip*".to_owned()],
    };
    let filter = TestSourceFilter::restricting_explicit_proba(
        Some(&existing),
        Path::new("/pkg/src"),
        Path::new("/pkg/src/cache.proba"),
    );
    assert_eq!(filter.include, vec!["cache.proba".to_owned()]);
    assert_eq!(filter.exclude, vec!["*skip*".to_owned()]);
    assert!(filter.allows_relative("cache.proba"));
    assert!(!filter.allows_relative("decode.proba"));
}

#[test]
fn bare_stem_matches_proba_file_name() {
    assert!(path_pattern_matches("math", "math.proba"));
    assert!(path_pattern_matches("math", "nested/math.proba"));
    assert!(!path_pattern_matches("math", "matrix.proba"));
}
