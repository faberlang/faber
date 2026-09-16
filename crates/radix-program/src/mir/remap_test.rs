//! Symbol-bearing intrinsic payloads must survive merging independently
//! interned modules.

use super::DefId;
use super::remap::remap_runtime_call_text_symbols;
use radix_module::lexer::{Interner, Span};
use radix_module::mir::{
    MirFilumCoordinate, MirFilumSumOp, MirIntrinsic, MirLocalId, MirProvider, MirProviderKind,
    MirRuntimeCall, MirSermoOp, MirStatement, MirStatementKind, MirType,
};
use radix_types::{Primitive, TypeTable};

#[test]
fn provider_route_symbols_keep_their_text_after_module_merge() {
    let mut source = Interner::new();
    let module = vec![source.intern("norma:process"), source.intern("process")];
    let name = source.intern("pid");
    let mut target = Interner::new();
    target.intern("unrelated-entry-symbol");
    target.intern("another-entry-symbol");
    target.intern("not-a-provider-name");
    let types = TypeTable::new();
    let mut call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Provider(MirProvider {
            kind: MirProviderKind::Package,
            module,
            name,
        }),
        args: vec![],
        return_ty: MirType::semantic(types.primitive(Primitive::Numerus)),
    };
    remap_runtime_call_text_symbols(&mut call, &source, &mut target);
    let MirIntrinsic::Provider(provider) = call.intrinsic else {
        panic!("provider identity changed")
    };
    assert_eq!(target.resolve(provider.module[0]), "norma:process");
    assert_eq!(target.resolve(provider.module[1]), "process");
    assert_eq!(target.resolve(provider.name), "pid");
    assert_eq!(provider.kind, MirProviderKind::Package);
}

#[test]
fn sermo_frame_data_field_keeps_its_text_after_module_merge() {
    let mut source = Interner::new();
    let data_field = source.intern("data");
    let mut target = Interner::new();
    target.intern("unrelated-entry-symbol");
    target.intern("another-entry-symbol");
    let types = TypeTable::new();
    let mut call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Sermo(MirSermoOp::Receive {
            frame: DefId(7),
            data_field,
        }),
        args: vec![],
        return_ty: MirType::semantic(types.primitive(Primitive::Numerus)),
    };
    remap_runtime_call_text_symbols(&mut call, &source, &mut target);
    let MirIntrinsic::Sermo(MirSermoOp::Receive { frame, data_field }) = call.intrinsic else {
        panic!("sermo identity changed")
    };
    assert_eq!(frame, DefId(7));
    assert_eq!(target.resolve(data_field), "data");
}

#[test]
fn filum_declaration_symbols_keep_their_text_after_module_merge() {
    let mut source = Interner::new();
    let filum_binder = source.intern("filum_f");
    let term_binder_name = source.intern("term");
    let coordinate_name = source.intern("lane");
    let template = source.intern("lane {}");
    let mut target = Interner::new();
    target.intern("unrelated-entry-symbol");
    target.intern("another-entry-symbol");
    target.intern("third-entry-symbol");
    let types = TypeTable::new();
    let mut call = MirRuntimeCall {
        intrinsic: MirIntrinsic::FilumSum(MirFilumSumOp {
            filum_binder: Some(filum_binder),
            term_binder: MirLocalId(0),
            term_binder_name: Some(term_binder_name),
            coordinates: vec![MirFilumCoordinate {
                local: MirLocalId(1),
                name: Some(coordinate_name),
            }],
            lane_width: 32,
            term_body: vec![MirStatement {
                kind: MirStatementKind::RuntimeCall {
                    destination: None,
                    call: MirRuntimeCall {
                        intrinsic: MirIntrinsic::FormatString { template },
                        args: vec![],
                        return_ty: MirType::semantic(types.primitive(Primitive::Textus)),
                    },
                },
                span: Span::default(),
            }],
            term_result: MirLocalId(2),
        }),
        args: vec![],
        return_ty: MirType::semantic(types.primitive(Primitive::Numerus)),
    };
    remap_runtime_call_text_symbols(&mut call, &source, &mut target);
    let MirIntrinsic::FilumSum(filum) = call.intrinsic else {
        panic!("filum identity changed")
    };
    let filum_binder = filum.filum_binder.expect("filum binder name");
    assert_eq!(target.resolve(filum_binder), "filum_f");
    let term_binder = filum.term_binder_name.expect("term binder name");
    assert_eq!(target.resolve(term_binder), "term");
    let coordinate = filum.coordinates[0].name.expect("coordinate name");
    assert_eq!(target.resolve(coordinate), "lane");
    // The captured term body is real MIR: its nested intrinsics ride the same
    // remap, or the merged program resolves a foreign symbol as template text.
    let MirStatementKind::RuntimeCall { call, .. } = &filum.term_body[0].kind else {
        panic!("expected nested runtime call")
    };
    let MirIntrinsic::FormatString { template } = &call.intrinsic else {
        panic!("expected nested format string")
    };
    assert_eq!(target.resolve(*template), "lane {}");
}
