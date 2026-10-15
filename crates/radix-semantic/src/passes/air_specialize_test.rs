use super::*;
use crate::test_support::analyze_source_for_target_la;
use radix_target::Target;

#[test]
fn concrete_shape_call_gets_a_pre_air_instance() {
    let fixture = analyze_source_for_target_la(
        r#"
@ radix lane "air"
functio generic_sum<magnitudo N>(tensor<f32, [N]> x) → f32 {
    itera ex x sit value { }
    redde 0.0
}

incipit {
    sit tensor<f32, [4]> x ← vacua
    sit f32 y ← generic_sum(x)
}
"#,
        Target::MirWasm,
    );
    assert!(fixture.success(), "diagnostics: {:?}", fixture.diagnostics);
    let functions = fixture
        .hir
        .as_ref()
        .expect("typed HIR")
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            HirItemKind::Function(function) => Some((item, function)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(functions.len(), 2, "template plus one concrete instance");
    assert_eq!(
        functions
            .iter()
            .filter(|(_, function)| function.type_params.is_empty())
            .count(),
        1,
        "the concrete instance is the only executable AIR function"
    );
    let (_, instance) = functions
        .iter()
        .find(|(_, function)| function.type_params.is_empty())
        .expect("concrete instance");
    assert!(
        instance
            .body
            .as_ref()
            .is_some_and(|body| !contains_itera(body)),
        "instance was unrolled"
    );
}

#[test]
fn equal_air_instance_keys_are_deduplicated_and_distinct_sizes_survive() {
    let fixture = analyze_source_for_target_la(
        r#"
@ radix lane "air"
functio generic_sum<magnitudo N>(tensor<f32, [N]> x) → f32 {
    itera ex x sit value { }
    redde 0.0
}

incipit {
    sit tensor<f32, [8]> x8 ← vacua
    sit tensor<f32, [64]> x64 ← vacua
    sit f32 first ← generic_sum(x8)
    sit f32 again ← generic_sum(x8)
    sit f32 other ← generic_sum(x64)
}
"#,
        Target::MirWasm,
    );
    assert!(fixture.success(), "diagnostics: {:?}", fixture.diagnostics);
    let functions = fixture
        .hir
        .as_ref()
        .expect("typed HIR")
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            HirItemKind::Function(function) => Some(function),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(functions.len(), 3, "one template plus two size instances");
    assert_eq!(
        functions
            .iter()
            .filter(|function| function.type_params.is_empty())
            .count(),
        2,
        "the repeated [8] call shares one concrete instance"
    );
}

#[test]
fn nested_inferred_air_call_uses_the_enclosing_concrete_binding() {
    let fixture = analyze_source_for_target_la(
        r#"
@ radix lane "air"
functio inner<magnitudo N>(tensor<f32, [N]> x) → f32 {
    itera ex x sit value { }
    redde 0.0
}

@ radix lane "air"
functio outer<magnitudo N>(tensor<f32, [N]> x) → f32 {
    redde inner(x)
}

incipit {
    sit tensor<f32, [8]> x ← vacua
    sit f32 y ← outer(x)
}
"#,
        Target::MirWasm,
    );
    assert!(fixture.success(), "diagnostics: {:?}", fixture.diagnostics);
    let functions = fixture
        .hir
        .as_ref()
        .expect("typed HIR")
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            HirItemKind::Function(function) => Some(function),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        functions.len(),
        4,
        "two templates plus two concrete instances"
    );
    assert_eq!(
        functions
            .iter()
            .filter(|function| function.type_params.is_empty())
            .count(),
        2,
        "outer and its inferred inner call each get a concrete instance"
    );
    assert_eq!(
        functions
            .iter()
            .filter(|function| {
                function
                    .body
                    .as_ref()
                    .is_some_and(|body| contains_itera(body))
            })
            .count(),
        1,
        "only the symbolic inner template retains the loop"
    );
}

#[test]
fn generic_rope_method_argument_is_specialized_with_the_instance() {
    let fixture = analyze_source_for_target_la(
        r#"
@ radix lane "air"
functio generic_rope<magnitudo D>(tf32[2, D] x, tf32[2, 4, 3] table) → tf32[2, D] {
    redde x.rope_norm<D>(0, table)
}

incipit {
    sit tf32[2, 8] x ← vacua
    sit tf32[2, 4, 3] table ← vacua
    sit tf32[2, 8] y ← generic_rope(x, table)
}
"#,
        Target::MirWasm,
    );
    assert!(fixture.success(), "diagnostics: {:?}", fixture.diagnostics);
    let concrete = fixture
        .hir
        .as_ref()
        .expect("typed HIR")
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            HirItemKind::Function(function) if function.type_params.is_empty() => Some(function),
            _ => None,
        })
        .next()
        .expect("one concrete generic-rope instance");
    struct FindRope {
        dim: Option<TypeId>,
    }
    impl HirVisitor for FindRope {
        fn visit_expr(&mut self, expr: &HirExpression) {
            if let HirExpressionKind::MethodCall(_, _, generic_args, _, _) = &expr.kind
                && self.dim.is_none()
                && generic_args.len() == 1
            {
                self.dim = Some(generic_args[0]);
            }
            walk_expr(self, expr);
        }
    }
    let mut find = FindRope { dim: None };
    find.visit_function(concrete);
    assert!(
        find.dim.is_some(),
        "concrete instance retains the rope generic slot"
    );
    assert!(
        matches!(find.dim, Some(dim) if matches!(fixture.types.get(dim), Type::Param(_))),
        "the specialized generic argument remains a concrete numeric type witness: {:?}",
        find.dim
    );
}

fn contains_itera(block: &radix_hir::HirBlock) -> bool {
    struct Find(bool);
    impl HirVisitor for Find {
        fn visit_expr(&mut self, expr: &HirExpression) {
            if matches!(expr.kind, HirExpressionKind::Itera { .. }) {
                self.0 = true;
            }
            walk_expr(self, expr);
        }
    }
    let mut find = Find(false);
    find.visit_block(block);
    find.0
}
