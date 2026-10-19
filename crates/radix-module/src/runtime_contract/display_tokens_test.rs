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

#[test]
fn null_value_and_type_words_follow_the_2026_10_17_ruling() {
    let rows = [
        ("ar", "غائب", "لاشيء"),
        ("hi", "अभाव", "कुछनहीं"),
        ("th-TH", "ขาดหาย", "ไม่มี"),
        ("vi", "vắng", "hư_không"),
        ("zh-Hans", "缺失", "无"),
        ("zh-Hant", "缺失", "無"),
    ];
    for (id, value_word, type_word) in rows {
        let p = pack(id);
        assert_eq!(p.keywords["nulla"].canonical, value_word, "{id}: nulla");
        assert_eq!(p.types["nihil"].canonical, type_word, "{id}: nihil");
        assert_ne!(value_word, type_word, "{id}: null words collide");
        assert_ne!(value_word, p.types["vacuum"].canonical, "{id}: nulla vs void");
        assert_ne!(type_word, p.types["vacuum"].canonical, "{id}: nihil vs void");
        assert_ne!(value_word, p.keywords["vacua"].canonical, "{id}: nulla vs empty");
        assert_ne!(type_word, p.keywords["vacua"].canonical, "{id}: nihil vs empty");
        assert_ne!(value_word, p.types["numquam"].canonical, "{id}: nulla vs never");
        assert_ne!(type_word, p.types["numquam"].canonical, "{id}: nihil vs never");
    }
}

#[test]
fn display_tokens_pack_missing_a_row_falls_back_per_token_to_latin() {
    let mut en = pack("en");
    en.keywords.remove("falsum");
    en.types.remove("nihil");
    let tokens = display_tokens_from_pack(&en);
    assert_eq!(tokens.true_, "true");
    assert_eq!(tokens.false_, "falsum");
    assert_eq!(tokens.none, "nihil");
    assert_eq!(tokens.tuple, "tuple");
}
