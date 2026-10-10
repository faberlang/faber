//! Reader-pack, frontmatter, diagnostic rejection, and target-policy feature tests.
//! Each test exercises the full `compile()` pipeline and inspects the result.

use super::compile;
use crate::codegen::Target;
use crate::driver::{Config, ModuleCompileRequest, Session};
use crate::test_support::test_session;

// ── reader-pack integration ──

#[test]
fn compile_uses_locale_pack_for_keyword_lexing() {
    let config = Config::default()
        .with_target(Target::HirRust)
        .with_locale_pack(super::zh_locale_pack());
    let session = Session::new(config);
    let source = "函数 salve() { }";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
}

#[test]
fn compile_accepts_en_require_surface_with_alternate_exit() {
    let pack_path = crate::locale::workspace_locale_pack_path("en");
    let pack = crate::locale::LocalePack::from_toml_path(&pack_path).expect("installed en pack should load");
    let config = Config::default()
        .with_target(Target::HirRust)
        .with_locale_pack(pack);
    let session = Session::new(config);
    let source = "fn f() → int ⇥ string {\n  require true throw \"bad\"\n  return 1\n}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
}

#[test]
fn compile_rejects_en_require_in_arrowless_function() {
    let pack_path = crate::locale::workspace_locale_pack_path("en");
    let pack = crate::locale::LocalePack::from_toml_path(&pack_path).expect("installed en pack should load");
    let config = Config::default()
        .with_target(Target::HirRust)
        .with_locale_pack(pack);
    let session = Session::new(config);
    let source = "fn f() → int {\n  require true throw \"bad\"\n  return 1\n}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(
        !result.success(),
        "en require in a ⇥-less function must fail: {:?}",
        result.diagnostics
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.is_error() && d.code == Some("SEM010") && d.issue() == Some("iace_requires_alternate_exit")),
        "expected SEM010 iace_requires_alternate_exit, got: {:?}",
        result.diagnostics
    );
}

#[test]
fn compile_en_empty_marker_and_is_empty_intrinsic() {
    // en-pack-surface-fidelity V-1: the empty-collection marker is `empty`,
    // and the collection intrinsic remains `is_empty`. Both spellings must
    // compile in one source so the concepts cannot cross-contaminate.
    let pack_path = crate::locale::workspace_locale_pack_path("en");
    let pack = crate::locale::LocalePack::from_toml_path(&pack_path).expect("installed en pack should load");
    let config = Config::default()
        .with_target(Target::HirRust)
        .with_locale_pack(pack);
    let session = Session::new(config);
    let source = "fn f() → int {\n  var list<int> xs ← empty\n  const _ vacant ← xs.is_empty()\n  return 0\n}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(
        result.success(),
        "en `empty` marker plus `is_empty` intrinsic must compile: {:?}",
        result.diagnostics
    );
}

#[test]
fn compile_en_vacua_marker_rejected() {
    // V-5: en rejects the Latin marker spelling; `vacua` is an ordinary
    // identifier and fails as unknown.
    let pack_path = crate::locale::workspace_locale_pack_path("en");
    let pack = crate::locale::LocalePack::from_toml_path(&pack_path).expect("installed en pack should load");
    let config = Config::default()
        .with_target(Target::HirRust)
        .with_locale_pack(pack);
    let session = Session::new(config);
    let source = "fn f() → int {\n  var list<int> xs ← vacua\n  return 0\n}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(
        !result.success(),
        "en `vacua` must not claim as the empty-collection marker: {:?}",
        result.diagnostics
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.is_error() && d.code == Some("SEM001") && d.issue() == Some("unknown_identifier")),
        "expected SEM001 unknown_identifier, got: {:?}",
        result.diagnostics
    );
}

#[test]
fn compile_la_vacua_marker_untouched() {
    let pack = crate::locale::latin_locale_pack();
    let config = Config::default()
        .with_target(Target::HirRust)
        .with_locale_pack(pack);
    let session = Session::new(config);
    let source = "functio f() → numerus {\n  varia lista<numerus> xs ← vacua\n  redde 0\n}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "la `vacua` marker must still claim: {:?}", result.diagnostics);
}

#[test]
fn compile_rejects_undeclared_latin_keyword_in_grammar_position() {
    // S2-U2 pack-authoritative gate: the zh-Hans pack does not declare the
    // Latin spelling `functio`, so it must NOT claim the function-declaration
    // slot. The word stays an ordinary identifier and the grammar-position use
    // fails normally — no fallback event, no LOCALE diagnostic. The declared
    // spelling `函数` is proven to succeed in
    // `compile_uses_locale_pack_for_keyword_lexing`.
    let config = Config::default()
        .with_target(Target::HirRust)
        .with_locale_pack(super::zh_locale_pack());
    let session = Session::new(config);
    let source = "functio salve() { }";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(
        !result.success(),
        "undeclared Latin keyword must not claim the grammar slot: {:?}",
        result.diagnostics
    );
    // The undeclared spelling stays an ordinary identifier: the failure is
    // exactly the undefined-name error an ordinary identifier use produces
    // (no keyword-claim diagnostic, no LOCALE code).
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.is_error() && d.code == Some("SEM001")),
        "undeclared spelling must fail as an ordinary undefined identifier: {:?}",
        result.diagnostics
    );
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| d.code.is_some_and(|code| code.starts_with("LOCALE"))),
        "no LOCALE diagnostic may be emitted on the flipped path: {:?}",
        result.diagnostics
    );
}

#[test]
fn compile_uses_locale_pack_for_primitive_type_surface() {
    let config = Config::default()
        .with_target(Target::HirRust)
        .with_locale_pack(super::th_locale_pack());
    let session = Session::new(config);
    let source = r#"ฟังก์ชัน salve() → ข้อความ {
  คืน "สวัสดี"
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
}

