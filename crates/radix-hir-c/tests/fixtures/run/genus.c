#include <inttypes.h>
#include <stdarg.h>
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

static const char *faber_int_text(faber_arena *arena, int64_t value) {
    char *out = faber_alloc(arena, 21);
    snprintf(out, 21, "%" PRId64, value);
    return out;
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

static size_t faber_quote(char *out, const char *text) {
    const unsigned char *cursor;
    size_t at = 0;
    for (cursor = (const unsigned char *)text; *cursor != 0; cursor++) {
        char escape[8];
        size_t length;
        switch (*cursor) {
        case '"':
            memcpy(escape, "\\\"", 3);
            length = 2;
            break;
        case '\\':
            memcpy(escape, "\\\\", 3);
            length = 2;
            break;
        case '\n':
            memcpy(escape, "\\n", 3);
            length = 2;
            break;
        case '\t':
            memcpy(escape, "\\t", 3);
            length = 2;
            break;
        case '\r':
            memcpy(escape, "\\r", 3);
            length = 2;
            break;
        default:
            if (*cursor < 0x20 || *cursor == 0x7f) {
                length = (size_t)snprintf(escape, sizeof escape, "\\u{%x}", (unsigned)*cursor);
            } else {
                escape[0] = (char)*cursor;
                length = 1;
            }
            break;
        }
        if (out != NULL) {
            memcpy(out + at, escape, length);
        }
        at += length;
    }
    return at;
}

static const char *faber_quoted(faber_arena *arena, const char *text) {
    size_t length = faber_quote(NULL, text);
    char *out = faber_alloc(arena, length + 3);
    out[0] = '"';
    faber_quote(out + 1, text);
    out[length + 1] = '"';
    out[length + 2] = '\0';
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

typedef struct g_Point g_Point;

struct g_Point {
    int64_t m_x;
    int64_t m_y;
    const char *m_tag;
};

static int64_t g_Point_sum(faber_arena *, g_Point *);
static void g_Point_shift(faber_arena *, g_Point *, int64_t);
static g_Point *f_make(faber_arena *, int64_t);
static const char *g_Point_show(faber_arena *faber_a, const g_Point *value);

static int64_t g_Point_sum(faber_arena *faber_a, g_Point *faber_self) {
    (void)faber_a;
    return (__extension__ ({ int faber_o = 0; int64_t faber_r = faber_add_o((faber_self)->m_x, (faber_self)->m_y, &faber_o); faber_o ? faber_store_i64(faber_add((faber_self)->m_x, (faber_self)->m_y), "return") : faber_r; }));
}

static void g_Point_shift(faber_arena *faber_a, g_Point *faber_self, int64_t p_dx) {
    (void)faber_a;
    (faber_self)->m_x = (__extension__ ({ int faber_o = 0; int64_t faber_r = faber_add_o((faber_self)->m_x, p_dx, &faber_o); faber_o ? faber_store_i64(faber_add((faber_self)->m_x, p_dx), "assignment to `x`") : faber_r; }));
}

static g_Point *f_make(faber_arena *faber_a, int64_t p_x) {
    return (__extension__ ({ g_Point *faber_t0 = faber_alloc(faber_a, sizeof *faber_t0); faber_t0->m_x = p_x; faber_t0->m_y = (__extension__ ({ int faber_o = 0; int64_t faber_r = faber_mul_o(p_x, INT64_C(2), &faber_o); faber_o ? faber_store_i64(faber_mul(p_x, INT64_C(2)), "field `y`") : faber_r; })); faber_t0->m_tag = "p"; faber_t0; }));
}

static const char *g_Point_show(faber_arena *faber_a, const g_Point *value) {
    return faber_join(faber_a, 8, "Point { ", "x = ", faber_int_text(faber_a, value->m_x), ", y = ", faber_int_text(faber_a, value->m_y), ", tag = ", faber_quoted(faber_a, value->m_tag), " }");
}

int main(void) {
    faber_arena faber_root = {0};
    faber_arena *const faber_a = &faber_root;
    g_Point *l_p = (__extension__ ({ g_Point *faber_t1 = faber_alloc(faber_a, sizeof *faber_t1); faber_t1->m_x = INT64_C(1); faber_t1->m_y = INT64_C(2); faber_t1->m_tag = "p"; faber_t1; }));
    {
        int64_t faber_t2 = (l_p)->m_x; int64_t faber_t3 = (l_p)->m_y; const char *faber_t4 = (l_p)->m_tag; int64_t faber_t5 = g_Point_sum(faber_a, l_p);
        printf("%" PRId64 " %" PRId64 " %s %" PRId64 "\n", faber_t2, faber_t3, faber_t4, faber_t5);
    }
    g_Point *l_q = l_p;
    g_Point_shift(faber_a, l_q, INT64_C(10));
    printf("%" PRId64 " %" PRId64 "\n", (l_p)->m_x, (l_q)->m_x);
    (l_p)->m_y = INT64_C(5);
    {
        int64_t faber_t6 = (l_q)->m_y; int64_t faber_t7 = g_Point_sum(faber_a, l_q);
        printf("%" PRId64 " %" PRId64 "\n", faber_t6, faber_t7);
    }
    g_Point *const l_m = f_make(faber_a, INT64_C(4));
    printf("%" PRId64 " %" PRId64 " %s\n", (l_m)->m_x, (l_m)->m_y, (l_m)->m_tag);
    printf("%s\n", g_Point_show(faber_a, l_p));
    return 0;
}
