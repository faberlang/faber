//! A call's `mut`/`ref` arguments are checked for overlap on storage, not on
//! the argument's own binding: `var alias ← g` names `g`'s tensor, so
//! `k(g, alias)` overlaps exactly like `k(g, g)`. Alias-spelled overlaps are
//! rejected with the same issues the direct spelling produces. Read/read
//! aliasing stays legal, `copy`/`own` binds do not alias, and call-scoped
//! borrows release at call end. A call result that can return an argument
//! aliases that argument's storage like a `←` bind of the place.

use crate::SemanticErrorKind;
use crate::test_support::{AnalyzedFixture, analyze_source_with_config, issue};

const PRELUDE: &str = r"fn mut_read(mut tf32[2, 2] a, tf32[2, 2] b) → void {
    a ⇇ b
}

fn mut_ref(mut tf32[2, 2] a, ref tf32[2, 2] b) → void {
    a ⇇ b
}

fn mut_mut(mut tf32[2, 2] a, mut tf32[2, 2] b) → void {
    a ⇇ b
}

fn read_read(tf32[2, 2] a, tf32[2, 2] b) → void {
    print a.flatten()
    print b.flatten()
}

fn id(tf32[2, 2] a) → tf32[2, 2] {
    return a
}

fn id_copy(copy tf32[2, 2] a) → tf32[2, 2] {
    return a
}
";

/// The fixtures are written in the English surface.
fn analyze_en(source: &str) -> AnalyzedFixture {
    let mut config = crate::PassConfig::for_target(radix_target::Target::HirRust);
    config.locale_pack = Some(
        crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
            .expect("load English locale pack"),
    );
    analyze_source_with_config(source, config)
}

/// `body` runs in `main` after `g` (a tensor) and `fresh` (an unrelated one).
fn program(body: &str) -> String {
    format!(
        r"{PRELUDE}
main {{
    const list<f32> flat ← [1.0, 2.0, 3.0, 4.0]
    var tf32[2, 2] g ← flat ↦ tf32[2, 2]
    var tf32[2, 2] fresh ← flat ↦ tf32[2, 2]
{body}
}}
"
    )
}

fn errors(body: &str) -> Vec<(SemanticErrorKind, String)> {
    analyze_en(&program(body))
        .result
        .diagnostics
        .iter()
        .filter(|d| d.is_error())
        .map(|d| (d.kind.clone(), issue(d).unwrap_or_default().to_string()))
        .collect()
}

fn assert_accepts(body: &str) {
    let found = errors(body);
    assert!(found.is_empty(), "expected no error, got: {found:?}");
}

fn assert_rejects(body: &str, expected: &str) {
    let found = errors(body);
    assert!(
        found.iter().any(|(_, issue)| issue == expected),
        "expected issue {expected:?}, got: {found:?}"
    );
}

// =============================================================================
// Rejected: an alias spelling overlaps like the direct spelling
// =============================================================================

#[test]
fn direct_spelling_of_the_same_overlap_is_rejected() {
    assert_rejects("    mut_read(mut g, g)", "use_while_mutably_borrowed");
    assert_rejects("    mut_mut(mut g, mut g)", "mutable_borrow_conflict");
}

#[test]
fn mut_then_aliased_read_argument_is_rejected() {
    assert_rejects(
        "    var tf32[2, 2] alias ← g\n    mut_read(mut g, alias)",
        "use_while_mutably_borrowed",
    );
}

#[test]
fn aliased_mut_then_read_argument_is_rejected() {
    assert_rejects(
        "    var tf32[2, 2] alias ← g\n    mut_read(mut alias, g)",
        "use_while_mutably_borrowed",
    );
}

#[test]
fn mut_then_aliased_ref_argument_is_a_shared_borrow_conflict() {
    assert_rejects(
        "    var tf32[2, 2] alias ← g\n    mut_ref(mut g, alias)",
        "shared_borrow_conflict",
    );
}

#[test]
fn aliased_mut_then_ref_argument_is_a_shared_borrow_conflict() {
    assert_rejects(
        "    var tf32[2, 2] alias ← g\n    mut_ref(mut alias, g)",
        "shared_borrow_conflict",
    );
}

