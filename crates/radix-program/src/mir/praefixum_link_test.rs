//! ST2-2.7: package analysis evaluates deferred `comptime` sites over the
//! linked local import closure. The tests analyze temp packages through the
//! compiler seams and assert on `code`, `issue`, named args and initializer
//! tree shape, never on prose.

use super::*;
use crate::test_support::{test_mir_seams, test_temp_dir};
use std::fs;
use std::path::{Path, PathBuf};

const CONSTS: &str = "@ public\nconst int BASE = 5\n\n@ public\nfn seven() → int { return 7 }\n";

fn library_home() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../lib")
}

fn package_config(dir: &Path) -> Config {
    let seams = test_mir_seams();
    Config::default()
        .with_target((seams.executed_lane_target)(&Config::default(), dir).unwrap())
        .with_library_home(library_home())
}

fn analyze(dir: &Path) -> AnalyzedProgram {
    let seams = test_mir_seams();
    crate::analyze::analyze_package(&package_config(dir), dir, &seams.analysis)
        .unwrap_or_else(|diagnostics| panic!("analysis failed: {diagnostics:?}"))
}

fn analyze_err(dir: &Path) -> Vec<Diagnostic> {
    let seams = test_mir_seams();
    match crate::analyze::analyze_package(&package_config(dir), dir, &seams.analysis) {
        Ok(_) => panic!("package analysis must fail"),
        Err(diagnostics) => diagnostics,
    }
}

fn write(dir: &Path, name: &str, source: &str) {
    fs::write(dir.join(name), source).unwrap();
}

fn constant<'a>(node: &'a AnalyzedProgramNode, name: &str) -> &'a HirExpression {
    node.analysis
        .hir
        .items
        .iter()
        .find_map(|item| match &item.kind {
            HirItemKind::Constant(konst) if node.analysis.interner.resolve(konst.name) == name => {
                Some(&konst.value)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("constant `{name}` is declared"))
}

fn int_literal(expression: &HirExpression) -> i128 {
    match &expression.kind {
        HirExpressionKind::Literal(HirLiteral::Int(value)) => i128::from(*value),
        other => panic!("expected an int literal, got {other:?}"),
    }
}

fn sem064<'a>(diagnostics: &'a [Diagnostic], issue: &str) -> &'a Diagnostic {
    diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == Some("SEM064") && diagnostic.issue() == Some(issue))
        .unwrap_or_else(|| panic!("expected SEM064 {issue}, got {diagnostics:?}"))
}

fn arg<'a>(diagnostic: &'a Diagnostic, name: &str) -> &'a str {
    diagnostic
        .args
        .iter()
        .find(|argument| argument.name == name)
        .map(|argument| argument.value.as_str())
        .unwrap_or_else(|| panic!("expected arg {name}, got {:?}", diagnostic.args))
}

/// The genus `DefId` of an expression whose declared type is a genus.
fn struct_def(node: &AnalyzedProgramNode, expression: &HirExpression) -> DefId {
    let ty = expression
        .ty
        .expect("the initializer carries a declared type");
    match node.analysis.types.get(ty) {
        Type::Struct(def_id) => *def_id,
        other => panic!("expected a genus type, got {other:?}"),
    }
}

#[test]
fn praefixum_link_evaluates_imported_constant() {
    let dir = test_temp_dir("praefixum-link-const");
    let dir = dir.as_ref();
    write(dir, "consts.fab", CONSTS);
    write(
        dir,
        "main.fab",
        "import from \"./consts\" consts\n\n\
         const int D = comptime {\n    return consts.BASE + 1\n}\n\n\
         main {\n    print D\n}\n",
    );
    let package = analyze(dir);
    let (_, node) = package.node_by_path(&dir.join("main.fab")).unwrap();
    assert_eq!(int_literal(constant(node, "D")), 6);
}

