//! The call-site storage-overlap rule is not tensor-specific: every type whose
//! unmarked bind aliases under the reference model (list, map, genus) names
//! one storage through `mut alias ← g`, so `k(g, alias)` overlaps exactly like
//! `k(g, g)`. Value types (numbers, text) copy at the bind and never alias.
//! Alias-spelled overlaps are rejected with the issues the direct spelling
//! produces; `copy`/`own` binds and read/read calls stay legal.

use crate::SemanticErrorKind;
use crate::test_support::{AnalyzedFixture, analyze_source_with_config, issue};

/// One aliasing carrier: its type spelling, its declarations, and how a
/// parameter body touches the value.
struct Carrier {
    ty: &'static str,
    decls: &'static str,
    init: &'static str,
    fresh: &'static str,
    mut_body: &'static str,
    read_body: &'static str,
}

const LIST: Carrier = Carrier {
    ty: "list<int>",
    decls: "",
    init: "[1, 2, 3]",
    fresh: "[4, 5]",
    mut_body: "a.append(b.length())",
    read_body: "print a.length()\n    print b.length()",
};

const MAP: Carrier = Carrier {
    ty: "map<string, int>",
    decls: "",
    init: r#"{ "alpha": 10 }"#,
    fresh: r#"{ "beta": 20 }"#,
    mut_body: r#"a.put("x", b.length())"#,
    read_body: "print a.length()\n    print b.length()",
};

const GENUS: Carrier = Carrier {
    ty: "Box",
    decls: "class Box {\n    mut int n\n}\n\n",
    init: "Box { n = 1 }",
    fresh: "Box { n = 2 }",
    mut_body: "a.n ← b.n",
    read_body: "print a.n\n    print b.n",
};

/// The fixtures are written in the English surface.
fn analyze_en(source: &str) -> AnalyzedFixture {
    let mut config = crate::PassConfig::for_target(radix_target::Target::HirRust);
    config.locale_pack = Some(
        crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
            .expect("load English locale pack"),
    );
    analyze_source_with_config(source, config)
}

fn program(carrier: &Carrier, body: &str) -> String {
    let ty = carrier.ty;
    let mut_body = carrier.mut_body;
    let read_body = carrier.read_body;
    format!(
        r"{decls}fn mut_read(mut {ty} a, {ty} b) → void {{
    {mut_body}
}}

fn mut_ref(mut {ty} a, ref {ty} b) → void {{
    {mut_body}
}}

fn mut_mut(mut {ty} a, mut {ty} b) → void {{
    {mut_body}
}}

fn read_read({ty} a, {ty} b) → void {{
    {read_body}
}}

main {{
    mut {ty} g ← {init}
    mut {ty} fresh ← {fresh}
{body}
}}
",
        decls = carrier.decls,
        init = carrier.init,
        fresh = carrier.fresh,
    )
}

fn errors_of(source: &str) -> Vec<(SemanticErrorKind, String)> {
    analyze_en(source)
        .result
        .diagnostics
        .iter()
        .filter(|d| d.is_error())
        .map(|d| (d.kind.clone(), issue(d).unwrap_or_default().to_string()))
        .collect()
}

fn assert_accepts(carrier: &Carrier, body: &str) {
    let found = errors_of(&program(carrier, body));
    assert!(
        found.is_empty(),
        "{}: expected no error for `{body}`, got: {found:?}",
        carrier.ty
    );
}

fn assert_rejects(carrier: &Carrier, body: &str, expected: &str) {
    let found = errors_of(&program(carrier, body));
    assert!(
        found.iter().any(|(_, issue)| issue == expected),
        "{}: expected issue {expected:?} for `{body}`, got: {found:?}",
        carrier.ty
    );
}

fn bind(carrier: &Carrier, name: &str, source: &str) -> String {
    format!("    mut {} {name} ← {source}\n", carrier.ty)
}

const ALL: [&Carrier; 3] = [&LIST, &MAP, &GENUS];

// =============================================================================
// Rejected: an alias spelling overlaps like the direct spelling
// =============================================================================

#[test]
fn direct_spelling_is_rejected() {
    for c in ALL {
        assert_rejects(c, "    mut_read(mut g, g)", "use_while_mutably_borrowed");
        assert_rejects(c, "    mut_mut(mut g, mut g)", "mutable_borrow_conflict");
    }
}

#[test]
fn mut_then_aliased_read_argument_is_rejected() {
    for c in ALL {
        let body = bind(c, "alias", "g") + "    mut_read(mut g, alias)";
        assert_rejects(c, &body, "use_while_mutably_borrowed");
    }
}

#[test]
fn aliased_mut_then_read_argument_is_rejected() {
    for c in ALL {
        let body = bind(c, "alias", "g") + "    mut_read(mut alias, g)";
        assert_rejects(c, &body, "use_while_mutably_borrowed");
    }
}

