#include <inttypes.h>
#include <stdbool.h>
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

static int64_t faber_store_i64(faber_i128 value, const char *at) {
    if (value < INT64_MIN || value > INT64_MAX) {
        char text[41];
        fprintf(stderr, "error: %s does not fit in `i64` (%s)\n", faber_i128_text(value, text), at);
        exit(1);
    }
    return (int64_t)value;
}

static inline int64_t faber_sub_o(int64_t a, int64_t b, int *overflow) {
    int64_t difference;
    *overflow |= __builtin_sub_overflow(a, b, &difference);
    return difference;
}

static inline int64_t faber_fmod_i64(int64_t a, int64_t b) {
    int64_t remainder = a % b;
    if (remainder != 0 && ((remainder < 0) != (b < 0))) {
        remainder += b;
    }
    return remainder;
}

static const int64_t k_LIMIT = INT64_C(10);
static const char *const k_GREETING = "hi";
static const bool k_LOUD = (1);

static int64_t f_fib(int64_t);
static int64_t f_noisy(int64_t);
static bool f_is_even(int64_t);
static void f_shout(const char *, bool);

static int64_t f_fib(int64_t p_n) {
    if (p_n < INT64_C(2)) {
        return p_n;
    }
    return faber_store_i64((__extension__ ({ int64_t faber_t0 = f_fib((__extension__ ({ int faber_o = 0; int64_t faber_r = faber_sub_o(p_n, INT64_C(1), &faber_o); faber_o ? faber_store_i64(faber_sub(p_n, INT64_C(1)), "argument") : faber_r; }))); int64_t faber_t1 = f_fib((__extension__ ({ int faber_o = 0; int64_t faber_r = faber_sub_o(p_n, INT64_C(2), &faber_o); faber_o ? faber_store_i64(faber_sub(p_n, INT64_C(2)), "argument") : faber_r; }))); faber_add(faber_t0, faber_t1); })), "return");
}

static int64_t f_noisy(int64_t p_x) {
    printf("%s %" PRId64 "\n", "noisy", p_x);
    return p_x;
}

static bool f_is_even(int64_t p_n) {
    return (faber_fmod_i64(p_n, INT64_C(2)) == INT64_C(0));
}

static void f_shout(const char *p_s, bool p_loud) {
    if (p_loud) {
        printf("%s %s\n", p_s, "!");
    } else {
        printf("%s\n", p_s);
    }
}

int main(void) {
    printf("%" PRId64 "\n", f_fib(k_LIMIT));
    printf("%s\n", faber_i128_text((__extension__ ({ int64_t faber_t2 = f_noisy(INT64_C(1)); int64_t faber_t3 = f_noisy(INT64_C(2)); faber_add(faber_t2, faber_t3); })), (char[41]){0}));
    {
        int64_t faber_t4 = f_noisy(INT64_C(3)); int64_t faber_t5 = f_noisy(INT64_C(4));
        printf("%" PRId64 " %" PRId64 "\n", faber_t4, faber_t5);
    }
    bool l_flag = (f_is_even(k_LIMIT));
    {
        bool faber_t6 = (l_flag); bool faber_t7 = (f_is_even(INT64_C(7))); bool faber_t8 = (k_LOUD);
        printf("%s %s %s\n", ((faber_t6) ? "true" : "false"), ((faber_t7) ? "true" : "false"), ((faber_t8) ? "true" : "false"));
    }
    f_shout(k_GREETING, (k_LOUD));
    f_shout("quiet", (0));
    l_flag = (!(l_flag));
    printf("%s\n", ((l_flag) ? "true" : "false"));
    return 0;
}
