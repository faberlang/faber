use super::promote_library_surface_visibility;

#[test]
fn visibility_promotion_ignores_string_literals() {
    let source = r#"pub(crate) fn exported() {}
let note = "pub(crate) fn hidden() {}";
    pub(crate) struct Visible;
// pub(crate) enum CommentOnly {}
"#;

    let promoted = promote_library_surface_visibility(source);

    assert!(promoted.contains("pub fn exported() {}"));
    assert!(promoted.contains("    pub struct Visible;"));
    assert!(promoted.contains("\"pub(crate) fn hidden() {}\""));
    assert!(promoted.contains("// pub(crate) enum CommentOnly {}"));
}