#[test]
fn compile_rejects_undeclared_tokenless_latin_keyword_in_grammar_slot() {
    // S2-U2 pack-authoritative gate: the zh-Hans pack declares neither
    // `functio` nor `iuncta`, so their Latin spellings must not claim their
    // grammar slots. The source fails normally with no fallback event and no
    // LOCALE diagnostic.
    let config = Config::default()
        .with_target(Target::HirRust)
        .with_locale_pack(super::zh_locale_pack());
    let session = Session::new(config);
    let source = "functio salve() → numerus {\n  fixum _ t ← iuncta<numerus> [1]\n  redde 0\n}\n";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(
        !result.success(),
        "undeclared tokenless Latin keyword must not claim its grammar slot: {:?}",
        result.diagnostics
    );
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| d.code.is_some_and(|code| code.starts_with("LOCALE"))),
        "no LOCALE diagnostic may be emitted for an undeclared tokenless keyword: {:?}",
        result.diagnostics
    );
}

#[test]
fn deny_warnings_does_not_escalate_undeclared_latin_keyword_failure() {
    use crate::driver::WarnPolicy;

    // The retired fallback notice no longer exists and the parser gate
    // (S2-U2) rejects undeclared Latin keywords under a non-`la` pack, so
    // deny-all-warnings has no fallback warning to escalate. The build fails
    // on the source's own ordinary parse/semantic errors, never on a locale
    // fallback diagnostic.
    let config = Config::default()
        .with_target(Target::HirRust)
        .with_locale_pack(super::zh_locale_pack())
        .with_warn_policy(WarnPolicy { deny_all_warnings: true, deny_codes: vec![] });
    let session = Session::new(config);
    let source = "functio salve() → numerus {\n  fixum _ t ← iuncta<numerus> [1]\n  redde 0\n}\n";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| d.code.is_some_and(|code| code.starts_with("LOCALE"))),
        "no locale fallback diagnostic may appear under deny-all-warnings: {:?}",
        result.diagnostics
    );
}

// ── S2-U2 keyword proof matrix: Latin as ordinary identifier ──

#[test]
fn compile_latin_spelling_usable_as_ordinary_identifier_under_pack() {
    // S2-U2 proof (b): an undeclared Latin spelling under a non-`la` pack is
    // an ordinary identifier — provable as a variable. `functio` (undeclared
    // under zh-Hans) binds as a variable name and is read back; only the
    // declared spelling `函数` may claim the function-declaration slot.
    let pack = crate::locale::LocalePack::from_toml_str(
        r#"
[pack]
id = "zh-Hans"
fallback = ["la"]

[keywords]
functio = "函数"
fixum = "常量"
redde = "redde"

[llm]
system_prompt_snippet = "emit Chinese reader-locale Faber"
exemplars = ["./exemplars/salve-munde.zh-Hans.fab"]
"#,
    )
    .expect("pack should validate");
    let config = Config::default()
        .with_target(Target::HirRust)
        .with_locale_pack(pack);
    let session = Session::new(config);
    // `functio` (undeclared) is an ordinary identifier: it binds as a variable
    // name (type-first binding syntax) and is used as a value, never a
    // keyword claim.
    let source = "函数 salve() → numerus {\n  常量 numerus functio ← 1\n  redde functio\n}\n";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(
        result.success(),
        "undeclared Latin spelling must be usable as an ordinary identifier: {:?}",
        result.diagnostics
    );
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| d.code.is_some_and(|code| code.starts_with("LOCALE"))),
        "no LOCALE diagnostic on the Latin-as-ordinary-identifier path: {:?}",
        result.diagnostics
    );
}

#[test]
fn compile_identity_row_accepts_canonical_spelling_under_pack() {
    // S2-U2 proof (c): an explicit identity row (`canonical == surface`)
    // makes the canonical Latin spelling accepted in grammar position under a
    // non-`la` pack — explicit declaration, not hidden fallback.
    let pack = crate::locale::LocalePack::from_toml_str(
        r#"
[pack]
id = "zh-Hans"
fallback = ["la"]

[keywords]
functio = "functio"

[llm]
system_prompt_snippet = "emit Chinese reader-locale Faber"
exemplars = ["./exemplars/salve-munde.zh-Hans.fab"]
"#,
    )
    .expect("pack should validate");
    let config = Config::default()
        .with_target(Target::HirRust)
        .with_locale_pack(pack);
    let session = Session::new(config);
    let source = "functio salve() { }";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(
        result.success(),
        "identity row must make the canonical spelling claimable: {:?}",
        result.diagnostics
    );
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| d.code.is_some_and(|code| code.starts_with("LOCALE"))),
        "no LOCALE diagnostic on the identity-row path: {:?}",
        result.diagnostics
    );
}

#[test]
fn compile_latin_keywords_under_la_and_no_pack() {
    // S2-U2 proof (d): `la` and no-pack stay registry-direct — the canonical
    // Latin spelling claims its grammar slot without a pack row.
    let config = Config::default()
        .with_target(Target::HirRust)
        .with_locale_pack(crate::locale::latin_locale_pack());
    let session = Session::new(config);
    let source = "functio salve() { }";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(
        result.success(),
        "canonical Latin keyword must claim under the la pack: {:?}",
        result.diagnostics
    );

    let session = test_session(Target::HirRust);
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(
        result.success(),
        "canonical Latin keyword must claim with no pack selected: {:?}",
        result.diagnostics
    );
}

