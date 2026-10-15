//! MONO-FIELD-ASSIGN: a size-instantiated generic struct's tensor fields
//! assign to the same-shape fields of an imported generic nominal
//! (dense-typed-assembly unit-5 seam, operator need).
//!
//! The seam runs across three package roles, mirroring gradus
//! `model/dense.fab` → `transformer.fab`:
//!   * `util.fab` — sibling library exporting the size-generic class and
//!     generic functions (the `TypedDenseLayer` role),
//!   * `model.fab` — a *library module* caller that instantiates a generic
//!     producer with literal sizes, projects its fields, and initializes the
//!     imported nominal from the monomorphized source fields,
//!   * `main.fab` — entry consuming the seam.
//!
//! Locked here (each red before the fix):
//!   * the semantic checker substitutes an imported generic class's size
//!     params into field types at an applied initializer
//!     (`collect_imported_file_structs` carried but dropped the param names),
//!   * the package merge rewrites `MirCallee::Definition` type args and
//!     re-interns `Type::Param`/`IndexExpr::Param` symbols by name — raw
//!     source-table ids aliased unrelated merged types,
//!   * package generic instantiation finalizes `Convert` payloads with the
//!     instance's shape bindings (unit-monomorphizer parity),
//!   * package generic instantiation types projected operands
//!     (`layer.ln1`) through merged struct metadata instead of binding the
//!     callee against the container nominal.
//!
//! Negatives keep the structured SEM010 posture: same-shape passes, wrong
//! shapes still reject.

use super::*;
use radix_module::semantic::{IndexExpr, Type};

const SEAM_UTIL: &str = r"@ public
class Box<size D, size Q> {
    mut tensor<f32, [D]> ln1
    mut tensor<f32, [D, Q]> wq
}

@ public
fn make<size D, size Q>(tensor<f32, [D]> ln1, tensor<f32, [D, Q]> wq) → Box<D, Q> {
    let Box<D, Q> b ← Box { ln1 = ln1, wq = wq }
    return b
}

# `layer.ln1` projects an applied nominal inside the instantiated body and
# `↦ list<f32>` is a Convert carrying pre-substitution payload types.
# Expand the projected row to x's shape before exact-shape addition.
@ public
fn axis0_cells<size T, size D, size Q>(tensor<f32, [T, D]> x, Box<D, Q> layer) → int {
    let tensor<f32, [T, D]> y ← x + layer.ln1.expanded(x.shape())
    let list<f32> data ← y ↦ list<f32>
    return data.length()
}

@ public
fn width_of<size A, size B>(tensor<f32, [A, B]> t) → int {
    let list<f32> data ← t ↦ list<f32>
    return data.length()
}
";

const SEAM_MODEL: &str = r#"import from "./util" util

fn flat(int n) → list<f32> {
    mut list<f32> out ← empty
    for range 0‥n let i {
        out.append(1.0)
    }
    return out
}

@ public
fn typed_seam() → int {
    do {
        let tensor<f32, [4]> ln1 ← flat(4) ↦ tensor<f32, [4]>
        let tensor<f32, [4, 4]> wq ← flat(16) ↦ tensor<f32, [4, 4]>
        let util.Box<4, 4> made ← util.make<4, 4>(ln1, wq)
        let list<util.Box<4, 4>> layers ← [made]
        let tensor<f32, [2, 4]> x ← flat(8) ↦ tensor<f32, [2, 4]>
        mut int total ← 0
        for from layers let layer {
            let util.Box<4, 4> typed ← util.Box { ln1 = layer.ln1, wq = layer.wq }
            total ← util.axis0_cells<2, 4, 4>(x, typed)
            total ← total + util.width_of(typed.wq)
        }
        return total
    }
    catch err {
        return 0
    }
}
"#;

const SEAM_MODEL_WRONG_FIELD: &str = r#"import from "./util" util

fn flat(int n) → list<f32> {
    mut list<f32> out ← empty
    for range 0‥n let i {
        out.append(1.0)
    }
    return out
}

@ public
fn typed_seam_wrong_field() → int {
    do {
        let tensor<f32, [8]> too_wide ← flat(8) ↦ tensor<f32, [8]>
        let tensor<f32, [4, 4]> wq ← flat(16) ↦ tensor<f32, [4, 4]>
        let util.Box<4, 4> typed ← util.Box { ln1 = too_wide, wq = wq }
        let list<f32> data ← typed.ln1 ↦ list<f32>
        return data.length()
    }
    catch err {
        return 0
    }
}
"#;

const SEAM_MODEL_WRONG_SIZE_ARG: &str = r#"import from "./util" util

fn flat(int n) → list<f32> {
    mut list<f32> out ← empty
    for range 0‥n let i {
        out.append(1.0)
    }
    return out
}