#[test]
fn praefixum_link_evaluates_imported_call() {
    let dir = test_temp_dir("praefixum-link-call");
    let dir = dir.as_ref();
    write(dir, "consts.fab", CONSTS);
    write(
        dir,
        "main.fab",
        "import from \"./consts\" consts\n\n\
         const int G = comptime {\n    return consts.seven()\n}\n\n\
         main {\n    print G\n}\n",
    );
    let package = analyze(dir);
    let (_, node) = package.node_by_path(&dir.join("main.fab")).unwrap();
    assert_eq!(int_literal(constant(node, "G")), 7);
}

#[test]
fn praefixum_link_evaluates_chained_comptime_modules() {
    let dir = test_temp_dir("praefixum-link-chain");
    let dir = dir.as_ref();
    write(dir, "base.fab", "@ public\nconst int BASE = 5\n");
    write(
        dir,
        "mid.fab",
        "import from \"./base\" base\n\n\
         @ public\nconst int MID = comptime {\n    return base.BASE + 1\n}\n",
    );
    write(
        dir,
        "main.fab",
        "import from \"./mid\" mid\n\n\
         const int TOP = comptime {\n    return mid.MID + 1\n}\n\n\
         main {\n    print TOP\n}\n",
    );
    let package = analyze(dir);
    let (_, mid) = package.node_by_path(&dir.join("mid.fab")).unwrap();
    assert_eq!(int_literal(constant(mid, "MID")), 6);
    let (_, main) = package.node_by_path(&dir.join("main.fab")).unwrap();
    assert_eq!(int_literal(constant(main, "TOP")), 7);
}

#[test]
fn praefixum_link_exported_comptime_feeds_ordinary_constant() {
    let dir = test_temp_dir("praefixum-link-exported-feeds-const");
    let dir = dir.as_ref();
    write(dir, "base.fab", "@ public\nconst int BASE = 5\n");
    write(
        dir,
        "mid.fab",
        "import from \"./base\" base\n\n\
         @ public\nconst int MID = comptime {\n    return base.BASE + 1\n}\n",
    );
    write(
        dir,
        "main.fab",
        "import from \"./mid\" mid\n\n\
         const int TOP = mid.MID + 1\n\n\
         main {\n    print TOP\n}\n",
    );
    let package = analyze(dir);
    let (_, main) = package.node_by_path(&dir.join("main.fab")).unwrap();
    assert_eq!(int_literal(constant(main, "TOP")), 7);
}

/// The exported `comptime` chain also feeds a function-local constant
/// (`const T x = e` in a block): the interface an importer reads must be the
/// one republished after the deferred fold, or the block site sees `mid.MID`
/// as non-constant. Success is the assertion — no walk into function bodies
/// for the local `X` exists in this file.
#[test]
fn praefixum_link_exported_comptime_feeds_block_constant() {
    let dir = test_temp_dir("praefixum-link-block-const");
    let dir = dir.as_ref();
    write(dir, "base.fab", "@ public\nconst int BASE = 5\n");
    write(
        dir,
        "mid.fab",
        "import from \"./base\" base\n\n\
         @ public\nconst int MID = comptime {\n    return base.BASE + 1\n}\n",
    );
    write(
        dir,
        "main.fab",
        "import from \"./mid\" mid\n\n\
         fn seven() → int {\n    const int X = mid.MID + 1\n    return X\n}\n\n\
         main {\n    print seven()\n}\n",
    );
    let package = analyze(dir);
    assert!(
        package.node_by_path(&dir.join("main.fab")).is_some(),
        "main.fab analyzes with the exported comptime chain feeding the block constant"
    );
}

