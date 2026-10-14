//! NS-TYPEARG-LINK: the namespace-call rewrite forwards call-site type
//! arguments onto the rewritten direct call (PAF-NS-002 burn-down).
//!
//! A size-generic function exported by a sibling package file must link and
//! instantiate through the package-MIR namespace-call rewrite both when the
//! caller spells the sizes (`utilModule.id<2>(t)`) and when the checker
//! infers them from the argument (`utilModule.id(t)`). Inferred call sites
//! arrive with empty HIR `type_args` — the semantic lowering writes them
//! only for spelled `<...>` arguments — so one forward-on-rewrite covers
//! both forms.

use super::*;
use radix_module::semantic::{IndexExpr, Type};

const TYPEARG_UTIL: &str = r"@ public
fn id<size N>(tensor<f32, [N]> x) → tensor<f32, [N]> {
    return x
}
";

const TYPEARG_EXPLICIT_MAIN: &str = r#"import from "./util" * as utilModule

fn run() → f32 {
    const tensor<f32, [2]> t ← [1.5, 2.5] ↦ tensor<f32, [2]>
    const tensor<f32, [2]> r ← utilModule.id<2>(t)
    return r[1]
}

main {
    print run()
}
"#;

const TYPEARG_INFERRED_MAIN: &str = r#"import from "./util" * as utilModule

fn run() → f32 {
    const tensor<f32, [3]> t ← [1.5, 2.5, 3.5] ↦ tensor<f32, [3]>
    const tensor<f32, [3]> r ← utilModule.id(t)
    return r[2]
}

main {
    print run()
}
"#;

const TYPEARG_FORWARD_MAIN: &str = r#"import from "./util" * as utilModule

fn forward<size N>(tensor<f32, [N]> x) → tensor<f32, [N]> {
    return utilModule.id<N>(x)
}

fn run() → f32 {
    const tensor<f32, [3]> a ← [1.5, 2.5, 3.5] ↦ tensor<f32, [3]>
    const tensor<f32, [5]> b ← [1.0, 2.0, 3.0, 4.0, 5.0] ↦ tensor<f32, [5]>
    const tensor<f32, [3]> ra ← forward<3>(a)
    const tensor<f32, [5]> rb ← forward<5>(b)
    const tensor<f32, [3]> rc ← ra ↦ tensor<f32, [3]>
    return rc[1]
}

main {
    print run()
}
"#;

fn write_typearg_package(dir: &std::path::Path, main_source: &str) -> std::path::PathBuf {
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create package src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "ns-typearg-link"
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
    fs::write(src.join("util.fab"), TYPEARG_UTIL).expect("write util.fab");
    fs::write(src.join("main.fab"), main_source).expect("write main.fab");
    src.join("main.fab")
}

// The failable sibling shape: a generic export whose error half is a
// declared union (the fault type of `norma:fs.read_file<T>`), with a bare
// `↦ T` conversion body. The synthetic namespace callee must carry the
// export signature's error half so the call lowers as a try-call.
const TYPEARG_FAILABLE_UTIL: &str = r"@ public
union Fault {
    Broken { string path }
}

@ public
@ radix type T mut string list<string> bytes
fn read<T>(string path) → T ⇥ Fault {
    return path ↦ T
}
";

const TYPEARG_FAILABLE_MAIN: &str = r#"import from "./util" * as utilModule

fn run() → string ⇥ utilModule.Fault {
    return utilModule.read<string>("x")
}

main {
    print "ok"
}
"#;

fn write_failable_typearg_package(dir: &std::path::Path) -> std::path::PathBuf {
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create package src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "ns-typearg-failable"
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
    fs::write(src.join("util.fab"), TYPEARG_FAILABLE_UTIL).expect("write util.fab");
    fs::write(src.join("main.fab"), TYPEARG_FAILABLE_MAIN).expect("write main.fab");
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

fn first_param_shape(lowered: &LoweredMirUnit<'_>, function_name: &str) -> Option<Vec<i64>> {
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
        .next()
}

fn named_param_shapes(lowered: &LoweredMirUnit<'_>, function_name: &str) -> Vec<Vec<i64>> {
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
        .collect()
}

#[test]
fn package_mir_links_explicit_type_arg_namespace_call() {
    let dir = crate::test_support::test_temp_dir("ns-typearg-explicit");
    let entry = write_typearg_package(dir.as_ref(), TYPEARG_EXPLICIT_MAIN);
    let config = Config::default();

    with_interpreted_lowered_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        |lowered| {
            assert_eq!(
                first_param_shape(lowered, "id"),
                Some(vec![2]),
                "explicit `utilModule.id<2>` must instantiate the sibling generic at [2]"
            );
        },
    )
    .unwrap_or_else(|errors| {
        panic!(
            "explicit type-arg namespace call must link, got {:?}",
            errors
        )
    });
}

