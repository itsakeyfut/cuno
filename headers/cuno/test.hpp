// cuno's test API for C++20 and later.
// See docs/en/specs/testing.md, "Modern API (C++20 and later)".
//
//     #ifdef CUNO_TEST
//     #include <cuno/test.hpp>
//     static cuno::test::suite tests = [] {
//         using namespace cuno::test;
//         test("add works", [] {
//             expect(eq(add(1, 2), 3));     // goes on after a failure
//             require(eq(add(0, 0), 0));    // ends the test on a failure
//         });
//     };
//     #endif
//
// Without CUNO_TEST (a normal build), a suite registers nothing and its
// tests are never compiled into the program.
#ifndef CUNO_TEST_HPP
#define CUNO_TEST_HPP

#include <cuno/test.h>

#include <concepts>
#include <cstddef>
#include <source_location>
#include <type_traits>
#include <utility>

namespace cuno::test {

/// What a comparison found, for `expect` and `require` to report.
struct check {
    bool ok = true;
    const char *what = "";
    // When set, the failure is this sentence rather than two values.
    const char *note = nullptr;
    // Each side as shown: any number in full, or a string cut with `..`.
    char left[96] = {};
    char right[96] = {};
};

namespace detail {

template <class T>
concept text = requires(const T &t) {
    { t.data() } -> std::convertible_to<const char *>;
    { t.size() } -> std::convertible_to<std::size_t>;
};

// Writes the `n` characters at `s` in quotes to `out`, which holds `size`
// bytes, cut and marked with `..` where they do not fit. The two quotes,
// `..` and the terminating NUL take 5 bytes.
inline void quote(const char *s, std::size_t n, char *out, std::size_t size) {
    const std::size_t room = size - 5;
    const std::size_t shown = n < room ? n : room;
    std::size_t at = 0;
    out[at++] = '"';
    for (std::size_t i = 0; i < shown; ++i) {
        out[at++] = s[i];
    }
    if (shown < n) {
        out[at++] = '.';
        out[at++] = '.';
    }
    out[at++] = '"';
    out[at] = '\0';
}

inline std::size_t length(const char *s) {
    std::size_t n = 0;
    while (s[n] != '\0') {
        ++n;
    }
    return n;
}

template <class T>
void show(const T &value, char *out, std::size_t size) {
    using U = std::remove_cvref_t<T>;
    if constexpr (std::is_same_v<U, bool>) {
        const char *word = value ? "true" : "false";
        std::size_t i = 0;
        for (; word[i] != '\0' && i + 1 < size; ++i) {
            out[i] = word[i];
        }
        out[i] = '\0';
    } else if constexpr (std::is_same_v<U, char *> || std::is_same_v<U, const char *>) {
        if (value == nullptr) {
            cuno_test_format(cuno_test_value_of(nullptr), out, size);
        } else {
            quote(value, length(value), out, size);
        }
    } else if constexpr (std::is_array_v<U> &&
                         std::is_same_v<std::remove_cv_t<std::remove_extent_t<U>>, char>) {
        quote(value, length(value), out, size);
    } else if constexpr (text<U>) {
        quote(value.data(), value.size(), out, size);
    } else if constexpr (std::is_enum_v<U>) {
        cuno_test_format(cuno_test_value_of(static_cast<std::underlying_type_t<U>>(value)), out,
                         size);
    } else if constexpr (std::is_arithmetic_v<U> || std::is_pointer_v<U> ||
                         std::is_null_pointer_v<U>) {
        cuno_test_format(cuno_test_value_of(value), out, size);
    } else {
        quote("{?}", 3, out, size);
    }
}

// The integer types std::cmp_equal and its relatives accept: not bool, and
// not the character types.
template <class T>
constexpr bool standard_integer =
    std::is_integral_v<T> && !std::is_same_v<T, bool> && !std::is_same_v<T, char> &&
    !std::is_same_v<T, wchar_t> && !std::is_same_v<T, char8_t> &&
    !std::is_same_v<T, char16_t> && !std::is_same_v<T, char32_t>;

// Integers compare by value, so `eq(v.size(), 3)` neither warns about
// mixing signedness nor gets it wrong.
template <class A, class B>
constexpr bool integers = standard_integer<A> && standard_integer<B>;

template <class A, class B, class ByValue, class ByOperator>
check compare(const char *what, const A &a, const B &b, ByValue by_value,
              ByOperator by_operator) {
    check result;
    result.what = what;
    if constexpr (integers<std::remove_cvref_t<A>, std::remove_cvref_t<B>>) {
        result.ok = by_value(a, b);
    } else {
        result.ok = by_operator(a, b);
    }
    if (!result.ok) {
        show(a, result.left, sizeof result.left);
        show(b, result.right, sizeof result.right);
    }
    return result;
}

inline void report(const check &c, const std::source_location &at, int fatal) {
    if (c.ok) {
        return;
    }
    if (c.note != nullptr) {
        // `what` is a short word (`throws`), kept under 100 so that the
        // suffix always fits; the note is cut to what is left.
        char message[256];
        std::size_t n = 0;
        for (const char *p = c.what; *p != '\0' && n < 100; ++p) {
            message[n++] = *p;
        }
        for (const char *p = " failed: "; *p != '\0'; ++p) {
            message[n++] = *p;
        }
        for (const char *p = c.note; *p != '\0' && n + 1 < sizeof message; ++p) {
            message[n++] = *p;
        }
        message[n] = '\0';
        cuno_test_fail(at.file_name(), static_cast<int>(at.line()), message, fatal);
        return;
    }
    cuno_test_values_failed(at.file_name(), static_cast<int>(at.line()), c.what, c.left,
                            c.right, fatal);
}

inline void report(bool ok, const char *what, const std::source_location &at, int fatal) {
    if (!ok) {
        cuno_test_fail(at.file_name(), static_cast<int>(at.line()), what, fatal);
    }
}

} // namespace detail

/// A group of tests: the function it is made from runs at program start and
/// registers the tests it declares. Kept as a static variable.
struct suite {
    template <class F>
    suite(F declare) {
#ifdef CUNO_TEST
        declare();
#else
        (void)declare;
#endif
    }
};

/// Declares one test. The copy of `body` is never freed: the registry holds
/// it until the process, which runs one test, ends.
template <class F>
void test(const char *name, F body,
          std::source_location at = std::source_location::current()) {
    cuno_test_register(name, at.file_name(), static_cast<int>(at.line()),
                       [](void *context) { (*static_cast<F *>(context))(); }, new F(body));
}

template <class A, class B>
check eq(const A &a, const B &b) {
    return detail::compare(
        "eq", a, b, [](const auto &x, const auto &y) { return std::cmp_equal(x, y); },
        [](const auto &x, const auto &y) { return x == y; });
}

template <class A, class B>
check ne(const A &a, const B &b) {
    return detail::compare(
        "ne", a, b, [](const auto &x, const auto &y) { return std::cmp_not_equal(x, y); },
        [](const auto &x, const auto &y) { return x != y; });
}

template <class A, class B>
check lt(const A &a, const B &b) {
    return detail::compare(
        "lt", a, b, [](const auto &x, const auto &y) { return std::cmp_less(x, y); },
        [](const auto &x, const auto &y) { return x < y; });
}

template <class A, class B>
check le(const A &a, const B &b) {
    return detail::compare(
        "le", a, b, [](const auto &x, const auto &y) { return std::cmp_less_equal(x, y); },
        [](const auto &x, const auto &y) { return x <= y; });
}

template <class A, class B>
check gt(const A &a, const B &b) {
    return detail::compare(
        "gt", a, b, [](const auto &x, const auto &y) { return std::cmp_greater(x, y); },
        [](const auto &x, const auto &y) { return x > y; });
}

template <class A, class B>
check ge(const A &a, const B &b) {
    return detail::compare(
        "ge", a, b, [](const auto &x, const auto &y) { return std::cmp_greater_equal(x, y); },
        [](const auto &x, const auto &y) { return x >= y; });
}

/// Whether `a` and `b` differ by at most `epsilon`.
inline check approx(long double a, long double b, long double epsilon) {
    check result;
    result.what = "approx";
    long double difference = a > b ? a - b : b - a;
    result.ok = difference <= epsilon;
    if (!result.ok) {
        detail::show(a, result.left, sizeof result.left);
        detail::show(b, result.right, sizeof result.right);
    }
    return result;
}

#if defined(__cpp_exceptions) || defined(_CPPUNWIND)
/// Whether calling `f` throws an `E`.
template <class E, class F>
check throws(F f) {
    check result;
    result.what = "throws";
    try {
        f();
        result.ok = false;
        result.note = "nothing was thrown";
    } catch (const E &) {
    } catch (...) {
        result.ok = false;
        result.note = "an exception of another type was thrown";
    }
    return result;
}

/// Whether calling `f` throws nothing.
template <class F>
check nothrow(F f) {
    check result;
    result.what = "nothrow";
    try {
        f();
    } catch (...) {
        result.ok = false;
        result.note = "an exception was thrown";
    }
    return result;
}
#endif

/// Checks `c`; the test goes on when it fails.
inline void expect(const check &c, std::source_location at = std::source_location::current()) {
    detail::report(c, at, 0);
}

/// Checks `ok`; the test goes on when it is false.
inline void expect(bool ok, std::source_location at = std::source_location::current()) {
    detail::report(ok, "expect failed", at, 0);
}

/// Checks `c`; the test ends when it fails.
inline void require(const check &c, std::source_location at = std::source_location::current()) {
    detail::report(c, at, 1);
}

/// Checks `ok`; the test ends when it is false.
inline void require(bool ok, std::source_location at = std::source_location::current()) {
    detail::report(ok, "require failed", at, 1);
}

/// Shows `values` alongside the test's failures, if it has any.
template <class... T>
void info(const T &...values) {
    char text[1024];
    std::size_t n = 0;
    auto append = [&](const auto &value) {
        char part[256];
        using U = std::remove_cvref_t<decltype(value)>;
        if constexpr (std::is_same_v<U, char *> || std::is_same_v<U, const char *> ||
                      std::is_array_v<U>) {
            for (const char *p = value; *p != '\0' && n + 1 < sizeof text; ++p) {
                text[n++] = *p;
            }
            return;
        } else if constexpr (detail::text<U>) {
            for (std::size_t i = 0; i < value.size() && n + 1 < sizeof text; ++i) {
                text[n++] = value.data()[i];
            }
            return;
        } else {
            detail::show(value, part, sizeof part);
            for (const char *p = part; *p != '\0' && n + 1 < sizeof text; ++p) {
                text[n++] = *p;
            }
        }
    };
    (append(values), ...);
    text[n] = '\0';
    cuno_test_info(text);
}

/// Ends the test as skipped, for `reason`.
inline void skip(const char *reason) { cuno_test_skip(reason); }

} // namespace cuno::test

#endif
