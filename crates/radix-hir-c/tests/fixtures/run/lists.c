#include <inttypes.h>
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

static int64_t faber_list_index(faber_i128 index, int64_t length) {
    if (index < 0) {
        faber_trap("expected non-negative numerus index");
    }
    if (index >= length) {
        faber_trap("array index out of bounds");
    }
    return (int64_t)index;
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

static faber_list_i64 *faber_list_i64_of(faber_arena *arena, int64_t count, const int64_t *items) {
    faber_list_i64 *list = faber_list_i64_new(arena, count);
    memcpy(list->items, items, (size_t)count * sizeof *items);
    list->len = count;
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

static void faber_list_i64_set(faber_list_i64 *list, faber_i128 index, int64_t item) {
    list->items[faber_list_index(index, list->len)] = item;
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

typedef const char *faber_text;

typedef struct faber_list_text {
    faber_text *items;
    int64_t len;
    int64_t cap;
} faber_list_text;

static faber_list_text *faber_list_text_new(faber_arena *arena, int64_t capacity) {
    faber_list_text *list = faber_alloc(arena, sizeof *list);
    list->items = capacity > 0 ? faber_alloc(arena, (size_t)capacity * sizeof *list->items) : NULL;
    list->len = 0;
    list->cap = capacity;
    return list;
}

static faber_list_text *faber_list_text_of(faber_arena *arena, int64_t count, const faber_text *items) {
    faber_list_text *list = faber_list_text_new(arena, count);
    memcpy(list->items, items, (size_t)count * sizeof *items);
    list->len = count;
    return list;
}

static const char *faber_list_text_show(faber_arena *arena, const faber_list_text *list) {
    size_t total = 3;
    char *out;
    size_t at = 0;
    int64_t index;
    for (index = 0; index < list->len; index++) {
        total += faber_quote(NULL, list->items[index]) + 4;
    }
    out = faber_alloc(arena, total);
    out[at++] = '[';
    for (index = 0; index < list->len; index++) {
        if (index > 0) {
            out[at++] = ',';
            out[at++] = ' ';
        }
        out[at++] = '"';
        at += faber_quote(out + at, list->items[index]);
        out[at++] = '"';
    }
    out[at++] = ']';
    out[at] = '\0';
    return out;
}

typedef struct faber_list_bool {
    bool *items;
    int64_t len;
    int64_t cap;
} faber_list_bool;

static faber_list_bool *faber_list_bool_new(faber_arena *arena, int64_t capacity) {
    faber_list_bool *list = faber_alloc(arena, sizeof *list);
    list->items = capacity > 0 ? faber_alloc(arena, (size_t)capacity * sizeof *list->items) : NULL;
    list->len = 0;
    list->cap = capacity;
    return list;
}

static void faber_list_bool_push(faber_arena *arena, faber_list_bool *list, bool item) {
    if (list->len == list->cap) {
        int64_t capacity = list->cap < 4 ? 4 : list->cap * 2;
        bool *items = faber_alloc(arena, (size_t)capacity * sizeof *items);
        if (list->len > 0) {
            memcpy(items, list->items, (size_t)list->len * sizeof *items);
        }
        list->items = items;
        list->cap = capacity;
    }
    list->items[list->len++] = item;
}

static const char *faber_list_bool_show(faber_arena *arena, const faber_list_bool *list, const char *yes, const char *no) {
    size_t yes_length = strlen(yes);
    size_t no_length = strlen(no);
    size_t width = yes_length > no_length ? yes_length : no_length;
    char *out = faber_alloc(arena, 3 + (size_t)list->len * (width + 2));
    size_t at = 0;
    int64_t index;
    out[at++] = '[';
    for (index = 0; index < list->len; index++) {
        const char *word = list->items[index] ? yes : no;
        size_t length = list->items[index] ? yes_length : no_length;
        if (index > 0) {
            out[at++] = ',';
            out[at++] = ' ';
        }
        memcpy(out + at, word, length);
        at += length;
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

int main(void) {
    faber_arena faber_root = {0};
    faber_arena *const faber_a = &faber_root;
    faber_list_i64 *l_xs = faber_list_i64_of(faber_a, 3, (int64_t[]){INT64_C(3), INT64_C(1), INT64_C(4)});
    printf("%s\n", faber_list_i64_show(faber_a, l_xs));
    printf("%" PRId64 " %s\n", (l_xs)->len, (((l_xs)->len == 0) ? "true" : "false"));
    faber_list_i64_push(faber_a, l_xs, INT64_C(1));
    faber_list_i64_push(faber_a, l_xs, INT64_C(5));
    printf("%s %" PRId64 " %" PRId64 "\n", faber_list_i64_show(faber_a, l_xs), faber_list_i64_at(l_xs, INT64_C(0)), faber_list_i64_at(l_xs, INT64_C(4)));
    faber_list_i64 *l_ys = l_xs;
    faber_list_i64_push(faber_a, l_ys, INT64_C(9));
    printf("%" PRId64 " %" PRId64 "\n", (l_xs)->len, (l_ys)->len);
    faber_list_i64_set(l_xs, INT64_C(1), INT64_C(10));
    printf("%s\n", faber_list_i64_show(faber_a, l_ys));
    int64_t l_total = INT64_C(0);
    {
        faber_list_i64 *const faber_t0 = l_xs;
        int64_t faber_i0;
        for (faber_i0 = 0; faber_i0 < faber_t0->len; faber_i0++) {
            const int64_t l_x = faber_t0->items[faber_i0];
            l_total = (__extension__ ({ int faber_o = 0; int64_t faber_r = faber_add_o(l_total, l_x, &faber_o); faber_o ? faber_store_i64(faber_add(l_total, l_x), "assignment to `total`") : faber_r; }));
        }
    }
    printf("%" PRId64 "\n", l_total);
    faber_list_text *const l_names = faber_list_text_of(faber_a, 2, (faber_text[]){"a", "b"});
    printf("%s\n", faber_list_text_show(faber_a, l_names));
    faber_list_bool *l_flags = faber_list_bool_new(faber_a, 0);
    faber_list_bool_push(faber_a, l_flags, (1));
    printf("%s %" PRId64 "\n", faber_list_bool_show(faber_a, l_flags, "true", "false"), (l_flags)->len);
    faber_list_i64 *l_empty = faber_list_i64_new(faber_a, 0);
    printf("%s %s\n", faber_list_i64_show(faber_a, l_empty), (((l_empty)->len == 0) ? "true" : "false"));
    return 0;
}
