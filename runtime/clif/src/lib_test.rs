use super::*;

#[test]
fn math_f64_code_6_is_the_real_cube_root() {
    assert_eq!(__faber_clif_v1_math_f64(6, -8.0), -2.0);
    assert_eq!(__faber_clif_v1_math_f64(6, 8.0), 2.0);
    assert!(__faber_clif_v1_math_f64(6, -0.0).is_sign_negative());
}

#[test]
fn text_concat_and_integer_text_keep_exact_bytes() {
    let left = text_new("héllo".as_bytes());
    let right = text_new(" world".as_bytes());
    let joined = unsafe { __faber_clif_v1_text_concat(left, right) };
    assert_eq!(unsafe { text_bytes(joined) }, "héllo world".as_bytes());

    for (value, expected) in [
        (0, "0"),
        (-1, "-1"),
        (i64::MIN, "-9223372036854775808"),
        (i64::MAX, "9223372036854775807"),
    ] {
        let text = __faber_clif_v1_text_from_i64(value);
        assert_eq!(unsafe { text_bytes(text) }, expected.as_bytes());
    }
}

#[test]
#[ignore = "timing oracle: run with --release -- --ignored --nocapture"]
fn indexed_map_insert_scaling_probe() {
    use std::time::Instant;
    for count in [256i64, 512, 1024, 2048] {
        let before = Instant::now();
        let mut entries = Vec::new();
        for key in 0..count {
            if entries
                .iter()
                .position(|&(tag, bits, _, _)| tag == TAG_INT && element_equal(TAG_INT, bits, key))
                .is_none()
            {
                entries.push((TAG_INT, key, TAG_INT, key));
            }
        }
        let before_us = before.elapsed().as_micros();

        let after = Instant::now();
        let map = __faber_clif_v1_map_new(0);
        for key in 0..count {
            unsafe { __faber_clif_v1_map_set(map, TAG_INT, key, TAG_INT, key) };
        }
        let after_us = after.elapsed().as_micros();
        println!("map_insert_scaling n={count} scan_us={before_us} indexed_us={after_us}");
    }
}

