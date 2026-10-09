//! The lowered program carries a display-token table and an id for every
//! function holding a print-family site (need, plumbing only).

use super::*;
use crate::codegen::Target;
use crate::driver::{AnalyzeRequest, Config, Session, analyze_source};
use radix_runtime_contract::display_tokens::DisplayTokens;

#[test]
fn english_print_and_interpolation_sites_carry_latin_id_zero() {
    let session = Session::new(Config::default().with_target(Target::HirTypeScript));
    let source = "+++
locale = \"en\"
+++
main {
    print \"v §\"(1)
    print true
}
";
    let unit = analyze_source(&session, "tokens.fab", source, AnalyzeRequest::default()).expect("analyzes");
    let lowered = lower_analyzed_unit(&unit, MirLoweringOptions::default()).expect("lowers");
    let validation = lowered.validated.validation();
    assert_eq!(validation.display_tokens.tokens, vec![DisplayTokens::latin()]);
    let entry = lowered
        .validated
        .program()
        .functions
        .last()
        .expect("entry function")
        .id;
    assert_eq!(
        validation.display_tokens.by_function.get(&entry),
        Some(&radix_mir::DisplayTokensId(0))
    );
}
