//! METHOD-CALL-MIR: method calls on linked library nominals with nested
//! enum/aggregate metadata must rewrite to synthetic-path calls and validate.
//!
//! Gradus `cache.proba` / `decode.proba` call `t.get(...)` and
//! `c.key().shape()` on `gradus:tensor` carriers whose fields and error
//! channel are nested library enums (`dtype.DType`, `TensorError`). Pre-fix
//! those receivers either stayed `MethodCall` (runtime/provider fallthrough)
//! or lowered with source-local `DefIds`, so package MIR died on
//! "method call before runtime/provider MIR lowering" or missing
//! enum/aggregate metadata.

use super::*;
use radix_mir_runner::BufferHost;

const DTYPE_LIB: &str = r#"+++
locale = "en"
+++
@ public { }
union DType {
    F32 { },
    I32 { }
}

@ public { }
fn f32() → DType {
    return variant F32 {}
}
"#;

const SHAPE_LIB: &str = r#"+++
locale = "en"
+++
@ public { }
union ShapeError {
    Negative {
        string message
    }
}

@ public { }
fn numel(list<int> shape) → int ⇥ ShapeError {
    mut int product ← 1
    for from shape let dim {
        require dim ≥ 0 throw variant Negative { message = "neg" }
        product ← product * dim
    }
    return product
}

@ public { }
fn message(ShapeError err) → string {
    match err {
        case Negative let message {
            return message
        }
    }
}
"#;

const TENSOR_LIB: &str = r#"+++
locale = "en"
+++
import from "dtypelib:dtype" dtype
import from "shapelib:shape" shape

@ public { }
union TensorError {
    OutOfBounds {
        string message
    },
    InvalidShape {
        string message
    }
}

@ public { }
class Tensor {
    mut dtype.DType dtype
    mut list<f32> data
    mut list<int> shape

    fn shape() → list<int> {
        return self.shape
    }

    fn get(list<int> indices) → f32 ⇥ TensorError {
        require indices.length() ≥ 1 throw variant OutOfBounds { message = "empty" }
        let int i ← indices.get(0) coalesce 0
        return self.data.get(i) coalesce 0.0
    }
}

@ private { }
fn _numel_shape(list<int> shape) → int ⇥ TensorError {
    do {
        return shape.numel(shape)
    }
    catch err {
        let string c ← shape.message(err)
        throw variant InvalidShape { message = c }
    }
}

@ public { }
fn construct(list<f32> data, list<int> shape) → Tensor ⇥ TensorError {
    let int product ← _numel_shape(shape)
    require data.length() ≡ product throw variant OutOfBounds { message = "mismatch" }
    return Tensor { dtype = dtype.f32(), data = data, shape = shape }
}

@ public { }
class Cache {
    mut Tensor key

    fn key() → Tensor {
        return self.key
    }
}

@ public { }
fn sample() → Cache {
    mut Tensor t ← Tensor { dtype = dtype.f32(), data = [], shape = [] }
    do {
        t ← construct([1.0, 2.0, 3.0, 4.0], [1, 4])
    }
    catch err {
        return Cache { key = t }
    }
    return Cache { key = t }
}
"#;

const TENSOR_MAIN: &str = r#"+++
locale = "en"
+++
import from "tensorlib:tensor" tensor

main {
    let tensor.Cache c ← tensor.sample()
    print c.key().shape().length()
    do {
        print c.key().get([0])
    }
    catch err {
        print "caught"
    }
}
"#;

const TENSOR_PROBA: &str = r#"+++
locale = "en"
+++
import from "tensorlib:tensor" tensor

fn prove_nested_tensor_methods() {
    let tensor.Cache c ← tensor.sample()
    print c.key().shape().length()
    do {
        print c.key().get([0])
    }
    catch err {
        print "caught"
    }
}
"#;