#[test]
fn two_mut_arguments_through_an_alias_are_rejected() {
    assert_rejects(
        "    var tf32[2, 2] alias ← g\n    mut_mut(mut g, mut alias)",
        "mutable_borrow_conflict",
    );
    assert_rejects(
        "    var tf32[2, 2] alias ← g\n    mut_mut(mut alias, mut g)",
        "mutable_borrow_conflict",
    );
}

#[test]
fn two_aliases_of_one_tensor_overlap() {
    assert_rejects(
        "    var tf32[2, 2] left ← g\n    var tf32[2, 2] right ← g\n    mut_mut(mut left, mut right)",
        "mutable_borrow_conflict",
    );
}

#[test]
fn alias_of_an_alias_overlaps_transitively() {
    assert_rejects(
        "    var tf32[2, 2] alias ← g\n    var tf32[2, 2] again ← alias\n    mut_mut(mut g, mut again)",
        "mutable_borrow_conflict",
    );
    assert_rejects(
        "    var tf32[2, 2] alias ← g\n    var tf32[2, 2] again ← alias\n    mut_read(mut again, g)",
        "use_while_mutably_borrowed",
    );
}

#[test]
fn view_of_an_alias_passed_with_the_original_mut_is_rejected() {
    assert_rejects(
        "    var tf32[2, 2] alias ← g\n    mut_read(mut g, alias.transpose())",
        "tensor_view_read_only",
    );
}

#[test]
fn view_of_the_original_passed_with_an_aliased_mut_is_rejected() {
    assert_rejects(
        "    var tf32[2, 2] alias ← g\n    mut_read(mut alias, g.transpose())",
        "tensor_view_read_only",
    );
}

#[test]
fn stored_view_passed_with_an_alias_of_its_origin_mut_is_rejected() {
    assert_rejects(
        "    var tf32[2, 2] alias ← g\n    const tf32[2, 2] v ← g.transpose()\n    mut_read(mut alias, v)",
        "tensor_view_read_only",
    );
}

// =============================================================================
// Accepted: no overlap, or overlap that is only read/read
// =============================================================================

#[test]
fn aliased_read_arguments_stay_legal() {
    assert_accepts("    var tf32[2, 2] alias ← g\n    read_read(g, alias)");
    assert_accepts("    var tf32[2, 2] alias ← g\n    read_read(alias, g)");
}

