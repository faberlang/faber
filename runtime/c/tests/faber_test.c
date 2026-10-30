/*
 * Runtime checks for faber.h, run by crates/radix-hir-c/scripta/diffcheck
 * under UndefinedBehaviorSanitizer and AddressSanitizer. Each check compares
 * with what the MIR runner prints for the same operation.
 */
#include "faber.h"

/* Backed literals: text values fed into helpers that read the header. */
FABER_TEXT_LITERAL(faber_LTA, "a", 1, 1);
FABER_TEXT_LITERAL(faber_LBC, "bc", 2, 2);
FABER_TEXT_LITERAL(faber_LXE, "x=", 2, 2);
FABER_TEXT_LITERAL(faber_LBANG, "!", 1, 1);
FABER_TEXT_LITERAL(faber_LQE, "q\"\\\n\t\r\x1b" "e", 8, 8);

typedef int *faber_test_genus_pointer;
FABER_LIST_FAMILY(test_genus_pointer, faber_test_genus_pointer)

static int failures;

static void check_text(const char *what, faber_text got, const char *want) {
    if (strcmp(got, want) != 0) {
        fprintf(stderr, "FAIL %s: got [%s], want [%s]\n", what, got, want);
        failures++;
    }
}

static void check_int(const char *what, faber_i128 got, faber_i128 want) {
    if (got != want) {
        char a[41];
        char b[41];
        fprintf(stderr, "FAIL %s: got %s, want %s\n", what, faber_i128_text(got, a),
                faber_i128_text(want, b));
        failures++;
    }
}

