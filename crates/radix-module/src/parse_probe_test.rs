/// The built-in Latin (`la`) reader pack: the canonical surface the parser
/// tests below are written in.
fn la_pack() -> crate::locale::LocalePack {
    crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("la"))
        .expect("installed la pack should load")
}

#[test]
fn parse_probe_qualified_constructor() {
    let source = "importa ex \"./util\" * ut utilModule\n\nfunctio run() → textus {\n    sit _ persona ← utilModule.Persona { nomen = \"salve\" }\n    redde utilModule.saluta(persona)\n}\n\nincipit {\n    nota run()\n}\n";
    let lex_result = crate::lexer::lex_with_locale_pack(source, &la_pack());
    let parsed = crate::parser::parse(lex_result);
    assert!(parsed.success(), "parse must succeed; errors: {:?}", parsed.errors);
}