#[test]
fn compile_recognizes_localized_tokenless_batch_a_keywords() {
    // Batch A made `modulus`/`iuncta` tokenless. Under a reader pack their
    // localized spellings must still lex, parse in their grammar slots, and
    // lower — the pack rows resolve to Latin registry identities via
    // `reader_keyword_latin_keys`, and the parser claims them by spelling.
    let pack = crate::locale::LocalePack::from_toml_str(
        r#"
[pack]
id = "zh-Hans"
fallback = ["la"]

[keywords]
functio = "函数"
iuncta = "元组"
modulus = "模数"
# S2-U2: Latin support vocabulary used by the fixture must be declared —
# identity rows (explicit declaration, not hidden fallback).
fixum = "fixum"
redde = "redde"

[llm]
system_prompt_snippet = "emit Chinese reader-locale Faber"
exemplars = ["./exemplars/salve-munde.zh-Hans.fab"]
"#,
    )
    .expect("pack should validate");
    let config = Config::default()
        .with_target(Target::HirRust)
        .with_locale_pack(pack);
    let session = Session::new(config);
    let source = r#"函数 checksum() → numerus {
  fixum 模数<u32> m ← 5
  fixum u32 checked ← 5
  fixum _ w ← checked ↦ 模数<u32>
  fixum 元组<textus, numerus> t ← 元组<textus, numerus> ["salve", 7]
  redde 0
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
}

#[test]
fn compile_recognizes_localized_tokenless_batch_b_keywords() {
    // Batch B made the builtin I/O words tokenless. Under a reader pack their
    // localized spellings must still lex, parse in their grammar slots, and
    // lower — the pack rows resolve to Latin registry identities via
    // `reader_keyword_latin_keys`, and the parser claims them by spelling.
    let pack = crate::locale::LocalePack::from_toml_str(
        r#"
[pack]
id = "zh-Hans"
fallback = ["la"]

[keywords]
functio = "函数"
nota = "显示"
mone = "警告"
vide = "调试"
lege = "读入"
lineam = "读行"
scriptum = "格式化"
# S2-U2: Latin support vocabulary used by the fixture must be declared.
fixum = "fixum"
redde = "redde"

[llm]
system_prompt_snippet = "emit Chinese reader-locale Faber"
exemplars = ["./exemplars/salve-munde.zh-Hans.fab"]
"#,
    )
    .expect("pack should validate");
    let config = Config::default()
        .with_target(Target::HirRust)
        .with_locale_pack(pack);
    let session = Session::new(config);
    let source = r#"函数 checksum() → numerus {
  显示 42
  警告 7
  格式化("salve {0}", 1)
  fixum _ s ← 读行
  fixum _ t ← 读入
  redde 0
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
}

#[test]
fn compile_recognizes_localized_tokenless_batch_d_keywords() {
    // Batch D made the logical operators tokenless. Under a reader pack their
    // localized spellings must still lex, parse in their grammar slots
    // (infix `aut`/`et`/`est`/`vel`, prefix `non`, `non est` chains), and
    // lower — the pack rows resolve to Latin registry identities via
    // `reader_keyword_latin_keys`, and the parser claims them by spelling.
    let pack = crate::locale::LocalePack::from_toml_str(
        r#"
[pack]
id = "zh-Hans"
fallback = ["la"]

[keywords]
functio = "函数"
aut = "或"
est = "是"
et = "且"
non = "非"
vel = "兜底"
# S2-U2: Latin support vocabulary used by the fixture must be declared.
fixum = "fixum"
nulla = "nulla"
redde = "redde"

[llm]
system_prompt_snippet = "emit Chinese reader-locale Faber"
exemplars = ["./exemplars/salve-munde.zh-Hans.fab"]
"#,
    )
    .expect("pack should validate");
    let config = Config::default()
        .with_target(Target::HirRust)
        .with_locale_pack(pack);
    let session = Session::new(config);
    let source = r"函数 checksum() → numerus {
  fixum _ a ← nulla 是 nihil
  fixum _ b ← 非 (nulla 是 nihil)
  fixum _ c ← nulla 是 nihil 且 nulla 是 nihil
  fixum _ d ← nulla 是 nihil 或 nulla 是 nihil
  fixum _ n ← nulla 兜底 0
  redde 0
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
}

#[test]
fn compile_recognizes_localized_tokenless_batch_e_keywords() {
    // Batch E made the declaration introducers tokenless. Under a reader pack
    // their localized spellings must still lex, parse in statement-initial
    // position, and lower — the pack rows resolve to Latin registry identities
    // via `reader_keyword_latin_keys`, and the parser claims them by spelling.
    let pack = crate::locale::LocalePack::from_toml_str(
        r#"
[pack]
id = "zh-Hans"
fallback = ["la"]

[keywords]
functio = "函数"
fixum = "常量"
varia = "变量"
sit = "设"
typus = "类型"
# S2-U2: Latin support vocabulary used by the fixture must be declared.
redde = "redde"

[llm]
system_prompt_snippet = "emit Chinese reader-locale Faber"
exemplars = ["./exemplars/salve-munde.zh-Hans.fab"]
"#,
    )
    .expect("pack should validate");
    let config = Config::default()
        .with_target(Target::HirRust)
        .with_locale_pack(pack);
    let session = Session::new(config);
    let source = r"函数 checksum() → numerus {
  常量 numerus x ← 5
  变量 numerus y ← 6
  设 z ← 7
  redde x
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
}

#[test]
fn compile_recognizes_localized_tokenless_batch_f_keywords() {
    // Batch F made the control-flow keywords tokenless. Under a reader pack
    // their localized spellings must still lex, parse in statement position,
    // and lower — the pack rows resolve to Latin registry identities via
    // `reader_keyword_latin_keys`, and the parser claims them by spelling.
    let pack = crate::locale::LocalePack::from_toml_str(
        r#"
[pack]
id = "zh-Hans"
fallback = ["la"]

[keywords]
functio = "函数"
si = "如果"
secus = "否则"
sin = "否则如果"
ergo = "则"
dum = "当"
apud = "在"
fac = "执行"
itera = "遍历"
rumpe = "中断"
perge = "继续"
redde = "返回"

[llm]
system_prompt_snippet = "emit Chinese reader-locale Faber"
exemplars = ["./exemplars/salve-munde.zh-Hans.fab"]
"#,
    )
    .expect("pack should validate");
    let config = Config::default()
        .with_target(Target::HirRust)
        .with_locale_pack(pack);
    let session = Session::new(config);
    let source = r"函数 checksum() → numerus {
  如果 1 ≻ 0 {
    返回 1
  } 否则 {
    返回 0
  }
  执行 {
    如果 1 ≻ 2 { 中断 }
    继续
  } 当 1 ≻ 0
  返回 0
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
}

