/*
 * Runtime checks for faber.h, run by crates/radix-hir-c/scripta/diffcheck
 * under UndefinedBehaviorSanitizer and AddressSanitizer. Each check compares
 * with what the MIR runner prints for the same operation.
 */
#include "faber.h"

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

int main(void) {
    faber_arena arena = {0};
    char buffer[41];
    int overflow = 0;
    faber_list_i64 *ints;
    faber_list_text *texts;
    faber_list_bool *flags;
    int64_t index;

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
    check_text("concat", faber_concat(&arena, "a", "bc"), "abc");
    check_text("join", faber_join(&arena, 3, "x=", faber_int_text(&arena, -12), "!"), "x=-12!");
    check_text("quoted", faber_quoted(&arena, "q\"\\\n\t\r\x1b" "e"), "\"q\\\"\\\\\\n\\t\\r\\u{1b}e\"");

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

    if (failures != 0) {
        fprintf(stderr, "faber_test: %d failed\n", failures);
        return 1;
    }
    printf("faber_test: ok\n");
    return 0;
}
