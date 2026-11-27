/*
 * cuno's test macros, for C11 and later and for C++17 and later.
 * See docs/en/specs/testing.md, "Macro API (C / C++17)".
 *
 *     #include <cuno/test.h>
 *
 *     cuno_test(add_works) {
 *         cuno_check_eq(add(1, 2), 3);     // goes on after a failure
 *         cuno_require_eq(add(0, 0), 0);   // ends the test on a failure
 *     }
 *
 * Without CUNO_TEST (a normal build), a test compiles to an unused inline
 * function and registers nothing, so the file builds as it would without
 * its tests.
 *
 * Names starting with cuno_test_ or CUNO_TEST_ are cuno's own; only the
 * cuno_ names documented in the spec are for use.
 */
#ifndef CUNO_TEST_H
#define CUNO_TEST_H

#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

/* The runtime's interface (test_main.c), shared with cuno/test.hpp. */

enum { CUNO_TEST_SIGNED, CUNO_TEST_UNSIGNED, CUNO_TEST_FLOAT, CUNO_TEST_POINTER };
enum { CUNO_TEST_EQ, CUNO_TEST_NE, CUNO_TEST_LT, CUNO_TEST_LE, CUNO_TEST_GT, CUNO_TEST_GE };

typedef struct cuno_test_value {
    int kind;
    union {
        long long i;
        unsigned long long u;
        long double f;
        const volatile void *p;
    } as;
} cuno_test_value;

/* Starts a cuno::test::suite: the tests registered until the next are
 * its own (testing.md, "Common rules"). */
void cuno_test_suite_begin(void);
void cuno_test_register(const char *name, const char *file, int line, void (*run)(void *),
                        void *context);
void cuno_test_register_macro(const char *name, const char *file, int line,
                              void (*run)(void *));
void cuno_test_fail(const char *file, int line, const char *message, int fatal);
void cuno_test_values_failed(const char *file, int line, const char *what, const char *left,
                             const char *right, int fatal);
void cuno_test_info(const char *text);
void cuno_test_infof(const char *format, ...);
void cuno_test_skip(const char *reason);
void cuno_test_format(cuno_test_value value, char *buffer, size_t size);
void cuno_test_compare(int op, cuno_test_value a, cuno_test_value b, const char *expression,
                       const char *file, int line, int fatal);
void cuno_test_approx(long double a, long double b, long double epsilon, const char *expression,
                      const char *file, int line, int fatal);
void cuno_test_truth(int value, int expected, const char *expression, const char *file,
                     int line, int fatal);

#ifdef __cplusplus
}
#endif

/* A value of any arithmetic or pointer type, for comparing and showing. */
#ifdef __cplusplus
#define CUNO_TEST_FROM(type, field, kind_)                                                     \
    inline cuno_test_value cuno_test_value_of(type v) {                                        \
        cuno_test_value r;                                                                     \
        r.kind = kind_;                                                                        \
        r.as.field = static_cast<decltype(r.as.field)>(v);                                     \
        return r;                                                                              \
    }
CUNO_TEST_FROM(bool, u, CUNO_TEST_UNSIGNED)
CUNO_TEST_FROM(char, i, CUNO_TEST_SIGNED)
CUNO_TEST_FROM(signed char, i, CUNO_TEST_SIGNED)
CUNO_TEST_FROM(unsigned char, u, CUNO_TEST_UNSIGNED)
CUNO_TEST_FROM(short, i, CUNO_TEST_SIGNED)
CUNO_TEST_FROM(unsigned short, u, CUNO_TEST_UNSIGNED)
CUNO_TEST_FROM(int, i, CUNO_TEST_SIGNED)
CUNO_TEST_FROM(unsigned int, u, CUNO_TEST_UNSIGNED)
CUNO_TEST_FROM(long, i, CUNO_TEST_SIGNED)
CUNO_TEST_FROM(unsigned long, u, CUNO_TEST_UNSIGNED)
CUNO_TEST_FROM(long long, i, CUNO_TEST_SIGNED)
CUNO_TEST_FROM(unsigned long long, u, CUNO_TEST_UNSIGNED)
CUNO_TEST_FROM(float, f, CUNO_TEST_FLOAT)
CUNO_TEST_FROM(double, f, CUNO_TEST_FLOAT)
CUNO_TEST_FROM(long double, f, CUNO_TEST_FLOAT)
CUNO_TEST_FROM(const volatile void *, p, CUNO_TEST_POINTER)
inline cuno_test_value cuno_test_value_of(decltype(nullptr)) {
    return cuno_test_value_of(static_cast<const volatile void *>(nullptr));
}
#undef CUNO_TEST_FROM
#define CUNO_TEST_VALUE(x) cuno_test_value_of(x)
#else
#define CUNO_TEST_FROM(name, type, field, kind_)                                               \
    static inline cuno_test_value cuno_test_##name(type v) {                                   \
        cuno_test_value r;                                                                     \
        r.kind = kind_;                                                                        \
        r.as.field = v;                                                                        \
        return r;                                                                              \
    }