int main(int argc, char **argv) {
    faber_arena arena = {0};
    char buffer[41];
    int overflow = 0;
    faber_list_i64 *ints;
    faber_list_text *texts;
    faber_list_bool *flags;
    faber_list_b8 *bits;
    int64_t index;

    if (argc == 2 && strcmp(argv[1], "--overflow-capacity") == 0) {
        (void)faber_list_i64_new(&arena, INT64_C(2305843009213693952));
        return 0;
    }
    if (argc != 1) {
        fprintf(stderr, "usage: faber_test [--overflow-capacity]\n");
        return 2;
    }

    /* Floor division and remainder take the divisor's sign. */
    check_int("div -7/2", faber_div(-7, 2), -4);
    check_int("rem -7%2", faber_rem(-7, 2), 1);
    check_int("div 7/-2", faber_div(7, -2), -4);
    check_int("rem 7%-2", faber_rem(7, -2), -1);
    check_int("fdiv -7/2", faber_fdiv_i64(-7, 2), -4);
    check_int("fmod -7%-3", faber_fmod_i64(-7, -3), -1);
    check_int("fmod max%-2", faber_fmod_i64(INT64_MAX, -2), -1);
    check_int("fdiv min/2", faber_fdiv_i64(INT64_MIN, 2), INT64_MIN / 2);

    /* The cap admits 2^64 - 1 and an intermediate beyond int64. */
    check_int("cap top", faber_add(INT64_MAX, (faber_i128)INT64_MAX + 1), (faber_i128)UINT64_MAX);
    check_text("i128 text", faber_i128_text(-((faber_i128)1 << 63), buffer), "-9223372036854775808");

    /* The fast path flags overflow and keeps going without UB. */
    (void)faber_mul_o(INT64_MAX, 2, &overflow);
    check_int("mul_o flags", overflow, 1);

    /* Text. */
    check_text("concat", faber_concat(&arena, faber_LTA.s, faber_LBC.s), "abc");
    check_text("join", faber_join(&arena, 3, faber_LXE.s, faber_int_text(&arena, -12), faber_LBANG.s), "x=-12!");
    check_text("quoted", faber_quoted(&arena, faber_LQE.s), "\"q\\\"\\\\\\n\\t\\r\\u{1b}e\"");

    /* Lists of genus store pointers and keep the pointed-to objects aliased. */
    {
        int first = 1;
        int second = 2;
        faber_list_test_genus_pointer *pointers = faber_list_test_genus_pointer_of(
            &arena, 2, (faber_test_genus_pointer[]){&first, &second});
        check_int("genus pointer list initial length", pointers->len, 2);
        *faber_list_test_genus_pointer_at(pointers, 0) = 41;
        check_int("genus pointer list alias", first, 41);
        faber_list_test_genus_pointer_set(pointers, 1, &first);
        if (faber_list_test_genus_pointer_at(pointers, 1) != &first) {
            fprintf(stderr, "FAIL genus pointer list indexed replacement\n");
            failures++;
        }
        faber_list_test_genus_pointer_push(&arena, pointers, &second);
        check_int("genus pointer list append length", pointers->len, 3);
        if (faber_list_test_genus_pointer_at(pointers, 2) != &second) {
            fprintf(stderr, "FAIL genus pointer list appended identity\n");
            failures++;
        }
    }

    /* Lists grow past their first chunk and show as the runner does. */
    ints = faber_list_i64_new(&arena, 0);
    for (index = 0; index < 100000; index++) {
        faber_list_i64_push(&arena, ints, index);
    }
    check_int("list len", ints->len, 100000);
    check_int("list at", faber_list_i64_at(ints, 99999), 99999);
    faber_list_i64_set(ints, 0, -5);
    ints->len = 3;
    check_text("list show", faber_list_i64_show(&arena, ints), "[-5, 1, 2]");
    texts = faber_list_text_of(&arena, 2, (faber_text[]){"a", "b\"c"});
    check_text("text show", faber_list_text_show(&arena, texts), "[\"a\", \"b\\\"c\"]");
    flags = faber_list_bool_of(&arena, 2, (bool[]){true, false});
    check_text("bool show", faber_list_bool_show(&arena, flags, "true", "false"), "[true, false]");
    check_text("empty show", faber_list_i64_show(&arena, faber_list_i64_new(&arena, 0)), "[]");
    bits = faber_list_b8_new(&arena, 0);
    faber_list_b8_push(&arena, bits, 0x00);
    faber_list_b8_push(&arena, bits, 0x0f);
    faber_list_b8_push(&arena, bits, 0xa5);
    faber_list_b8_push(&arena, bits, 0xff);
    check_text("raw bits list show", faber_list_b8_show(&arena, bits),
               "[0x00, 0x0f, 0xa5, 0xff]");

    /* Floats print as the runner prints them. */
    {
        char text[FABER_F64_TEXT];
        double zero = 0.0;
        check_text("f64 0.1+0.2", faber_f64_text(0.1 + 0.2, text), "0.30000000000000004");
        check_text("f64 1/3", faber_f64_text(1.0 / 3.0, text), "0.3333333333333333");
        check_text("f64 1e-7", faber_f64_text(1e-7, text), "0.0000001");
        check_text("f64 -2.5", faber_f64_text(-2.5, text), "-2.5");
        check_text("f64 1.0", faber_f64_text(1.0, text), "1.0");
        check_text("f64 -0.0", faber_f64_text(-0.0, text), "-0.0");
        check_text("f64 big", faber_f64_text(123456789012345680000.0, text), "123456789012345683968.0");
        check_text("f64 1e22", faber_f64_text(1e22, text), "10000000000000000000000.0");
        check_text("f64 inf", faber_f64_text(1.0 / zero, text), "inf");
        check_text("f64 -inf", faber_f64_text(-1.0 / zero, text), "-inf");
        check_text("f64 nan", faber_f64_text(zero / zero, text), "NaN");
        {
            /* The smallest subnormal: "0.", 323 zeros, "5" (checked against
             * `faber run`). */
            char want[330] = "0.";
            memset(want + 2, '0', 323);
            want[325] = '5';
            want[326] = '\0';
            check_text("f64 tiny", faber_f64_text(5e-324, text), want);
        }
    }
    check_int("fmod -7.5%2", (faber_i128)(faber_fmod_f64(-7.5, 2.0) * 10), 5);
    check_int("fmod 7.5%-2", (faber_i128)(faber_fmod_f64(7.5, -2.0) * 10), -5);
    check_int("nearest 2.5", faber_nearest_i64(2.5), 2);
    check_int("nearest 3.5", faber_nearest_i64(3.5), 4);
    check_int("nearest -2.5", faber_nearest_i64(-2.5), -2);
    check_int("saturate big", faber_nearest_i64(1e300), INT64_MAX);
    check_int("int < float exact", faber_int_lt_float(9007199254740993, 9007199254740992.0), 0);
    check_int("int > float exact", faber_int_gt_float(9007199254740993, 9007199254740992.0), 1);

    /* The set index: growth past the first capacity, duplicate adds, delete
     * compaction with a rebuild, and a re-add after the delete. */
    {
        faber_list_i64 *set = faber_list_i64_new(&arena, 0);
        int64_t sum = 0;
        for (index = 0; index < 100; index++) {
            check_int("set add new", faber_set_i64_add(&arena, set, index * 37 % 100), 1);
        }
        check_int("set add duplicate", faber_set_i64_add(&arena, set, 7), 0);
        check_int("set len", set->len, 100);
        for (index = 0; index < 100; index++) {
            check_int("set has member", faber_set_i64_has(set, index), 1);
        }
        check_int("set has absent", faber_set_i64_has(set, 100), 0);
        check_int("set delete member", faber_set_i64_delete(set, 41), 1);
        check_int("set delete absent", faber_set_i64_delete(set, 41), 0);
        check_int("set len after delete", set->len, 99);
        check_int("set has deleted", faber_set_i64_has(set, 41), 0);
        for (index = 0; index < 100; index++) {
            sum += faber_set_i64_has(set, index);
        }
        check_int("set has all but one", sum, 99);
        check_int("set order keeps first", set->items[0], 0);
        check_int("set order keeps second", set->items[1], 37);
        check_int("set order compacts", set->items[93], 78);
        check_int("set re-add after delete", faber_set_i64_add(&arena, set, 41), 1);
        check_int("set len after re-add", set->len, 100);
        check_int("set re-add is last", set->items[99], 41);
    }

    /* Signed zero shares a bucket (== says they are equal); NaN is never
     * found (== says they are not), so it accumulates and cannot be
     * deleted. */
    {
        faber_list_f64 *set = faber_list_f64_new(&arena, 0);
        double zero = 0.0;
        double negative_zero = -0.0;
        double nan = zero / zero;
        check_int("f64 set add zero", faber_set_f64_add(&arena, set, zero), 1);
        check_int("f64 set add minus zero", faber_set_f64_add(&arena, set, negative_zero), 0);
        check_int("f64 set len one zero", set->len, 1);
        check_int("f64 set has minus zero", faber_set_f64_has(set, negative_zero), 1);
        check_int("f64 set add nan", faber_set_f64_add(&arena, set, nan), 1);
        check_int("f64 set add nan again", faber_set_f64_add(&arena, set, nan), 1);
        check_int("f64 set len with nans", set->len, 3);
        check_int("f64 set has nan", faber_set_f64_has(set, nan), 0);
        check_int("f64 set delete nan", faber_set_f64_delete(set, nan), 0);
        check_int("f64 set len keeps nans", set->len, 3);
    }

    /* The map index: growth past the first capacity, an overwrite in place,
     * delete compaction with a rebuild, and a re-add appended at the end. */
    {
        faber_map_i64_i64 *map = faber_map_i64_i64_new(&arena);
        int64_t value = 0;
        int64_t sum = 0;
        for (index = 0; index < 100; index++) {
            faber_map_i64_i64_put(&arena, map, index * 37 % 100, index);
        }
        faber_map_i64_i64_put(&arena, map, 7, 7777);
        check_int("map len", map->keys->len, 100);
        check_int("map get overwritten", faber_map_i64_i64_get(map, 7, &value), 1);
        check_int("map overwritten value", value, 7777);
        for (index = 0; index < 100; index++) {
            sum += faber_map_i64_i64_has(map, index);
        }
        check_int("map has all", sum, 100);
        check_int("map get absent", faber_map_i64_i64_get(map, 100, &value), 0);
        check_int("map delete member", faber_map_i64_i64_delete(map, 41), 1);
        check_int("map delete absent", faber_map_i64_i64_delete(map, 41), 0);
        check_int("map has deleted", faber_map_i64_i64_has(map, 41), 0);
        check_int("map order keeps first", map->keys->items[0], 0);
        check_int("map order compacts", map->keys->items[93], 78);
        faber_map_i64_i64_put(&arena, map, 41, 4141);
        check_int("map re-add is last", map->keys->items[99], 41);
        check_int("map re-add value", faber_map_i64_i64_get(map, 41, &value), 1);
        check_int("map re-add value read", value, 4141);
    }

    /* Text keys hash their bytes and stay strcmp-equal. */
    {
        faber_map_text_i64 *map = faber_map_text_i64_new(&arena);
        int64_t value = 0;
        faber_map_text_i64_put(&arena, map, "alpha", 1);
        faber_map_text_i64_put(&arena, map, "beta", 2);
        faber_map_text_i64_put(&arena, map, "gamma", 3);
        check_int("text map len", map->keys->len, 3);
        check_int("text map get", faber_map_text_i64_get(map, "beta", &value), 1);
        check_int("text map value", value, 2);
        check_int("text map has", faber_map_text_i64_has(map, "gamma"), 1);
        check_int("text map has absent", faber_map_text_i64_has(map, "delta"), 0);
        check_int("text map delete", faber_map_text_i64_delete(map, "alpha"), 1);
        check_int("text map len after delete", map->keys->len, 2);
        check_int("text map has deleted", faber_map_text_i64_has(map, "alpha"), 0);
    }

    /* The fault wire contract: a registered pair names its stable code, an
     * unknown pair is unregistered, and a fault carries kind + path. */
    check_text("fault code not found", faber_fault_code("IoFault", "NotFound"), "ERR001");
    check_text("fault code cancelled", faber_fault_code("IoFault", "Cancelled"), "ERR015");
    check_int("fault code unknown variant", faber_fault_code("IoFault", "NoSuch") == NULL, 1);
    check_int("fault code unknown family", faber_fault_code("NoSuchFault", "NotFound") == NULL, 1);
    {
        faber_fault fault = faber_fault_of(faber_fault_code("IoFault", "NotFound"), "a.txt");
        check_text("fault kind", fault.kind, "ERR001");
        check_text("fault path", fault.path, "a.txt");
    }

    /* An interleaved delete/has stress over a size that forces index growth:
     * after every delete, a survivor must stay findable and the deleted key
     * must not be (a stale slot would answer wrong). */
    {
        faber_list_i64 *set = faber_list_i64_new(&arena, 0);
        faber_map_i64_i64 *map = faber_map_i64_i64_new(&arena);
        int64_t value = 0;
        for (index = 0; index < 200; index++) {
            faber_set_i64_add(&arena, set, index);
            faber_map_i64_i64_put(&arena, map, index, index * 3);
        }
        check_int("stress len", set->len, 200);
        for (index = 0; index < 199; index++) {
            check_int("stress delete", faber_set_i64_delete(set, index), 1);
            check_int("stress delete again", faber_set_i64_delete(set, index), 0);
            check_int("stress has deleted", faber_set_i64_has(set, index), 0);
            check_int("stress has survivor", faber_set_i64_has(set, 199), 1);
            check_int("stress map delete", faber_map_i64_i64_delete(map, index), 1);
            check_int("stress map has deleted", faber_map_i64_i64_has(map, index), 0);
            check_int("stress map has survivor", faber_map_i64_i64_has(map, 199), 1);
        }
        check_int("stress final len", set->len, 1);
        check_int("stress map final len", map->keys->len, 1);
        check_int("stress map get survivor", faber_map_i64_i64_get(map, 199, &value), 1);
        check_int("stress map survivor value", value, 199 * 3);
    }

    /* `push_all` reserves once and copies in bulk; a spread of a list into
     * itself duplicates the elements instead of walking forever. */
    {
        int64_t items[300];
        faber_list_i64 *source;
        faber_list_i64 *target;
        int64_t sum = 0;
        for (index = 0; index < 300; index++) {
            items[index] = index;
        }
        source = faber_list_i64_of(&arena, 300, items);
        target = faber_list_i64_new(&arena, 0);
        faber_list_i64_push_all(&arena, target, source);
        check_int("push_all len", target->len, 300);
        for (index = 0; index < 300; index++) {
            sum += target->items[index] == index;
        }
        check_int("push_all contents", sum, 300);
        faber_list_i64_push_all(&arena, target, target);
        check_int("push_all self len", target->len, 600);
        check_int("push_all self first", target->items[0], 0);
        check_int("push_all self last", target->items[599], 299);
        faber_list_i64_push_all(&arena, target, faber_list_i64_new(&arena, 0));
        check_int("push_all empty len", target->len, 600);
    }

    if (failures != 0) {
        fprintf(stderr, "faber_test: %d failed\n", failures);
        return 1;
    }
    printf("faber_test: ok\n");
    return 0;
}
