use super::{path_pattern_matches, TestSourceFilter};
use std::path::Path;

#[test]
fn bare_stem_matches_proba_file_name() {
    assert!(path_pattern_matches("math", "math.proba"));
    assert!(path_pattern_matches("math", "nested/math.proba"));
    assert!(!path_pattern_matches("math", "matrix.proba"));
}

#[test]
fn glob_star_matches_across_segments() {
    assert!(path_pattern_matches("*.proba", "math.proba"));
    assert!(path_pattern_matches("*.proba", "nested/extra.proba"));
    assert!(path_pattern_matches("nested/*", "nested/extra.proba"));
    assert!(!path_pattern_matches("nested/*", "other/extra.proba"));
}

#[test]
fn include_exclude_filter() {
    let filter = TestSourceFilter {
        include: vec!["*.proba".to_owned()],
        exclude: vec!["*edge*".to_owned()],
    };
    assert!(filter.allows_relative("math.proba"));
    assert!(!filter.allows_relative("math_edge.proba"));
    assert!(!filter.allows_relative("math.fab")); // include requires .proba via pattern
}

#[test]
fn empty_filter_allows_all() {
    let filter = TestSourceFilter::default();
    assert!(filter.allows_relative("anything.proba"));
    assert!(filter.allows_path(Path::new("/pkg/src"), Path::new("/pkg/src/x.proba")));
}

#[test]
fn include_only_restricts() {
    let filter = TestSourceFilter {
        include: vec!["math*".to_owned()],
        exclude: vec![],
    };
    assert!(filter.allows_relative("math.proba"));
    assert!(filter.allows_relative("math_edge.proba"));
    assert!(!filter.allows_relative("scene.proba"));
}
