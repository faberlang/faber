//! Runner proofs for copy-with-changes (DECISIONS D15.1–D15.3).
//!
//! `Genus { field = value } from source` overrides the braced fields and
//! copies every other field from the source. The copy is shallow: a list field
//! is shared with the source (D15.3). Overrides evaluate before the source, in
//! source order, and the source evaluates once.

use crate::script::{BufferHost, interpret_source};

const PUNCTUM: &str = "
class Punctum {
    const int x
    var int y
    const list<int> gradus
}
";

fn run(body: &str) -> Vec<String> {
    let mut host = BufferHost::default();
    interpret_source(
        "construction_source.fab",
        &format!("{PUNCTUM}{body}"),
        &mut host,
    )
    .expect("copy-with-changes fixture runs");
    host.stdout_lines
}

#[test]
fn braced_fields_override_and_the_rest_copy_through() {
    let lines = run("
main {
    const Punctum p ← Punctum { x = 1, y = 2, gradus = [4] }
    const Punctum q ← Punctum { y = 9 } from p
    print q.x
    print q.y
    print q.gradus.length()
    print p.y
}
");
    assert_eq!(lines, ["1", "9", "1", "2"]);
}

#[test]
fn list_fields_are_shared_with_the_source() {
    let lines = run("
main {
    const Punctum p ← Punctum { x = 1, y = 2, gradus = [4] }
    const Punctum q ← Punctum { y = 9 } from p
    q.gradus.append(5)
    print p.gradus.length()
}
");
    assert_eq!(lines, ["2"]);
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
    const Punctum q ← Punctum { y = septem() } from fons()
    print q.y
}
"#);
    assert_eq!(lines, ["septem", "fons", "7"]);
}