#[test]
fn compile_recognizes_localized_tokenless_batch_g_keywords() {
    // Batch G (the long tail) made these words tokenless. Under a reader pack
    // their localized spellings must still lex, parse in their grammar slots,
    // and lower — the pack rows resolve to Latin registry identities via
    // `reader_keyword_latin_keys`, and the parser claims them by spelling.
    let pack = crate::locale::LocalePack::from_toml_str(
        r#"
[pack]
id = "zh-Hans"
fallback = ["la"]

[keywords]
functio = "函数"
fixum = "常量"
usque = "到"
redde = "返回"

[llm]
system_prompt_snippet = "emit Chinese reader-locale Faber"
exemplars = ["./exemplars/salve-munde.zh-Hans.fab"]
"#,
    )
    .expect("pack should validate");
    let config = Config::default()
        .with_target(Target::HirRust)
        .with_locale_pack(pack);
    let session = Session::new(config);
    let source = r"函数 checksum() → numerus {
  常量 numerus x ← 0
  常量 _ r ← 1 到 5
  返回 x
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
}

#[test]
fn compile_exact_tokenless_spellings_do_not_suggest_reader002() {
    // Regression (P1): an exact declared keyword/type spelling is not a
    // near-miss. Before the fix, `keyword_or_ident` recorded the latin key for
    // tokenless rows and then fell through to `keyword_suggestion`, which
    // self-matched the exact spelling and emitted a false LOCALE002 — fatal
    // under `--deny-warnings`.
    let pack = crate::locale::LocalePack::from_toml_str(
        r#"
[pack]
id = "zh-Hans"
fallback = ["la"]

[keywords]
incipit = "入口"
nota = "显示"
nulla = "空"

[llm]
system_prompt_snippet = "emit Chinese reader-locale Faber"
exemplars = ["./exemplars/salve-munde.zh-Hans.fab"]
"#,
    )
    .expect("pack should validate");

    let source = "入口 { 显示 空 }";

    let config = Config::default()
        .with_target(Target::HirRust)
        .with_locale_pack(pack.clone());
    let session = Session::new(config);
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| d.code == Some("LOCALE002")),
        "exact declared spellings must not produce LOCALE002: {:?}",
        result.diagnostics
    );

    // The same source must also pass with all warnings denied.
    use crate::driver::WarnPolicy;
    let config = Config::default()
        .with_target(Target::HirRust)
        .with_locale_pack(pack)
        .with_warn_policy(WarnPolicy { deny_all_warnings: true, deny_codes: vec![] });
    let session = Session::new(config);
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(
        result.success(),
        "exact declared spellings must pass --deny-warnings: {:?}",
        result.diagnostics
    );
}

#[test]
fn compile_one_letter_parameter_names_do_not_suggest_reader002() {
    let pack_path = crate::locale::workspace_locale_pack_path("en");
    let pack = crate::locale::LocalePack::from_toml_path(pack_path).expect("installed English pack should load");

    use crate::driver::WarnPolicy;
    let config = Config::default()
        .with_target(Target::HirRust)
        .with_locale_pack(pack)
        .with_warn_policy(WarnPolicy { deny_all_warnings: true, deny_codes: vec![] });
    let session = Session::new(config);
    let source = r"fn double(int n) → int {
    return n * 2
}

main {
    print double(21)
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(
        result.success(),
        "one-letter parameter names must pass --deny-warnings: {:?}",
        result.diagnostics
    );
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| d.code == Some("LOCALE002")),
        "one-letter parameter name must not produce LOCALE002: {:?}",
        result.diagnostics
    );
}

#[test]
fn compile_localized_est_type_check_resolves_primitive_type() {
    // Regression (P2): `est <type>` with a fully localized primitive type
    // spelling. Before the fix, `peek_starts_type_name` only matched Latin
    // primitive texts, so `是 整数` fell through to the value-expression path
    // and the localized type resolved as an unknown identifier.
    let pack = crate::locale::LocalePack::from_toml_str(
        r#"
[pack]
id = "zh-Hans"
fallback = ["la"]

[keywords]
functio = "函数"
redde = "返回"
est = "是"

[types]
numerus = "整数"
bivalens = "布尔"

[llm]
system_prompt_snippet = "emit Chinese reader-locale Faber"
exemplars = ["./exemplars/salve-munde.zh-Hans.fab"]
"#,
    )
    .expect("pack should validate");
    let config = Config::default()
        .with_target(Target::HirRust)
        .with_locale_pack(pack);
    let session = Session::new(config);
    let source = "函数 f(整数 x) → 布尔 { 返回 x 是 整数 }";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| d.code.is_some_and(|c| c.starts_with("SEM"))),
        "localized est <type> must not produce a semantic error: {:?}",
        result.diagnostics
    );
}

#[test]
fn compile_accepts_exempla_metadata_in_frontmatter() {
    let session = test_session(Target::HirRust);
    let source = "+++\n[exempla]\n\"test\" = \"./exempla/test.fab\"\n+++\n\nincipit {\n  nota 42\n}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
}

#[test]
fn compile_strips_frontmatter_before_analysis() {
    let session = test_session(Target::HirRust);
    let source = "+++\ntitle = \"salve\"\n+++\n\nincipit {\n  nota 42\n}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
}

#[test]
fn compile_rejects_invalid_frontmatter_toml() {
    let session = test_session(Target::HirRust);
    let source = "+++\nbad toml [[\n+++\n\nincipit {\n  nota 42\n}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(!result.success());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == Some("PARSE052"))
    );
}

#[test]
fn compile_rejects_line_start_section_directive() {
    let session = test_session(Target::HirRust);
    let source = "§ 1.0\nincipit {\n  nota 42\n}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(!result.success());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.is_error() && d.code == Some("LEX004") && d.issue() == Some("unexpected_character"))
    );
}

