use crate::test_support::parse_ok;
use radix_syntax::{BindingPattern, StmtKind};

#[test]
fn parses_range_underscore_binder_as_identifier() {
    let result = parse_ok("itera ex 0‥3 fixum _ { tacet }");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Itera(iter_stmt) = &program.statements[0].kind else {
        panic!("expected itera statement");
    };
    let BindingPattern::Ident(binding) = &iter_stmt.binding else {
        panic!("range underscore binder must remain an identifier");
    };
    assert_eq!(result.interner.resolve(binding.name), "_");
}