@ public
fn typed_seam_wrong_size_arg() → int {
    do {
        let tensor<f32, [4]> ln1 ← flat(4) ↦ tensor<f32, [4]>
        let tensor<f32, [4, 4]> wq ← flat(16) ↦ tensor<f32, [4, 4]>
        let util.Box<4, 4> made ← util.make<4, 4>(ln1, wq)
        let tensor<f32, [2, 4]> x ← flat(8) ↦ tensor<f32, [2, 4]>
        return util.axis0_cells<2, 8, 4>(x, made)
    }
    catch err {
        return 0
    }
}
"#;

const SEAM_MAIN: &str = r#"import from "./model" model

fn run() → int {
    return model.typed_seam()
}

main {
    print run()
}
"#;

const SEAM_MAIN_WRONG_FIELD: &str = r#"import from "./model" model

fn run() → int {
    return model.typed_seam_wrong_field()
}

main {
    print run()
}
"#;

const SEAM_MAIN_WRONG_SIZE_ARG: &str = r#"import from "./model" model

fn run() → int {
    return model.typed_seam_wrong_size_arg()
}

main {
    print run()
}
"#;

fn write_seam_package(
    dir: &std::path::Path,
    model_source: &str,
    main_source: &str,
) -> std::path::PathBuf {
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create package src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "mono-field-assign"
version = "1.0.0"
edition = "2026"

[paths]
source = "src"
entry = "main.fab"

[build]
kind = "bin"
"#,
    )
    .expect("write faber.toml");
    fs::write(src.join("util.fab"), SEAM_UTIL).expect("write util.fab");
    fs::write(src.join("model.fab"), model_source).expect("write model.fab");
    fs::write(src.join("main.fab"), main_source).expect("write main.fab");
    src.join("main.fab")
}

fn literal_dims(
    types: &radix_types::TypeTable,
    shape: radix_module::semantic::IndexId,
) -> Option<Vec<i64>> {
    match types.get_index(shape) {
        IndexExpr::Literal(value) => Some(vec![i64::try_from(*value).ok()?]),
        IndexExpr::Tuple(dims) => dims
            .iter()
            .map(|dim| match types.get_index(*dim) {
                IndexExpr::Literal(value) => i64::try_from(*value).ok(),
                _ => None,
            })
            .collect(),
        _ => None,
    }
}

fn instantiated_param_shape(lowered: &LoweredMirUnit<'_>, function_name: &str) -> Option<Vec<i64>> {
    let types = lowered.validated.validation().types.as_table();
    lowered
        .validated
        .program()
        .functions
        .iter()
        .filter(|function| {
            function
                .name
                .is_some_and(|name| lowered.interner.resolve(name) == function_name)
        })
        .filter_map(|function| {
            let param = function.params.first()?;
            match types.get(param.ty.semantic_id()) {
                Type::Tensor(_, shape) => literal_dims(types, *shape),
                _ => None,
            }
        })
        .find(|dims| dims == &vec![2, 4])
}

#[test]
fn package_links_monomorphized_field_assign_to_imported_nominal() {
    let dir = crate::test_support::test_temp_dir("mono-field-assign");
    let entry = write_seam_package(dir.as_ref(), SEAM_MODEL, SEAM_MAIN);
    let config = Config::default();

    with_interpreted_lowered_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        |lowered| {
            assert_eq!(
                instantiated_param_shape(lowered, "axis0_cells"),
                Some(vec![2, 4]),
                "`util.axis0_cells<2, 4, 4>` from a library module must instantiate at [2, 4]"
            );
        },
    )
    .unwrap_or_else(|errors| {
        panic!(
            "monomorphized field-assign seam must link, got {:?}",
            errors
        );
    });
}

#[test]
fn wrong_shape_initializer_on_imported_nominal_keeps_sem010() {
    let dir = crate::test_support::test_temp_dir("mono-field-assign-neg-field");
    let entry = write_seam_package(dir.as_ref(), SEAM_MODEL_WRONG_FIELD, SEAM_MAIN_WRONG_FIELD);
    let config = Config::default();
    let diagnostics =
        crate::analyze::check_package(&config, &entry, &crate::test_support::test_analysis_seams());
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.issue() == Some("field_initializer_type_mismatch")),
        "wrong-width `ln1` on `util.Box<4, 4>` must keep the structured SEM010 rejection, got {:?}",
        diagnostics
            .iter()
            .map(|diag| diag.issue())
            .collect::<Vec<_>>()
    );
}

#[test]
fn wrong_explicit_size_arg_on_namespace_generic_keeps_rejection() {
    let dir = crate::test_support::test_temp_dir("mono-field-assign-neg-size");
    let entry = write_seam_package(
        dir.as_ref(),
        SEAM_MODEL_WRONG_SIZE_ARG,
        SEAM_MAIN_WRONG_SIZE_ARG,
    );
    let config = Config::default();
    let diagnostics =
        crate::analyze::check_package(&config, &entry, &crate::test_support::test_analysis_seams());
    assert!(
        !diagnostics.is_empty(),
        "`util.axis0_cells<2, 8, 4>` on a [2, 4] tensor must reject"
    );
}
