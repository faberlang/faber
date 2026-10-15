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

static inline int64_t faber_mul_o(int64_t a, int64_t b, int *overflow) {
    int64_t product;
    *overflow |= __builtin_mul_overflow(a, b, &product);
    return product;
}

typedef struct g_Tag g_Tag;
typedef struct g_Item g_Item;

struct g_Tag {
    const char *m_label;
    bool m_hot;
};

struct g_Item {
    int64_t m_id;
    g_Tag *m_tag;
    faber_list_i64 *m_counts;
};

static const char *g_Item_describe(faber_arena *, g_Item *);
static void f_heat(faber_arena *, g_Item *);
static const char *g_Item_show(faber_arena *faber_a, const g_Item *value);
static const char *g_Tag_show(faber_arena *faber_a, const g_Tag *value);

static const char *g_Item_describe(faber_arena *faber_a, g_Item *faber_self) {
    return (__extension__ ({ int64_t faber_t0 = (faber_self)->m_id; const char *faber_t1 = ((faber_self)->m_tag)->m_label; faber_join(faber_a, 4, "item ", faber_int_text(faber_a, faber_t0), ": ", faber_t1); }));
}

static void f_heat(faber_arena *faber_a, g_Item *p_item) {
    ((p_item)->m_tag)->m_hot = (1);
    faber_list_i64_push(faber_a, (p_item)->m_counts, (__extension__ ({ int faber_o = 0; int64_t faber_r = faber_mul_o((p_item)->m_id, INT64_C(10), &faber_o); faber_o ? faber_store_i64(faber_mul((p_item)->m_id, INT64_C(10)), "argument") : faber_r; })));
}

static const char *g_Item_show(faber_arena *faber_a, const g_Item *value) {
    return faber_join(faber_a, 8, "Item { ", "id = ", faber_int_text(faber_a, value->m_id), ", tag = ", g_Tag_show(faber_a, value->m_tag), ", counts = ", faber_list_i64_show(faber_a, value->m_counts), " }");
}

static const char *g_Tag_show(faber_arena *faber_a, const g_Tag *value) {
    return faber_join(faber_a, 6, "Tag { ", "label = ", faber_quoted(faber_a, value->m_label), ", hot = ", ((value->m_hot) ? "true" : "false"), " }");
}

int main(void) {
    faber_arena faber_root = {0};
    faber_arena *const faber_a = &faber_root;
    g_Item *l_a = (__extension__ ({ g_Item *faber_t2 = faber_alloc(faber_a, sizeof *faber_t2); faber_t2->m_id = INT64_C(1); faber_t2->m_tag = (__extension__ ({ g_Tag *faber_t3 = faber_alloc(faber_a, sizeof *faber_t3); faber_t3->m_label = "x\ty"; faber_t3->m_hot = (0); faber_t3; })); faber_t2->m_counts = faber_list_i64_new(faber_a, 0); faber_t2; }));
    g_Item *l_b = (__extension__ ({ g_Item *faber_t4 = faber_alloc(faber_a, sizeof *faber_t4); faber_t4->m_id = INT64_C(2); faber_t4->m_tag = (__extension__ ({ g_Tag *faber_t5 = faber_alloc(faber_a, sizeof *faber_t5); faber_t5->m_label = "default"; faber_t5->m_hot = (0); faber_t5; })); faber_t4->m_counts = faber_list_i64_new(faber_a, 0); faber_t4; }));
    f_heat(faber_a, l_a);
    f_heat(faber_a, l_a);
    printf("%s\n", g_Item_show(faber_a, l_a));
    printf("%s\n", g_Item_show(faber_a, l_b));
    {
        const char *faber_t6 = g_Item_describe(faber_a, l_a); const char *faber_t7 = g_Item_describe(faber_a, l_b);
        printf("%s %s\n", faber_t6, faber_t7);
    }
    (l_b)->m_tag = (l_a)->m_tag;
    ((l_b)->m_tag)->m_label = "shared";
    printf("%s %s\n", ((l_a)->m_tag)->m_label, ((((l_b)->m_tag)->m_hot) ? "true" : "false"));
    return 0;
}