#[test]
fn package_mir_links_generic_failable_namespace_call_with_error_type() {
    // The `norma:fs.read_file<T>` shape: a generic sibling export with a
    // declared union error type. The synthetic callee must carry the export
    // signature's error half, or the try-call edge (and the error channel)
    // is lost in package MIR.
    let dir = crate::test_support::test_temp_dir("ns-typearg-failable");
    let entry = write_failable_typearg_package(dir.as_ref());
    let config = Config::default();

    with_interpreted_lowered_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        |_lowered| {},
    )
    .unwrap_or_else(|errors| {
        panic!(
            "generic failable namespace call with a declared error type must link, got {:?}",
            errors.iter().map(|diag| diag.issue()).collect::<Vec<_>>()
        )
    });
}

#[test]
fn package_mir_links_inferred_type_arg_namespace_call() {
    // Inferred sites write no HIR type args; the checker unifies N from the
    // argument. The rewrite must carry them through the same empty-args
    // path without the retired explicit-only rejection.
    let dir = crate::test_support::test_temp_dir("ns-typearg-inferred");
    let entry = write_typearg_package(dir.as_ref(), TYPEARG_INFERRED_MAIN);
    let config = Config::default();

    with_interpreted_lowered_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        |lowered| {
            assert_eq!(
                first_param_shape(lowered, "id"),
                Some(vec![3]),
                "inferred `utilModule.id(t)` must instantiate the sibling generic at [3]"
            );
        },
    )
    .unwrap_or_else(|errors| {
        panic!(
            "inferred type-arg namespace call must link, got {:?}",
            errors
        )
    });
}

#[test]
fn package_mir_links_generic_caller_forwarding_type_args_to_namespace_callee() {
    // GSF-3: the generic caller spells the callee sizes with its own symbols
    // (`utilModule.id<N>`). The namespace rewrite plus package monomorphization
    // must instantiate the imported `id` at EVERY forwarded concrete size, not
    // merely keep the caller's own static signature consistent.
    let dir = crate::test_support::test_temp_dir("ns-typearg-forward");
    let entry = write_typearg_package(dir.as_ref(), TYPEARG_FORWARD_MAIN);
    let config = Config::default();

    with_interpreted_lowered_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        |lowered| {
            let id_shapes = named_param_shapes(lowered, "id");
            for size in [3i64, 5] {
                assert!(
                    id_shapes.iter().any(|shape| shape == &vec![size]),
                    "imported `utilModule.id` forwarded from `forward<{size}>` must \
                     instantiate at the concrete [{size}] signature, got {id_shapes:?}"
                );
            }
            let forward_shapes = named_param_shapes(lowered, "forward");
            for size in [3i64, 5] {
                assert!(
                    forward_shapes.iter().any(|shape| shape == &vec![size]),
                    "`forward<{size}>` must instantiate at [{size}], got {forward_shapes:?}"
                );
            }
        },
    )
    .unwrap_or_else(|errors| {
        panic!(
            "generic namespace forwarding package must link and run, got {:?}",
            errors
        );
    });
}

const AIR_TYPEARG_UTIL: &str = r#"@ radix lane "air"
@ public
fn rope<size D>(tf32[2, D] x, tf32[2, 4, 3] table) → tf32[2, D] {
    return x.rope_norm<D>(0, table)
}
"#;

const AIR_TYPEARG_MAIN: &str = r#"import from "./util" * as utilModule

fn main() → void {
    const tf32[2, 8] x ← [[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0], [9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0]] ↦ tf32[2, 8]
    const list<f32> y_flat ← [1.0, 2.0, 3.0, 4.0]
    const tf32[2, 4, 3] table ← empty
    do {
        const tf32[2, 64] y ← y_flat ↦ tf32[2, 64]
        const _ a ← utilModule.rope<8>(x, table)
        const _ b ← utilModule.rope<64>(y, table)
        const _ c ← utilModule.rope(y, table)
    }
    catch err {
    }
}
"#;