#[test]
fn mut_and_ref_arguments_through_an_alias_are_shared_borrow_conflicts() {
    for c in ALL {
        let body = bind(c, "alias", "g") + "    mut_ref(mut g, alias)";
        assert_rejects(c, &body, "shared_borrow_conflict");
        let body = bind(c, "alias", "g") + "    mut_ref(mut alias, g)";
        assert_rejects(c, &body, "shared_borrow_conflict");
    }
}

#[test]
fn two_mut_arguments_through_an_alias_are_rejected() {
    for c in ALL {
        let body = bind(c, "alias", "g") + "    mut_mut(mut g, mut alias)";
        assert_rejects(c, &body, "mutable_borrow_conflict");
        let body = bind(c, "alias", "g") + "    mut_mut(mut alias, mut g)";
        assert_rejects(c, &body, "mutable_borrow_conflict");
    }
}

#[test]
fn two_aliases_of_one_value_overlap() {
    for c in ALL {
        let body =
            bind(c, "left", "g") + &bind(c, "right", "g") + "    mut_mut(mut left, mut right)";
        assert_rejects(c, &body, "mutable_borrow_conflict");
    }
}

#[test]
fn alias_of_an_alias_overlaps_transitively() {
    for c in ALL {
        let body =
            bind(c, "alias", "g") + &bind(c, "again", "alias") + "    mut_mut(mut g, mut again)";
        assert_rejects(c, &body, "mutable_borrow_conflict");
        let body =
            bind(c, "alias", "g") + &bind(c, "again", "alias") + "    mut_read(mut again, g)";
        assert_rejects(c, &body, "use_while_mutably_borrowed");
    }
}

// =============================================================================
// Accepted: no overlap, no alias, or overlap that is only read/read
// =============================================================================

#[test]
fn aliased_read_arguments_stay_legal() {
    for c in ALL {
        let body = bind(c, "alias", "g") + "    read_read(g, alias)";
        assert_accepts(c, &body);
        let body = bind(c, "alias", "g") + "    read_read(alias, g)";
        assert_accepts(c, &body);
    }
}

#[test]
fn copy_bind_does_not_alias() {
    for c in ALL {
        let dup = format!("    mut {} dup ← copy g\n", c.ty);
        assert_accepts(c, &(dup.clone() + "    mut_read(mut g, dup)"));
        assert_accepts(c, &(dup.clone() + "    mut_read(mut dup, g)"));
        assert_accepts(c, &(dup + "    mut_mut(mut g, mut dup)"));
    }
}

#[test]
fn own_bind_does_not_alias() {
    for c in ALL {
        let body = format!(
            "    let {} taken ← own g\n    mut_mut(mut g, mut taken)",
            c.ty
        );
        let found = errors_of(&program(c, &body));
        assert!(
            !found.iter().any(|(_, issue)| matches!(
                issue.as_str(),
                "mutable_borrow_conflict" | "shared_borrow_conflict" | "use_while_mutably_borrowed"
            )),
            "{}: {found:?}",
            c.ty
        );
    }
}

#[test]
fn alias_of_another_value_does_not_overlap_this_one() {
    for c in ALL {
        let body = bind(c, "alias", "fresh") + "    mut_read(mut g, alias)";
        assert_accepts(c, &body);
        let body = bind(c, "alias", "fresh") + "    mut_mut(mut alias, mut g)";
        assert_accepts(c, &body);
        let body = bind(c, "alias", "g") + "    mut_mut(mut alias, mut fresh)";
        assert_accepts(c, &body);
    }
}

#[test]
fn sequential_calls_release_call_scoped_borrows() {
    for c in ALL {
        let body = bind(c, "alias", "g")
            + "    mut_read(mut g, fresh)\n    mut_read(mut alias, fresh)\n    mut_mut(mut fresh, mut g)\n    mut_read(mut fresh, alias)";
        assert_accepts(c, &body);
    }
}

// =============================================================================
// Value types copy at the bind: never an alias
// =============================================================================

#[test]
fn number_bind_is_a_copy_not_an_alias() {
    let source = r"fn bump(mut int a, int b) → void {
    a ← a + b
}

main {
    mut int g ← 1
    mut int copy_of_g ← g
    bump(mut g, copy_of_g)
    bump(mut copy_of_g, g)
}
";
    let found = errors_of(source);
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn text_bind_is_a_copy_not_an_alias() {
    let source = r#"fn join(mut string a, string b) → void {
    a ← a + b
}

main {
    mut string s ← "a"
    mut string copy_of_s ← s
    join(mut s, copy_of_s)
    join(mut copy_of_s, s)
}
"#;
    let found = errors_of(source);
    assert!(found.is_empty(), "{found:?}");
}
