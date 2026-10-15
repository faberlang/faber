#include <inttypes.h>
#include <stdarg.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

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

typedef struct faber_chunk {
    struct faber_chunk *next;
    size_t size;
    size_t used;
    unsigned char data[];
} faber_chunk;

typedef struct faber_arena {
    faber_chunk *head;
} faber_arena;

static void *faber_alloc(faber_arena *arena, size_t size) {
    const size_t align = _Alignof(max_align_t);
    faber_chunk *chunk = arena->head;
    size_t capacity;
    if (chunk != NULL) {
        unsigned char *next = chunk->data + chunk->used;
        size_t pad = (align - (uintptr_t)next % align) % align;
        if (pad <= chunk->size - chunk->used && size <= chunk->size - chunk->used - pad) {
            chunk->used += pad + size;
            return next + pad;
        }
    }
    if (size > SIZE_MAX - sizeof(faber_chunk) - align) {
        faber_trap("out of memory");
    }
    capacity = size + align > 65536 ? size + align : 65536;
    chunk = malloc(sizeof(faber_chunk) + capacity);
    if (chunk == NULL) {
        faber_trap("out of memory");
    }
    chunk->next = arena->head;
    chunk->size = capacity;
    chunk->used = 0;
    arena->head = chunk;
    return faber_alloc(arena, size);
}

static const char *faber_concat(faber_arena *arena, const char *a, const char *b) {
    size_t a_len = strlen(a);
    size_t b_len = strlen(b);
    char *out = faber_alloc(arena, a_len + b_len + 1);
    memcpy(out, a, a_len);
    memcpy(out + a_len, b, b_len + 1);
    return out;
}

static const char *faber_int_text(faber_arena *arena, int64_t value) {
    char *out = faber_alloc(arena, 21);
    snprintf(out, 21, "%" PRId64, value);
    return out;
}

static const char *faber_exact_text(faber_arena *arena, faber_i128 value) {
    return faber_i128_text(value, faber_alloc(arena, 41));
}

static const char *faber_join(faber_arena *arena, int count, ...) {
    va_list pieces;
    size_t total = 1;
    char *out;
    char *cursor;
    int index;
    va_start(pieces, count);
    for (index = 0; index < count; index++) {
        total += strlen(va_arg(pieces, const char *));
    }
    va_end(pieces);
    out = faber_alloc(arena, total);
    cursor = out;
    va_start(pieces, count);
    for (index = 0; index < count; index++) {
        const char *piece = va_arg(pieces, const char *);
        size_t length = strlen(piece);
        memcpy(cursor, piece, length);
        cursor += length;
    }
    va_end(pieces);
    *cursor = '\0';
    return out;
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

static faber_i128 faber_mul(faber_i128 a, faber_i128 b) {
    faber_i128 product;
    if (__builtin_mul_overflow(a, b, &product)) {
        faber_trap("numerus overflow");
    }
    return faber_cap(product);
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

static const char *f_label(faber_arena *, const char *, int64_t);
static int64_t f_quiet(faber_arena *);

static const char *f_label(faber_arena *faber_a, const char *p_name, int64_t p_n) {
    return faber_concat(faber_a, faber_concat(faber_a, faber_concat(faber_a, faber_concat(faber_a, "<", p_name), ":"), faber_join(faber_a, 1, faber_int_text(faber_a, p_n))), ">");
}

static int64_t f_quiet(faber_arena *faber_a) {
    (void)faber_a;
    return INT64_C(3);
}

int main(void) {
    faber_arena faber_root = {0};
    faber_arena *const faber_a = &faber_root;
    int64_t l_a = INT64_C(7);
    const char *l_s = "x";
    printf("%s\n", faber_join(faber_a, 8, "a=", faber_int_text(faber_a, l_a), ", s=", l_s, ", again ", faber_int_text(faber_a, l_a), " ", l_s));
    printf("%s\n", faber_join(faber_a, 3, l_s, "-", faber_int_text(faber_a, l_a)));
    printf("%s\n", (__extension__ ({ faber_i128 faber_t0 = faber_add(l_a, INT64_C(1)); bool faber_t1 = (l_a > INT64_C(3)); faber_i128 faber_t2 = faber_mul(l_a, INT64_C(1000000000000)); faber_join(faber_a, 6, "sum ", faber_exact_text(faber_a, faber_t0), ", flag ", ((faber_t1) ? "true" : "false"), ", big ", faber_exact_text(faber_a, faber_t2)); })));
    const char *l_t = faber_concat(faber_a, faber_concat(faber_a, "<", l_s), ">");
    printf("%s\n", l_t);
    printf("%s\n", (__extension__ ({ const char *faber_t3 = "item"; int64_t faber_t4 = f_quiet(faber_a); f_label(faber_a, faber_t3, faber_t4); })));
    const char *l_acc = "";
    int64_t l_i = INT64_C(0);
    while (l_i < INT64_C(5)) {
        l_acc = faber_concat(faber_a, l_acc, faber_join(faber_a, 2, faber_int_text(faber_a, l_i), ","));
        l_i = (__extension__ ({ int faber_o = 0; int64_t faber_r = faber_add_o(l_i, INT64_C(1), &faber_o); faber_o ? faber_store_i64(faber_add(l_i, INT64_C(1)), "assignment to `i`") : faber_r; }));
    }
    printf("%s\n", l_acc);
    return 0;
}
