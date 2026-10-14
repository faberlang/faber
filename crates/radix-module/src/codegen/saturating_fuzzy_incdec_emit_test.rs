//! Emission pins for the Rust, Swift and Go backends: `/` and `%` over
//! `saturating` operands are exact floor division that traps on a zero divisor
//! with the integer text (rulings 3, 14; only a store into a `saturating`
//! slot clamps), `≈` unscales a
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
    const d64 d ← 0.1
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
    var list<u8> xs ← [1, 2]
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

#[cfg(feature = "hir-rust")]
#[test]
fn rust_saturating_division_is_exact_floor_division() {
    let code = emit(Target::HirRust, SATURATING);
    assert!(code.contains("faber::exact::div(l_a as i128, l_two as i128)"), "{code}");
    assert!(code.contains("faber::exact::rem(l_a as i128, l_two as i128)"), "{code}");
    assert!(!code.contains("saturatus division failed"), "{code}");
}

#[cfg(feature = "hir-swift")]
#[test]
fn swift_saturating_division_is_exact_floor_division() {
    let code = emit(Target::HirSwift, SATURATING);
    assert!(code.contains("Swift.print(faberXDiv(Int128(l_a), Int128(l_two)))"), "{code}");
    assert!(code.contains("Swift.print(faberXMod(Int128(l_a), Int128(l_two)))"), "{code}");
    assert!(!code.contains("dividedReportingOverflow"), "{code}");
}

#[cfg(feature = "hir-go")]
#[test]
fn go_saturating_division_by_zero_uses_the_integer_text() {
    let code = emit(Target::HirGo, SATURATING);
    assert!(
        code.contains("rt.ExactDiv(rt.ExactI(int64(l_a)), rt.ExactI(int64(l_two)))"),
        "{code}"
    );
    // The integer trap text lives in `rt.ExactDiv` (`runtime/go/exact_test.go`).
    assert!(!code.contains("saturatus division failed"), "{code}");
}

#[cfg(all(feature = "hir-rust", feature = "hir-swift", feature = "hir-go"))]
#[test]
fn decimal_fuzzy_operand_is_unscaled_on_every_backend() {
    let rust = emit(Target::HirRust, FUZZY);
    assert!(rust.contains("faber::dec::f64(faber::dec::of("), "{rust}");
    let swift = emit(Target::HirSwift, FUZZY);
    assert!(swift.contains("faberDecF64(faberDecOf("), "{swift}");
    let go = emit(Target::HirGo, FUZZY);
    assert!(go.contains("rt.DecF64(rt.DecOf("), "{go}");
}

#[cfg(feature = "hir-rust")]
#[test]
fn rust_list_element_inc_dec_is_one_checked_store() {
    let code = emit(Target::HirRust, INC_DEC);
    assert!(
        code.contains("faber::exact::store::<u8>((*") && code.contains(" as i128) + 1, \"assignment\")"),
        "{code}"
    );
    assert!(!code.contains(").cloned(), faber::Trap::ListaIndex) += 1"), "{code}");
}

#[cfg(feature = "hir-swift")]
#[test]
fn swift_list_element_inc_dec_binds_the_index_once() {
    let code = emit(Target::HirSwift, INC_DEC);
    assert!(code.contains("let _fabidx = idx();"), "{code}");
    assert!(code.contains("l_xs[_fabidx] = faberXStore(Int128(l_xs[_fabidx]) + 1"), "{code}");
}
