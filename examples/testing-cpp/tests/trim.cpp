// An integration test: it uses the package the way main.cpp does, through
// its header, and is its own test executable.
#include <cuno/test.hpp>

#include "text.hpp"

static cuno::test::suite tests = [] {
    using namespace cuno::test;

    test("trim keeps the spaces inside", [] {
        auto trimmed = trim("  one comfortable tool  ");
        require(eq(trimmed.size(), std::size_t{20}));
        expect(eq(trimmed, std::string("one comfortable tool")));
    });
};
