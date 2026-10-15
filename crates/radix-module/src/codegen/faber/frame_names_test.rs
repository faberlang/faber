//! DECISIONS D6.16 (ad-serving U1): English names for the conversation family.
//!
//! Proves: the en pack reads `channel` / `frame` / `send<T>` / `recv<T>` as
//! the Latin `sermo` / `scrinium` / `meus<T>` / `tuus<T>` (type positions and
//! the sermo view methods); the words stay ordinary identifiers everywhere
//! else; and the HIR-backed Faber emitter re-spells the family per target
//! pack, so la→en→la round-trips.

use crate::codegen::Target;
use crate::driver::reemit_faber;
use crate::driver::{Config, Session};
use crate::locale::LocalePack;
use crate::services::check_diagnostics_for_source_with_locale_pack;

fn installed_pack(id: &str) -> LocalePack {
    LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path(id)).expect("installed pack should load")
}

fn assert_checks(name: &str, pack_id: &str, source: &str) {
    let pack = installed_pack(pack_id);
    let result = check_diagnostics_for_source_with_locale_pack(name, source, false, Some(&pack), None);
    assert!(
        result.success,
        "{name} must check under {pack_id}: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.code, d.issue(), d.args.clone()))
            .collect::<Vec<_>>()
    );
}

fn convert(pack_id: &str, source: &str) -> String {
    let pack = installed_pack(pack_id);
    let session = Session::new(
        Config::default()
            .with_target(Target::HirFaber)
            .with_dev_locale_root(),
    );
    let result = reemit_faber(&session, &pack, "frame-names.fab", source);
    assert!(
        result.success(),
        "convert to {pack_id} must succeed: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.code, d.issue(), d.args.clone()))
            .collect::<Vec<_>>()
    );
    result.output.expect("convert output").code
}

const EN_FAMILY: &str = r#"+++
locale = "en"
+++
fn drain(channel s) → int {
    mut int n ← 0
    for from s.recv<string>().cursor() let f {
        n ← n + 1
    }
    return n
}

main {
    let channel s ← call 'runtime:echo' ("salve")
    let send<value> outbound ← s.send<value>()
    let status closed ← outbound.fini()
    let recv<string> t ← s.recv<string>()
    let frame<string> ∪ none received ← t.accipe()
    print closed
    print received
    print drain(call 'runtime:echo' ("salve"))
}
"#;

const LA_FAMILY: &str = r#"+++
locale = "la"
+++
functio drain(sermo s) → numerus {
    varia numerus n ← 0
    itera ex s.tuus<textus>().cursor() sit f {
        n ← n + 1
    }
    redde n
}

incipit {
    sit sermo s ← ad 'runtime:echo' ("salve")
    sit meus<valor> outbound ← s.meus<valor>()
    sit status closed ← outbound.fini()
    sit tuus<textus> t ← s.tuus<textus>()
    sit scrinium<textus> ∪ nihil received ← t.accipe()
    nota closed
    nota received
    nota drain(ad 'runtime:echo' ("salve"))
}
"#;

#[test]
fn en_family_names_check() {
    assert_checks("en-family.fab", "en", EN_FAMILY);
}

#[test]
fn la_family_names_still_check() {
    assert_checks("la-family.fab", "la", LA_FAMILY);
}

#[test]
fn en_family_words_stay_ordinary_identifiers() {
    // Type names are not reserved: existing programs that bind `channel`,
    // `frame`, `send` or `recv` as values, functions, or fields stay legal.
    let source = r#"+++
locale = "en"
+++
class Packet {
    mut int frame = 0
    mut string channel = ""
}

fn send(int recv) → int {
    return recv + 1
}

main {
    let int frame ← 3
    let string channel ← "c"
    let Packet p ← Packet { frame = frame, channel = channel }
    let int recv ← send(p.frame)
    print recv
    print channel
    for from [1, 2] let frame {
        print frame
    }
}
"#;
    assert_checks("en-family-idents.fab", "en", source);
}

#[test]
fn convert_la_to_en_spells_family_in_en() {
    let en = convert("en", LA_FAMILY);
    for needle in [
        "fn drain(channel s)",
        "let channel s ←",
        "let send<value> outbound ← s.send<value>()",
        "let recv<string> t ← s.recv<string>()",
        "let frame<string> ∪ none received",
        "s.recv<string>().cursor()",
    ] {
        assert!(en.contains(needle), "en convert must contain `{needle}`:\n{en}");
    }
    for latin in ["sermo", "scrinium", "meus", "tuus"] {
        assert!(!en.contains(latin), "en convert must not keep `{latin}`:\n{en}");
    }
    assert_checks("converted-en.fab", "en", &en);
}

#[test]
fn convert_en_to_la_round_trips_family() {
    // Re-emit output is the body only; the reader locale rides frontmatter.
    let en = format!("+++\nlocale = \"en\"\n+++\n{}", convert("en", LA_FAMILY));
    let la = convert("la", &en);
    for needle in [
        "functio drain(sermo s)",
        "sit sermo s ←",
        "sit meus<valor> outbound ← s.meus<valor>()",
        "sit tuus<textus> t ← s.tuus<textus>()",
        "sit scrinium<textus> ∪ nihil received",
    ] {
        assert!(la.contains(needle), "la convert must contain `{needle}`:\n{la}");
    }
    for en_word in ["channel", "frame<", "send<", "recv<"] {
        assert!(!la.contains(en_word), "la convert must not keep `{en_word}`:\n{la}");
    }
    assert_checks("converted-la.fab", "la", &la);
}

#[test]
fn convert_en_family_source_to_la() {
    let la = convert("la", EN_FAMILY);
    assert!(la.contains("sermo s"), "{la}");
    assert!(la.contains("s.tuus<textus>()"), "{la}");
    assert!(la.contains("s.meus<valor>()"), "{la}");
    assert!(la.contains("scrinium<textus>"), "{la}");
}

#[test]
fn en_view_method_alone_registers_frame_types() {
    // A module that never opens `ad` but calls `s.recv<T>()` must still see
    // the builtin conversation family, as the Latin `s.tuus<T>()` does.
    let source = r#"+++
locale = "en"
+++
fn drain(channel s) → int {
    mut int n ← 0
    for from s.recv<string>().cursor() let f {
        n ← n + 1
    }
    return n
}
"#;
    assert_checks("en-view-no-ad.fab", "en", source);
}
