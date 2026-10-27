//! inf track U2: the analyzed-HIR package (FHIR) carries the unbounded
//! integer and its four authored spellings. The FHIR presentation validator
//! admits the `wrapping<inf>` / `saturating<inf>` levels over the one `inf`
//! witness, and a loaded unit re-emits the author's spelling.

use super::*;
use crate::codegen::{Target, generate_from_analyzed};
use crate::driver::{AnalyzeRequest, Config, Session, analyze_source};
use crate::locale::{KeywordSurface, latin_locale_pack};

const INF_SOURCE: &str = r"
functio tere(inf a, exactus<inf> b, modulus<inf> c, saturatus<inf> d) → inf {
    sit inf plain ← a
    sit exactus<inf> t ← b
    sit modulus<inf> w ← c
    sit saturatus<inf> s ← d
    redde plain
}
";

#[test]
fn an_inf_module_round_trips_through_the_package_with_its_spellings() {
    let session = Session::new(Config::default().with_latin_locale());
    let analysis =
        analyze_source(&session, "main.fab", INF_SOURCE, AnalyzeRequest::default()).expect("analyze inf source");
    let pkg = build_package(
        PackageIdentityWire { name: "inf".to_owned(), version: "1.0.0".to_owned(), edition: "2026".to_owned() },
        "main.fab",
        None,
        vec![PackageUnitInput {
            relative_path: "main.fab".to_owned(),
            module_segments: vec!["main.fab".to_owned()],
            is_entry: true,
            export_names: vec![],
            local_links: vec![],
            library_imports: vec![],
            content_hash: "sha256:inf".to_owned(),
            analysis: &analysis,
        }],
        vec![],
    )
    .expect("build package");
    let bytes = encode_package_msgpack(&pkg).expect("encode package");
    let loaded = load_package(&bytes).expect("the presentation validator must admit inf levels");

    let latin = latin_locale_pack();
    let surface = KeywordSurface::new(&latin);
    let unit = &loaded.modules[0].unit;
    let crate::Output::Faber(output) = generate_from_analyzed(Target::HirFaber, unit, &surface).expect("faber re-emit")
    else {
        panic!("expected Faber output");
    };
    for spelling in [
        "inf a",
        "exactus<inf> b",
        "modulus<inf> c",
        "saturatus<inf> d",
        "sit exactus<inf> t",
        "sit modulus<inf> w",
        "sit saturatus<inf> s",
    ] {
        assert!(output.code.contains(spelling), "lost `{spelling}`:\n{}", output.code);
    }
}