fn write_air_typearg_package(dir: &std::path::Path) -> std::path::PathBuf {
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create package src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "air-typearg-link"
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
    fs::write(src.join("util.fab"), AIR_TYPEARG_UTIL).expect("write util.fab");
    fs::write(src.join("main.fab"), AIR_TYPEARG_MAIN).expect("write main.fab");
    src.join("main.fab")
}

#[test]
fn package_mir_specializes_imported_air_rope_before_lowering() {
    let dir = crate::test_support::test_temp_dir("air-typearg-link");
    let entry = write_air_typearg_package(dir.as_ref());

    with_interpreted_lowered_package_mir(
        &Config::default(),
        &entry,
        &crate::test_support::test_mir_seams(),
        |lowered| {
            let mut shapes = named_param_shapes(lowered, "rope");
            shapes.sort();
            assert_eq!(
                shapes,
                vec![vec![2, 8], vec![2, 64]],
                "imported AIR rope must lower one concrete provider instance per distinct dimension"
            );
            assert_eq!(
                lowered
                    .generic_templates
                    .iter()
                    .filter(|template| template
                        .name
                        .is_some_and(|name| lowered.interner.resolve(name) == "rope"))
                    .count(),
                0,
                "a generic AIR provider template must not enter executable MIR"
            );
        },
    )
    .unwrap_or_else(|errors| panic!("imported AIR rope package must lower, got {errors:?}"));
}

const FAILABLE_NAMESPACE_UTIL: &str = r"@ public
fn pick<T>(T x) → T ⇥ string {
    return x
}
";

const FAILABLE_NAMESPACE_MAIN: &str = r#"import from "./util" * as utilModule

fn main() → void {
    do {
        print utilModule.pick<string>("x")
    }
    catch err {
        print "caught"
    }
}
"#;

fn write_failable_namespace_package(dir: &std::path::Path) -> std::path::PathBuf {
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create package src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "ns-failable-link"
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
    fs::write(src.join("util.fab"), FAILABLE_NAMESPACE_UTIL).expect("write util.fab");
    fs::write(src.join("main.fab"), FAILABLE_NAMESPACE_MAIN).expect("write main.fab");
    src.join("main.fab")
}

/// A failable generic sibling export (`pick<T> → T ⇥ string`) is called from
/// `main` with a spelled type argument. The namespace rewrite plus the
/// synthetic-callee type recovery must keep the error half so the call lowers
/// as a try-call edge in `main`, not as an ordinary call.
#[test]
fn package_mir_links_generic_failable_namespace_call() {
    let dir = crate::test_support::test_temp_dir("ns-typearg-failable");
    let entry = write_failable_namespace_package(dir.as_ref());
    let config = Config::default();

    with_interpreted_lowered_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        |lowered| {
            let main = lowered
                .validated
                .program()
                .functions
                .iter()
                .find(|function| {
                    function
                        .name
                        .is_some_and(|name| lowered.interner.resolve(name) == "main")
                })
                .unwrap_or_else(|| panic!("`main` must exist in the merged program"));
            assert!(
                main.blocks.iter().any(|block| matches!(
                    block.terminator.kind,
                    MirTerminatorKind::TryCall { .. }
                )),
                "failable namespace call must lower to a try-call edge in main"
            );
        },
    )
    .unwrap_or_else(|errors| {
        panic!(
            "failable type-arg namespace call must link, got {:?}",
            errors
        )
    });
}

const NORMA_FS_READ_FILE_MAIN: &str = r#"import from "norma:fs" fs

main {
    print fs.read_file<string>("/definitely/not/here")
}
"#;

const NORMA_FS_READ_FILE_OPERAND_MAIN: &str = r#"import from "norma:fs" fs

@ cli "tool"
@ operand string root
main args args {
    print fs.read_file<string>(args.root)
}
"#;

const NORMA_FS_READ_FILE_LITERAL_IN_CLI_MAIN: &str = r#"import from "norma:fs" fs

@ cli "tool"
@ operand string root
main args args {
    print fs.read_file<string>("/definitely/not/here")
}
"#;

const NORMA_FS_READ_FILE_CLI_OPERAND_PRINT_ONLY_MAIN: &str = r#"import from "norma:fs" fs

@ cli "tool"
@ operand string root
main args args {
    print args.root
}
"#;

