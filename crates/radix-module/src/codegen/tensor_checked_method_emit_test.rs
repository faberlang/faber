//! The failable tensor method forms `t.accipe(idx)` / `t.ponde(idx, v)` emit
//! the checked carrier call on Rust, TypeScript and Go: an unhandled in-range
//! call is a plain value, a `⇥ textus` function propagates the text error, a
//! `fac`/`cape` handler receives it, and `accipe ⊥ default` yields the default.

use crate::codegen::Target;
use crate::{Compiler, Config, Output};

const SOURCE: &str = r"
functio lege(tensor<f32, [2, 2]> grid) → f32 {
    sit f32 got ← grid.accipe([0, 1])
    redde got
}

functio dynamica(tensor<f32, [2, 2]> grid, lista<numerus> idx) → f32 ⇥ textus {
    sit f32 got ← grid.accipe(idx)
    redde got
}

functio pone(varia tensor<f32, [2, 2]> grid, lista<numerus> idx, f32 v) → vacuum ⇥ textus {
    grid.ponde(idx, v)
}

functio praefinita(tensor<f32, [2, 2]> grid, lista<numerus> idx) → f32 {
    sit f32 got ← grid.accipe(idx) ⊥ 7.0
    redde got
}

functio cepit(varia tensor<f32, [2, 2]> grid, lista<numerus> idx) → f32 {
    varia f32 r ← 0.0
    fac {
        r ← grid.accipe(idx)
    } cape err {
        nota err
    }
    redde r
}

incipit {
}
";

fn emit(target: Target) -> String {
    let result = Compiler::new(Config::default().with_latin_locale().with_target(target))
        .compile_str("tensor_checked.fab", SOURCE);
    match result.output {
        Some(Output::Rust(out)) => out.code,
        Some(Output::TypeScript(out)) => out.code,
        Some(Output::Go(out)) => out.code,
        _ => panic!("{target:?}: no output: {:?}", result.diagnostics),
    }
}

#[cfg(feature = "hir-rust")]
#[test]
fn rust_tensor_method_forms_use_the_checked_carrier_call() {
    let code = emit(Target::HirRust);
    assert!(code.contains(".accipe_checked("), "{code}");
    assert!(
        code.contains("map_err(|_| String::from(\"tensor accipe invalid index\"))?"),
        "a `⇥ textus` function propagates the accipe error:\n{code}"
    );
    assert!(
        code.contains("map_err(|_| String::from(\"tensor ponde invalid index\"))?"),
        "a `⇥ textus` function propagates the ponde error:\n{code}"
    );
    assert!(code.contains("Err(_) => 7.0"), "the default is the error arm:\n{code}");
}

#[cfg(feature = "hir-ts")]
#[test]
fn typescript_tensor_method_forms_use_the_checked_carrier_call() {
    let code = emit(Target::HirTypeScript);
    assert!(
        code.contains("p_grid.accipeChecked([0, 1])"),
        "an in-range literal call is unwrapped to a plain value:\n{code}"
    );
    assert!(
        code.contains("throw { __faberFail: true, error: r.error }; } return r.value; })(p_grid.accipeChecked(p_idx))"),
        "a `⇥ textus` function propagates the accipe error:\n{code}"
    );
    assert!(code.contains("p_grid.pondeChecked(p_idx, p_v)"), "{code}");
    assert!(
        code.contains("r.ok ? r.value : d())(p_grid.accipeChecked(p_idx), () => 7)"),
        "the default is lazy and used only on failure:\n{code}"
    );
    assert!(
        !code.contains("grid.accipe(") && !code.contains("grid.ponde("),
        "no unchecked method-form call is left:\n{code}"
    );
}

#[cfg(feature = "hir-go")]
#[test]
fn go_tensor_method_forms_use_the_checked_carrier_call() {
    let code = emit(Target::HirGo);
    assert!(
        // The capacity-2 `[0, 1]` literal fills its carrier exactly, so the
        // exact-fill carrier law (a literal whose length equals its capacity
        // emits the compact composite literal — pinned by radix-hir-go
        // `bounded_list_test`) passes the plain `[]int{0, 1}` instead of the
        // reserve-cells accumulator closure.
        code.contains("v, err := p_grid.AccipeChecked([]int{0, 1}); if err != nil { panic(err) }; return v"),
        "an in-range literal call is a plain value:\n{code}"
    );
    assert!(
        code.contains("l_got, err := p_grid.AccipeChecked(p_idx)\n    if err != nil {\n        return 0, err\n    }"),
        "a `⇥ textus` function propagates the accipe error:\n{code}"
    );
    assert!(code.contains("(*p_grid).PondeChecked(p_idx, p_v)"), "{code}");
    assert!(
        code.contains("if err != nil { return 7.0 }"),
        "the default is returned only on failure:\n{code}"
    );
}
