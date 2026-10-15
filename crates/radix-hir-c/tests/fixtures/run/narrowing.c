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

static int64_t faber_fit_i64(faber_i128 value) {
    if (value < INT64_MIN || value > INT64_MAX) {
        faber_trap("numerus overflow");
    }
    return (int64_t)value;
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
    int64_t l_a = -INT64_C(7);
    printf("%" PRId64 "\n", faber_fdiv_i64(l_a, INT64_C(2)));
    printf("%" PRId64 "\n", faber_fmod_i64(l_a, INT64_C(2)));
    printf("%" PRId64 "\n", faber_fdiv_i64(l_a, -INT64_C(2)));
    printf("%" PRId64 "\n", faber_fmod_i64(l_a, -INT64_C(2)));
    printf("%" PRId64 "\n", faber_fdiv_i64(l_a, INT64_C(1)));
    printf("%" PRId64 "\n", faber_fmod_i64(l_a, INT64_C(1)));
    printf("%" PRId64 "\n", faber_fdiv_i64(l_a, INT64_C(3)));
    printf("%" PRId64 "\n", faber_fmod_i64(l_a, -INT64_C(3)));
    int64_t l_big = INT64_C(9223372036854775807);
    int64_t l_low = (__extension__ ({ int faber_o = 0; int64_t faber_r = faber_sub_o(faber_sub_o(INT64_C(0), l_big, &faber_o), INT64_C(1), &faber_o); faber_o ? faber_fit_i64(faber_sub(faber_sub(INT64_C(0), l_big), INT64_C(1))) : faber_r; }));
    printf("%" PRId64 "\n", faber_fdiv_i64(l_big, INT64_C(2)));
    printf("%" PRId64 "\n", faber_fmod_i64(l_big, -INT64_C(2)));
    printf("%" PRId64 "\n", faber_fdiv_i64(l_low, INT64_C(2)));
    printf("%" PRId64 "\n", faber_fmod_i64(l_low, INT64_C(3)));
    printf("%" PRId64 "\n", faber_fdiv_i64(l_low, -INT64_C(2)));
    printf("%" PRId64 "\n", faber_fmod_i64(l_low, -INT64_C(2)));
    int64_t l_half = (__extension__ ({ int faber_o = 0; int64_t faber_r = faber_fdiv_i64(faber_mul_o(l_big, INT64_C(2), &faber_o), INT64_C(4)); faber_o ? faber_store_i64(faber_div(faber_mul(l_big, INT64_C(2)), INT64_C(4)), "declaration of `half`") : faber_r; }));
    printf("%" PRId64 "\n", l_half);
    int64_t l_back = (__extension__ ({ int faber_o = 0; int64_t faber_r = faber_sub_o(faber_add_o(l_big, INT64_C(1), &faber_o), INT64_C(1), &faber_o); faber_o ? faber_fit_i64(faber_sub(faber_add(l_big, INT64_C(1)), INT64_C(1))) : faber_r; }));
    printf("%" PRId64 "\n", l_back);
    return 0;
}
