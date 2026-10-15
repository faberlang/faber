//! Runner proofs for copy-with-changes (DECISIONS D15.1–D15.3).
//!
//! `Genus { field = value } from source` overrides the braced fields and
//! copies every other field from the source as a VALUE (need, rule 5):
//! a list or nested object field is duplicated, never shared with the source.
//! Overrides evaluate before the source, in source order, and the source
//! evaluates once.

use crate::script::{BufferHost, interpret_source};

const PUNCTUM: &str = "
class Punctum {
    let int x
    mut int y
    let list<int> gradus
}
";

const MUTABILE: &str = "
class Mutabile {
    mut int n
    mut list<int> gradus
}
";

fn run(body: &str) -> Vec<String> {
    let mut host = BufferHost::default();
    interpret_source(
        "construction_source.fab",
        &format!("{PUNCTUM}{MUTABILE}{body}"),
        &mut host,
    )
    .expect("copy-with-changes fixture runs");
    host.stdout_lines
}

#[test]
fn braced_fields_override_and_the_rest_copy_through() {
    let lines = run("
main {
    let Punctum p ← Punctum { x = 1, y = 2, gradus = [4] }
    let Punctum q ← Punctum { y = 9 } from p
    print q.x
    print q.y
    print q.gradus.length()
    print p.y
}
");
    assert_eq!(lines, ["1", "9", "1", "2"]);
}

#[test]
fn list_fields_are_copied_from_the_source() {
    let lines = run("
main {
    mut Mutabile p ← Mutabile { n = 1, gradus = [4] }
    mut Mutabile q ← Mutabile { n = 9 } from p
    q.gradus.append(5)
    print p.gradus.length()
    print q.gradus.length()
}
");
    assert_eq!(lines, ["1", "2"]);
}

#[test]
fn nested_object_fields_are_copied_from_the_source() {
    let lines = run("
class Intus {
    mut int n
}

class Cista {
    mut int k
    mut Intus intus
    mut list<int> gradus
}

main {
    mut Cista a ← Cista { k = 1, intus = Intus { n = 5 }, gradus = [1] }
    mut Cista b ← Cista { k = 9 } from a
    b.intus.n ← 6
    b.gradus.append(2)
    print a.intus.n
    print b.intus.n
    print a.gradus.length()
    print b.gradus.length()
    print b.k
}
");
    assert_eq!(lines, ["5", "6", "1", "2", "9"]);
}

#[test]
fn an_explicit_field_is_not_replaced_by_the_source_copy() {
    let lines = run("
main {
    let Punctum p ← Punctum { x = 1, y = 2, gradus = [4] }
    let Punctum q ← Punctum { gradus = [7, 8, 9] } from p
    print q.gradus.length()
    print q.gradus[0]
    print p.gradus.length()
}
");
    assert_eq!(lines, ["3", "7", "1"]);
}

#[test]
fn overrides_evaluate_before_the_single_source_evaluation() {
    let lines = run(r#"
fn fons() → Punctum {
    print "fons"
    return Punctum { x = 1, y = 2, gradus = [] }
}

fn septem() → int {
    print "septem"
    return 7
}

main {
    let Punctum q ← Punctum { y = septem() } from fons()
    print q.y
}
"#);
    assert_eq!(lines, ["septem", "fons", "7"]);
}
