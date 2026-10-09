use super::DisplayTokens;

#[test]
fn display_tokens_latin_is_verum_falsum_nihil_iuncta() {
    let tokens = DisplayTokens::latin();
    assert_eq!(tokens.true_, "verum");
    assert_eq!(tokens.false_, "falsum");
    assert_eq!(tokens.none, "nihil");
    assert_eq!(tokens.tuple, "iuncta");
}