#[test]
fn compile_rejects_c_style_line_comment() {
    let session = test_session(Target::HirRust);
    let source = "// This should fail\nincipit {\n  nota 42\n}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(!result.success());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.is_error() && d.code == Some("LEX006") && d.issue() == Some("c_style_line_comment"))
    );
}

#[test]
fn compile_accepts_frontmatter_only_file() {
    let session = test_session(Target::HirRust);
    let source = "+++\nkey = \"value\"\n+++";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "frontmatter-only file must succeed as empty program");
}

// ── diagnostic rejection (phase-gating) ──

#[test]
fn compile_reports_lex_errors() {
    let session = test_session(Target::HirRust);
    let source = "incipit {\n  fixum _ s ← \"hello\n}"; // unterminated string → LEX001
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(!result.success());
    assert!(result.diagnostics.iter().any(|d| d.code == Some("LEX001")));
    assert!(result.output.is_none());
}

#[test]
fn compile_reports_parse_errors() {
    let session = test_session(Target::HirRust);
    let source = "incipit {\n  fixum _ ← 1 + \n}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(!result.success());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == Some("PARSE001"))
    );
    assert!(result.output.is_none());
}

#[test]
fn compile_rejects_itera_pro_mode() {
    let session = test_session(Target::HirRust);
    let source = "incipit {\n  fixum _ xs ← [1, 2, 3]\n  itera pro fixum x de xs {\n    nota x\n  }\n}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(!result.success());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.is_error() && d.code == Some("PARSE001") && d.issue() == Some("itera_mode"))
    );
    assert!(result.output.is_none());
}

#[test]
fn compile_reports_semantic_errors() {
    let session = test_session(Target::HirRust);
    let source = r"incipit {
  fixum numerus n ← 1
  fixum textus t ← n
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(!result.success());
    assert!(result.diagnostics.iter().any(|d| d.code == Some("SEM010")));
    assert!(result.output.is_none());
}

#[test]
fn compile_rejects_itera_ab_over_collection() {
    let session = test_session(Target::HirRust);
    let source = "incipit {\n  fixum _ xs ← [1, 2, 3]\n  itera ab xs fixum i {\n    nota i\n  }\n}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(!result.success());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.is_error() && d.code == Some("SEM010") && d.issue() == Some("itera_ab_source_requires_range"))
    );
}

// ── return type / effect only ──

#[test]
fn compile_rejects_redde_without_explicit_return_type() {
    let session = test_session(Target::HirRust);
    let source = "functio bad() {\n  redde 1\n}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(!result.success());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.is_error() && d.code == Some("SEM033"))
    );
}

#[test]
fn compile_accepts_effect_only_failable_function_without_return_arrow() {
    let session = test_session(Target::HirTypeScript);
    let source = r"functio write(textus path, textus data) ⇥ textus {
  nota path
  nota data
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(result.success());
}

// ── nondum interface method checks ──

#[test]
fn compile_rejects_nondum_interface_method_for_target() {
    let session = test_session(Target::HirRust);
    let source = r#"implendum Drawable {
  @ nondum
  functio draw() → vacuum
}

genus Circle implet Drawable {
  functio draw() {
    nota "ok"
  }
}

incipit {
  fixum _ c ← Circle {}
  c.draw()
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(
        result.success(),
        "nondum interface method should compile: {:?}",
        result.diagnostics
    );
}

#[test]
fn compile_rejects_target_only_nondum_interface_method_for_target() {
    let session = test_session(Target::HirRust);
    let source = r#"implendum Drawable {
  @ nondum "swift"
  functio draw() → vacuum
}

genus Circle implet Drawable {
  functio draw() {
    nota "ok"
  }
}

incipit {
  fixum _ c ← Circle {}
  c.draw()
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(
        result.success(),
        "target-constrained nondum should not be flagged for other targets: {:?}",
        result.diagnostics
    );
    assert!(
        !result.diagnostics.iter().any(|d| d.code == Some("WARN006")),
        "no nondum warning expected for Rust target"
    );
}

// ── iace / error channel ──

#[test]
fn compile_rejects_iace_without_function_error_channel() {
    let session = test_session(Target::HirTypeScript);
    let source = r#"functio fails() {
  iace "bad"
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    assert!(!result.success());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.is_error() && d.code == Some("SEM010") && d.issue() == Some("iace_requires_alternate_exit"))
    );
}

// ── incipit argumenta / custodi / fac topology ──

#[test]
#[cfg(feature = "hir-faber")]
fn incipit_argumenta_compiles_for_faber_target() {
    let source = r"incipit argumenta args {
  fixum _ first ← args.accipe(0)
}";
    let result = super::faber_emit("test.fab", source);

    assert!(result.success(), "{:?}", result.diagnostics);
    assert!(result.output.is_some(), "expected Faber output from Faber re-emission");
}

#[test]
fn custodi_multi_guard_emits_guards_in_source_order() {
    let session = test_session(Target::HirRust);
    let source = r#"incipit {
  fixum numerus x ← 5
  custodi {
    si x ≺ 0 ergo { nota "neg" }
    si x ≡ 0 ergo { nota "zero" }
  }
  nota x
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "{:?}", result.diagnostics);
    let output = super::rust_output(result);
    // Verify guards appear in source order in the emitted code.
    let neg_pos = output.code.find("neg").unwrap_or(usize::MAX);
    let zero_pos = output.code.find("zero").unwrap_or(usize::MAX);
    assert!(neg_pos < zero_pos, "negative guard should appear before zero guard");
}

#[test]
#[cfg(feature = "hir-faber")]
fn structured_cape_iace_fac_custodi_compile_for_faber_target() {
    let source = r#"functio probe(numerus n) → vacuum ⇥ textus {
  fac {
    si n ≡ 0 {
      iace "zero"
    }
    nota "ok"
  } cape err {
    nota err
  }
}"#;
    let result = super::faber_emit("test.fab", source);

    assert!(result.success(), "{:?}", result.diagnostics);
}