CUNO_TEST_FROM(signed, long long, i, CUNO_TEST_SIGNED)
CUNO_TEST_FROM(unsigned, unsigned long long, u, CUNO_TEST_UNSIGNED)
CUNO_TEST_FROM(float, long double, f, CUNO_TEST_FLOAT)
CUNO_TEST_FROM(pointer, const volatile void *, p, CUNO_TEST_POINTER)
#undef CUNO_TEST_FROM
/* Chosen by the type of `1 ? (x) : (x)` rather than of x: the conditional
 * promotes an enum to its integer type, which MSVC's _Generic otherwise
 * matches with no integer type at all. It is not evaluated. Its two
 * branches are the same on purpose, so clang-tidy's bugprone-branch-clone
 * is told so where it would otherwise flag every caller. */
#define CUNO_TEST_VALUE(x)                                                                     \
    _Generic(1 ? (x) : (x), /* NOLINT(bugprone-branch-clone) */                                \
        _Bool: cuno_test_unsigned,                                                             \
        char: cuno_test_signed,                                                                \
        signed char: cuno_test_signed,                                                         \
        unsigned char: cuno_test_unsigned,                                                     \
        short: cuno_test_signed,                                                               \
        unsigned short: cuno_test_unsigned,                                                    \
        int: cuno_test_signed,                                                                 \
        unsigned int: cuno_test_unsigned,                                                      \
        long: cuno_test_signed,                                                                \
        unsigned long: cuno_test_unsigned,                                                     \
        long long: cuno_test_signed,                                                           \
        unsigned long long: cuno_test_unsigned,                                                \
        float: cuno_test_float,                                                                \
        double: cuno_test_float,                                                               \
        long double: cuno_test_float,                                                          \
        default: cuno_test_pointer)(x)
#endif

/* Registration: a function the C runtime calls before main. In C++ a static
 * object's initialiser does it. In C, Clang (clang-cl included) and GCC take
 * the constructor attribute; the MSVC compiler has none, so it is given the
 * function's pointer in the CRT's initialiser section, .CRT$XCU. That
 * pointer has an external name the linker is told to keep (/include):
 * nothing refers to it, so with /GL and /LTCG the linker would otherwise
 * drop it, and the test with it (testing.md, "Common rules"). */
