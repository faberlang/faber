//! Per-family author idempotency on historically failing `#` comment exempla.

use radix_module::forma::test_gate::assert_author_idempotent;
use std::fs;
use std::path::PathBuf;

fn corpus_file(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus")
        .join(rel)
}

fn assert_corpus_author_idempotent(rel: &str) {
    let path = corpus_file(rel);
    let source = fs::read_to_string(&path).unwrap_or_else(|err| {
        panic!("read {rel}: {err}");
    });
    if let Err(reason) = assert_author_idempotent(rel, &source) {
        panic!("{rel}: {reason}");
    }
}

#[test]
fn author_family_ab() {
    assert_corpus_author_idempotent("ab/ab.fab");
}

#[test]
fn author_family_ad() {
    assert_corpus_author_idempotent("ad/sermo-conversio.fab");
}

#[test]
fn author_family_destructura_objectum() {
    assert_corpus_author_idempotent("destructura/objectum.fab");
}

#[test]
fn author_family_ex() {
    assert_corpus_author_idempotent("ex/ex.fab");
}

#[test]
fn author_family_finge() {
    assert_corpus_author_idempotent("finge/finge.fab");
}

#[test]
fn author_family_integratio_arena_mixta() {
    assert_corpus_author_idempotent("integratio/arena-mixta.fab");
}

#[test]
fn author_family_integratio_discerne_insanum() {
    assert_corpus_author_idempotent("integratio/discerne-insanum.fab");
}

#[test]
fn author_family_itera_intervallum_gradus() {
    assert_corpus_author_idempotent("itera/intervallum-gradus.fab");
}

#[test]
fn author_family_literalia_block_string() {
    assert_corpus_author_idempotent("literalia/block-string.fab");
}

#[test]
fn author_family_literalia_forma() {
    assert_corpus_author_idempotent("literalia/forma.fab");
}

#[test]
fn author_family_per() {
    assert_corpus_author_idempotent("per/per.fab");
}

#[test]
fn author_family_unio() {
    assert_corpus_author_idempotent("unio/unio.fab");
}
