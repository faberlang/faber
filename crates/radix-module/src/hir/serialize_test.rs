//! Authored size arguments of a size-generic application survive nesting.

use super::record_named_size_args_in_form;
use radix_hir::{HirTypeForm, HirTypeFormArg, HirTypeFormHead};
use radix_types::{AppliedArg, DefId, IndexExpr, Type, TypeTable};

#[test]
fn a_size_generic_application_under_a_union_gets_its_size_child() {
    let mut types = TypeTable::new();
    let base = types.intern(Type::Struct(DefId(7)));
    let three = types.intern_index(IndexExpr::Literal(3));
    let applied = types.intern(Type::Applied(base, vec![AppliedArg::Size(three)]));
    let optional = types.intern(Type::Option(applied));
    let mut form = HirTypeForm {
        ty: optional,
        head: HirTypeFormHead::Semantic(radix_syntax::TypeFormLevel::LONG),
        args: vec![HirTypeFormArg::Type(Box::new(HirTypeForm {
            ty: applied,
            head: HirTypeFormHead::Named { def_id: DefId(7), path: Vec::new() },
            args: Vec::new(),
            label: None,
        }))],
        label: None,
    };

    record_named_size_args_in_form(&mut form, &types);

    let [HirTypeFormArg::Type(child)] = form.args.as_slice() else {
        panic!("the optional keeps its one child: {:?}", form.args);
    };
    assert_eq!(child.args, vec![HirTypeFormArg::Size(three)]);
}