fn write_nested_tensor_package(dir: &std::path::Path) -> std::path::PathBuf {
    let src = dir.join("src");
    let libhome = dir.join("libhome");
    let dtype = libhome.join("dtypelib");
    let shape = libhome.join("shapelib");
    let tensor = libhome.join("tensorlib");
    fs::create_dir_all(&src).expect("create package src");
    fs::create_dir_all(dtype.join("src")).expect("create dtype lib src");
    fs::create_dir_all(shape.join("src")).expect("create shape lib src");
    fs::create_dir_all(tensor.join("src")).expect("create tensor lib src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "method-call-mir"
version = "1.0.0"
edition = "2026"

[paths]
source = "src"
entry = "main.fab"

[build]
target = "fmir"
kind = "bin"

[dependencies]
dtypelib = "1.0.0"
shapelib = "1.0.0"
tensorlib = "1.0.0"
"#,
    )
    .expect("write faber.toml");
    let dtype_display = dtype.display();
    let shape_display = shape.display();
    let tensor_display = tensor.display();
    let lock = format!(
        "[[package]]\nname = \"dtypelib\"\nversion = \"1.0.0\"\nsource = \"path:{dtype_display}\"\npackage_root = \"{dtype_display}\"\nkind = \"source\"\ntarget_language = \"\"\ntarget_triple = \"\"\ntarget_manifest = \"\"\ninterface_root = \"{dtype_display}/src\"\nartifact = \"\"\ncrate = \"\"\nrustc = \"\"\n[[package]]\nname = \"shapelib\"\nversion = \"1.0.0\"\nsource = \"path:{shape_display}\"\npackage_root = \"{shape_display}\"\nkind = \"source\"\ntarget_language = \"\"\ntarget_triple = \"\"\ntarget_manifest = \"\"\ninterface_root = \"{shape_display}/src\"\nartifact = \"\"\ncrate = \"\"\nrustc = \"\"\n[[package]]\nname = \"tensorlib\"\nversion = \"1.0.0\"\nsource = \"path:{tensor_display}\"\npackage_root = \"{tensor_display}\"\nkind = \"source\"\ntarget_language = \"\"\ntarget_triple = \"\"\ntarget_manifest = \"\"\ninterface_root = \"{tensor_display}/src\"\nartifact = \"\"\ncrate = \"\"\nrustc = \"\"\n"
    );
    fs::write(dir.join("faber.lock"), lock).expect("write faber.lock");
    fs::write(dtype.join("src/dtype.fab"), DTYPE_LIB).expect("write dtype library");
    fs::write(shape.join("src/shape.fab"), SHAPE_LIB).expect("write shape library");
    fs::write(tensor.join("src/tensor.fab"), TENSOR_LIB).expect("write tensor library");
    fs::write(src.join("main.fab"), TENSOR_MAIN).expect("write main.fab");
    fs::write(src.join("tensor.proba"), TENSOR_PROBA).expect("write tensor.proba");
    src.join("main.fab")
}

fn assert_nested_tensor_output(output: Vec<String>) {
    assert_eq!(
        output,
        vec!["2".to_owned(), "1.0".to_owned()],
        "key().shape().length() is 2 and get([0]) reads the first f32 cell"
    );
}

#[test]
fn package_mir_nested_library_method_calls_execute() {
    let dir = crate::test_support::test_temp_dir("method-call-mir-run");
    let entry = write_nested_tensor_package(dir.as_ref());
    let config = Config::default().with_library_home(dir.join("libhome"));

    let mut host = BufferHost::default();
    run_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        &mut host,
    )
    .unwrap_or_else(|diagnostics| panic!("nested tensor method calls must link: {diagnostics:?}"));
    assert_nested_tensor_output(host.stdout_lines);
}

fn write_self_provider_tensor_package(dir: &std::path::Path) -> std::path::PathBuf {
    let package_root = dir.join("libhome").join("tensorlib");
    let src = package_root.join("src");
    fs::create_dir_all(&src).expect("create self-provider src");
    fs::write(
        package_root.join("faber.toml"),
        r#"
[package]
name = "tensorlib"
version = "1.0.0"
edition = "2026"

[library]
provider = "tensorlib"

[paths]
source = "src"

[build]
kind = "lib"
targets = ["fmir"]

[locale]
locale = "en"
"#,
    )
    .expect("write self-provider faber.toml");
    fs::write(src.join("dtype.fab"), DTYPE_LIB).expect("write dtype.fab");
    fs::write(src.join("shape.fab"), SHAPE_LIB).expect("write shape.fab");
    fs::write(
        src.join("tensor.fab"),
        TENSOR_LIB
            .replace("dtypelib:dtype", "tensorlib:dtype")
            .replace("shapelib:shape", "tensorlib:shape"),
    )
    .expect("write tensor.fab");
    fs::write(src.join("tensor.proba"), TENSOR_PROBA).expect("write tensor.proba");
    src.join("tensor.proba")
}

