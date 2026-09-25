use super::*;

#[test]
fn hygiene_names_use_single_letter_prefixes() {
    assert_eq!(temp(3), "t3");
    assert_eq!(item(7), "i7");
    assert_eq!(range_start(0), "s0");
    assert_eq!(range_end(0), "e0");
    assert_eq!(range_step(0), "d0");
    assert_eq!(filter(2), "f2");
    assert_eq!(text_arg(5), "a5");
    assert_eq!(FAC_OK, "ok");
    assert_eq!(FAC_ERR, "err");
}

#[test]
fn hygiene_temp_and_item_have_expected_format() {
    assert_eq!(temp(0), "t0");
    assert_eq!(temp(99), "t99");
    assert_eq!(item(1), "i1");
    assert_eq!(item(100), "i100");
}

#[test]
fn hygiene_range_names_have_expected_format() {
    assert_eq!(range_start(0), "s0");
    assert_eq!(range_end(0), "e0");
    assert_eq!(range_step(0), "d0");
    assert_eq!(range_limit(0), "l0");
    assert_eq!(range_cursor(0), "n0");
    assert_eq!(range_values(0), "v0");
}

#[test]
fn hygiene_closure_and_text_names_have_expected_format() {
    assert_eq!(filter(2), "f2");
    assert_eq!(mapper(3), "m3");
    assert_eq!(reducer(4), "h4");
    assert_eq!(count(5), "c5");
    assert_eq!(text_arg(5), "a5");
    assert_eq!(text_sep(6), "j6");
    assert_eq!(text_old(7), "o7");
    assert_eq!(text_new(8), "p8");
    assert_eq!(rune_index(9), "n9");
    assert_eq!(extremum_operands(4), ("b4".to_owned(), "r4".to_owned()));
}

#[test]
fn hygiene_constants_are_correct() {
    assert_eq!(FAC_OK, "ok");
    assert_eq!(FAC_ERR, "err");
    assert_eq!(YIELDED, "y0");
    assert_eq!(INPUT, "u0");
    assert_eq!(OPTIONAL, "o0");
    assert_eq!(CLOSURE_ITEM, "x");
    assert_eq!(CLOSURE_ACC, "a");
    assert_eq!(CLOSURE_SPLIT, "q");
    assert_eq!(MAP_KEY, "k");
    assert_eq!(MAP_VAL, "w");
    assert_eq!(RUNES, "g0");
    assert_eq!(SLICE_OUT, "out0");
}
