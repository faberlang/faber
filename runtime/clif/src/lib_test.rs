use super::*;

#[test]
fn math_f64_code_6_is_the_real_cube_root() {
    assert_eq!(
        __faber_clif_v1_math_f64(crate::test_arena_ptr(), 6, -8.0),
        -2.0
    );
    assert_eq!(
        __faber_clif_v1_math_f64(crate::test_arena_ptr(), 6, 8.0),
        2.0
    );
    assert!(__faber_clif_v1_math_f64(crate::test_arena_ptr(), 6, -0.0).is_sign_negative());
}

#[test]
fn text_concat_and_integer_text_keep_exact_bytes() {
    let left = text_new(crate::test_arena_ptr(), "héllo".as_bytes());
    let right = text_new(crate::test_arena_ptr(), " world".as_bytes());
    let joined = unsafe { __faber_clif_v1_text_concat(crate::test_arena_ptr(), left, right) };
    assert_eq!(unsafe { text_bytes(joined) }, "héllo world".as_bytes());

    for (value, expected) in [
        (0, "0"),
        (-1, "-1"),
        (i64::MIN, "-9223372036854775808"),
        (i64::MAX, "9223372036854775807"),
    ] {
        let text = __faber_clif_v1_text_from_i64(crate::test_arena_ptr(), value);
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
        let map = __faber_clif_v1_map_new(crate::test_arena_ptr(), 0);
        for key in 0..count {
            unsafe {
                __faber_clif_v1_map_set(crate::test_arena_ptr(), map, TAG_INT, key, TAG_INT, key)
            };
        }
        let after_us = after.elapsed().as_micros();
        println!("map_insert_scaling n={count} scan_us={before_us} indexed_us={after_us}");
    }
}