#[test]
fn package_test_self_provider_nested_tensor_methods_execute() {
    // Gradus cache.proba topology: the package tests itself through
    // `provider:module` specifiers, and method calls on sibling nominals
    // (`t.get`, `c.key().shape()`) carry nested enum/aggregate metadata from
    // other siblings (`dtype.DType`, `shape.numel`).
    let dir = crate::test_support::test_temp_dir("method-call-mir-self");
    let proba_path = write_self_provider_tensor_package(dir.as_ref());
    let package_root = proba_path.parent().unwrap().parent().unwrap();
    let config = Config::default().with_library_home(dir.join("libhome"));
    let mut package = crate::analyze::analyze_package_for_tests(
        &config,
        package_root,
        None,
        &crate::test_support::test_analysis_seams(),
    )
    .expect("analyze self-provider nested tensor package tests");
    let (root_id, _) = package
        .node_by_path(&proba_path)
        .expect("tensor.proba is an analyzed node");
    let root_id = root_id.clone();
    let included = package.imports.local_closure(std::iter::once(&root_id));
    package.nodes.retain(|id, _| included.contains(id));
    package.entry = Some(root_id.clone());
    package.roots = vec![root_id];
    let mut functions = Vec::new();
    for unit in package.nodes.values() {
        let is_proba = unit
            .path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext == "proba");
        if !is_proba {
            continue;
        }
        for item in &unit.analysis.hir.items {
            if let HirItemKind::Function(function) = &item.kind
                && unit.analysis.interner.resolve(function.name) == "prove_nested_tensor_methods"
            {
                functions.push((unit.path.clone(), item.def_id));
            }
        }
    }
    assert_eq!(
        functions.len(),
        1,
        "expected one prove_nested_tensor_methods case"
    );
    let scope = TestPackageScope::for_program(&package, functions);
    let output = with_linked_test_package_mir(
        package,
        &scope,
        &config,
        &crate::test_support::test_mir_seams(),
        |validated, function_ids| {
            let mut host = BufferHost::default();
            for function_id in function_ids.values() {
                radix_mir_runner::run_function(validated, *function_id, &mut host)
                    .expect("self-provider nested tensor methods must execute");
            }
            host.stdout_lines
        },
    )
    .unwrap_or_else(|diagnostics| {
        panic!("self-provider nested tensor methods must lower: {diagnostics:?}",)
    });
    assert_nested_tensor_output(output);
}

const COLOR_LIB: &str = r#"+++
locale = "en"
+++
@ public { }
union Color {
    Red {
        int shade
    },
    Blue { }
}

@ public { }
fn red(int shade) → Color {
    return variant Red { shade = shade }
}

@ public { }
fn shade(Color c) → int {
    match c {
        case Red let shade {
            return shade
        }
        case Blue {
            return 0
        }
    }
}
"#;

const TONE_LIB: &str = r#"+++
locale = "en"
+++
@ public { }
union Tone {
    Dark {
        int shade
    },
    Light { }
}

@ public { }
fn dark(int shade) → Tone {
    return variant Dark { shade = shade }
}

@ public { }
fn shade(Tone t) → int {
    match t {
        case Dark let shade {
            return shade
        }
        case Light {
            return 0
        }
    }
}
"#;

const UNION_PROBA: &str = r#"+++
locale = "en"
+++
import from "unionlib:color" color
import from "unionlib:tone" tone

fn prove_sibling_union_payloads() {
    print color.shade(color.red(4))
    print tone.shade(tone.dark(5))
}
"#;

fn write_self_provider_union_package(dir: &std::path::Path) -> std::path::PathBuf {
    let package_root = dir.join("libhome").join("unionlib");
    let src = package_root.join("src");
    fs::create_dir_all(&src).expect("create self-provider union src");
    fs::write(
        package_root.join("faber.toml"),
        r#"
[package]
name = "unionlib"
version = "1.0.0"
edition = "2026"

[library]
provider = "unionlib"

[paths]
source = "src"

[build]
kind = "lib"
targets = ["fmir"]

[locale]
locale = "en"
"#,
    )
    .expect("write self-provider union faber.toml");
    fs::write(src.join("color.fab"), COLOR_LIB).expect("write color.fab");
    fs::write(src.join("tone.fab"), TONE_LIB).expect("write tone.fab");
    fs::write(src.join("union.proba"), UNION_PROBA).expect("write union.proba");
    src.join("union.proba")
}

