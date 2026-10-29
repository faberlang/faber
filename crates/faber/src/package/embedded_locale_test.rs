use super::*;
use std::fs;

fn tree_pack_bytes(locale: &str) -> Vec<u8> {
    let path = radix::module::locale::workspace_locale_root()
        .join(locale)
        .join("pack.toml");
    fs::read(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

#[test]
fn embedded_ids_are_the_eight_reader_locales() {
    assert_eq!(
        EMBEDDED_LOCALE_IDS,
        ["ar", "en", "hi", "la", "th-TH", "vi", "zh-Hans", "zh-Hant"]
    );
}

#[test]
fn all_embedded_packs_decompress_to_tree_bytes_and_parse() {
    for locale in EMBEDDED_LOCALE_IDS {
        let gzip = compressed_pack_bytes(locale)
            .unwrap_or_else(|| panic!("missing embedded gzip for {locale}"));
        let decoded =
            decompress_gzip(gzip).unwrap_or_else(|err| panic!("decompress {locale}: {err}"));
        assert_eq!(
            decoded,
            tree_pack_bytes(locale),
            "embedded {locale} must byte-match locale/{locale}/pack.toml"
        );
        let toml = std::str::from_utf8(&decoded).unwrap_or_else(|err| {
            panic!("embedded {locale} is not UTF-8: {err}");
        });
        let pack = LocalePack::from_toml_str(toml)
            .unwrap_or_else(|err| panic!("parse embedded {locale}: {err}"));
        assert_eq!(pack.metadata.id, *locale);
    }
}

#[test]
fn loader_loads_every_embedded_locale() {
    for locale in EMBEDDED_LOCALE_IDS {
        let pack = load_embedded_locale_pack(locale)
            .unwrap_or_else(|err| panic!("load embedded {locale}: {err}"));
        assert_eq!(pack.metadata.id, *locale);
    }
}

#[test]
fn load_decompresses_only_the_requested_locale() {
    reset_decompressions();
    let pack = parse_embedded_locale_pack("vi").expect("load vi");
    assert_eq!(pack.metadata.id, "vi");
    assert_eq!(decompressions(), 1, "one locale in, one gzip decode");

    let pack = parse_embedded_locale_pack("zh-Hant").expect("load zh-Hant");
    assert_eq!(pack.metadata.id, "zh-Hant");
    assert_eq!(
        decompressions(),
        2,
        "second locale is a second decode, not all eight"
    );
}

#[test]
fn unknown_locale_does_not_decompress() {
    reset_decompressions();
    match parse_embedded_locale_pack("zz") {
        Err(EmbeddedLocaleError::UnknownLocale(locale)) => assert_eq!(locale, "zz"),
        other => panic!("expected UnknownLocale, got {other:?}"),
    }
    assert_eq!(decompressions(), 0);
}

#[test]
fn loader_needs_no_share_tree_install_prefix_or_checkout() {
    // The carrier takes only a locale id. Payload is include_bytes! of build
    // artifacts; decompress + from_toml_str never open share/, an install
    // prefix, or a sibling stdlib checkout.
    for locale in EMBEDDED_LOCALE_IDS {
        let gzip = compressed_pack_bytes(locale)
            .unwrap_or_else(|| panic!("{locale} pack is a 'static gzip blob"));
        assert!(
            !gzip.is_empty(),
            "{locale} embedded payload is in the binary, not read from disk"
        );
        let pack = load_embedded_locale_pack(locale)
            .unwrap_or_else(|err| panic!("load {locale} from embedded bytes: {err}"));
        assert_eq!(pack.metadata.id, *locale);
    }
}

#[test]
fn loader_parses_each_locale_once_per_process() {
    reset_decompressions();
    let first = load_embedded_locale_pack("hi").expect("first load hi");
    let after_first = decompressions();
    let second = load_embedded_locale_pack("hi").expect("second load hi");
    assert_eq!(first, second);
    assert_eq!(
        decompressions(),
        after_first,
        "second load of a cached locale must not decompress again"
    );
}