#[test]
fn aliased_ref_arguments_stay_legal() {
    let found = errors(
        "    var tf32[2, 2] alias ← g\n    const ref tf32[2, 2] r ← g\n    read_read(alias, r)",
    );
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn copy_bind_does_not_alias() {
    assert_accepts("    var tf32[2, 2] dup ← copy g\n    mut_read(mut g, dup)");
    assert_accepts("    var tf32[2, 2] dup ← copy g\n    mut_read(mut dup, g)");
    assert_accepts("    var tf32[2, 2] dup ← copy g\n    mut_mut(mut g, mut dup)");
}

#[test]
fn own_bind_does_not_alias() {
    let found = errors("    const tf32[2, 2] taken ← own g\n    mut_mut(mut g, mut taken)");
    assert!(
        !found.iter().any(|(_, issue)| matches!(
            issue.as_str(),
            "mutable_borrow_conflict" | "shared_borrow_conflict" | "use_while_mutably_borrowed"
        )),
        "{found:?}"
    );
}

#[test]
fn unrelated_tensor_is_not_an_overlap() {
    assert_accepts("    mut_read(mut g, fresh)");
    assert_accepts("    mut_mut(mut g, mut fresh)");
    assert_accepts("    var tf32[2, 2] alias ← g\n    mut_mut(mut alias, mut fresh)");
    assert_accepts("    var tf32[2, 2] alias ← g\n    mut_mut(mut fresh, mut alias)");
}

#[test]
fn alias_of_another_tensor_does_not_overlap_this_one() {
    assert_accepts("    var tf32[2, 2] alias ← fresh\n    mut_read(mut g, alias)");
    assert_accepts("    var tf32[2, 2] alias ← fresh\n    mut_mut(mut alias, mut g)");
}

#[test]
fn sequential_calls_release_call_scoped_borrows() {
    assert_accepts(
        "    var tf32[2, 2] alias ← g\n    mut_read(mut g, fresh)\n    mut_read(mut alias, fresh)\n    mut_mut(mut fresh, mut g)\n    mut_read(mut fresh, alias)",
    );
}

// =============================================================================
// Flow: an unconditional rebind detaches; a conditional rebind may-aliases
// =============================================================================

#[test]
fn source_rebind_detaches_the_stale_alias() {
    assert_accepts(
        "    var tf32[2, 2] alias ← g\n    g ← fresh\n    mut_read(mut g, alias)\n    mut_mut(mut alias, mut g)",
    );
}

#[test]
fn source_rebind_keeps_the_other_aliases_of_the_old_storage() {
    assert_rejects(
        "    var tf32[2, 2] left ← g\n    var tf32[2, 2] right ← g\n    g ← fresh\n    mut_mut(mut left, mut right)",
        "mutable_borrow_conflict",
    );
}

#[test]
fn conditional_alias_rebind_keeps_the_may_alias() {
    assert_rejects(
        "    var tf32[2, 2] alias ← g\n    if true {\n        alias ← fresh\n    }\n    mut_read(mut g, alias)",
        "use_while_mutably_borrowed",
    );
}

#[test]
fn conditional_source_rebind_keeps_the_may_alias() {
    assert_rejects(
        "    var tf32[2, 2] alias ← g\n    if true {\n        g ← fresh\n    }\n    mut_read(mut g, alias)",
        "use_while_mutably_borrowed",
    );
}

#[test]
fn unconditional_alias_rebind_clears_the_alias() {
    assert_accepts("    var tf32[2, 2] alias ← g\n    alias ← fresh\n    mut_read(mut g, alias)");
}

// =============================================================================
// Call results: a result that can return an argument aliases its storage
// =============================================================================

#[test]
fn call_result_aliases_the_returned_argument() {
    assert_rejects(
        "    var tf32[2, 2] viaid ← id(g)\n    mut_ref(mut g, viaid)",
        "shared_borrow_conflict",
    );
    assert_rejects(
        "    var tf32[2, 2] viaid ← id(g)\n    mut_read(mut viaid, g)",
        "use_while_mutably_borrowed",
    );
}

#[test]
fn call_result_alias_chains_through_a_bind_alias() {
    assert_rejects(
        "    var tf32[2, 2] alias ← g\n    var tf32[2, 2] viaid ← id(alias)\n    mut_ref(mut g, viaid)",
        "shared_borrow_conflict",
    );
}

#[test]
fn call_result_of_a_copied_argument_does_not_alias() {
    assert_accepts("    var tf32[2, 2] viaid ← id_copy(copy g)\n    mut_ref(mut g, viaid)");
}

#[test]
fn call_result_of_a_copied_bind_does_not_alias() {
    assert_accepts(
        "    var tf32[2, 2] dup ← copy g\n    var tf32[2, 2] viaid ← id(dup)\n    mut_ref(mut g, viaid)",
    );
}

#[test]
fn call_result_of_an_unrelated_tensor_does_not_overlap() {
    assert_accepts("    var tf32[2, 2] viaid ← id(fresh)\n    mut_ref(mut g, viaid)");
    assert_accepts("    var tf32[2, 2] viaid ← id(flat ↦ tf32[2, 2])\n    mut_ref(mut g, viaid)");
}

#[test]
fn call_result_alias_is_replaced_by_a_rebind() {
    assert_accepts(
        "    var tf32[2, 2] viaid ← id(g)\n    viaid ← fresh\n    mut_ref(mut g, viaid)",
    );
}

// =============================================================================
// Container elements: an element of a literal aliases the places it lists
// =============================================================================

#[test]
fn container_element_aliases_the_listed_place() {
    assert_rejects(
        "    var list<tf32[2, 2]> xs ← [g]\n    mut_ref(mut g, xs[0])",
        "shared_borrow_conflict",
    );
    assert_rejects(
        "    var list<tf32[2, 2]> xs ← [g]\n    mut_read(mut g, xs[0])",
        "use_while_mutably_borrowed",
    );
    assert_rejects(
        "    var list<tf32[2, 2]> xs ← [g]\n    mut_read(mut xs[0], g)",
        "use_while_mutably_borrowed",
    );
}

#[test]
fn container_element_matches_the_direct_spelling() {
    assert_rejects("    mut_ref(mut g, g)", "shared_borrow_conflict");
    assert_rejects(
        "    var list<tf32[2, 2]> xs ← [g]\n    mut_ref(mut g, xs[0])",
        "shared_borrow_conflict",
    );
}

#[test]
fn container_element_of_any_index_may_alias_any_listed_place() {
    assert_rejects(
        "    var list<tf32[2, 2]> xs ← [fresh, g]\n    mut_ref(mut g, xs[0])",
        "shared_borrow_conflict",
    );
}

#[test]
fn container_element_chains_through_a_bind_alias() {
    assert_rejects(
        "    var tf32[2, 2] alias ← g\n    var list<tf32[2, 2]> xs ← [alias]\n    mut_ref(mut g, xs[0])",
        "shared_borrow_conflict",
    );
}

#[test]
fn nested_container_element_aliases_the_listed_place() {
    assert_rejects(
        "    var list<list<tf32[2, 2]>> xs ← [[g]]\n    mut_ref(mut g, xs[0][0])",
        "shared_borrow_conflict",
    );
}

#[test]
fn container_element_survives_a_bind_of_the_container() {
    assert_rejects(
        "    var list<tf32[2, 2]> xs ← [g]\n    var list<tf32[2, 2]> ys ← xs\n    mut_ref(mut g, ys[0])",
        "shared_borrow_conflict",
    );
}

#[test]
fn container_element_of_an_unrelated_tensor_does_not_overlap() {
    assert_accepts("    var list<tf32[2, 2]> xs ← [fresh]\n    mut_ref(mut g, xs[0])");
}

#[test]
fn container_element_read_read_stays_legal() {
    assert_accepts("    var list<tf32[2, 2]> xs ← [g]\n    read_read(g, xs[0])");
}

#[test]
fn container_rebind_replaces_the_element_aliases() {
    assert_accepts(
        "    var list<tf32[2, 2]> xs ← [g]\n    xs ← [fresh]\n    mut_ref(mut g, xs[0])",
    );
}

#[test]
fn container_element_names_the_old_storage_after_a_source_rebind() {
    assert_accepts("    var list<tf32[2, 2]> xs ← [g]\n    g ← fresh\n    mut_ref(mut g, xs[0])");
}

// =============================================================================
// User method calls: the receiver's methods obey the same overlap rule
// =============================================================================

const METHOD_PRELUDE: &str = r"class Kern {
    fn mut_read(mut tf32[2, 2] a, tf32[2, 2] b) → void {
        a ⇇ b
    }

    fn mut_ref(mut tf32[2, 2] a, ref tf32[2, 2] b) → void {
        a ⇇ b
    }

    fn mut_mut(mut tf32[2, 2] a, mut tf32[2, 2] b) → void {
        a ⇇ b
    }

    fn read_read(tf32[2, 2] a, tf32[2, 2] b) → void {
        print a.flatten()
        print b.flatten()
    }
}
";

