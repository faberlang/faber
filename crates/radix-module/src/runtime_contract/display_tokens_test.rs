use super::display_tokens_from_pack;
use crate::locale::{LocalePack, workspace_locale_pack_path};

const OTHER_LOCALES: [&str; 6] = ["ar", "hi", "th-TH", "vi", "zh-Hans", "zh-Hant"];

fn pack(id: &str) -> LocalePack {
    LocalePack::from_toml_path(workspace_locale_pack_path(id)).expect("shipped pack loads")
}

#[test]
fn display_tokens_en_pack_gives_true_false_none_tuple() {
    let tokens = display_tokens_from_pack(&pack("en"));
    assert_eq!(tokens.true_, "true");
    assert_eq!(tokens.false_, "false");
    assert_eq!(tokens.none, "none");
    assert_eq!(tokens.tuple, "tuple");
}

#[test]
fn display_tokens_la_pack_gives_verum_falsum_nihil_iuncta() {
    let tokens = display_tokens_from_pack(&pack("la"));
    assert_eq!(tokens.true_, "verum");
    assert_eq!(tokens.false_, "falsum");
    assert_eq!(tokens.none, "nihil");
    assert_eq!(tokens.tuple, "iuncta");
}

#[test]
fn display_tokens_every_other_shipped_pack_gives_four_distinct_nonempty_strings() {
    for id in OTHER_LOCALES {
        let tokens = display_tokens_from_pack(&pack(id));
        let words = [&tokens.true_, &tokens.false_, &tokens.none, &tokens.tuple];
        for word in words {
            assert!(!word.is_empty(), "{id}: empty token");
        }
        for (i, a) in words.iter().enumerate() {
            for b in &words[i + 1..] {
                assert_ne!(a, b, "{id}: duplicate token {a}");
            }
        }
    }
}
