//! D19.4: `itera` element typing has no silent `numerus` fallback.
//!
//! Every iterable binds its real element type (`copia<T>` binds `T`,
//! `textus` binds `littera` (D10.1), tensors bind their element), and a
//! non-iterable source is `itera_source_not_iterable`. Assertions are
//! structured (issue identity), never rendered prose.

use crate::test_support;

const NOT_ITERABLE: &str = "itera_source_not_iterable";

#[test]
fn itera_ex_copia_textus_binds_textus() {
    test_support::assert_accepts(
        "functio f(copia<textus> t) → vacuum {
    itera ex t sit x {
        sit textus y ← x
        nota y
    }
}",
    );
}

#[test]
fn itera_ex_copia_textus_does_not_bind_numerus() {
    test_support::assert_rejects_with_issue(
        "functio f(copia<textus> t) → vacuum {
    itera ex t sit x {
        sit numerus y ← x
        nota y
    }
}",
        "initializer_annotation_mismatch",
    );
}

#[test]
fn itera_ex_genus_rejects_as_not_iterable() {
    test_support::assert_rejects_with_issue(
        "genus Arbor { varia numerus magnitudo }
functio f(Arbor a) → vacuum {
    itera ex a sit n {
        nota n
    }
}",
        NOT_ITERABLE,
    );
}

#[test]
fn itera_de_genus_rejects_as_not_iterable() {
    test_support::assert_rejects_with_issue(
        "genus Arbor { varia numerus magnitudo }
functio f(Arbor a) → vacuum {
    itera de a sit n {
        nota n
    }
}",
        NOT_ITERABLE,
    );
}

#[test]
fn itera_ex_bivalens_rejects_as_not_iterable() {
    test_support::assert_rejects_with_issue(
        "functio f(bivalens b) → vacuum {
    itera ex b sit n {
        nota n
    }
}",
        NOT_ITERABLE,
    );
}

#[test]
fn itera_ex_textus_binds_littera() {
    test_support::assert_accepts(
        "functio f(textus s) → vacuum {
    itera ex s sit c {
        sit littera x ← c
        nota x
    }
}",
    );
}

#[test]
fn itera_ex_textus_element_compares_with_char_literal() {
    test_support::assert_accepts(
        "functio f(textus s) → bivalens {
    itera ex s sit c {
        si c ≡ 'b' { redde verum }
    }
    redde falsum
}",
    );
}

#[test]
fn itera_ex_textus_element_is_not_numerus() {
    test_support::assert_rejects_with_issue(
        "functio f(textus s) → vacuum {
    itera ex s sit c {
        sit numerus x ← c
        nota x
    }
}",
        "initializer_annotation_mismatch",
    );
}

#[test]
fn itera_ex_octeti_binds_index_element_type() {
    test_support::assert_accepts(
        "functio f(octeti b) → vacuum {
    itera ex b sit x {
        sit b8 y ← x
        nota y
    }
}",
    );
}

#[test]
fn itera_ex_symbolic_rank1_tensor_binds_element_type() {
    test_support::assert_accepts(
        "functio f<magnitudo N>(tensor<f32, [N]> v) → vacuum {
    itera ex v sit x {
        sit f32 y ← x
        nota y
    }
}",
    );
}
