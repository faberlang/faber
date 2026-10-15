#include <inttypes.h>
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

static int64_t faber_list_index(faber_i128 index, int64_t length) {
    if (index < 0) {
        faber_trap("expected non-negative numerus index");
    }
    if (index >= length) {
        faber_trap("array index out of bounds");
    }
    return (int64_t)index;
}

typedef struct faber_list_i64 {
    int64_t *items;
    int64_t len;
    int64_t cap;
} faber_list_i64;

static faber_list_i64 *faber_list_i64_new(faber_arena *arena, int64_t capacity) {
    faber_list_i64 *list = faber_alloc(arena, sizeof *list);
    list->items = capacity > 0 ? faber_alloc(arena, (size_t)capacity * sizeof *list->items) : NULL;
    list->len = 0;
    list->cap = capacity;
    return list;
}

static void faber_list_i64_push(faber_arena *arena, faber_list_i64 *list, int64_t item) {
    if (list->len == list->cap) {
        int64_t capacity = list->cap < 4 ? 4 : list->cap * 2;
        int64_t *items = faber_alloc(arena, (size_t)capacity * sizeof *items);
        if (list->len > 0) {
            memcpy(items, list->items, (size_t)list->len * sizeof *items);
        }
        list->items = items;
        list->cap = capacity;
    }
    list->items[list->len++] = item;
}

static int64_t faber_list_i64_at(const faber_list_i64 *list, faber_i128 index) {
    return list->items[faber_list_index(index, list->len)];
}

static const char *faber_list_i64_show(faber_arena *arena, const faber_list_i64 *list) {
    size_t capacity = 3 + (size_t)list->len * 22;
    char *out = faber_alloc(arena, capacity);
    size_t at = 0;
    int64_t index;
    out[at++] = '[';
    for (index = 0; index < list->len; index++) {
        if (index > 0) {
            out[at++] = ',';
            out[at++] = ' ';
        }
        at += (size_t)snprintf(out + at, capacity - at, "%" PRId64, list->items[index]);
    }
    out[at++] = ']';
    out[at] = '\0';
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

static inline int64_t faber_mul_o(int64_t a, int64_t b, int *overflow) {
    int64_t product;
    *overflow |= __builtin_mul_overflow(a, b, &product);
    return product;
}

static inline int64_t faber_fmod_i64(int64_t a, int64_t b) {
    int64_t remainder = a % b;
    if (remainder != 0 && ((remainder < 0) != (b < 0))) {
        remainder += b;
    }
    return remainder;
}

static void f_fill(faber_arena *, faber_list_i64 *, int64_t);
static faber_list_i64 *f_evens(faber_arena *, int64_t);
static int64_t f_total(faber_arena *, faber_list_i64 *);

static void f_fill(faber_arena *faber_a, faber_list_i64 *p_xs, int64_t p_count) {
    int64_t l_i = INT64_C(0);
    while (l_i < p_count) {
        faber_list_i64_push(faber_a, p_xs, (__extension__ ({ int faber_o = 0; int64_t faber_r = faber_mul_o(l_i, l_i, &faber_o); faber_o ? faber_store_i64(faber_mul(l_i, l_i), "argument") : faber_r; })));
        l_i = (__extension__ ({ int faber_o = 0; int64_t faber_r = faber_add_o(l_i, INT64_C(1), &faber_o); faber_o ? faber_store_i64(faber_add(l_i, INT64_C(1)), "assignment to `i`") : faber_r; }));
    }
}

static faber_list_i64 *f_evens(faber_arena *faber_a, int64_t p_limit) {
    faber_list_i64 *l_out = faber_list_i64_new(faber_a, 0);
    int64_t l_i = INT64_C(0);
    while (l_i < p_limit) {
        if (faber_fmod_i64(l_i, INT64_C(2)) == INT64_C(0)) {
            faber_list_i64_push(faber_a, l_out, l_i);
        }
        l_i = (__extension__ ({ int faber_o = 0; int64_t faber_r = faber_add_o(l_i, INT64_C(1), &faber_o); faber_o ? faber_store_i64(faber_add(l_i, INT64_C(1)), "assignment to `i`") : faber_r; }));
    }
    return l_out;
}

static int64_t f_total(faber_arena *faber_a, faber_list_i64 *p_xs) {
    (void)faber_a;
    int64_t l_sum = INT64_C(0);
    {
        faber_list_i64 *const faber_t0 = p_xs;
        int64_t faber_i0;
        for (faber_i0 = 0; faber_i0 < faber_t0->len; faber_i0++) {
            const int64_t l_x = faber_t0->items[faber_i0];
            l_sum = (__extension__ ({ int faber_o = 0; int64_t faber_r = faber_add_o(l_sum, l_x, &faber_o); faber_o ? faber_store_i64(faber_add(l_sum, l_x), "assignment to `sum`") : faber_r; }));
        }
    }
    return l_sum;
}

int main(void) {
    faber_arena faber_root = {0};
    faber_arena *const faber_a = &faber_root;
    faber_list_i64 *l_squares = faber_list_i64_new(faber_a, 0);
    f_fill(faber_a, l_squares, INT64_C(6));
    {
        faber_list_i64 *faber_t1 = l_squares; int64_t faber_t2 = f_total(faber_a, l_squares);
        printf("%s %" PRId64 "\n", faber_list_i64_show(faber_a, faber_t1), faber_t2);
    }
    faber_list_i64 *const l_e = f_evens(faber_a, INT64_C(9));
    {
        faber_list_i64 *faber_t3 = l_e; int64_t faber_t4 = (l_e)->len; int64_t faber_t5 = f_total(faber_a, l_e);
        printf("%s %" PRId64 " %" PRId64 "\n", faber_list_i64_show(faber_a, faber_t3), faber_t4, faber_t5);
    }
    faber_list_i64 *l_big = faber_list_i64_new(faber_a, 0);
    int64_t l_n = INT64_C(0);
    while (l_n < INT64_C(1000)) {
        faber_list_i64_push(faber_a, l_big, l_n);
        l_n = (__extension__ ({ int faber_o = 0; int64_t faber_r = faber_add_o(l_n, INT64_C(1), &faber_o); faber_o ? faber_store_i64(faber_add(l_n, INT64_C(1)), "assignment to `n`") : faber_r; }));
    }
    {
        int64_t faber_t6 = (l_big)->len; int64_t faber_t7 = faber_list_i64_at(l_big, INT64_C(999)); int64_t faber_t8 = f_total(faber_a, l_big);
        printf("%" PRId64 " %" PRId64 " %" PRId64 "\n", faber_t6, faber_t7, faber_t8);
    }
    return 0;
}