#[test]
#[cfg(feature = "hir-faber")]
fn ergo_iace_mori_and_tacet_compile_for_faber_target() {
    let source = r#"functio probe(bivalens ok) → vacuum ⇥ textus {
  si ok ergo iace "nope"
  mori "end"
  tacet
}"#;
    let result = super::faber_emit("test.fab", source);

    assert!(result.success(), "{:?}", result.diagnostics);
}

// ── target-specific feature gating ──

#[test]
fn rust_target_accepts_fac_cape_and_iace() {
    let session = test_session(Target::HirRust);
    let source = r#"functio probe(bivalens ok) → vacuum {
  fac {
    si ok {
      nota "ok"
    }
  } cape err {
    nota err
  }
}

incipit {
  probe(verum)
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "{:?}", result.diagnostics);
    assert!(matches!(result.output, Some(crate::Output::Rust(_))));
}

#[test]
fn rust_target_accepts_si_cape_attachments() {
    let session = test_session(Target::HirRust);
    let source = r#"functio probe(bivalens ok) → vacuum {
  si ok {
    nota "ok"
  } cape err {
    nota err
  }
}

incipit {
  probe(verum)
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "{:?}", result.diagnostics);
    assert!(matches!(result.output, Some(crate::Output::Rust(_))));
    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.issue() != Some("rust_target_cape_unsupported")),
        "si cape should emit; got: {:?}",
        result.diagnostics
    );
}

#[test]
fn rust_target_accepts_elige_cape_attachments() {
    let session = test_session(Target::HirRust);
    let source = r"functio probe(numerus x) → vacuum {
  elige x {
    casu 1 {
      tacet
    }
  } cape err {
    nota err
  }
}

incipit {
  probe(1)
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "{:?}", result.diagnostics);
    assert!(matches!(result.output, Some(crate::Output::Rust(_))));
}

#[test]
fn rust_target_accepts_lege_expression() {
    let session = test_session(Target::HirRust);
    let source = r"incipit {
  fixum _ x ← lege
  nota x
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    assert!(matches!(result.output, Some(crate::Output::Rust(_))));
}

#[test]
#[cfg(feature = "hir-faber")]
fn regex_conversio_no_longer_reports_unsupported_lowering_error() {
    let source = r#"incipit {
  fixum _ pattern ← "(?g)\d+" ↦ regex
  nota pattern
}"#;
    let result = super::faber_emit("test.fab", source);

    assert!(result.success());
    let Some(output) = result.output else {
        panic!("expected faber output");
    };
    let code = output.code;
    assert!(code.contains("\"(?g)\\d+\""));
    assert!(code.contains("↦ regex"));
    assert!(result.diagnostics.iter().all(|d| {
        d.issue()
            .is_none_or(|issue| issue != "unsupported_expression_kind")
    }));
}

#[test]
fn sed_expression_fails_with_generic_unknown_identifier() {
    let session = test_session(Target::HirRust);
    let source = r#"incipit { fixum _ r ← sed "\d+" g }"#;
    let result = compile(&session, "sed-retired.fab", source, ModuleCompileRequest::default());
    // `sed` is a plain identifier; the source fails with generic
    // unknown-identifier errors.
    assert!(!result.success());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diag| diag.code == Some("SEM001"))
    );
}

#[test]
fn warning_only_semantic_diagnostics_still_emit_output() {
    let session = test_session(Target::HirRust);
    let source = r"incipit {
  fixum _ unused ← 1
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(matches!(result.output, Some(crate::Output::Rust(_))));
    assert!(result.diagnostics.iter().any(|d| !d.is_error()));
    assert!(result.diagnostics.iter().all(|d| !d.is_error()));
}

// ── textus concatenation (slow, spawns external toolchain) ──

#[ignore = "slow: spawns external toolchain smoke (cargo/go/tsc); run with --ignored"]
#[test]
fn compile_accepts_textus_concatenation_and_explicit_append() {
    let session = test_session(Target::HirRust);
    let source = r#"incipit {
  varia textus s ← "salve"
  s ← s + "!"
  nota "dicit: " + s
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success());
    let output = super::rust_output(result);
    crate::backend_smoke::assert_hir_emitted_smoke(Target::HirRust, &output.code, "textus-concat");
}

#[ignore = "slow: spawns external toolchain smoke (cargo/go/tsc); run with --ignored"]
#[test]
fn compile_accepts_textus_explicit_append_with_ascii_operand() {
    let session = test_session(Target::HirRust);
    let source = r#"incipit {
  varia textus buf ← "x-"
  fixum _ tag ← 'suffix'
  buf ← buf + tag
  nota buf
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success());
    crate::backend_smoke::assert_hir_emitted_smoke(
        Target::HirRust,
        &super::rust_output(result).code,
        "textus-append-ascii",
    );
}

// ── Go target feature gating ──

#[test]
fn go_target_rejects_ad_with_explicit_policy_diagnostic() {
    let session = test_session(Target::HirGo);
    let source = r#"incipit {
  ad "fasciculus:lege" ("hello.txt") → textus response ⇥ textus {
    nota response
  }
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.output.is_none());
    assert!(!result.success());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.is_error() && d.code == Some("PARSE020") && d.issue() == Some("ad_expression_only"))
    );
}

#[test]
fn go_target_rejects_externa_ignotum_binding_before_target_policy() {
    let session = test_session(Target::HirGo);
    let source = r"@ externa
fixum ignotum process

incipit {
  nota process.argv
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.output.is_none());
    assert!(!result.success());
    // `@ externa` is a retired surface; annotation-contract resolution now
    // rejects it as an unknown contract before any Go-target policy runs.
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.issue() == Some("annotatio_unknown_contract"))
    );
}

