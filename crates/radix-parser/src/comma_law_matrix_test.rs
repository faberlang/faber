//! CS-4 accept/reject matrix for every CS-1 parser comma position.
//!
//! Completeness oracle: `docs/archived/comma-separator-law/audit.md` §4
//! (37 parser sites). Each row has ≥1 lawful accept source and ≥1 reject
//! source (missing separator, trailing comma, or an inserted declaration-block
//! comma). Fixed-shape sites reject with their existing targeted issues.

use crate::test_support::{parse_ok_la, parse_program_la};

const CS1_ROWS: u8 = 37;

/// Rows 33 (`discerne` subjects) and 36 (arm-head patterns) are no longer
/// comma-law positions: `et` is the only separator there and a comma is the
/// named migration diagnostic (`discerne_subject_comma_retired`,
/// `discerne_pattern_comma_retired`, pinned in `discerne_separator_test.rs`).
const RETIRED_ROWS: [u8; 2] = [33, 36];

#[derive(Clone, Copy, Debug)]
enum Expect {
    Accept,
    Trailing(&'static str),
    Missing(&'static str),
    Inserted(&'static str),
    FixedIssue(&'static str),
    Rejected,
}

struct Case {
    row: u8,
    source: &'static str,
    expect: Expect,
}

const fn accept(row: u8, source: &'static str) -> Case {
    Case {
        row,
        source,
        expect: Expect::Accept,
    }
}

const fn trailing(row: u8, position: &'static str, source: &'static str) -> Case {
    Case {
        row,
        source,
        expect: Expect::Trailing(position),
    }
}

const fn missing(row: u8, position: &'static str, source: &'static str) -> Case {
    Case {
        row,
        source,
        expect: Expect::Missing(position),
    }
}

const fn inserted(row: u8, position: &'static str, source: &'static str) -> Case {
    Case {
        row,
        source,
        expect: Expect::Inserted(position),
    }
}

const fn fixed_issue(row: u8, issue: &'static str, source: &'static str) -> Case {
    Case {
        row,
        source,
        expect: Expect::FixedIssue(issue),
    }
}

const fn rejected(row: u8, source: &'static str) -> Case {
    Case {
        row,
        source,
        expect: Expect::Rejected,
    }
}

/// One accept and at least one reject for each CS-1 §4 parser site.
const CASES: &[Case] = &[
    // 1. array binding pattern elements
    accept(1, "sit [a, b] ← values"),
    trailing(1, "array_binding_pattern", "sit [a, b,] ← values"),
    missing(1, "array_binding_pattern", "sit [a b] ← values"),
    // 2. object binding pattern fields
    accept(2, "sit { a, b } ← obj"),
    trailing(2, "object_binding_pattern", "sit { a, b, } ← obj"),
    missing(2, "object_binding_pattern", "sit { a b } ← obj"),
    // 3. object rest trailing-comma guard
    accept(3, "sit { a, ceteri rest } ← obj"),
    trailing(3, "object_binding_pattern", "sit { a, ceteri rest, } ← obj"),
    // 4. array rest trailing-comma guard
    accept(4, "sit [a, ceteri rest] ← values"),
    trailing(4, "array_binding_pattern", "sit [a, ceteri rest,] ← values"),
    // 5. function parameters
    accept(5, "functio f(numerus a, numerus b) → vacuum { }"),
    trailing(
        5,
        "function_parameters",
        "functio f(numerus a, numerus b,) → vacuum { }",
    ),
    missing(
        5,
        "function_parameters",
        "functio f(numerus a numerus b) → vacuum { }",
    ),
    // 6. genus implet list
    accept(6, "genus Animal implet Canens, Currens { }"),
    trailing(
        6,
        "class_implements",
        "genus Animal implet Canens, Currens, { }",
    ),
    missing(
        6,
        "class_implements",
        "genus Animal implet Canens Currens { }",
    ),
    // 7. enum members
    accept(7, "ordo Color { Rubrum, Viridis }"),
    trailing(7, "enum_members", "ordo Color { Rubrum, Viridis, }"),
    missing(7, "enum_members", "ordo Color { Rubrum Viridis }"),
    // 8. discretio payload fields (declaration block)
    accept(
        8,
        "discretio Color { Rubrum { numerus red numerus green } }",
    ),
    inserted(
        8,
        "discretio_payload",
        "discretio Color { Rubrum { numerus red, numerus green } }",
    ),
    // 9. discretio variants
    accept(9, "discretio Color { Rubrum, Viridis }"),
    trailing(
        9,
        "discretio_variants",
        "discretio Color { Rubrum, Viridis, }",
    ),
    missing(
        9,
        "discretio_variants",
        "discretio Color { Rubrum Viridis }",
    ),
    // 10. braced import fields
    accept(10, r#"importa { ex = "norma:solum", nomen = solum }"#),
    trailing(
        10,
        "import_fields",
        r#"importa { ex = "norma:solum", nomen = solum, }"#,
    ),
    missing(
        10,
        "import_fields",
        r#"importa { ex = "norma:solum" nomen = solum }"#,
    ),
    // 11. braced annotation fields
    accept(
        11,
        r#"@ optio { binding = verbose, brevis = "v" }
functio f() → vacuum { }"#,
    ),
    trailing(
        11,
        "annotation_fields",
        r#"@ optio { binding = verbose, brevis = "v", }
functio f() → vacuum { }"#,
    ),
    missing(
        11,
        "annotation_fields",
        r#"@ optio { binding = verbose brevis = "v" }
functio f() → vacuum { }"#,
    ),
    // 12. braced nucleum field (singleton item list)
    accept(
        12,
        "@ nucleum { fragment = verum }\nfunctio k() → vacuum { }",
    ),
    trailing(
        12,
        "nucleum_fields",
        "@ nucleum { fragment = verum, }\nfunctio k() → vacuum { }",
    ),
    missing(
        12,
        "nucleum_fields",
        "@ nucleum { fragment = verum extra = 1 }\nfunctio k() → vacuum { }",
    ),
    // 13. generic declaration parameters
    accept(13, "functio f<T, U>(T a, U b) → vacuum { }"),
    trailing(
        13,
        "generic_parameters",
        "functio f<T, U,>(T a) → vacuum { }",
    ),
    missing(13, "generic_parameters", "functio f<T U>(T a) → vacuum { }"),
    // 14. gradient selectors
    accept(14, "sit _ g ← loss(x, y) ∇ [x, y]"),
    trailing(14, "gradient_selectors", "sit _ g ← loss(x, y) ∇ [x, y,]"),
    missing(14, "gradient_selectors", "sit _ g ← loss(x, y) ∇ [x y]"),
    // 15. tensor/sparsa/vector/matrix shape-sugar separator (call-position type
    // args parse as ordinary generic calls; type-name callees reject in check)
    accept(15, "functio f(tensor<f32, [2, 3]> value) → vacuum { }"),
    accept(15, "sit _ t ← tensor<f32>(x)"),
    rejected(15, "sit _ t ← tensor<f32, [2],>(x)"),
    // 16. finge named field initializers
    accept(16, r#"sit _ e ← finge Bonum { nuntius = "x", extra = 1 }"#),
    trailing(
        16,
        "variant_field_initializers",
        r#"sit _ e ← finge Bonum { nuntius = "x", extra = 1, }"#,
    ),
    missing(
        16,
        "variant_field_initializers",
        r#"sit _ e ← finge Bonum { nuntius = "x" extra = 1 }"#,
    ),
    // 17. iuncta elements
    accept(17, r#"sit _ pair ← iuncta<textus, numerus> ["salve", 42]"#),
    trailing(
        17,
        "iuncta_elements",
        r#"sit _ pair ← iuncta<textus, numerus> ["salve", 42,]"#,
    ),
    missing(
        17,
        "iuncta_elements",
        r#"sit _ pair ← iuncta<textus, numerus> ["salve" 42]"#,
    ),
    // 18. keyword-clausura signature params
    accept(18, "sit _ f ← clausura numerus a, numerus b: a"),
    accept(18, "sit _ f ← clausura numerus a { redde a }"),
    trailing(
        18,
        "closure_parameters",
        "sit _ f ← clausura numerus a, numerus b,: a",
    ),
    missing(
        18,
        "closure_parameters",
        "sit _ f ← clausura numerus a numerus b: a",
    ),
    // 19. compact-clausura param-list helper. Trailing/missing comma errors
    // are produced inside the helper, then compact-clausura speculation
    // rolls them back (Therefore has not been consumed yet) and the
    // parenthesized-expression recovery fails. The family slug for this
    // position is proven by row 18; these cases prove the helper still
    // rejects the unlawful forms.
    accept(19, "sit _ f ← (numerus a, numerus b) ∴ a"),
    rejected(19, "sit _ f ← (numerus a, numerus b,) ∴ a"),
    rejected(19, "sit _ f ← (numerus a numerus b) ∴ a"),
    // 20. call arguments
    accept(20, "sit _ x ← f(1, 2)"),
    trailing(20, "call_arguments", "sit _ x ← f(1, 2,)"),
    missing(20, "call_arguments", "sit _ x ← f(1 2)"),
    // 21. array/list elements
    accept(21, "sit _ v ← [1, 2]"),
    trailing(21, "array_elements", "sit _ v ← [1, 2,]"),
    missing(21, "array_elements", "sit _ v ← [1 2]"),
    // 22. typed object fields
    accept(
        22,
        "genus Point { varia numerus x = 0 varia numerus y = 0 }\nsit _ p ← Point { x = 10, y = 20 }",
    ),
    trailing(
        22,
        "object_fields",
        "genus Point { varia numerus x = 0 varia numerus y = 0 }\nsit _ p ← Point { x = 10, y = 20, }",
    ),
    missing(
        22,
        "object_fields",
        "genus Point { varia numerus x = 0 varia numerus y = 0 }\nsit _ p ← Point { x = 10 y = 20 }",
    ),
    // 23. JSON object members
    accept(23, r#"sit _ v ← { "a": 1, "b": 2 }"#),
    trailing(
        23,
        "json_object_members",
        r#"sit _ v ← { "a": 1, "b": 2, }"#,
    ),
    missing(23, "json_object_members", r#"sit _ v ← { "a": 1 "b": 2 }"#),
    // 24. JSON array elements
    accept(24, r#"sit _ v ← { "ids": [1, 2] }"#),
    trailing(24, "json_array_elements", r#"sit _ v ← { "ids": [1, 2,] }"#),
    missing(24, "json_array_elements", r#"sit _ v ← { "ids": [1 2] }"#),
    // 25. type arguments
    accept(25, r#"sit _ pair ← iuncta<textus, numerus> ["salve", 42]"#),
    trailing(
        25,
        "type_arguments",
        r#"sit _ pair ← iuncta<textus, numerus,> ["salve", 42]"#,
    ),
    missing(
        25,
        "type_arguments",
        r#"sit _ pair ← iuncta<textus numerus> ["salve", 42]"#,
    ),
    // 26. scriptum arguments
    accept(26, r#"sit _ s ← scriptum("{0} {1}", 1, 2)"#),
    trailing(26, "scriptum_arguments", r#"sit _ s ← scriptum("{0}", 1,)"#),
    missing(26, "scriptum_arguments", r#"sit _ s ← scriptum("{0}" 1)"#),
    // 27. atomic<T> singleton (comma forbidden)
    accept(27, "functio f(atomic<i32> cell) → vacuum { }"),
    fixed_issue(
        27,
        "atomic_single_element",
        "functio f(atomic<i32, u32> cell) → vacuum { }",
    ),
    // 28. promissum<T> alternate (comma forbidden; uses ⇥)
    accept(
        28,
        "functio f(promissum<numerus ⇥ textus> future) → vacuum { }",
    ),
    fixed_issue(
        28,
        "promissum_alternate_uses_exit_arrow",
        "functio f(promissum<numerus, textus> future) → vacuum { }",
    ),
    // 29. tensor-family element/figura pair
    accept(29, "functio f(tensor<f32, [2, 3]> value) → vacuum { }"),
    fixed_issue(
        29,
        "tensor_figura_after_element",
        "functio f(tensor<f32> value) → vacuum { }",
    ),
    rejected(29, "functio f(tensor<f32, [2, 3],> value) → vacuum { }"),
    // 30. named type parameters
    accept(30, "functio f(iuncta<textus, numerus> pair) → vacuum { }"),
    trailing(
        30,
        "type_parameters",
        "functio f(iuncta<textus, numerus,> pair) → vacuum { }",
    ),
    missing(
        30,
        "type_parameters",
        "functio f(iuncta<textus numerus> pair) → vacuum { }",
    ),
    // 31. function-type params
    accept(31, "typus Op = (numerus, textus) → vacuum"),
    trailing(
        31,
        "function_type_parameters",
        "typus Op = (numerus, textus,) → vacuum",
    ),
    missing(
        31,
        "function_type_parameters",
        "typus Op = (numerus textus) → vacuum",
    ),
    // 32. figura dimensions
    accept(32, "functio f(tensor<f32, [2, 3]> value) → vacuum { }"),
    trailing(
        32,
        "figura_dimensions",
        "functio f(tensor<f32, [2, 3,]> value) → vacuum { }",
    ),
    missing(
        32,
        "figura_dimensions",
        "functio f(tensor<f32, [2 3]> value) → vacuum { }",
    ),
    // 34. scribe/vide/mone/nota args
    accept(34, "incipit { nota 1, 2 }"),
    trailing(34, "scribe_arguments", "incipit { nota 1, 2, }"),
    missing(34, "scribe_arguments", "incipit { nota 1 2 }"),
    // 35. ex destructure fields
    accept(35, "ex source varia nomen, aetas"),
    trailing(35, "extract_fields", "ex source varia nomen, aetas,"),
    missing(35, "extract_fields", "ex source varia nomen aetas"),
    trailing(35, "extract_fields", "ex source varia nomen, ceteri rest,"),
    // 37. pattern bindings
    accept(37, "discerne event { casu Ok sit payload, error { } }"),
    trailing(
        37,
        "pattern_bindings",
        "discerne event { casu Ok sit payload, error, { } }",
    ),
    missing(
        37,
        "pattern_bindings",
        "discerne event { casu Ok fixum payload error { } }",
    ),
];

#[test]
fn comma_law_matrix_covers_every_cs1_parser_site() {
    let mut saw_accept = [false; CS1_ROWS as usize + 1];
    let mut saw_reject = [false; CS1_ROWS as usize + 1];
    for case in CASES {
        assert!(
            (1..=CS1_ROWS).contains(&case.row) && !RETIRED_ROWS.contains(&case.row),
            "case row {} is outside the live CS-1 inventory 1..={CS1_ROWS}",
            case.row
        );
        match case.expect {
            Expect::Accept => saw_accept[usize::from(case.row)] = true,
            Expect::Trailing(_)
            | Expect::Missing(_)
            | Expect::Inserted(_)
            | Expect::FixedIssue(_)
            | Expect::Rejected => saw_reject[usize::from(case.row)] = true,
        }
    }
    for row in (1..=CS1_ROWS).filter(|row| !RETIRED_ROWS.contains(row)) {
        assert!(
            saw_accept[usize::from(row)],
            "CS-1 row {row} is missing an accept case"
        );
        assert!(
            saw_reject[usize::from(row)],
            "CS-1 row {row} is missing a reject case"
        );
    }
}

#[test]
fn comma_law_matrix_accept_cases_parse() {
    for case in CASES {
        if matches!(case.expect, Expect::Accept) {
            let _ = parse_ok_la(case.source);
        }
    }
}

#[test]
fn comma_law_matrix_reject_cases_match_family() {
    let mut failures = Vec::new();
    for case in CASES {
        let result = parse_program_la(case.source);
        let ok = match case.expect {
            Expect::Accept => continue,
            Expect::Trailing(position) => {
                has_issue_at(&result, "item_list_trailing_comma", position)
            }
            Expect::Missing(position) => {
                has_issue_at(&result, "item_list_missing_separator", position)
            }
            Expect::Inserted(position) => {
                has_issue_at(&result, "declaration_block_comma", position)
            }
            Expect::FixedIssue(issue) => has_issue(&result, issue),
            Expect::Rejected => !result.errors.is_empty(),
        };
        if !ok {
            failures.push(format!(
                "row {} {:?}: {} → {:?}",
                case.row, case.expect, case.source, result.errors
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "reject-case mismatches:\n{}",
        failures.join("\n")
    );
}

fn has_issue_at(result: &crate::ParseResult, issue: &str, position: &str) -> bool {
    result
        .errors
        .iter()
        .any(|error| has_arg(error, "issue", issue) && has_arg(error, "position", position))
}

fn has_issue(result: &crate::ParseResult, issue: &str) -> bool {
    result
        .errors
        .iter()
        .any(|error| has_arg(error, "issue", issue))
}

fn has_arg(error: &crate::ParseError, name: &str, value: &str) -> bool {
    error
        .args
        .iter()
        .any(|arg| arg.name == name && arg.value == value)
}
