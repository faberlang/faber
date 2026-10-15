#include <inttypes.h>
#include <stdio.h>

int main(void) {
    printf("%" PRId64 "\n", INT64_C(3));
    printf("%s\n", "hi");
    printf("%" PRId64 "\n", INT64_C(10));
    const int64_t l_neg = -INT64_C(7);
    printf("%" PRId64 "\n", -INT64_C(4));
    printf("%" PRId64 "\n", INT64_C(1));
    printf("%s\n", "say \"hi\"\tand\\done");
    const int64_t l_answer = INT64_C(42);
    const char *const l_greeting = "h\303\251llo";
    printf("%" PRId64 " %s\n", l_answer, l_greeting);
    return 0;
}
