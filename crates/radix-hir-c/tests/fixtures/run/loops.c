#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>

__extension__ typedef __int128 faber_i128;
__extension__ typedef unsigned __int128 faber_u128;

static _Noreturn void faber_trap(const char *reason) {
    fprintf(stderr, "error: %s\n", reason);
    exit(1);
}

static const char *faber_i128_text(faber_i128 value, char *buffer) {
    char *cursor = buffer + 40;
    faber_u128 magnitude = value < 0 ? -(faber_u128)value : (faber_u128)value;
    *cursor = '\0';
    do {
        *--cursor = (char)('0' + (int)(magnitude % 10));
        magnitude /= 10;
    } while (magnitude != 0);
    if (value < 0) {
        *--cursor = '-';
    }
    return cursor;
}

static faber_i128 faber_cap(faber_i128 value) {
    if (value < INT64_MIN || value > (faber_i128)UINT64_MAX) {
        faber_trap("numerus overflow");
    }
    return value;
}

static faber_i128 faber_add(faber_i128 a, faber_i128 b) {
    return faber_cap(a + b);
}

static faber_i128 faber_sub(faber_i128 a, faber_i128 b) {
    return faber_cap(a - b);
}

static faber_i128 faber_mul(faber_i128 a, faber_i128 b) {
    faber_i128 product;
    if (__builtin_mul_overflow(a, b, &product)) {
        faber_trap("numerus overflow");
    }
    return faber_cap(product);
}

static faber_i128 faber_div(faber_i128 a, faber_i128 b) {
    faber_i128 quotient;
    if (b == 0) {
        faber_trap("numerus division failed");
    }
    quotient = a / b;
    if (a % b != 0 && ((a < 0) != (b < 0))) {
        quotient -= 1;
    }
    return quotient;
}

static faber_i128 faber_rem(faber_i128 a, faber_i128 b) {
    return a - b * faber_div(a, b);
}

static int64_t faber_store_i64(faber_i128 value, const char *at) {
    if (value < INT64_MIN || value > INT64_MAX) {
        char text[41];
        fprintf(stderr, "error: %s does not fit in `i64` (%s)\n", faber_i128_text(value, text), at);
        exit(1);
    }
    return (int64_t)value;
}

static inline int64_t faber_add_o(int64_t a, int64_t b, int *overflow) {
    int64_t sum;
    *overflow |= __builtin_add_overflow(a, b, &sum);
    return sum;
}

static inline int64_t faber_sub_o(int64_t a, int64_t b, int *overflow) {
    int64_t difference;
    *overflow |= __builtin_sub_overflow(a, b, &difference);
    return difference;
}

static inline int64_t faber_mul_o(int64_t a, int64_t b, int *overflow) {
    int64_t product;
    *overflow |= __builtin_mul_overflow(a, b, &product);
    return product;
}

static inline int64_t faber_fdiv_i64(int64_t a, int64_t b) {
    int64_t quotient = a / b;
    if (a % b != 0 && ((a < 0) != (b < 0))) {
        quotient -= 1;
    }
    return quotient;
}

static inline int64_t faber_fmod_i64(int64_t a, int64_t b) {
    int64_t remainder = a % b;
    if (remainder != 0 && ((remainder < 0) != (b < 0))) {
        remainder += b;
    }
    return remainder;
}

int main(void) {
    int64_t l_best = INT64_C(0);
    int64_t l_best_start = INT64_C(0);
    int64_t l_start = INT64_C(1);
    while (l_start < INT64_C(1000)) {
        int64_t l_n = l_start;
        int64_t l_steps = INT64_C(0);
        while (l_n != INT64_C(1)) {
            if (faber_fmod_i64(l_n, INT64_C(2)) == INT64_C(0)) {
                l_n = faber_fdiv_i64(l_n, INT64_C(2));
            } else {
                l_n = (__extension__ ({ int faber_o = 0; int64_t faber_r = faber_add_o(faber_mul_o(INT64_C(3), l_n, &faber_o), INT64_C(1), &faber_o); faber_o ? faber_store_i64(faber_add(faber_mul(INT64_C(3), l_n), INT64_C(1)), "assignment to `n`") : faber_r; }));
            }
            l_steps = (__extension__ ({ int faber_o = 0; int64_t faber_r = faber_add_o(l_steps, INT64_C(1), &faber_o); faber_o ? faber_store_i64(faber_add(l_steps, INT64_C(1)), "assignment to `steps`") : faber_r; }));
        }
        if (l_steps > l_best) {
            l_best = l_steps;
            l_best_start = l_start;
        }
        l_start = (__extension__ ({ int faber_o = 0; int64_t faber_r = faber_add_o(l_start, INT64_C(1), &faber_o); faber_o ? faber_store_i64(faber_add(l_start, INT64_C(1)), "assignment to `start`") : faber_r; }));
    }
    printf("%" PRId64 " %" PRId64 "\n", l_best_start, l_best);
    int64_t l_a = -INT64_C(7);
    int64_t l_b = INT64_C(2);
    int64_t l_seven = INT64_C(7);
    int64_t l_nb = (__extension__ ({ int faber_o = 0; int64_t faber_r = faber_sub_o(INT64_C(0), l_b, &faber_o); faber_o ? faber_store_i64(faber_sub(INT64_C(0), l_b), "declaration of `nb`") : faber_r; }));
    printf("%s %s %s %s %s\n", faber_i128_text(faber_div(l_a, l_b), (char[41]){0}), faber_i128_text(faber_rem(l_a, l_b), (char[41]){0}), faber_i128_text(faber_div(l_seven, l_nb), (char[41]){0}), faber_i128_text(faber_rem(l_seven, l_nb), (char[41]){0}), faber_i128_text(faber_sub(0, l_a), (char[41]){0}));
    int64_t l_i = INT64_C(0);
    int64_t l_total = INT64_C(0);
    while (1) {
        l_i = (__extension__ ({ int faber_o = 0; int64_t faber_r = faber_add_o(l_i, INT64_C(1), &faber_o); faber_o ? faber_store_i64(faber_add(l_i, INT64_C(1)), "assignment to `i`") : faber_r; }));
        if (l_i > INT64_C(20)) {
            break;
        }
        if ((faber_fmod_i64(l_i, INT64_C(3)) == INT64_C(0)) || (faber_fmod_i64(l_i, INT64_C(5)) == INT64_C(0))) {
            continue;
        }
        if ((l_i >= INT64_C(2)) && (!(l_i == INT64_C(7)))) {
            l_total = (__extension__ ({ int faber_o = 0; int64_t faber_r = faber_add_o(l_total, l_i, &faber_o); faber_o ? faber_store_i64(faber_add(l_total, l_i), "assignment to `total`") : faber_r; }));
        }
    }
    printf("%" PRId64 "\n", l_total);
    const int64_t l_top = INT64_C(9223372036854775807);
    int64_t l_one = INT64_C(1);
    printf("%s\n", faber_i128_text(faber_add(l_top, l_one), (char[41]){0}));
    return 0;
}