#[test]
fn package_test_sibling_union_payloads_project() {
    // Two sibling unions with the same source-local DefId layout (Color /
    // Tone, each with a payload variant + a unit variant). Intern must not
    // collapse them onto one Type::Enum; variant field projection must
    // resolve against the remapped union identity.
    let dir = crate::test_support::test_temp_dir("union-defid-intern");
    let proba_path = write_self_provider_union_package(dir.as_ref());
    let package_root = proba_path.parent().unwrap().parent().unwrap();
    let config = Config::default().with_library_home(dir.join("libhome"));
    let mut package = crate::analyze::analyze_package_for_tests(
        &config,
        package_root,
        None,
        &crate::test_support::test_analysis_seams(),
    )
    .expect("analyze self-provider sibling union package tests");
    let (root_id, _) = package
        .node_by_path(&proba_path)
        .expect("union.proba is an analyzed node");
    let root_id = root_id.clone();
    let included = package.imports.local_closure(std::iter::once(&root_id));
    package.nodes.retain(|id, _| included.contains(id));
    package.entry = Some(root_id.clone());
    package.roots = vec![root_id];
    let mut functions = Vec::new();
    for unit in package.nodes.values() {
        let is_proba = unit
            .path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext == "proba");
        if !is_proba {
            continue;
        }
        for item in &unit.analysis.hir.items {
            if let HirItemKind::Function(function) = &item.kind
                && unit.analysis.interner.resolve(function.name) == "prove_sibling_union_payloads"
            {
                functions.push((unit.path.clone(), item.def_id));
            }
        }
    }
    assert_eq!(
        functions.len(),
        1,
        "expected one prove_sibling_union_payloads case"
    );
    let scope = TestPackageScope::for_program(&package, functions);
    let output = with_linked_test_package_mir(
        package,
        &scope,
        &config,
        &crate::test_support::test_mir_seams(),
        |validated, function_ids| {
            let mut host = BufferHost::default();
            for function_id in function_ids.values() {
                radix_mir_runner::run_function(validated, *function_id, &mut host)
                    .expect("sibling union payload projections must execute");
            }
            host.stdout_lines
        },
    )
    .unwrap_or_else(|diagnostics| {
        panic!("sibling union payload projections must link: {diagnostics:?}",)
    });
    assert_eq!(
        output,
        vec!["4".to_owned(), "5".to_owned()],
        "Color.Red.shade and Tone.Dark.shade project independently"
    );
}

#[test]
fn package_mir_nested_library_method_calls_link_through_test_package() {
    let dir = crate::test_support::test_temp_dir("method-call-mir-test");
    let entry = write_nested_tensor_package(dir.as_ref());
    let package_root = entry.parent().unwrap().parent().unwrap();
    let config = Config::default().with_library_home(dir.join("libhome"));
    let mut package = crate::analyze::analyze_package_for_tests(
        &config,
        package_root,
        None,
        &crate::test_support::test_analysis_seams(),
    )
    .expect("analyze nested tensor package tests");
    let proba_path = package_root.join("src/tensor.proba");
    let (root_id, _) = package
        .node_by_path(&proba_path)
        .expect("tensor.proba is an analyzed node");
    let root_id = root_id.clone();
    let included = package.imports.local_closure(std::iter::once(&root_id));
    package.nodes.retain(|id, _| included.contains(id));
    package.entry = Some(root_id.clone());
    package.roots = vec![root_id];
    let mut functions = Vec::new();
    for unit in package.nodes.values() {
        let is_proba = unit
            .path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext == "proba");
        if !is_proba {
            continue;
        }
        for item in &unit.analysis.hir.items {
            if let HirItemKind::Function(function) = &item.kind
                && unit.analysis.interner.resolve(function.name) == "prove_nested_tensor_methods"
            {
                functions.push((unit.path.clone(), item.def_id));
            }
        }
    }
    assert_eq!(
        functions.len(),
        1,
        "expected one prove_nested_tensor_methods case"
    );
    let scope = TestPackageScope::for_program(&package, functions);
    let output = with_linked_test_package_mir(
        package,
        &scope,
        &config,
        &crate::test_support::test_mir_seams(),
        |validated, function_ids| {
            let mut host = BufferHost::default();
            for function_id in function_ids.values() {
                radix_mir_runner::run_function(validated, *function_id, &mut host)
                    .expect("nested tensor methods must execute on the test-link lane");
            }
            host.stdout_lines
        },
    )
    .unwrap_or_else(|diagnostics| {
        panic!("nested tensor method calls must lower on the test-link lane: {diagnostics:?}",)
    });
    assert_nested_tensor_output(output);
}