#if defined(__cplusplus)
#define CUNO_TEST_REGISTER(name)                                                               \
    static const int cuno_test_registered_##name =                                             \
        (cuno_test_register_macro(#name, __FILE__, __LINE__, cuno_test_run_##name), 0);
#elif defined(_MSC_VER) && !defined(__clang__)
#pragma section(".CRT$XCU", read)
#if defined(_M_IX86)
#define CUNO_TEST_SYMBOL_PREFIX "_"
#else
#define CUNO_TEST_SYMBOL_PREFIX ""
#endif
#define CUNO_TEST_REGISTER(name)                                                               \
    static void cuno_test_init_##name(void) {                                                  \
        cuno_test_register_macro(#name, __FILE__, __LINE__, cuno_test_run_##name);             \
    }                                                                                          \
    __pragma(comment(linker, "/include:" CUNO_TEST_SYMBOL_PREFIX "cuno_test_initialiser_" #name)) \
    __declspec(allocate(".CRT$XCU")) void (*cuno_test_initialiser_##name)(void) =             \
        cuno_test_init_##name;
#else
#define CUNO_TEST_REGISTER(name)                                                               \
    __attribute__((constructor)) static void cuno_test_init_##name(void) {                     \
        cuno_test_register_macro(#name, __FILE__, __LINE__, cuno_test_run_##name);             \
    }
#endif

#ifdef CUNO_TEST
#define cuno_test(name)                                                                        \
    static void cuno_test_body_##name(void);                                                   \
    static void cuno_test_run_##name(void *context) {                                          \
        (void)context;                                                                         \
        cuno_test_body_##name();                                                               \
    }                                                                                          \
    CUNO_TEST_REGISTER(name)                                                                   \
    static void cuno_test_body_##name(void)
#else
/* Marked unused for Clang, which warns about an unused static function in
 * the file being compiled, inline or not. */
#if defined(__GNUC__) || defined(__clang__)
#define CUNO_TEST_UNUSED __attribute__((unused))
#else
#define CUNO_TEST_UNUSED
#endif
#define cuno_test(name) CUNO_TEST_UNUSED static inline void cuno_test_body_##name(void)
#endif

#define CUNO_TEST_TRUTH(e, expected, fatal)                                                    \
    cuno_test_truth((e) ? 1 : 0, expected, #e, __FILE__, __LINE__, fatal)
#define CUNO_TEST_COMPARE(op, a, b, fatal)                                                     \
    cuno_test_compare(op, CUNO_TEST_VALUE(a), CUNO_TEST_VALUE(b), #a ", " #b, __FILE__,       \
                      __LINE__, fatal)
#define CUNO_TEST_APPROX(a, b, epsilon, fatal)                                                 \
    cuno_test_approx((long double)(a), (long double)(b), (long double)(epsilon),               \
                     #a ", " #b ", " #epsilon, __FILE__, __LINE__, fatal)

#define cuno_check(e) CUNO_TEST_TRUTH(e, 1, 0)
#define cuno_require(e) CUNO_TEST_TRUTH(e, 1, 1)
#define cuno_check_false(e) CUNO_TEST_TRUTH(e, 0, 0)
#define cuno_require_false(e) CUNO_TEST_TRUTH(e, 0, 1)
#define cuno_check_eq(a, b) CUNO_TEST_COMPARE(CUNO_TEST_EQ, a, b, 0)
#define cuno_check_ne(a, b) CUNO_TEST_COMPARE(CUNO_TEST_NE, a, b, 0)
#define cuno_check_lt(a, b) CUNO_TEST_COMPARE(CUNO_TEST_LT, a, b, 0)
#define cuno_check_le(a, b) CUNO_TEST_COMPARE(CUNO_TEST_LE, a, b, 0)
#define cuno_check_gt(a, b) CUNO_TEST_COMPARE(CUNO_TEST_GT, a, b, 0)
#define cuno_check_ge(a, b) CUNO_TEST_COMPARE(CUNO_TEST_GE, a, b, 0)
#define cuno_require_eq(a, b) CUNO_TEST_COMPARE(CUNO_TEST_EQ, a, b, 1)
#define cuno_require_ne(a, b) CUNO_TEST_COMPARE(CUNO_TEST_NE, a, b, 1)
#define cuno_require_lt(a, b) CUNO_TEST_COMPARE(CUNO_TEST_LT, a, b, 1)
#define cuno_require_le(a, b) CUNO_TEST_COMPARE(CUNO_TEST_LE, a, b, 1)
#define cuno_require_gt(a, b) CUNO_TEST_COMPARE(CUNO_TEST_GT, a, b, 1)
#define cuno_require_ge(a, b) CUNO_TEST_COMPARE(CUNO_TEST_GE, a, b, 1)
#define cuno_check_approx(a, b, epsilon) CUNO_TEST_APPROX(a, b, epsilon, 0)
#define cuno_require_approx(a, b, epsilon) CUNO_TEST_APPROX(a, b, epsilon, 1)
#define cuno_info(...) cuno_test_infof(__VA_ARGS__)
#define cuno_skip(reason) cuno_test_skip(reason)

#endif
