//! Provider route identities must survive merging independently interned modules.

use super::remap::remap_runtime_call_text_symbols;
use radix_module::lexer::Interner;
use radix_module::mir::{MirIntrinsic, MirProvider, MirProviderKind, MirRuntimeCall, MirType};
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
