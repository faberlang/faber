use crate::PassConfig;
use crate::locale::LocalePack;
use crate::test_support::analyze_source_with_config;
use radix_target::Target;

#[test]
fn generic_alias_expansion_accepts_applied_uses() {
    let mut config = PassConfig::for_target(Target::HirRust);
    config.locale_pack = Some(
        LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
            .expect("load English locale pack"),
    );
    let result = analyze_source_with_config(
        r#"type Pairs<T> = list<T>
type Table<K, V> = map<K, V>

fn count(Pairs<int> p) → int {
    return p.length()
}

main {
    let Pairs<int> p ← [1, 2]
    print p.length()
    print count(p)
    print p
    mut Table<string, int> t ← empty
    t.put("a", 1)
    print t.length()
}"#,
        config,
    );
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
}