#[test]
fn indexed_map_preserves_order_equality_and_delete_probe_chains() {
    let map = __faber_clif_v1_map_new(crate::test_arena_ptr(), 1);
    for key in 0..512 {
        unsafe {
            __faber_clif_v1_map_set(
                crate::test_arena_ptr(),
                map,
                TAG_INT,
                key,
                TAG_INT,
                key * 10,
            )
        };
    }
    for key in (0..512).step_by(3) {
        assert_eq!(
            unsafe { __faber_clif_v1_map_remove(crate::test_arena_ptr(), map, TAG_INT, key) },
            1
        );
        assert_eq!(
            unsafe { __faber_clif_v1_map_has(crate::test_arena_ptr(), map, TAG_INT, key) },
            0
        );
        for probe in key.saturating_sub(2).max(0)..=(key + 2).min(511) {
            assert_eq!(
                unsafe { __faber_clif_v1_map_has(crate::test_arena_ptr(), map, TAG_INT, probe) },
                i8::from(probe % 3 != 0 || probe > key),
                "removed {key}, probing {probe}"
            );
        }
    }
    for key in 0..512 {
        assert_eq!(
            unsafe { __faber_clif_v1_map_has(crate::test_arena_ptr(), map, TAG_INT, key) },
            i8::from(key % 3 != 0)
        );
    }
    let expected: Vec<i64> = (0..512).filter(|key| key % 3 != 0).collect();
    assert_eq!(
        unsafe { list_ref(__faber_clif_v1_map_keys(crate::test_arena_ptr(), map)) }.items,
        expected
            .iter()
            .map(|&key| (TAG_INT, key))
            .collect::<Vec<_>>()
    );

    // Equal text at distinct addresses must share a bucket and replace.
    let text_map = __faber_clif_v1_map_new(crate::test_arena_ptr(), 0);
    let first = text_new(crate::test_arena_ptr(), b"same key");
    let second = text_new(crate::test_arena_ptr(), b"same key");
    for key in 0..12 {
        let text = text_new(crate::test_arena_ptr(), format!("key-{key}").as_bytes());
        unsafe {
            __faber_clif_v1_map_set(
                crate::test_arena_ptr(),
                text_map,
                TAG_TEXT,
                text as i64,
                TAG_INT,
                key,
            )
        };
    }
    unsafe {
        __faber_clif_v1_map_set(
            crate::test_arena_ptr(),
            text_map,
            TAG_TEXT,
            first as i64,
            TAG_INT,
            1,
        )
    };
    unsafe {
        __faber_clif_v1_map_set(
            crate::test_arena_ptr(),
            text_map,
            TAG_TEXT,
            second as i64,
            TAG_INT,
            99,
        )
    };
    assert_eq!(
        unsafe { __faber_clif_v1_map_len(crate::test_arena_ptr(), text_map) },
        13
    );
    assert_eq!(
        unsafe {
            __faber_clif_v1_map_get(crate::test_arena_ptr(), text_map, TAG_TEXT, first as i64)
        },
        99
    );

    // Deep/pointer-backed keys stay on the equality scan even after the
    // scalar index has been built; distinct but equal list handles match.
    let deep_map = __faber_clif_v1_map_new(crate::test_arena_ptr(), 0);
    for key in 0..8 {
        unsafe {
            __faber_clif_v1_map_set(
                crate::test_arena_ptr(),
                deep_map,
                TAG_INT,
                key,
                TAG_INT,
                key,
            )
        };
    }
    let left = list_new_from(crate::test_arena_ptr(), vec![(TAG_INT, 41), (TAG_BOOL, 1)]);
    let right = list_new_from(crate::test_arena_ptr(), vec![(TAG_INT, 41), (TAG_BOOL, 1)]);
    unsafe {
        __faber_clif_v1_map_set(
            crate::test_arena_ptr(),
            deep_map,
            TAG_LIST,
            left as i64,
            TAG_INT,
            77,
        )
    };
    assert_eq!(
        unsafe {
            __faber_clif_v1_map_get(crate::test_arena_ptr(), deep_map, TAG_LIST, right as i64)
        },
        77
    );

    // Floats use the fallback so the existing EQ rules stay authoritative.
    let float_map = __faber_clif_v1_map_new(crate::test_arena_ptr(), 0);
    for value in 2..10 {
        unsafe {
            __faber_clif_v1_map_set(
                crate::test_arena_ptr(),
                float_map,
                TAG_FLOAT,
                (value as f64).to_bits() as i64,
                TAG_INT,
                value,
            )
        };
    }
    unsafe {
        __faber_clif_v1_map_set(
            crate::test_arena_ptr(),
            float_map,
            TAG_FLOAT,
            0.0f64.to_bits() as i64,
            TAG_INT,
            5,
        )
    };
    assert_eq!(
        unsafe {
            __faber_clif_v1_map_has(
                crate::test_arena_ptr(),
                float_map,
                TAG_FLOAT,
                (-0.0f64).to_bits() as i64,
            )
        },
        1
    );
    let nan = f64::NAN.to_bits() as i64;
    unsafe {
        __faber_clif_v1_map_set(
            crate::test_arena_ptr(),
            float_map,
            TAG_FLOAT,
            nan,
            TAG_INT,
            6,
        )
    };
    assert_eq!(
        unsafe { __faber_clif_v1_map_has(crate::test_arena_ptr(), float_map, TAG_FLOAT, nan) },
        0
    );

    let list = list_new_from(
        crate::test_arena_ptr(),
        vec![(TAG_INT, 7), (TAG_INT, 8), (TAG_INT, 7), (TAG_INT, 9)],
    );
    let set = unsafe { __faber_clif_v1_list_to_set(crate::test_arena_ptr(), list) };
    assert_eq!(
        unsafe { __faber_clif_v1_map_len(crate::test_arena_ptr(), set) },
        3
    );
    assert_eq!(
        unsafe { list_ref(__faber_clif_v1_map_keys(crate::test_arena_ptr(), set)) }.items,
        vec![(TAG_INT, 7), (TAG_INT, 8), (TAG_INT, 9)]
    );
    assert_eq!(
        unsafe { __faber_clif_v1_map_has(crate::test_arena_ptr(), set, TAG_INT, 8) },
        1
    );
    assert_eq!(
        unsafe { __faber_clif_v1_map_remove(crate::test_arena_ptr(), set, TAG_INT, 8) },
        1
    );
    assert_eq!(
        unsafe { __faber_clif_v1_map_has(crate::test_arena_ptr(), set, TAG_INT, 8) },
        0
    );
    assert_eq!(
        unsafe { __faber_clif_v1_map_has(crate::test_arena_ptr(), set, TAG_INT, 9) },
        1
    );

    let ordered_other = list_new_from(crate::test_arena_ptr(), vec![(TAG_INT, 9), (TAG_INT, 7)]);
    let other_set = unsafe { __faber_clif_v1_list_to_set(crate::test_arena_ptr(), ordered_other) };
    assert_eq!(
        unsafe { __faber_clif_v1_set_subset(crate::test_arena_ptr(), set, other_set, 0) },
        1
    );
    assert_eq!(
        unsafe { __faber_clif_v1_set_subset(crate::test_arena_ptr(), other_set, set, 0) },
        1
    );
}

