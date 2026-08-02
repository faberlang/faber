//! [`CodeWriter`] indentation policy: line-start detection, raw-write bypass, and
//! scoped depth restoration.

use crate::CodeWriter;

#[test]
fn write_indents_nonblank_lines_only() {
    let mut writer = CodeWriter::new();

    writer.indent();
    writer.write("alpha\n\nbeta");

    assert_eq!(writer.finish(), "    alpha\n\n    beta");
}

#[test]
fn write_raw_skips_literal_interior_indentation() {
    let mut writer = CodeWriter::new();

    writer.indent();
    writer.write("«");
    writer.write_raw("line one\nstill line one");
    writer.write("»");

    assert_eq!(writer.finish(), "    «line one\nstill line one»");
}

#[test]
fn scoped_indentation_restores_outer_depth() {
    let mut writer = CodeWriter::new();

    writer.writeln("root");
    writer.indented(|writer| {
        writer.writeln("child");
        writer.indented(|writer| writer.writeln("grandchild"));
        writer.writeln("sibling");
    });
    writer.writeln("root-again");

    assert_eq!(
        writer.finish(),
        "root\n    child\n        grandchild\n    sibling\nroot-again\n"
    );
}
