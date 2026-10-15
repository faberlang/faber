//! Reader-locale round trips for every `cape` attachment form.
//!
//! `cape` attaches to `fac` (with or without `dum`), conditional arms (`si`,
//! `sin`, `secus`), `dum`, `itera`, and `elige`. Each source converts
//! en→la and back la→en through [`reemit_faber`] (which re-validates every
//! emitted body), and en→en format must agree with the round-tripped en so the
//! handler attachment is preserved rather than moved to a stray statement.

use crate::codegen::Target;
use crate::driver::reemit_faber;
use crate::driver::{Config, Session};
use crate::locale::LocalePack;

fn pack(locale: &str) -> LocalePack {
    LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path(locale))
        .unwrap_or_else(|err| panic!("installed {locale} pack should load: {err:?}"))
}

fn reemit(source: &str, to: &str) -> String {
    let session = Session::new(
        Config::default()
            .with_target(Target::HirFaber)
            .with_dev_locale_root(),
    );
    let result = reemit_faber(&session, &pack(to), "test.fab", source);
    assert!(
        result.success(),
        "source must re-emit to {to} and re-validate:\n{source}\n{:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.code, d.issue(), d.args.clone()))
            .collect::<Vec<_>>()
    );
    result
        .output
        .expect("successful re-emit must carry output")
        .code
}

/// Round-trip `body` (an en document without frontmatter) and return the
/// Latin conversion for shape assertions.
fn assert_round_trips(body: &str) -> String {
    let en = format!("+++\nlocale = \"en\"\n+++\n{body}");
    let la = reemit(&en, "la");
    let back = reemit(&la, "en");
    let formatted = reemit(&en, "en");
    assert_eq!(
        back, formatted,
        "en→la→en must agree with en format (attachment preserved)\nla:\n{la}"
    );
    la
}

const RISKY: &str = "fn risky(int i) → int ⇥ string {\n    if i ≡ 1 { throw \"bad\" }\n    return i\n}\n";

#[test]
fn do_catch_while_round_trips_with_cape_before_dum() {
    let la = assert_round_trips(&format!(
        "{RISKY}main {{\n    mut int n ← 0\n    do {{\n        n ↑\n        if n ≡ 2 {{ continue }}\n        print risky(n)\n    }} catch err {{\n        print \"caught\"\n    }} while n ≺ 4\n}}\n"
    ));
    let cape_at = la.find("cape err").expect("handler must survive");
    let dum_at = la.find("dum n").expect("loop condition must survive");
    assert!(cape_at < dum_at, "cape must precede dum:\n{la}");
}

#[test]
fn if_else_arm_catches_round_trip() {
    let la = assert_round_trips(&format!(
        "{RISKY}main {{\n    let int x ← 1\n    if x ≡ 0 {{\n        print risky(0)\n    }} catch err {{\n        print \"first\"\n    }} else {{\n        print risky(1)\n    }} catch err {{\n        print \"else\"\n    }}\n}}\n"
    ));
    assert_eq!(la.matches("cape err").count(), 2, "both arm handlers must survive:\n{la}");
    assert!(
        !la.contains("ergo redde fac"),
        "else arm must not compact into ergo redde:\n{la}"
    );
}

#[test]
fn elif_arm_catch_round_trips() {
    let la = assert_round_trips(&format!(
        "{RISKY}main {{\n    let int x ← 1\n    if x ≡ 0 {{\n        print \"zero\"\n    }} elif x ≡ 1 {{\n        print risky(1)\n    }} catch err {{\n        print \"elif\"\n    }} else {{\n        print risky(1)\n    }} catch err {{\n        print \"else\"\n    }}\n}}\n"
    ));
    assert!(la.contains("sin x ≡ 1"), "handled elif arm must stay a sin arm:\n{la}");
    assert_eq!(la.matches("cape err").count(), 2, "both arm handlers must survive:\n{la}");
}

#[test]
fn then_joint_arm_catches_round_trip() {
    assert_round_trips(&format!(
        "{RISKY}main {{\n    let int x ← 1\n    if x ≡ 1 then print risky(1) catch err {{\n        print \"then\"\n    }}\n    if x ≡ 0 {{ print \"zero\" }} else then print risky(1) catch err {{\n        print \"else\"\n    }}\n}}\n"
    ));
}

#[test]
fn loop_and_switch_catches_round_trip() {
    assert_round_trips(&format!(
        "{RISKY}main {{\n    mut int n ← 0\n    while n ≺ 3 {{\n        print risky(n)\n        n ↑\n    }} catch err {{\n        print \"while\"\n    }}\n    for range 0‥3 const i {{\n        print risky(i)\n    }} catch err {{\n        print \"range\"\n    }}\n    let list<int> xs ← [0, 1, 2]\n    for from xs const x {{\n        print risky(x)\n    }} catch err {{\n        print \"from\"\n    }}\n    switch n {{\n        case 1 {{ print risky(1) }}\n        default {{ print \"other\" }}\n    }} catch err {{\n        print \"switch\"\n    }}\n}}\n"
    ));
}