/// `body` runs in `main` after `g`, `fresh` and a `Kern` receiver `k`.
fn method_program(body: &str) -> String {
    format!(
        r"{METHOD_PRELUDE}
main {{
    const list<f32> flat ← [1.0, 2.0, 3.0, 4.0]
    var tf32[2, 2] g ← flat ↦ tf32[2, 2]
    var tf32[2, 2] fresh ← flat ↦ tf32[2, 2]
    const Kern k ← Kern {{}}
{body}
}}
"
    )
}

fn method_errors(body: &str) -> Vec<(SemanticErrorKind, String)> {
    analyze_en(&method_program(body))
        .result
        .diagnostics
        .iter()
        .filter(|d| d.is_error())
        .map(|d| (d.kind.clone(), issue(d).unwrap_or_default().to_string()))
        .collect()
}

fn assert_method_accepts(body: &str) {
    let found = method_errors(body);
    assert!(found.is_empty(), "expected no error, got: {found:?}");
}

fn assert_method_rejects(body: &str, expected: &str) {
    let found = method_errors(body);
    assert!(
        found.iter().any(|(_, issue)| issue == expected),
        "expected issue {expected:?}, got: {found:?}"
    );
}

#[test]
fn method_call_with_the_same_tensor_mut_and_read_is_rejected() {
    assert_method_rejects("    k.mut_read(mut g, g)", "use_while_mutably_borrowed");
}