#[test]
fn indexed_map_preserves_order_equality_and_delete_probe_chains() {
    let map = __faber_clif_v1_map_new(1);
    for key in 0..512 {
        unsafe { __faber_clif_v1_map_set(map, TAG_INT, key, TAG_INT, key * 10) };
    }
    for key in (0..512).step_by(3) {
        assert_eq!(unsafe { __faber_clif_v1_map_remove(map, TAG_INT, key) }, 1);
        assert_eq!(unsafe { __faber_clif_v1_map_has(map, TAG_INT, key) }, 0);
        for probe in key.saturating_sub(2).max(0)..=(key + 2).min(511) {
            assert_eq!(
                unsafe { __faber_clif_v1_map_has(map, TAG_INT, probe) },
                i8::from(probe % 3 != 0 || probe > key),
                "removed {key}, probing {probe}"
            );
        }
    }
    for key in 0..512 {
        assert_eq!(
            unsafe { __faber_clif_v1_map_has(map, TAG_INT, key) },
            i8::from(key % 3 != 0)
        );
    }
    let expected: Vec<i64> = (0..512).filter(|key| key % 3 != 0).collect();
    assert_eq!(
        unsafe { list_ref(__faber_clif_v1_map_keys(map)) }.items,
        expected
            .iter()
            .map(|&key| (TAG_INT, key))
            .collect::<Vec<_>>()
    );

    // Equal text at distinct addresses must share a bucket and replace.
    let text_map = __faber_clif_v1_map_new(0);
    let first = text_new(b"same key");
    let second = text_new(b"same key");
    for key in 0..12 {
        let text = text_new(format!("key-{key}").as_bytes());
        unsafe { __faber_clif_v1_map_set(text_map, TAG_TEXT, text as i64, TAG_INT, key) };
    }
    unsafe { __faber_clif_v1_map_set(text_map, TAG_TEXT, first as i64, TAG_INT, 1) };
    unsafe { __faber_clif_v1_map_set(text_map, TAG_TEXT, second as i64, TAG_INT, 99) };
    assert_eq!(unsafe { __faber_clif_v1_map_len(text_map) }, 13);
    assert_eq!(
        unsafe { __faber_clif_v1_map_get(text_map, TAG_TEXT, first as i64) },
        99
    );

    // Deep/pointer-backed keys stay on the equality scan even after the
    // scalar index has been built; distinct but equal list handles match.
    let deep_map = __faber_clif_v1_map_new(0);
    for key in 0..8 {
        unsafe { __faber_clif_v1_map_set(deep_map, TAG_INT, key, TAG_INT, key) };
    }
    let left = list_new_from(vec![(TAG_INT, 41), (TAG_BOOL, 1)]);
    let right = list_new_from(vec![(TAG_INT, 41), (TAG_BOOL, 1)]);
    unsafe { __faber_clif_v1_map_set(deep_map, TAG_LIST, left as i64, TAG_INT, 77) };
    assert_eq!(
        unsafe { __faber_clif_v1_map_get(deep_map, TAG_LIST, right as i64) },
        77
    );

    // Floats use the fallback so the existing EQ rules stay authoritative.
    let float_map = __faber_clif_v1_map_new(0);
    for value in 2..10 {
        unsafe {
            __faber_clif_v1_map_set(
                float_map,
                TAG_FLOAT,
                (value as f64).to_bits() as i64,
                TAG_INT,
                value,
            )
        };
    }
    unsafe { __faber_clif_v1_map_set(float_map, TAG_FLOAT, 0.0f64.to_bits() as i64, TAG_INT, 5) };
    assert_eq!(
        unsafe { __faber_clif_v1_map_has(float_map, TAG_FLOAT, (-0.0f64).to_bits() as i64) },
        1
    );
    let nan = f64::NAN.to_bits() as i64;
    unsafe { __faber_clif_v1_map_set(float_map, TAG_FLOAT, nan, TAG_INT, 6) };
    assert_eq!(
        unsafe { __faber_clif_v1_map_has(float_map, TAG_FLOAT, nan) },
        0
    );

    let list = list_new_from(vec![(TAG_INT, 7), (TAG_INT, 8), (TAG_INT, 7), (TAG_INT, 9)]);
    let set = unsafe { __faber_clif_v1_list_to_set(list) };
    assert_eq!(unsafe { __faber_clif_v1_map_len(set) }, 3);
    assert_eq!(
        unsafe { list_ref(__faber_clif_v1_map_keys(set)) }.items,
        vec![(TAG_INT, 7), (TAG_INT, 8), (TAG_INT, 9)]
    );
    assert_eq!(unsafe { __faber_clif_v1_map_has(set, TAG_INT, 8) }, 1);
    assert_eq!(unsafe { __faber_clif_v1_map_remove(set, TAG_INT, 8) }, 1);
    assert_eq!(unsafe { __faber_clif_v1_map_has(set, TAG_INT, 8) }, 0);
    assert_eq!(unsafe { __faber_clif_v1_map_has(set, TAG_INT, 9) }, 1);

    let ordered_other = list_new_from(vec![(TAG_INT, 9), (TAG_INT, 7)]);
    let other_set = unsafe { __faber_clif_v1_list_to_set(ordered_other) };
    assert_eq!(unsafe { __faber_clif_v1_set_subset(set, other_set, 0) }, 1);
    assert_eq!(unsafe { __faber_clif_v1_set_subset(other_set, set, 0) }, 1);
}

#[test]
fn text_parse_bool_returns_one_zero_and_minus_one() {
    // The installed words, not the Latin defaults: an installed token set
    // must not make this test flaky.
    let words = tokens();
    let yes = text_new(words.true_.as_bytes());
    let no = text_new(words.false_.as_bytes());
    let unknown = text_new(b"neither word");

    assert_eq!(unsafe { __faber_clif_v1_text_parse_bool(yes) }, 1);
    assert_eq!(unsafe { __faber_clif_v1_text_parse_bool(no) }, 0);
    assert_eq!(unsafe { __faber_clif_v1_text_parse_bool(unknown) }, -1);
}

#[test]
fn arguments_skip_program_name_and_preserve_invalid_utf8() {
    let program = b"program\0";
    let text = b"ordinary\0";
    let invalid = [b'b', b'a', b'd', 0xff, 0];
    let argv = [
        program.as_ptr().cast(),
        text.as_ptr().cast(),
        invalid.as_ptr().cast(),
    ];

    assert_eq!(
        capture_arguments(argv.len() as i32, argv.as_ptr()),
        vec!["ordinary".to_owned(), "bad�".to_owned()]
    );
}

/// An `f32`-tagged float cell carries the `f64` bits of the `f32`-rounded
/// value: it renders at single precision and compares by its bits like any
/// other float cell.
#[test]
fn f32_tagged_float_cells_render_and_compare_at_single_precision() {
    let rounded = f64::from(0.1f32);
    let bits = rounded.to_bits() as i64;
    let list = list_new_from(vec![
        (TAG_FLOAT32, bits),
        (TAG_FLOAT, 0.1f64.to_bits() as i64),
    ]);
    let text = list_text(unsafe { &*list });
    assert_eq!(text, "[0.1, 0.1]");
    assert!(element_equal(TAG_FLOAT32, bits, bits));
    assert_eq!(
        element_order(TAG_FLOAT32, bits, (f64::from(0.2f32)).to_bits() as i64),
        std::cmp::Ordering::Less
    );
}
