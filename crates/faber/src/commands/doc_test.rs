use super::{DocFile, DocItem, document_program, render_json, render_text};

/// Parse a Latin-surface body and document it (no frontmatter, no package).
fn doc_for(source: &str) -> DocFile {
    let lexed = radix::module::lexer::lex(source);
    assert!(lexed.success(), "lex failed: {:?}", lexed.errors);
    let parsed = radix::module::parser::parse(lexed);
    assert!(parsed.success(), "parse failed: {:?}", parsed.errors);
    let program = parsed.program.expect("program");
    document_program("probe.fab", None, &program, &parsed.interner, source)
}

fn item<'a>(file: &'a DocFile, name: &str) -> &'a DocItem {
    file.items
        .iter()
        .find(|item| item.name == name)
        .unwrap_or_else(|| panic!("no documented item `{name}` in {:?}", file.items))
}

#[test]
fn comment_block_directly_above_is_the_doc() {
    let file = doc_for(
        "# Adds one.\n# Second line.\n@ publica\nfunctio adde(numerus a) → numerus {\n    redde a + 1\n}\n",
    );
    let adde = item(&file, "adde");
    assert_eq!(adde.doc.as_deref(), Some("Adds one.\nSecond line."));
    assert_eq!(adde.summary.as_deref(), Some("Adds one."));
    assert_eq!(adde.signature, "functio adde(numerus a) → numerus");
    assert_eq!(adde.annotations, vec!["@ publica".to_owned()]);
}

#[test]
fn comment_block_followed_by_blank_line_is_a_note() {
    let file = doc_for(
        "# ==== SECTION ====\n\n@ publica\nfunctio adde(numerus a) → numerus {\n    redde a + 1\n}\n",
    );
    assert_eq!(item(&file, "adde").doc, None);
}

#[test]
fn only_the_block_nearest_the_declaration_is_the_doc() {
    let file = doc_for(
        "# A note.\n\n# The doc.\n@ publica\nfunctio adde(numerus a) → numerus {\n    redde a + 1\n}\n",
    );
    assert_eq!(item(&file, "adde").doc.as_deref(), Some("The doc."));
}

#[test]
fn private_declarations_are_never_extracted() {
    let file = doc_for(
        "# Hidden.\nfunctio occulta() → numerus {\n    redde 1\n}\n\n# Also hidden.\n@ privata\nfunctio privata_f() → numerus {\n    redde 2\n}\n\n# Package reach.\n@ interna\nfunctio intra() → numerus {\n    redde 3\n}\n",
    );
    let names: Vec<&str> = file.items.iter().map(|item| item.name.as_str()).collect();
    assert_eq!(names, vec!["intra"]);
    assert_eq!(item(&file, "intra").visibility, "interna");
}

#[test]
fn statement_comments_inside_bodies_are_never_extracted() {
    let file = doc_for(
        "# Doc.\n@ publica\nfunctio f() → numerus {\n    # TODO: inner note\n    redde 1\n}\n",
    );
    let rendered = render_text(std::slice::from_ref(&file));
    assert!(!rendered.contains("TODO"), "{rendered}");
}

#[test]
fn genus_members_render_under_their_type_unless_privata() {
    let file = doc_for(
        "# A point.\n@ publica\ngenus Punctum {\n    # Horizontal.\n    sit numerus x\n    @ privata\n    sit numerus secretum\n    @ interna\n    sit numerus intra\n\n    # Length from origin.\n    functio longitudo() → numerus {\n        redde ego.x\n    }\n}\n",
    );
    let punctum = item(&file, "Punctum");
    assert_eq!(punctum.kind, "genus");
    assert_eq!(punctum.doc.as_deref(), Some("A point."));
    let members: Vec<(&str, &str, &str)> = punctum
        .members
        .iter()
        .map(|member| (member.kind, member.name.as_str(), member.visibility))
        .collect();
    assert_eq!(
        members,
        vec![
            ("field", "x", "publica"),
            ("field", "intra", "interna"),
            ("method", "longitudo", "publica"),
        ]
    );
    assert_eq!(punctum.members[0].doc.as_deref(), Some("Horizontal."));
    assert_eq!(punctum.members[0].signature, "sit numerus x");
    assert_eq!(
        punctum.members[2].signature,
        "functio longitudo() → numerus"
    );
    assert_eq!(
        punctum.members[2].doc.as_deref(),
        Some("Length from origin.")
    );
}

#[test]
fn module_doc_anchors_on_regio() {
    let file = doc_for(
        "# Geometry helpers.\nregio geometria\n\n@ publica\nfunctio f() → numerus {\n    redde 1\n}\n",
    );
    let module = file.module.as_ref().expect("module");
    assert_eq!(module.name, "geometria");
    assert_eq!(module.doc.as_deref(), Some("Geometry helpers."));
}

#[test]
fn file_without_regio_has_no_module_doc() {
    let file = doc_for("# Leading block.\n\n@ publica\nfunctio f() → numerus {\n    redde 1\n}\n");
    assert!(file.module.is_none());
}

#[test]
fn summary_is_the_first_sentence() {
    let file = doc_for(
        "# Adds one. Never overflows.\n@ publica\nfunctio adde(numerus a) → numerus {\n    redde a + 1\n}\n",
    );
    assert_eq!(item(&file, "adde").summary.as_deref(), Some("Adds one."));
}

#[test]
fn json_shape_is_pinned() {
    let file = doc_for(
        "# Geo.\nregio geo\n\n# Adds one.\n@ publica\nfunctio adde(numerus a) → numerus {\n    redde a + 1\n}\n",
    );
    let json = render_json(&[file]).expect("json");
    assert_eq!(
        json,
        concat!(
            r#"{"schema_version":1,"files":[{"path":"probe.fab","locale":null,"#,
            r#""module":{"name":"geo","doc":"Geo."},"#,
            r#""items":[{"kind":"function","name":"adde","visibility":"publica","#,
            r#""annotations":["@ publica"],"signature":"functio adde(numerus a) → numerus","#,
            r#""doc":"Adds one.","summary":"Adds one.","members":[]}]}]}"#,
            "\n"
        )
    );
}

#[test]
fn english_file_renders_in_its_own_locale() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("geo.fab");
    std::fs::write(
        &path,
        "+++\nlocale = \"en\"\n+++\n\n# Geometry.\nmodule geometry\n\n# Adds one.\n@ public\nfn add(int a) → int {\n    return a + 1\n}\n",
    )
    .expect("write");
    let files = super::document_input(&path).expect("doc");
    assert_eq!(files.len(), 1);
    let file = &files[0];
    assert_eq!(file.locale.as_deref(), Some("en"));
    assert_eq!(
        file.module.as_ref().map(|m| m.name.as_str()),
        Some("geometry")
    );
    let add = item(file, "add");
    assert_eq!(add.signature, "fn add(int a) → int");
    assert_eq!(add.annotations, vec!["@ public".to_owned()]);
}

#[test]
fn text_render_lists_signature_and_doc() {
    let file = doc_for(
        "# Adds one.\n@ publica\nfunctio adde(numerus a) → numerus {\n    redde a + 1\n}\n",
    );
    let text = render_text(&[file]);
    assert!(
        text.contains("@ publica\nfunctio adde(numerus a) → numerus\n    Adds one.\n"),
        "{text}"
    );
}