#[test]
fn go_target_rejects_bodyless_externa_at_parse_boundary() {
    let session = test_session(Target::HirGo);
    let source = r"@ externa
functio argv() → ignotum

incipit {
  fixum _ args ← argv() ∷ lista<textus>
  nota args.longitudo()
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(!result.success());
    assert!(result.output.is_none());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diag| diag.code == Some("PARSE012"))
    );
}

// ── retired syntax / legacy ──

#[test]
fn ab_pipeline_is_not_active_rust_syntax() {
    let session = test_session(Target::HirRust);
    let source = r"incipit {
  fixum _ users ← [{ activus = verum }]
  fixum _ active ← ab users activus
  nota active
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(!result.success());
    assert!(result.output.is_none());
}

#[test]
fn triple_quote_is_not_a_block_string_surface() {
    let session = test_session(Target::HirRust);
    // A bare `"""` fails as an ordinary unterminated string.
    let source = "incipit {\n  fixum _ value ← \"\"\"\n  nota value\n}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(!result.success());
    assert!(result.diagnostics.iter().any(|d| d.code == Some("LEX001")));
}

#[test]
fn slash_line_comment_is_not_faber_syntax() {
    let session = test_session(Target::HirRust);
    let source = "incipit {\n  // old comment\n  nota \"ok\"\n}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(!result.success());
    assert!(result.output.is_none());
}

#[test]
fn slash_block_comment_is_not_faber_syntax() {
    let session = test_session(Target::HirRust);
    let source = "incipit {\n  /* old block */\n  nota \"ok\"\n}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(!result.success());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| { d.is_error() && d.code == Some("LEX006") && d.issue() == Some("c_style_block_comment") })
    );
}

// ── finge / ad legacy / destructuring ──

#[test]
fn compile_accepts_finge_variant_construction() {
    let session = test_session(Target::HirRust);
    let source = r"discretio Event {
  Click { numerus x numerus y },
  Quit
}

incipit {
  fixum Event e1 ← finge Click { x = 1, y = 2 } ∷ Event
  fixum Event e2 ← finge Quit ∷ Event
  nota e1
  nota e2
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success());
    assert!(matches!(result.output, Some(crate::Output::Rust(_))));
}

#[test]
fn rejects_statement_level_legacy_ad_string_route() {
    let session = test_session(Target::HirRust);
    let source = r#"incipit {
  ad "notificatio:mitte" ("User logged in", userId)
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(!result.success());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.is_error() && d.code == Some("PARSE020") && d.issue() == Some("ad_expression_only"))
    );
}

#[test]
fn compile_accepts_array_and_ex_destructuring_bindings() {
    let session = test_session(Target::HirRust);
    let source = r#"genus Persona { varia textus name }
incipit {
  fixum _ xs ← [1, 2]
  fixum [a, b] ← xs
  nota a
  nota b

  fixum _ person ← Persona { name = "Marcus" }
  ex person fixum name
  nota name
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success());
    assert!(matches!(result.output, Some(crate::Output::Rust(_))));
}

#[test]
fn enum_member_access_no_longer_reports_unknown_identifier() {
    let session = test_session(Target::HirRust);
    let source = r#"ordo Color { rubrum, viridis }

incipit {
  fixum _ color ← Color.rubrum
  elige color {
    casu Color.rubrum { nota "r" }
    casu Color.viridis { nota "v" }
  }
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "{:?}", result.diagnostics);
    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.issue() != Some("unknown_identifier"))
    );
}

#[test]
fn enum_variant_path_resolves_but_bare_enum_name_reports_name_not_value() {
    let session = test_session(Target::HirRust);
    // `Color.rubrum` must resolve clean; bare `Color` in expr position
    // must still report name_not_value under F-069.
    let source = r"ordo Color { rubrum, viridis }

incipit {
  fixum _ c ← Color.rubrum
  fixum _ e ← Color
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    // Bare `Color` is not a value — expect a name_not_value diagnostic.
    let name_not_value_issues: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.issue() == Some("name_not_value"))
        .collect();
    assert!(
        !name_not_value_issues.is_empty(),
        "expected name_not_value for bare `Color`, got {:?}",
        result.diagnostics
    );

    // `Color.rubrum` must NOT produce name_not_value or unknown_identifier.
    let member_issues: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.issue() == Some("unknown_identifier") || d.issue() == Some("name_not_value"))
        .collect();
    assert_eq!(
        member_issues.len(),
        1,
        "expected exactly one name_not_value (bare Color), got {member_issues:?}"
    );
}

#[test]
fn compile_accepts_param_alias_binding() {
    let session = test_session(Target::HirRust);
    let source = r#"functio greet(textus name, bivalens formal sponte ut f) → vacuum {
  si f {
    nota name
  }
}

incipit {
  greet("A", verum)
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success());
    assert!(matches!(result.output, Some(crate::Output::Rust(_))));
}

#[test]
fn est_verum_and_falsum_are_rejected_as_value_targets() {
    // `est` tests a type (DECISIONS D23.1); the boolean literals are values,
    // so both spellings are rejected by name rather than compared.
    let session = test_session(Target::HirRust);
    let source = r#"functio check(ignotum x) → vacuum {
  si x est verum { nota "t" }
  si x est falsum { nota "f" }
}

incipit {
  check(verum)
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    let hits = result
        .diagnostics
        .iter()
        .filter(|d| d.issue() == Some("est_value_rhs"))
        .count();
    assert_eq!(hits, 2, "{:?}", result.diagnostics);
}

