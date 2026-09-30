//! Emission pins for the Rust, Swift and Go backends: signed saturating `/`
//! and `%` floor and trap with the runner's text (ruling 14), `≈` unscales a
//! decimal operand (ruling 22), and `xs[i]↑` is a single-evaluation checked
//! element store. The corpus goldens prove the behavior; these name the shape.

use crate::codegen::Target;
use crate::driver::{Config, ModuleCompileRequest, Session};

const SATURATING: &str = r#"+++
locale = "en"
+++
main {
    const saturating<i8> a ← -7
    const saturating<i8> two ← 2
    print a / two
    print a % two
}
"#;

const FUZZY: &str = r#"+++
locale = "en"
+++
main {
    const int<d64> d ← 0.1
    const float f ← 0.1
    print d ≈ f
}
"#;

const INC_DEC: &str = r#"+++
locale = "en"
+++
fn idx() → int {
    return 1
}
main {
    var list<int<u8>> xs ← [1, 2]
    xs[idx()] ↑
    print xs
}
"#;

fn emit(target: Target, source: &str) -> String {
    let session = Session::new(Config::default().with_target(target));
    let result = crate::driver::compile(&session, "probe.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "{target} compilation failed: {:?}", result.diagnostics);
    match result.output.expect("output") {
        crate::Output::Rust(out) => out.code,
        crate::Output::Go(out) => out.code,
        crate::Output::Swift(out) => out.code,
        _ => panic!("unexpected output for {target}"),
    }
}

#[test]
fn rust_saturating_division_floors_and_traps_with_runner_text() {
    let code = emit(Target::HirRust, SATURATING);
    assert!(code.contains("panic!(\"saturatus division failed\")"), "{code}");
    assert!(code.contains("let q = l / r; let m = l % r;"), "{code}");
    assert!(code.contains("{ m + r }"), "{code}");
}

#[test]
fn swift_saturating_division_floors() {
    let code = emit(Target::HirSwift, SATURATING);
    assert!(code.contains("var q = l / r; let m = l % r;"), "{code}");
    assert!(code.contains("m += r"), "{code}");
    assert!(!code.contains("dividedReportingOverflow"), "{code}");
}

#[test]
fn go_saturating_division_by_zero_uses_runner_text() {
    let code = emit(Target::HirGo, SATURATING);
    assert!(
        code.contains("if right == 0 { panic(\"saturatus division failed\") }"),
        "{code}"
    );
}

#[test]
fn decimal_fuzzy_operand_is_unscaled_on_every_backend() {
    let rust = emit(Target::HirRust, FUZZY);
    assert!(rust.contains("as f64 / 1000000000.0_f64"), "{rust}");
    let swift = emit(Target::HirSwift, FUZZY);
    assert!(swift.contains(") / 1000000000.0)"), "{swift}");
    let go = emit(Target::HirGo, FUZZY);
    assert!(go.contains(") / 1000000000.0"), "{go}");
}

#[test]
fn rust_list_element_inc_dec_is_one_checked_store() {
    let code = emit(Target::HirRust, INC_DEC);
    assert!(
        code.contains("__faber_store::<u8>((*") && code.contains(" as i128) + 1, \"assignment\", \"\")"),
        "{code}"
    );
    assert!(!code.contains(").cloned().expect(\"lista index trap\") += 1"), "{code}");
}

#[test]
fn swift_list_element_inc_dec_binds_the_index_once() {
    let code = emit(Target::HirSwift, INC_DEC);
    assert!(code.contains("let _fabidx = idx();"), "{code}");
    assert!(code.contains("xs[_fabidx] = faberXStore(Int128(xs[_fabidx]) + 1"), "{code}");
}
