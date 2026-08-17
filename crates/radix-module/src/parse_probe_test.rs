#[test]
fn parse_probe_qualified_constructor() {
    let source = "importa ex \"./util\" * ut utilModule\n\nfunctio run() → textus {\n    fixum _ persona ← utilModule.Persona { nomen = \"salve\" }\n    redde utilModule.saluta(persona)\n}\n\nincipit {\n    nota run()\n}\n";
    let lex_result = crate::lexer::lex(source);
    let parsed = crate::parser::parse(lex_result);
    assert!(parsed.success(), "parse must succeed; errors: {:?}", parsed.errors);
}