#[test]
fn praefixum_link_carried_provider_holds_evaluated_constant() {
    let dir = test_temp_dir("praefixum-link-carried-provider");
    let dir = dir.as_ref();
    write(dir, "consts.fab", CONSTS);
    write(
        dir,
        "c.fab",
        "import from \"./consts\" consts\n\n\
         @ public\nconst int G = comptime {\n    return consts.seven()\n}\n",
    );
    write(
        dir,
        "main.fab",
        "import from \"./c\" c\n\n\
         main {\n    print c.G\n}\n",
    );
    let package = analyze(dir);
    let (_, a) = package.node_by_path(&dir.join("main.fab")).unwrap();
    let carried = a
        .analysis
        .provider_units
        .get("./c")
        .expect("the entry carries a provider unit for ./c");
    let unit = carried
        .lock_unit()
        .expect("c's carried provider unit is its evaluated analysis");
    let value = unit
        .hir
        .items
        .iter()
        .find_map(|item| match &item.kind {
            HirItemKind::Constant(konst) if unit.interner.resolve(konst.name) == "G" => {
                Some(&konst.value)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("c's carried unit declares G"));
    assert_eq!(int_literal(value), 7);
}

#[test]
fn praefixum_link_imported_callee_that_prints_is_forbidden() {
    let dir = test_temp_dir("praefixum-link-effect");
    let dir = dir.as_ref();
    write(
        dir,
        "consts.fab",
        "@ public\nfn loud() → int {\n    print 7\n    return 7\n}\n",
    );
    write(
        dir,
        "main.fab",
        "import from \"./consts\" consts\n\n\
         const int G = comptime {\n    return consts.loud()\n}\n\n\
         main {\n    print G\n}\n",
    );
    let diagnostics = analyze_err(dir);
    let diagnostic = sem064(&diagnostics, "praefixum_forbidden");
    assert_eq!(arg(diagnostic, "name"), "G");
    assert_eq!(arg(diagnostic, "effect"), "scribe");
    assert!(
        arg(diagnostic, "chain").contains("consts.loud"),
        "chain names the imported hop: {:?}",
        arg(diagnostic, "chain")
    );
}

#[test]
fn praefixum_link_endless_imported_callee_hits_the_step_limit() {
    let dir = test_temp_dir("praefixum-link-step");
    let dir = dir.as_ref();
    write(
        dir,
        "consts.fab",
        "@ public\nfn spin() → int {\n    var int i ← 0\n    while true {\n        i ← i + 1\n    }\n    return i\n}\n",
    );
    write(
        dir,
        "main.fab",
        "import from \"./consts\" consts\n\n\
         const int G = comptime {\n    return consts.spin()\n}\n\n\
         main {\n    print G\n}\n",
    );
    let diagnostics = analyze_err(dir);
    let diagnostic = sem064(&diagnostics, "praefixum_step_limit");
    assert_eq!(arg(diagnostic, "name"), "G");
}

#[test]
fn praefixum_link_library_import_is_unavailable() {
    let dir = test_temp_dir("praefixum-link-library");
    let dir = dir.as_ref();
    write(
        dir,
        "main.fab",
        "import from \"norma:math\" math\n\n\
         const float L = comptime {\n    return math.add(1.5, 2.5)\n}\n\n\
         main {\n    print L\n}\n",
    );
    let diagnostics = analyze_err(dir);
    let diagnostic = sem064(&diagnostics, "praefixum_import_unavailable");
    assert_eq!(arg(diagnostic, "name"), "L");
    assert_eq!(arg(diagnostic, "callee"), "math.add");
}

#[test]
fn praefixum_link_module_without_imports_is_untouched() {
    let dir = test_temp_dir("praefixum-link-local");
    let dir = dir.as_ref();
    write(
        dir,
        "main.fab",
        "const int K = comptime {\n    return 2 + 3\n}\n\n\
         main {\n    print K\n}\n",
    );
    let package = analyze(dir);
    let (_, node) = package.node_by_path(&dir.join("main.fab")).unwrap();
    assert_eq!(int_literal(constant(node, "K")), 5);
}

#[test]
fn praefixum_link_imported_genus_result_is_the_importers_own() {
    let dir = test_temp_dir("praefixum-link-genus");
    let dir = dir.as_ref();
    write(
        dir,
        "consts.fab",
        "@ public\nclass Box {\n    var int n\n}\n",
    );
    write(
        dir,
        "main.fab",
        "import from \"./consts\" consts\n\n\
         const consts.Box G = comptime {\n    return consts.Box { n = 7 }\n}\n\n\
         main {\n    print 0\n}\n",
    );
    match crate::analyze::analyze_package(&package_config(dir), dir, &test_mir_seams().analysis) {
        Ok(package) => {
            let (_, node) = package.node_by_path(&dir.join("main.fab")).unwrap();
            let value = constant(node, "G");
            assert!(
                !matches!(value.kind, HirExpressionKind::Praefixum(_)),
                "the imported-genus site must not stay deferred"
            );
            let HirExpressionKind::Verte {
                entries: Some(entries),
                ..
            } = &value.kind
            else {
                panic!(
                    "outcome must be the refusal issue or a typed construction, got {:?}",
                    value.kind
                );
            };
            let n = entries
                .iter()
                .find_map(|field| match (&field.key, &field.value) {
                    (radix_module::hir::HirObjectKey::Ident(name), Some(value))
                        if node.analysis.interner.resolve(*name) == "n" =>
                    {
                        Some(int_literal(value))
                    }
                    _ => None,
                });
            assert_eq!(n, Some(7));
            let def = struct_def(node, value);
            let (_, sibling) = package.node_by_path(&dir.join("consts.fab")).unwrap();
            let sibling_box = sibling
                .analysis
                .hir
                .items
                .iter()
                .find_map(|item| match &item.kind {
                    HirItemKind::Struct(strukt)
                        if sibling.analysis.interner.resolve(strukt.name) == "Box" =>
                    {
                        Some(item.def_id)
                    }
                    _ => None,
                })
                .expect("the sibling declares Box");
            assert_ne!(
                def, sibling_box,
                "a literal must never carry the sibling's DefId"
            );
        }
        Err(diagnostics) => {
            sem064(&diagnostics, "praefixum_result_not_literal");
        }
    }
}

#[test]
fn praefixum_link_method_call_evaluates_to_literal() {
    let dir = test_temp_dir("praefixum-link-method-literal");
    let dir = dir.as_ref();
    write(
        dir,
        "consts.fab",
        "@ public\nclass Box {\n    var int n\n\n    fn get() → int {\n        return self.n\n    }\n}\n",
    );
    write(
        dir,
        "main.fab",
        "import from \"./consts\" consts\n\n\
         const int M = comptime {\n    const consts.Box b ← consts.Box { n = 7 }\n    return b.get()\n}\n\n\
         main {\n    print M\n}\n",
    );
    let package = analyze(dir);
    let (_, node) = package.node_by_path(&dir.join("main.fab")).unwrap();
    assert_eq!(int_literal(constant(node, "M")), 7);
}

#[test]
fn praefixum_link_method_that_prints_is_forbidden() {
    let dir = test_temp_dir("praefixum-link-method-effect");
    let dir = dir.as_ref();
    write(
        dir,
        "consts.fab",
        "@ public\nclass Box {\n    var int n\n\n    fn get() → int {\n        print 7\n        return self.n\n    }\n}\n",
    );
    write(
        dir,
        "main.fab",
        "import from \"./consts\" consts\n\n\
         const int M = comptime {\n    const consts.Box b ← consts.Box { n = 7 }\n    return b.get()\n}\n\n\
         main {\n    print M\n}\n",
    );
    let diagnostics = analyze_err(dir);
    let diagnostic = sem064(&diagnostics, "praefixum_forbidden");
    assert_eq!(arg(diagnostic, "name"), "M");
    assert_eq!(arg(diagnostic, "effect"), "scribe");
    assert!(
        arg(diagnostic, "chain").contains("get"),
        "chain names the method hop: {:?}",
        arg(diagnostic, "chain")
    );
}

/// Two sibling modules export a genus named `Box`; the receiver is `a.Box`, so
/// the method home is `a` by receiver DefId and `b`'s different `get` body
/// (which would add 100) must not contaminate the evaluation.
#[test]
fn praefixum_link_method_homonym_genus_evaluates_in_its_own_home() {
    let dir = test_temp_dir("praefixum-link-homonym-genus");
    let dir = dir.as_ref();
    write(
        dir,
        "a.fab",
        "@ public\nclass Box {\n    var int n\n\n    fn get() → int {\n        return self.n\n    }\n}\n",
    );
    write(
        dir,
        "b.fab",
        "@ public\nclass Box {\n    var int n\n\n    fn get() → int {\n        return self.n + 100\n    }\n}\n",
    );
    write(
        dir,
        "main.fab",
        "import from \"./a\" a\nimport from \"./b\" b\n\n\
         const int M = comptime {\n    const a.Box x ← a.Box { n = 7 }\n    return x.get()\n}\n\n\
         main {\n    print M\n}\n",
    );
    let package = analyze(dir);
    let (_, node) = package.node_by_path(&dir.join("main.fab")).unwrap();
    assert_eq!(int_literal(constant(node, "M")), 7);
}

/// The effect set of a homonym genus method is its own: `b.Box.get` prints and
/// `a.Box.get` is pure, so a site on `a.Box` evaluates and a site on `b.Box`
/// is refused as forbidden.
#[test]
fn praefixum_link_method_homonym_genus_effect_is_its_own() {
    let a_source = "@ public\nclass Box {\n    var int n\n\n    fn get() → int {\n        return self.n\n    }\n}\n";
    let b_source = "@ public\nclass Box {\n    var int n\n\n    fn get() → int {\n        print 7\n        return self.n\n    }\n}\n";
    let main_source = |home: &str| {
        format!(
            "import from \"./a\" a\nimport from \"./b\" b\n\n\
             const int M = comptime {{\n    const {home}.Box x ← {home}.Box {{ n = 7 }}\n    return x.get()\n}}\n\n\
             main {{\n    print M\n}}\n"
        )
    };

    let pure_dir = test_temp_dir("praefixum-link-homonym-effect-pure");
    let pure_dir = pure_dir.as_ref();
    write(pure_dir, "a.fab", a_source);
    write(pure_dir, "b.fab", b_source);
    write(pure_dir, "main.fab", &main_source("a"));
    let package = analyze(pure_dir);
    let (_, node) = package.node_by_path(&pure_dir.join("main.fab")).unwrap();
    assert_eq!(int_literal(constant(node, "M")), 7);

    let effect_dir = test_temp_dir("praefixum-link-homonym-effect-prints");
    let effect_dir = effect_dir.as_ref();
    write(effect_dir, "a.fab", a_source);
    write(effect_dir, "b.fab", b_source);
    write(effect_dir, "main.fab", &main_source("b"));
    let diagnostics = analyze_err(effect_dir);
    let diagnostic = sem064(&diagnostics, "praefixum_forbidden");
    assert_eq!(arg(diagnostic, "name"), "M");
}

/// ST2-2.11c: a library callee is available only when the analyzed body of
/// the library reaches the import-effect resolver. The package analysis hook
/// supplies no library bodies, so the site is refused as unavailable; the
/// evaluated-4.0 arm stays asserted for the day bodies are supplied.
#[test]
fn praefixum_link_library_callee_evaluates_or_is_unavailable() {
    let dir = test_temp_dir("praefixum-link-library-callee");
    let dir = dir.as_ref();
    write(
        dir,
        "main.fab",
        "import from \"norma:math\" math\n\n\
         const float L = comptime {\n    return math.add(1.5, 2.5)\n}\n\n\
         main {\n    print L\n}\n",
    );
    let seams = test_mir_seams();
    match crate::analyze::analyze_package(&package_config(dir), dir, &seams.analysis) {
        Ok(package) => {
            let (_, node) = package.node_by_path(&dir.join("main.fab")).unwrap();
            match &constant(node, "L").kind {
                HirExpressionKind::Literal(HirLiteral::Float(value)) => {
                    assert_eq!(*value, 4.0, "the library body evaluates math.add");
                }
                other => panic!("expected the evaluated float literal, got {other:?}"),
            }
        }
        Err(diagnostics) => {
            let matches: Vec<&Diagnostic> = diagnostics
                .iter()
                .filter(|diagnostic| {
                    diagnostic.code == Some("SEM064")
                        && diagnostic.issue() == Some("praefixum_import_unavailable")
                })
                .collect();
            assert_eq!(
                matches.len(),
                1,
                "an unresolved library callee yields exactly one refusal: {diagnostics:?}"
            );
            assert_eq!(arg(matches[0], "name"), "L");
            assert_eq!(arg(matches[0], "callee"), "math.add");
        }
    }
}