#[test]
fn text_parse_bool_returns_one_zero_and_minus_one() {
    // The installed words, not the Latin defaults: an installed token set
    // must not make this test flaky.
    let words = tokens();
    let yes = text_new(crate::test_arena_ptr(), words.true_.as_bytes());
    let no = text_new(crate::test_arena_ptr(), words.false_.as_bytes());
    let unknown = text_new(crate::test_arena_ptr(), b"neither word");

    assert_eq!(
        unsafe { __faber_clif_v1_text_parse_bool(crate::test_arena_ptr(), yes) },
        1
    );
    assert_eq!(
        unsafe { __faber_clif_v1_text_parse_bool(crate::test_arena_ptr(), no) },
        0
    );
    assert_eq!(
        unsafe { __faber_clif_v1_text_parse_bool(crate::test_arena_ptr(), unknown) },
        -1
    );
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
    let list = list_new_from(
        crate::test_arena_ptr(),
        vec![(TAG_FLOAT32, bits), (TAG_FLOAT, 0.1f64.to_bits() as i64)],
    );
    let text = list_text(unsafe { &*list });
    assert_eq!(text, "[0.1, 0.1]");
    assert!(element_equal(TAG_FLOAT32, bits, bits));
    assert_eq!(
        element_order(TAG_FLOAT32, bits, (f64::from(0.2f32)).to_bits() as i64),
        std::cmp::Ordering::Less
    );
}

#[test]
#[ignore = "one-off before/after root-arena churn measure"]
fn arena_churn_measurement() {
    use std::hint::black_box;
    use std::time::Instant;

    const ROUNDS: usize = 20_000;
    const TEXT: &[u8] = b"allocation-heavy text churn, 32 bytes";

    let before = Instant::now();
    for round in 0..ROUNDS {
        let mut bytes = Vec::with_capacity(8 + TEXT.len());
        bytes.extend_from_slice(&i64::try_from(TEXT.len()).unwrap().to_ne_bytes());
        bytes.extend_from_slice(TEXT);
        let text = Box::leak(bytes.into_boxed_slice());
        let mut list = Vec::new();
        for index in 0..8 {
            list.push((TAG_INT, (round + index) as i64));
        }
        black_box(text.as_ptr());
        black_box(Box::into_raw(Box::new(list)));
    }
    let legacy = before.elapsed();

    let arena = crate::Arena::new();
    let arena = std::ptr::from_ref(&arena);
    let before = Instant::now();
    for round in 0..ROUNDS {
        let text = text_new(arena, TEXT);
        let list = __faber_clif_v1_list_new(arena);
        for index in 0..8 {
            unsafe {
                __faber_clif_v1_list_push(arena, list, TAG_INT, (round + index) as i64);
            }
        }
        black_box(text);
        black_box(list);
    }
    let arena_time = before.elapsed();
    println!("text/list churn rounds={ROUNDS} legacy_box_vec={legacy:?} root_arena={arena_time:?}");
}

#[test]
fn repeated_small_extends_grow_geometrically_not_one_run_per_extend() {
    let arena = Arena::new();
    let arena = std::ptr::from_ref(&arena);
    // SAFETY: the arena outlives every use here and the runtime is
    // single-threaded.
    let allocations = || unsafe { (*arena).allocations() };

    const EXTENDS: usize = 5_000;
    let mut buffer = crate::arena::ArenaBuf::new();
    for value in 0..EXTENDS {
        let element = value as i64;
        buffer.extend_from_slice(arena, std::slice::from_ref(&element));
    }
    assert_eq!(buffer.len(), EXTENDS);
    assert_eq!(buffer.as_slice()[0], 0);
    assert_eq!(buffer.as_slice()[EXTENDS / 2], (EXTENDS / 2) as i64);
    assert_eq!(buffer.as_slice()[EXTENDS - 1], (EXTENDS - 1) as i64);
    // Exact growth allocates one fresh run per extend and copies every live
    // element again each time (quadratic work, quadratic retained garbage);
    // geometric growth needs only a logarithmic number of runs and leaves
    // headroom under twice the demand.
    let runs = allocations();
    assert!(
        runs <= 24,
        "each single-element extend allocated its own run: {runs} runs for {EXTENDS} extends"
    );
    let capacity = buffer.capacity();
    assert!(
        capacity > EXTENDS,
        "no headroom after {EXTENDS} extends: capacity {capacity}"
    );
    assert!(
        capacity <= 2 * EXTENDS,
        "capacity {capacity} overshoots doubling for {EXTENDS} extends"
    );

    // The resize path grows by the same law.
    let before = allocations();
    let mut buffer = crate::arena::ArenaBuf::new();
    for length in 1..=EXTENDS {
        buffer.resize(arena, length, 7);
    }
    assert_eq!(buffer.len(), EXTENDS);
    assert_eq!(buffer.as_slice()[0], 7);
    assert_eq!(buffer.as_slice()[EXTENDS - 1], 7);
    let runs = allocations() - before;
    assert!(
        runs <= 24,
        "each one-element resize allocated its own run: {runs} runs for {EXTENDS} resizes"
    );
    assert!(buffer.capacity() > EXTENDS);

    // Direct construction stays exact: one run of the asked capacity.
    let before = allocations();
    let exact = crate::arena::ArenaBuf::from_slice(arena, &[1i64, 2, 3]);
    assert_eq!(exact.capacity(), 3);
    assert_eq!(allocations() - before, 1);
}

