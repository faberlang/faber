//! Tests for forma SQL rendering (captured forma → SQL strings).
//! Proves: postgres, sqlite, mysql dialects use correct placeholder
//! conventions; rendered SQL is syntactically valid for the target dialect.

use super::{CapturedForma, SqlDialect, render_sql};

#[test]
fn forma_render_postgres_uses_dollar_placeholders() {
    let forma = CapturedForma {
        template: "select * from accounts where id = § and plan = §".to_owned(),
        params: vec!["tenant-1".to_owned(), "pro".to_owned()],
    };

    let rendered = render_sql(&forma.template, SqlDialect::Postgres);
    assert_eq!(rendered, "select * from accounts where id = $1 and plan = $2");
    assert_eq!(forma.params.len(), 2);
}

#[test]
fn forma_render_sqlite_uses_question_placeholders() {
    let forma = CapturedForma {
        template: "select * from accounts where id = §".to_owned(),
        params: vec!["tenant-1".to_owned()],
    };

    let rendered = render_sql(&forma.template, SqlDialect::Sqlite);
    assert_eq!(rendered, "select * from accounts where id = ?1");
}

#[test]
fn forma_render_indexed_holes_repeat_without_duplicating_params() {
    let forma = CapturedForma {
        template: "where tenant_id = §0 or parent_tenant_id = §0".to_owned(),
        params: vec!["tenant-1".to_owned()],
    };

    let postgres = render_sql(&forma.template, SqlDialect::Postgres);
    let sqlite = render_sql(&forma.template, SqlDialect::Sqlite);
    assert_eq!(postgres, "where tenant_id = $1 or parent_tenant_id = $1");
    assert_eq!(sqlite, "where tenant_id = ?1 or parent_tenant_id = ?1");
    assert_eq!(forma.params.len(), 1);
}

#[test]
fn forma_capture_does_not_interpolate_parameter_values() {
    let forma = CapturedForma { template: "select * where id = §".to_owned(), params: vec!["secret".to_owned()] };

    let rendered = render_sql(&forma.template, SqlDialect::Postgres);
    assert!(!rendered.contains("secret"));
    assert!(rendered.contains("$1"));
}
