#include "checksum.h"

unsigned checksum(const char* text) {
    unsigned sum = 0;
    for (; *text; ++text) {
        sum += (unsigned char)*text;
    }
    return sum;
}

/* A C file in a C++ package writes its tests with the C header,
 * cuno/test.h; they run in the same test executable as the C++ ones. */
#ifdef CUNO_TEST
#include <cuno/test.h>

cuno_test(checksum_of_nothing_is_zero) { cuno_check_eq(checksum(""), 0u); }
#endif