#[test]
fn method_call_with_the_same_tensor_in_two_mut_arguments_is_rejected() {
    assert_method_rejects("    k.mut_mut(mut g, mut g)", "mutable_borrow_conflict");
}

#[test]
fn method_call_with_the_same_tensor_mut_and_ref_is_rejected() {
    assert_method_rejects("    k.mut_ref(mut g, g)", "shared_borrow_conflict");
}

#[test]
fn method_call_overlap_through_an_alias_is_rejected() {
    assert_method_rejects(
        "    var tf32[2, 2] alias ← g\n    k.mut_read(mut g, alias)",
        "use_while_mutably_borrowed",
    );
    assert_method_rejects(
        "    var tf32[2, 2] alias ← g\n    k.mut_read(mut alias, g)",
        "use_while_mutably_borrowed",
    );
    assert_method_rejects(
        "    var tf32[2, 2] alias ← g\n    k.mut_mut(mut g, mut alias)",
        "mutable_borrow_conflict",
    );
}

#[test]
fn method_call_view_of_the_mut_argument_is_rejected() {
    assert_method_rejects(
        "    k.mut_read(mut g, g.transpose())",
        "tensor_view_read_only",
    );
}

#[test]
fn method_call_without_overlap_stays_legal() {
    assert_method_accepts("    k.mut_read(mut g, fresh)");
    assert_method_accepts("    k.mut_mut(mut g, mut fresh)");
    assert_method_accepts("    k.read_read(g, g)");
    assert_method_accepts("    var tf32[2, 2] dup ← copy g\n    k.mut_read(mut g, dup)");
}

#[test]
fn sequential_method_calls_release_call_scoped_borrows() {
    assert_method_accepts("    k.mut_read(mut g, fresh)\n    k.mut_read(mut g, fresh)");
    assert_method_accepts(
        "    var tf32[2, 2] alias ← g\n    k.mut_read(mut g, fresh)\n    k.mut_read(mut alias, fresh)",
    );
}

#[test]
fn method_call_overlap_through_a_container_element_is_rejected() {
    assert_method_rejects(
        "    var list<tf32[2, 2]> xs ← [g]\n    k.mut_ref(mut g, xs[0])",
        "shared_borrow_conflict",
    );
}

// =============================================================================
// Imported method calls (R02): a receiver whose class arrives through a file
// interface is held to its parameter modes like a locally declared class
// =============================================================================

