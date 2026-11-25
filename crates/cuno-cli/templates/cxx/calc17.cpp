#include "calc.h"

int add(int a, int b) { return a + b; }

// Tests sit beside the code they check, and are compiled only by
// `cuno test`, which defines CUNO_TEST.
#ifdef CUNO_TEST
#include <cuno/test.h>

cuno_test(add_adds) {
    cuno_check_eq(add(1, 2), 3);
    cuno_check_eq(add(-1, 1), 0);
}
#endif
