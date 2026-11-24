#include "text.hpp"

namespace {

// Not visible outside this file, and still tested below.
bool is_space(char c) {
    return c == ' ' || c == '\t' || c == '\n' || c == '\r';
}

}  // namespace

std::string trim(std::string_view text) {
    while (!text.empty() && is_space(text.front())) {
        text.remove_prefix(1);
    }
    while (!text.empty() && is_space(text.back())) {
        text.remove_suffix(1);
    }
    return std::string(text);
}

// Unit tests sit beside the code they check, and are compiled only by
// `cuno test`, which defines CUNO_TEST.
#ifdef CUNO_TEST
#include <cuno/test.hpp>

static cuno::test::suite tests = [] {
    using namespace cuno::test;

    test("is_space knows tabs and newlines", [] {
        expect(is_space('\t'));
        expect(is_space('\n'));
        expect(!is_space('x'));
    });

    test("trim of only spaces is empty", [] {
        expect(eq(trim("   "), std::string()));
    });
};
#endif