/// The library surface an importing package sees: a `Mutator` class whose
/// `read` takes `mut list<int> a` and a plain `list<int> b`.
fn imported_mutator_interface() -> crate::file_interface::FileInterface {
    use crate::file_interface::{
        FileExport, FileExportKind, FileInterface, InterfaceCallable, InterfaceMethodExport,
        InterfaceParam, InterfaceStructExport, InterfaceTypeSnapshot,
    };
    use crate::{Primitive, SemanticMutability, SemanticParamMode};

    // Locally, `mut list<int> a` lowers the marker into the parameter type
    // (`Type::Ref(Mut, …)`); an imported snapshot carries the same shape.
    let list_int = || {
        InterfaceTypeSnapshot::Array(Box::new(InterfaceTypeSnapshot::Primitive(
            Primitive::Numerus,
        )))
    };
    let mut_list_int = || {
        InterfaceTypeSnapshot::Ref(SemanticMutability::Mutable, Box::new(list_int()))
    };
    let read = InterfaceMethodExport {
        name: "read".to_owned(),
        callable: InterfaceCallable {
            type_params: Vec::new(),
            tensor_edge_policy_requirements: Vec::new(),
            params: vec![
                InterfaceParam {
                    ty: mut_list_int(),
                    mode: SemanticParamMode::MutRef,
                    optional: false,
                    sponte: false,
                },
                InterfaceParam {
                    ty: list_int(),
                    mode: SemanticParamMode::Owned,
                    optional: false,
                    sponte: false,
                },
            ],
            ret: Box::new(InterfaceTypeSnapshot::Primitive(Primitive::Vacuum)),
            err: None,
            is_async: false,
            is_generator: false,
            device: None,
        },
        visibility: radix_syntax::MemberVisibility::Publica,
        unavailable: Vec::new(),
    };
    let mut interface = FileInterface::new();
    interface.insert(FileExport {
        name: "Mutator".to_owned(),
        kind: FileExportKind::Struct(InterfaceStructExport {
            is_json: false,
            implements: Vec::new(),
            name: "Mutator".to_owned(),
            params: Vec::new(),
            fields: Vec::new(),
            methods: vec![read],
            annotation_contract: None,
        }),
    });
    interface
}

/// The fixtures are written in the English surface; `library` is the imported
/// namespace (the exported surface of another file in the package).
fn analyze_imported_en(source: &str) -> AnalyzedFixture {
    use crate::passes::visibility_annotation::VisibilityTier;
    let mut config = crate::PassConfig::for_target(radix_target::Target::HirRust);
    config.locale_pack = Some(
        crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
            .expect("load English locale pack"),
    );
    config.namespace_exports = std::collections::BTreeMap::from([(
        "library".to_owned(),
        std::collections::BTreeMap::from([("Mutator".to_owned(), VisibilityTier::Publica)]),
    )]);
    config.file_interfaces =
        std::collections::BTreeMap::from([("library".to_owned(), imported_mutator_interface())]);
    analyze_source_with_config(source, config)
}

/// `body` runs in `main` after `xs` (the overlapped list), the unrelated `ys`,
/// and the imported `Mutator` receiver `m`.
fn imported_program(body: &str) -> String {
    format!(
        r#"import from "./library" library

main {{
    var list<int> xs ← [1]
    var list<int> ys ← [2]
    const library.Mutator m ← library.Mutator {{}}
{body}
}}
"#
    )
}

fn imported_errors(source: &str) -> Vec<(SemanticErrorKind, String)> {
    analyze_imported_en(source)
        .result
        .diagnostics
        .iter()
        .filter(|d| d.is_error())
        .map(|d| (d.kind.clone(), issue(d).unwrap_or_default().to_string()))
        .collect()
}

#[test]
fn imported_method_same_storage_overlap_is_rejected() {
    let found = imported_errors(&imported_program("    m.read(mut xs, xs)"));
    assert!(
        found
            .iter()
            .any(|(kind, issue)| *kind == SemanticErrorKind::MutableBorrowConflict
                && issue == "use_while_mutably_borrowed"),
        "expected SEM052 use_while_mutably_borrowed, got: {found:?}"
    );
}

#[test]
fn imported_method_alias_spelled_overlap_is_rejected() {
    let found = imported_errors(&imported_program(
        "    var list<int> alias ← xs\n    m.read(mut xs, alias)",
    ));
    assert!(
        found
            .iter()
            .any(|(kind, issue)| *kind == SemanticErrorKind::MutableBorrowConflict
                && issue == "use_while_mutably_borrowed"),
        "expected SEM052 use_while_mutably_borrowed, got: {found:?}"
    );
}

#[test]
fn imported_method_disjoint_storage_call_is_accepted() {
    let found = imported_errors(&imported_program("    m.read(mut xs, ys)"));
    assert!(found.is_empty(), "expected no error, got: {found:?}");
}