/// `norma:fs.read_file<string>` is a failable generic library export
/// (`T ⇥ fault.IoFault`, with the union interned from the library's own
/// second-level `norma:fault` import). Called from a package `main` with a
/// spelled type argument, the package must link and lower with no
/// link/typecheck diagnostic.
#[test]
fn package_mir_links_norma_fs_read_file_generic_failable() {
    let dir = crate::test_support::test_temp_dir("norma-fs-read-file");
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create package src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "norma-fs-read-file"
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
    fs::write(src.join("main.fab"), NORMA_FS_READ_FILE_MAIN).expect("write main.fab");
    let entry = src.join("main.fab");

    let result = with_lowered_package_mir(
        &Config::default(),
        &entry,
        &crate::test_support::test_mir_seams(),
        |_| {},
    );
    if let Err(diagnostics) = &result {
        let facts = diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code, diagnostic.issue()))
            .collect::<Vec<_>>();
        panic!("norma:fs read_file package must lower, got {facts:?}");
    }
}

/// The same failable generic library export must link and validate when the
/// argument is a CLI-planned operand field (`args.root`) rather than a
/// literal.
#[test]
fn package_mir_links_norma_fs_read_file_operand_arg_validates() {
    let dir = crate::test_support::test_temp_dir("norma-fs-read-file-operand");
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create package src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "norma-fs-read-file-operand"
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
    fs::write(src.join("main.fab"), NORMA_FS_READ_FILE_OPERAND_MAIN).expect("write main.fab");
    let entry = src.join("main.fab");

    // `with_lowered_package_mir` supplies no argumenta, so a required
    // `@ operand` never receives a value and the CLI plan fails before
    // lowering (`package_mir_cli_surface_unsupported`, surface "CLI operand
    // value"). Lower with the operand value supplied — the `cli_plan_test`
    // call form — to reach the lowering/validation gate this test probes.
    let argumenta = vec!["/definitely/not/here".to_owned()];
    let result = with_prepared_package_mir(
        &Config::default(),
        &entry,
        &argumenta,
        &crate::test_support::test_mir_seams(),
        |_, _| Ok(()),
    );
    if let Err(diagnostics) = &result {
        let facts = diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code, diagnostic.issue()))
            .collect::<Vec<_>>();
        panic!("norma:fs read_file operand package must lower, got {facts:?}");
    }
}

/// A CLI `main` whose `@ operand` is declared but whose body calls
/// `norma:fs.read_file<string>` with a literal must link and validate. The
/// sibling operand probe isolates the `Struct` operand; this probe isolates
/// the call path in a CLI-programmed main (P1).
#[test]
fn package_mir_links_norma_fs_read_file_literal_in_cli_main_validates() {
    let dir = crate::test_support::test_temp_dir("norma-fs-read-file-literal-in-cli-main");
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create package src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "norma-fs-read-file-literal-in-cli-main"
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
    fs::write(src.join("main.fab"), NORMA_FS_READ_FILE_LITERAL_IN_CLI_MAIN)
        .expect("write main.fab");
    let entry = src.join("main.fab");

    let argumenta = vec!["/definitely/not/here".to_owned()];
    let result = with_prepared_package_mir(
        &Config::default(),
        &entry,
        &argumenta,
        &crate::test_support::test_mir_seams(),
        |_, _| Ok(()),
    );
    if let Err(diagnostics) = &result {
        let facts = diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code, diagnostic.issue()))
            .collect::<Vec<_>>();
        panic!(
            "norma:fs read_file literal-in-cli-main package must lower, got {facts:?}\n{diagnostics:#?}"
        );
    }
}

/// A CLI `main` whose declared `@ operand string root` is only printed must
/// link and validate. This probe narrows whether the `Struct` operand reaches
/// the `textus` call parameter or is already mistyped at the operand surface
/// (P2).
#[test]
fn package_mir_cli_operand_print_only_validates() {
    let dir = crate::test_support::test_temp_dir("norma-fs-read-file-cli-operand-print-only");
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create package src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "norma-fs-read-file-cli-operand-print-only"
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
    fs::write(
        src.join("main.fab"),
        NORMA_FS_READ_FILE_CLI_OPERAND_PRINT_ONLY_MAIN,
    )
    .expect("write main.fab");
    let entry = src.join("main.fab");

    let argumenta = vec!["/definitely/not/here".to_owned()];
    let result = with_prepared_package_mir(
        &Config::default(),
        &entry,
        &argumenta,
        &crate::test_support::test_mir_seams(),
        |_, _| Ok(()),
    );
    if let Err(diagnostics) = &result {
        let facts = diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code, diagnostic.issue()))
            .collect::<Vec<_>>();
        panic!(
            "norma:fs cli operand print-only package must lower, got {facts:?}\n{diagnostics:#?}"
        );
    }
}