#[test]
fn est_value_literals_name_equality() {
    // `est` tests types (DECISIONS D23.1); a null, number or string literal on
    // the right is a value, so each gets the one named diagnostic whose help
    // names the equality family.
    let session = test_session(Target::HirRust);
    for rhs in ["nulla", "1", "\"a\""] {
        let source = format!(
            "functio check(ignotum x) → vacuum {{\n  si x est {rhs} {{ nota \"t\" }}\n}}\n\nincipit {{\n  check(verum)\n}}"
        );
        let result = compile(&session, "test.fab", &source, ModuleCompileRequest::default());
        let hits: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.issue() == Some("est_value_literal_names_equality"))
            .collect();
        assert_eq!(hits.len(), 1, "{rhs}: {:?}", result.diagnostics);
        let help = radix_semantic::catalog_syntax::parse_help(radix_parser::ParseErrorKind::TypeInvalid, &hits[0].args)
            .unwrap_or_default();
        assert!(
            help.contains('≡') && help.contains('≅') && help.contains('≈'),
            "{rhs}: help must name the equality family: {help:?}"
        );
    }
}

#[test]
fn externa_top_level_var_without_initializer_is_rejected() {
    let session = test_session(Target::HirRust);
    let source = r"@ externa
fixum ignotum Bun

incipit {
  nota Bun
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(!result.success());
    // `@ externa` is a retired surface; the unknown-annotation rejection
    // (SEM018) fires before the top-level-initializer check.
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.issue() == Some("annotatio_unknown_contract"))
    );
}

#[test]
fn spread_array_argument_can_satisfy_multi_parameter_function() {
    let session = test_session(Target::HirRust);
    let source = r"functio add(numerus a, numerus b) → numerus {
  redde a + b
}

incipit {
  fixum lista<numerus> values ← [3, 7]
  nota add(sparge values)
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.issue() != Some("wrong_number_of_arguments"))
    );
    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.issue() != Some("argument_type_mismatch"))
    );
}

#[test]
fn import_alias_usage_no_longer_reports_unknown_identifier() {
    let session = test_session(Target::HirRust);
    let source = r#"importa ex "./norma/consolum" consolum

    incipit {
  consolum.fundeLineam("x")
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(matches!(result.output, Some(crate::Output::Rust(_))));
    assert!(result.diagnostics.iter().all(|d| !d.is_error()));
    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.issue() != Some("unknown_identifier"))
    );
}

#[test]
fn duplicate_import_module_bindings_no_longer_report_duplicate_definition() {
    let session = test_session(Target::HirRust);
    let source = r#"importa ex "./helpers" item
importa ex "./helpers" item

incipit {
  nota "ok"
}"#;
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "{:?}", result.diagnostics);
    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.issue() != Some("duplicate_definition"))
    );
}

#[test]
fn externa_ignotum_receiver_no_longer_reaches_type_inference() {
    let session = test_session(Target::HirRust);
    let source = r"@ externa
fixum ignotum process

incipit {
  fixum _ args ← process.argv ∷ lista<textus>
  nota args.longitudo()
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(!result.success());
    // `@ externa` is a retired surface; the unknown-annotation rejection
    // (SEM018) now fires before the receiver ever reaches type inference.
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.issue() == Some("annotatio_unknown_contract"))
    );
}

// ── ex object varia reassignment ──

#[test]
fn ex_object_varia_bindings_accept_same_type_reassignment() {
    let session = test_session(Target::HirRust);
    let source = r"genus Datum { varia numerus count  varia bivalens active }
incipit {
  fixum _ data ← Datum { count = 100, active = verum }
  ex data varia count, active
  count ← 200
  active ← falsum
  nota count, active
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success());
    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.issue() != Some("expression_type_mismatch"))
    );
    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.issue() != Some("assignment_type_mismatch"))
    );
}

// ── LOCALE002 edit-distance suggestions fire only at an error span (WB-1) ──

fn locale002_spellings(diagnostics: &[crate::diagnostics::Diagnostic]) -> Vec<String> {
    diagnostics
        .iter()
        .filter(|d| d.code == Some("LOCALE002"))
        .filter_map(|d| {
            d.args
                .iter()
                .find(|arg| arg.name == "spelling")
                .map(|arg| arg.value.clone())
        })
        .collect()
}

fn compile_with_installed_pack(locale: &str, source: &str) -> crate::CompileResult {
    let pack_path = crate::locale::workspace_locale_pack_path(locale);
    let pack = crate::locale::LocalePack::from_toml_path(pack_path).expect("installed pack should load");
    let session = Session::new(
        Config::default()
            .with_target(Target::HirRust)
            .with_locale_pack(pack),
    );
    compile(&session, "test.fab", source, ModuleCompileRequest::default())
}

#[test]
fn compile_valid_identifier_near_keyword_draws_no_locale002() {
    // `clamp` is within the edit-distance threshold of an English keyword, but
    // it is a valid local name: no error sits at its span, so no guess.
    let result = compile_with_installed_pack("en", "main {\n  const int clamp ← 1\n  print clamp\n}\n");

    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    assert!(
        locale002_spellings(&result.diagnostics).is_empty(),
        "a valid identifier must not draw LOCALE002: {:?}",
        result.diagnostics
    );
}

#[test]
fn compile_misspelled_name_at_resolve_error_keeps_locale002() {
    // The valid `clamp` stays quiet; the misspelled `clamq` is rejected by the
    // resolver at its own span, so its guess is shown.
    let result = compile_with_installed_pack("en", "main {\n  const int clamp ← 1\n  print clamq\n}\n");

    assert!(!result.success());
    assert_eq!(
        locale002_spellings(&result.diagnostics),
        vec!["clamq".to_string()],
        "diagnostics: {:?}",
        result.diagnostics
    );
}

#[test]
fn compile_misspelled_keyword_at_parse_error_keeps_locale002() {
    // `retorn` is one edit from the English `return`; the parser rejects it.
    let result = compile_with_installed_pack("en", "main {\n    print 1 retorn\n}\n");

    assert!(!result.success());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.is_error() && d.phase == crate::diagnostics::DiagnosticPhase::Parse),
        "expected a parse error: {:?}",
        result.diagnostics
    );
    assert_eq!(
        locale002_spellings(&result.diagnostics),
        vec!["retorn".to_string()],
        "diagnostics: {:?}",
        result.diagnostics
    );
}
