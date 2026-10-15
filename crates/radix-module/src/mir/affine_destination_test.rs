//! A call to a generic affine function stores into a declared tensor type.
//!
//! The checker types such a call's result with an expression-origin runtime
//! axis at the store boundary while the callee instance carries the concrete
//! extent. The validator accepts that pair (the store's runtime shape check
//! owns the equality) and still refuses any other destination mismatch.

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, run_source};

fn session() -> Session {
    Session::new(Config::default().with_target(Target::HirTypeScript))
}

fn run(source: &str) -> Vec<String> {
    let source = format!("+++\nlocale = \"en\"\n+++\n{source}");
    let mut host = BufferHost::default();
    run_source(&session(), "affine-destination.fab", &source, &mut host)
        .unwrap_or_else(|error| panic!("affine destination source must run: {error:?}"));
    host.stdout_lines
}

#[test]
fn static_affine_decrement_result_stores_into_declared_tensor() {
    let source = r"
fn drop_one<size N>(tensor<f32, [N - 1]> value) → tensor<f32, [N - 1]> {
    return value
}
main {
    let list<f32> values ← [1.0, 2.0, 3.0]
    let tensor<f32, [3]> three ← values ↦ tensor<f32, [3]>
    let tensor<f32, [3]> tail ← drop_one(three)
    print tail.shape()[0]
}
";
    assert_eq!(run(source), ["3"]);
}

#[test]
fn dynamic_affine_decrement_result_stores_into_declared_tensor() {
    let source = r"
fn drop_one<size N>(tensor<f32, [N - 1]> value) → tensor<f32, [N - 1]> {
    return value
}
fn runtime_size(int value) → int {
    return value
}
main {
    let list<f32> values ← [1.0, 2.0, 3.0]
    let int n ← runtime_size(4)
    let tensor<f32, [size (n - 1)]> dynamic ← values ↦ tensor<f32, [size (n - 1)]>
    let tensor<f32, [size (n - 1)]> dynamic_tail ← drop_one(dynamic)
    print dynamic_tail.shape()[0]
}
";
    assert_eq!(run(source), ["3"]);
}

#[test]
fn affine_increment_result_stores_into_declared_tensor() {
    let source = r"
fn grow<size N>(tensor<f32, [N + 1]> value) → tensor<f32, [N + 1]> {
    return value
}
main {
    let list<f32> values ← [1.0, 2.0, 3.0, 4.0]
    let tensor<f32, [4]> four ← values ↦ tensor<f32, [4]>
    let tensor<f32, [4]> grown ← grow(four)
    print grown.shape()[0]
}
";
    assert_eq!(run(source), ["4"]);
}

#[test]
fn plain_generic_result_stores_into_declared_tensor() {
    let source = r"
fn ident<size N>(tensor<f32, [N]> value) → tensor<f32, [N]> {
    return value
}
main {
    let list<f32> values ← [1.0, 2.0, 3.0, 4.0]
    let tensor<f32, [4]> four ← values ↦ tensor<f32, [4]>
    let tensor<f32, [4]> same ← ident(four)
    print same.shape()[0]
}
";
    assert_eq!(run(source), ["4"]);
}