#[test]
fn chunk_layout_refuses_an_overflowing_header_addition() {
    use crate::arena::{CHUNK_ALIGN, CHUNK_DATA, HEADER, chunk_layout};

    assert!(
        chunk_layout(usize::MAX).is_none(),
        "a capacity that overflows the header addition must refuse the layout"
    );
    let layout = chunk_layout(CHUNK_DATA).expect("an ordinary chunk has a layout");
    assert_eq!(layout.size(), HEADER + CHUNK_DATA);
    assert_eq!(layout.align(), CHUNK_ALIGN);
}

#[test]
fn allocations_span_chunks_and_straddle_chunk_boundaries() {
    use crate::arena::{ArenaPointer, CHUNK_DATA};

    // An allocation larger than one fresh chunk gets a chunk of its own and
    // reads back whole.
    let arena = Arena::new();
    let arena = std::ptr::from_ref(&arena);
    let oversized = vec![0xA5u8; CHUNK_DATA + 1];
    let big = ArenaPointer::put_slice(arena, &oversized);
    // SAFETY: `big` is a live slice of this test's arena.
    let read_back = unsafe { &*big };
    assert_eq!(read_back.len(), CHUNK_DATA + 1);
    assert!(read_back.iter().all(|&byte| byte == 0xA5));
    assert_eq!(
        // SAFETY: see above.
        unsafe { (*arena).chunk_count() },
        1,
        "an oversized allocation lives in a single chunk"
    );

    // A pair straddling the chunk boundary: the first allocation ends on the
    // chunk's last byte, the second must open the next chunk.
    let arena = Arena::new();
    let arena = std::ptr::from_ref(&arena);
    let almost_all = vec![0x11u8; CHUNK_DATA - 1];
    let a = ArenaPointer::put_slice(arena, &almost_all);
    let last = ArenaPointer::put_slice(arena, &[0x22u8]);
    assert_eq!(
        // SAFETY: `arena` is live for the whole test.
        unsafe { (*arena).chunk_count() },
        1,
        "an allocation filling the chunk's last byte stays in the first chunk"
    );
    let first = ArenaPointer::put_slice(arena, &[0x33u8]);
    assert_eq!(
        // SAFETY: `arena` is live for the whole test.
        unsafe { (*arena).chunk_count() },
        2,
        "the byte after a full chunk opens the next chunk"
    );

    // SAFETY: all three slices are live in this test's arena.
    unsafe {
        assert!((*a).iter().all(|&byte| byte == 0x11));
        assert_eq!((*last)[0], 0x22);
        assert_eq!((*first)[0], 0x33);
    }

    // Addresses stay stable as the arena grows past the boundary.
    let addresses = [
        a.cast::<u8>() as usize,
        last.cast::<u8>() as usize,
        first.cast::<u8>() as usize,
    ];
    let trailing = ArenaPointer::put_slice(arena, &[0x44u8; 128]);
    assert_eq!(
        [
            a.cast::<u8>() as usize,
            last.cast::<u8>() as usize,
            first.cast::<u8>() as usize
        ],
        addresses,
        "arena addresses are stable for the program's life"
    );
    // SAFETY: `trailing` is a live slice of this test's arena.
    unsafe {
        assert!((*trailing).iter().all(|&byte| byte == 0x44));
    }
}