fn write_norma_fs_read_file_package(dir: &std::path::Path) -> std::path::PathBuf {
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create package src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "norma-fs-read-file"
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
    fs::write(src.join("main.fab"), NORMA_FS_READ_FILE_MAIN).expect("write main.fab");
    src.join("main.fab")
}

/// The `norma:fs.read_file<string>` call in a package `main` is a failable
/// generic library call, so the interpreted lowering of `main` must keep the
/// error half and carry a try-call terminator, not a plain call.
#[test]
fn package_mir_norma_fs_read_file_main_is_try_call() {
    let dir = crate::test_support::test_temp_dir("norma-fs-read-file-try-call");
    let entry = write_norma_fs_read_file_package(dir.as_ref());
    let config = Config::default();

    with_interpreted_lowered_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        |lowered| {
            // The `main {}` entry lowers to the source-less, nameless entry
            // function (backends spell it `incipit`), so locate it by the same
            // shape `MirNames` uses rather than by a `main` symbol.
            let names = radix_mir::MirNames::new(
                lowered.validated.program(),
                lowered.validated.validation().types.as_table(),
                &lowered.interner,
            );
            let main = names
                .explicit_entry_function()
                .unwrap_or_else(|| panic!("`main` must exist in the merged program"));
            assert!(
                main.blocks.iter().any(|block| matches!(
                    block.terminator.kind,
                    MirTerminatorKind::TryCall { .. }
                )),
                "norma:fs read_file call in main must lower to a try-call terminator"
            );
        },
    )
    .unwrap_or_else(|errors| panic!("norma:fs read_file package must lower, got {errors:?}"));
}

/// After instantiation with `T = string`, the instantiated `norma:fs`
/// `read_file` body must reach a conversion of its `solum:lege` result to
/// the interned `textus` primitive, not leave a generic `Type::Param`
/// conversion target.
///
/// A bare failable `sermo ↦ T` lowers through a synthetic conversion helper
/// (so its failure row applies), so the conversion statement lives in the
/// helper the instantiated `read_file` calls, not in its own blocks. The
/// probe (`docs/audits/pumpfix-review1-f5-c-probe.md`) shows the one
/// `read_file` instance is already `T = textus` and its `TryCall` callee
/// carries the `Convert` whose target is the interned `textus`.
#[test]
#[ignore = "pumpfix-gfd594086: read_file<string> body convert target not substituted"]
fn package_mir_norma_fs_read_file_string_body_converts_to_textus() {
    let dir = crate::test_support::test_temp_dir("norma-fs-read-file-string-body");
    let entry = write_norma_fs_read_file_package(dir.as_ref());
    let config = Config::default();

    with_interpreted_lowered_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        |lowered| {
            let program = lowered.validated.program();
            let types = lowered.validated.validation().types.as_table();
            let textus = types.primitive(Primitive::Textus);

            let read_file = program
                .functions
                .iter()
                .find(|function| {
                    function
                        .name
                        .is_some_and(|name| lowered.interner.resolve(name) == "read_file")
                })
                .unwrap_or_else(|| {
                    panic!("instantiated `read_file` must exist in the merged program")
                });
            // The `↦ T` conversion is emitted in the synthetic helper the
            // failable `read_file` body calls, so inspect that callee: its
            // `Convert` is the instantiated result conversion the
            // `read_file` body itself no longer carries.
            let read_file = read_file
                .blocks
                .iter()
                .find_map(|block| match &block.terminator.kind {
                    MirTerminatorKind::TryCall {
                        callee: MirCallee::Function(callee),
                        ..
                    } => program.functions.iter().find(|function| function.id == *callee),
                    _ => None,
                })
                .unwrap_or_else(|| {
                    panic!("instantiated `read_file` must call its conversion helper")
                });
            assert!(
                read_file
                    .blocks
                    .iter()
                    .any(|block| block.statements.iter().any(|statement| matches!(
                        &statement.kind,
                        MirStatementKind::RuntimeCall { call, .. }
                            if matches!(
                                &call.intrinsic,
                                MirIntrinsic::Convert(conversion)
                                    if conversion.target_ty.semantic_id() == textus
                            )
                    ))),
                "instantiated read_file must convert its result to the interned textus type"
            );
        },
    )
    .unwrap_or_else(|errors| panic!("norma:fs read_file package must lower, got {errors:?}"));
}
