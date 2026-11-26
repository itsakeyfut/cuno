/* An integration test: it uses the package the way main.c does, through
 * its header, and is its own test executable. */
#include <cuno/test.h>

#include "stats.h"

cuno_test(mean_rounds_down) {
    int values[] = {1, 2, 4};
    cuno_require_eq(sum(values, 3), 7);
    cuno_check_eq(mean(values, 3), 2);
}
