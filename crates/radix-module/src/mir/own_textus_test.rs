use super::*;
use crate::codegen::Target;
use crate::driver::{AnalyzeRequest, Config, Session};

#[test]
fn lowers_own_textus_param_as_by_value_mir_storage() {
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_target(Target::HirTypeScript),
    );
    let source = r#"
functio consume(own textus buffer) → textus {
    redde buffer
}

incipit {
    nota consume(own "salve")
}
"#;
    let unit = crate::driver::analyze_source(&session, "own-textus.fab", source, AnalyzeRequest::default())
        .expect("analysis succeeds");
    let textus = unit.types.primitive(crate::semantic::Primitive::Textus);
    let lowered =
        lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default()).expect("MIR lowering succeeds");
    let function = &lowered.validated.program().functions[0];

    assert_eq!(function.params[0].mode, MirParamMode::Owned);
    assert_eq!(function.params[0].ty, MirType::semantic(textus));
    assert_eq!(function.locals[0].ty, MirType::semantic(textus));
    assert_eq!(function.return_ty, MirType::semantic(textus));
}
