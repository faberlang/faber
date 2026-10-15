#include <inttypes.h>
#include <stdio.h>

int main(void) {
    printf("%" PRId64 "\n", INT64_MIN);
    printf("%" PRId64 "\n", INT64_C(9223372036854775807));
    printf("%s\n", "100%s and %d stay text");
    printf("%s\n", "what\?\?!");
    printf("%s\n", "caf\303\251");
    printf("%s\n", "h\303\251ab");
    const int64_t stdout_1 = INT64_C(1);
    const int64_t size_t_v = INT64_C(2);
    const int64_t v_INT64_MAX = INT64_C(3);
    const char *const printf_1 = "shadow";
    printf("%" PRId64 " %" PRId64 " %" PRId64 " %s\n", stdout_1, size_t_v, v_INT64_MAX, printf_1);
    int64_t count = INT64_C(5);
    const char *label = "mutable";
    printf("%" PRId64 " %s\n", count, label);
    return 0;
}
