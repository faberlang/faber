//! The lowered program carries a display-token table and an id for every
//! function holding a print-family site.

use super::*;
use crate::codegen::Target;
use crate::driver::{AnalyzeRequest, Config, Session, analyze_source};
use radix_runtime_contract::display_tokens::DisplayTokens;

#[test]
fn english_print_and_interpolation_sites_carry_english_id_zero() {
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
    let lowered = lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default()).expect("lowers");
    let validation = lowered.validated.validation();
    assert_eq!(
        validation.display_tokens.tokens,
        vec![DisplayTokens {
            true_: "true".to_owned(),
            false_: "false".to_owned(),
            none: "none".to_owned(),
            tuple: "tuple".to_owned(),
        }]
    );
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

/// Need: the llvm and wasm hosts read the entry function's tokens
/// once at start-up, so the entry carries its file's tokens even when the
/// printing happens in another function.
#[test]
fn english_print_free_entry_carries_english_id_zero() {
    let session = Session::new(Config::default().with_target(Target::HirTypeScript));
    let source = "+++
locale = \"en\"
+++
fn show() {
    let int ∪ none x ← null
    print x
}

main {
    show()
}
";
    let unit = analyze_source(&session, "tokens.fab", source, AnalyzeRequest::default()).expect("analyzes");
    let lowered = lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default()).expect("lowers");
    let validation = lowered.validated.validation();
    let program = lowered.validated.program();
    let names = radix_mir::MirNames::new(
        program,
        validation.types.as_table(),
        validation.interner.as_interner().expect("interner"),
    );
    let entry = names.explicit_entry_function().expect("entry function");
    assert!(!holds_print_site(entry), "entry holds no print site");
    assert_eq!(
        validation
            .display_tokens
            .for_function(entry.id)
            .map(|tokens| tokens.none.as_str()),
        Some("none")
    );
}
