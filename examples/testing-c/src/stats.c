#include "stats.h"

/* Not visible outside this file, and still tested below. */
static int at_least_zero(int count) {
    return count < 0 ? 0 : count;
}

int sum(const int* values, int count) {
    int total = 0;
    for (int i = 0; i < at_least_zero(count); ++i) {
        total += values[i];
    }
    return total;
}

int mean(const int* values, int count) {
    count = at_least_zero(count);
    return count == 0 ? 0 : sum(values, count) / count;
}

/* Unit tests sit beside the code they check, and are compiled only by
 * `cuno test`, which defines CUNO_TEST. */
#ifdef CUNO_TEST
#include <cuno/test.h>

cuno_test(at_least_zero_turns_a_negative_count_into_zero) {
    cuno_check_eq(at_least_zero(-3), 0);
    cuno_check_eq(at_least_zero(4), 4);
}

cuno_test(mean_of_no_values_is_zero) {
    int values[] = {5};
    cuno_check_eq(mean(values, 0), 0);
    cuno_check_eq(mean(values, -1), 0);
}
#endif
