#include "calc.h"

int add(int a, int b) { return a + b; }

// Tests sit beside the code they check, and are compiled only by
// `cuno test`, which defines CUNO_TEST.
#ifdef CUNO_TEST
#include <cuno/test.hpp>

static cuno::test::suite tests = [] {
    using namespace cuno::test;

    test("add adds", [] {
        expect(eq(add(1, 2), 3));
        expect(eq(add(-1, 1), 0));
    });
};
#endif
