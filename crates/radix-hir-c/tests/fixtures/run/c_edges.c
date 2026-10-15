#include <inttypes.h>
#include <stdio.h>

int main(void) {
    printf("%" PRId64 "\n", INT64_MIN);
    printf("%" PRId64 "\n", INT64_C(9223372036854775807));
    printf("%s\n", "100%s and %d stay text");
    printf("%s\n", "what\?\?!");
    printf("%s\n", "caf\303\251");
    printf("%s\n", "h\303\251ab");
    const int64_t l_char = INT64_C(1);
    const int64_t l_goto = INT64_C(2);
    const int64_t l_stdout = INT64_C(3);
    const int64_t l_size_t = INT64_C(4);
    const int64_t l_INT64_MAX = INT64_C(5);
    const char *const l_printf = "shadow";
    printf("%" PRId64 " %" PRId64 " %" PRId64 " %" PRId64 " %" PRId64 " %s\n", l_char, l_goto, l_stdout, l_size_t, l_INT64_MAX, l_printf);
    int64_t l_count = INT64_C(5);
    const char *l_label = "mutable";
    printf("%" PRId64 " %s\n", l_count, l_label);
    return 0;
}
