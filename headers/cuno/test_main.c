/*
 * The runtime of cuno's tests: the registry both entries (cuno/test.h and
 * cuno/test.hpp) register into, and the `main` of every test executable.
 * cuno compiles this file into each test build; it is not included.
 * See docs/en/specs/testing.md.
 *
 * Command line (the contract with cuno):
 *
 *   --list          write a `test` record for every registration, and exit 0
 *   --exact <name>  run that test and exit 0 when it passed or was skipped,
 *                   1 when it failed
 *
 * Both write to the file named by the environment variable
 * CUNO_TEST_RESULT (to stderr when it is not set), never to stdout, which is
 * the user's. On Windows the name and the path are read as UTF-16, so they
 * need not fit the code page. One record per line, ending in LF, fields
 * separated by a tab, with backslash, tab, CR and LF in a field written as
 * \\, \t, \r and \n:
 *
 *   test <name> <file> <line> <api>  a registration (--list); <api> is
 *                                  `modern` (cuno/test.hpp) or `macro`
 *                                  (cuno/test.h). One test compiled into
 *                                  several translation units has a record
 *                                  for each; cuno decides which are the same
 *                                  place, by the file system rather than by
 *                                  how <file> is spelled.
 *   fail <file> <line> <message>   a check failed (the test goes on unless
 *                                  it was a require)
 *   info <text>                    supplementary information
 *   skip <reason>                  the test was skipped; nothing follows
 *   done                           the test ended; nothing follows
 *
 * A test passed when the file ends with `done` and holds no `fail`; it was
 * skipped when it ends with `skip` and holds no `fail`. A file that ends any
 * other way means the test did not finish (a crash, a call to exit), which
 * cuno reports as a failure. Each record is flushed as it is written, so
 * what came before a crash is kept.
 *
 * A failed require, and a skip, end the process with exit(): every test
 * runs in a process of its own, so this needs neither exceptions nor
 * longjmp and works in builds without exceptions (testing.md, "Common
 * rules").
 */

/* MSVC's CRT deprecates getenv and fopen in favour of its _s functions,
 * which other C libraries lack; /W4 /WX would reject the file. */
#if defined(_MSC_VER) && !defined(_CRT_SECURE_NO_WARNINGS)
#define _CRT_SECURE_NO_WARNINGS
#endif

#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#endif

#include "test.h"

typedef struct cuno_test_entry {
    const char *name;
    const char *file;
    int line;
    void (*run)(void *);
    void *context;
    /* Registered by cuno_test (test.h) rather than by test.hpp. */
    int from_macro;
    struct cuno_test_entry *next;
} cuno_test_entry;

/* Zero-initialised, so registering works from any static initialiser,
 * whatever order translation units are initialised in. */
static cuno_test_entry *cuno_test_head;
static cuno_test_entry *cuno_test_tail;
static FILE *cuno_test_out;
static int cuno_test_failed;

static void cuno_test_add(const char *name, const char *file, int line, void (*run)(void *),
                          void *context, int from_macro) {
    cuno_test_entry *entry = (cuno_test_entry *)calloc(1, sizeof *entry);
    if (entry == NULL) {
        fputs("error: out of memory registering tests\n", stderr);
        exit(1);
    }
    entry->name = name;
    entry->file = file;
    entry->line = line;
    entry->run = run;
    entry->context = context;
    entry->from_macro = from_macro;
    if (cuno_test_tail == NULL) {
        cuno_test_head = entry;
    } else {
        cuno_test_tail->next = entry;
    }
    cuno_test_tail = entry;
}

void cuno_test_register(const char *name, const char *file, int line, void (*run)(void *),
                        void *context) {
    cuno_test_add(name, file, line, run, context, 0);
}

void cuno_test_register_macro(const char *name, const char *file, int line,
                              void (*run)(void *)) {
    cuno_test_add(name, file, line, run, NULL, 1);
}

static void cuno_test_field(const char *text) {
    fputc('\t', cuno_test_out);
    for (; *text != '\0'; text++) {
        switch (*text) {
        case '\\': fputs("\\\\", cuno_test_out); break;
        case '\t': fputs("\\t", cuno_test_out); break;
        case '\r': fputs("\\r", cuno_test_out); break;
        case '\n': fputs("\\n", cuno_test_out); break;
        default: fputc(*text, cuno_test_out); break;
        }
    }
}

static void cuno_test_number(int value) {
    char number[16];
    snprintf(number, sizeof number, "%d", value);
    cuno_test_field(number);
}

static void cuno_test_end_record(void) {
    fputc('\n', cuno_test_out);
    fflush(cuno_test_out);
}

void cuno_test_fail(const char *file, int line, const char *message, int fatal) {
    cuno_test_failed = 1;
    fputs("fail", cuno_test_out);
    cuno_test_field(file);
    cuno_test_number(line);
    cuno_test_field(message);
    cuno_test_end_record();
    if (fatal) {
        fputs("done", cuno_test_out);
        cuno_test_end_record();
        exit(1);
    }
}

void cuno_test_info(const char *text) {
    fputs("info", cuno_test_out);
    cuno_test_field(text);
    cuno_test_end_record();
}

/* A skip after a failed check still fails the test (testing.md, "Contract
 * between cuno and test executables"). */
void cuno_test_skip(const char *reason) {
    fputs("skip", cuno_test_out);
    cuno_test_field(reason);
    cuno_test_end_record();
    exit(cuno_test_failed ? 1 : 0);
}

void cuno_test_infof(const char *format, ...) {
    char text[1024];
    va_list args;
    va_start(args, format);
    vsnprintf(text, sizeof text, format, args);
    va_end(args);
    cuno_test_info(text);
}

void cuno_test_format(cuno_test_value value, char *buffer, size_t size) {
    switch (value.kind) {
    case CUNO_TEST_SIGNED: snprintf(buffer, size, "%lld", value.as.i); break;
    case CUNO_TEST_UNSIGNED: snprintf(buffer, size, "%llu", value.as.u); break;
    case CUNO_TEST_FLOAT: snprintf(buffer, size, "%.17Lg", value.as.f); break;
    case CUNO_TEST_POINTER:
        if (value.as.p == NULL) {
            snprintf(buffer, size, "NULL");
        } else {
            snprintf(buffer, size, "%p", value.as.p);
        }
        break;
    default: snprintf(buffer, size, "?"); break;
    }
}

/* How a compares with b. */
enum { CUNO_TEST_BELOW, CUNO_TEST_SAME, CUNO_TEST_ABOVE, CUNO_TEST_UNORDERED };

/* Compares the values themselves: a negative signed value is below every
 * unsigned one, which C's own conversions would get wrong, and a NaN is
 * unordered with everything, so that only `ne` holds for it. */
static int cuno_test_order(cuno_test_value a, cuno_test_value b) {
    if (a.kind == CUNO_TEST_FLOAT || b.kind == CUNO_TEST_FLOAT) {
        long double x = a.kind == CUNO_TEST_FLOAT ? a.as.f
                        : a.kind == CUNO_TEST_SIGNED ? (long double)a.as.i
                                                     : (long double)a.as.u;
        long double y = b.kind == CUNO_TEST_FLOAT ? b.as.f
                        : b.kind == CUNO_TEST_SIGNED ? (long double)b.as.i
                                                     : (long double)b.as.u;
        return x < y    ? CUNO_TEST_BELOW
               : x > y  ? CUNO_TEST_ABOVE
               : x == y ? CUNO_TEST_SAME
                        : CUNO_TEST_UNORDERED;
    }
    if (a.kind == CUNO_TEST_POINTER || b.kind == CUNO_TEST_POINTER) {
        /* A pointer against an integer is a pointer against NULL written as
         * 0, which C++ allows. */
        const char *x = a.kind == CUNO_TEST_POINTER ? (const char *)a.as.p
                                                    : (const char *)(size_t)a.as.u;
        const char *y = b.kind == CUNO_TEST_POINTER ? (const char *)b.as.p
                                                    : (const char *)(size_t)b.as.u;
        return x < y ? CUNO_TEST_BELOW : x > y ? CUNO_TEST_ABOVE : CUNO_TEST_SAME;
    }
    if (a.kind == CUNO_TEST_SIGNED && b.kind == CUNO_TEST_SIGNED) {
        return a.as.i < b.as.i   ? CUNO_TEST_BELOW
               : a.as.i > b.as.i ? CUNO_TEST_ABOVE
                                 : CUNO_TEST_SAME;
    }
    if (a.kind == CUNO_TEST_SIGNED && a.as.i < 0) {
        return CUNO_TEST_BELOW;
    }
    if (b.kind == CUNO_TEST_SIGNED && b.as.i < 0) {
        return CUNO_TEST_ABOVE;
    }
    {
        unsigned long long x = a.kind == CUNO_TEST_SIGNED ? (unsigned long long)a.as.i : a.as.u;
        unsigned long long y = b.kind == CUNO_TEST_SIGNED ? (unsigned long long)b.as.i : b.as.u;
        return x < y ? CUNO_TEST_BELOW : x > y ? CUNO_TEST_ABOVE : CUNO_TEST_SAME;
    }
}

static const char *const cuno_test_op_names[] = {"eq", "ne", "lt", "le", "gt", "ge"};

static int cuno_test_holds(int op, int order) {
    switch (op) {
    case CUNO_TEST_EQ: return order == CUNO_TEST_SAME;
    case CUNO_TEST_NE: return order != CUNO_TEST_SAME;
    case CUNO_TEST_LT: return order == CUNO_TEST_BELOW;
    case CUNO_TEST_LE: return order == CUNO_TEST_BELOW || order == CUNO_TEST_SAME;
    case CUNO_TEST_GT: return order == CUNO_TEST_ABOVE;
    default: return order == CUNO_TEST_ABOVE || order == CUNO_TEST_SAME;
    }
}

void cuno_test_values_failed(const char *file, int line, const char *what, const char *left,
                             const char *right, int fatal) {
    char message[1024];
    snprintf(message, sizeof message, "%s failed\n  left:  %s\n  right: %s", what, left,
             right);
    cuno_test_fail(file, line, message, fatal);
}

void cuno_test_compare(int op, cuno_test_value a, cuno_test_value b, const char *expression,
                       const char *file, int line, int fatal) {
    /* Room for any formatted number or pointer. */
    char left[64];
    char right[64];
    char what[512];
    if (cuno_test_holds(op, cuno_test_order(a, b))) {
        return;
    }
    cuno_test_format(a, left, sizeof left);
    cuno_test_format(b, right, sizeof right);
    snprintf(what, sizeof what, "%s(%s)", cuno_test_op_names[op], expression);
    cuno_test_values_failed(file, line, what, left, right, fatal);
}

void cuno_test_approx(long double a, long double b, long double epsilon,
                      const char *expression, const char *file, int line, int fatal) {
    char left[64];
    char right[64];
    char what[512];
    long double difference = a > b ? a - b : b - a;
    if (difference <= epsilon) {
        return;
    }
    snprintf(left, sizeof left, "%.17Lg", a);
    snprintf(right, sizeof right, "%.17Lg", b);
    snprintf(what, sizeof what, "approx(%s)", expression);
    cuno_test_values_failed(file, line, what, left, right, fatal);
}

void cuno_test_truth(int value, int expected, const char *expression, const char *file,
                     int line, int fatal) {
    char message[512];
    if ((value != 0) == (expected != 0)) {
        return;
    }
    snprintf(message, sizeof message, "%s(%s) failed",
             fatal ? (expected ? "require" : "require_false")
                   : (expected ? "check" : "check_false"),
             expression);
    cuno_test_fail(file, line, message, fatal);
}

/* A name has to be one line to be chosen with --exact. */
static int cuno_test_check_names(void) {
    const cuno_test_entry *entry;
    int ok = 1;
    for (entry = cuno_test_head; entry != NULL; entry = entry->next) {
        if (strpbrk(entry->name, "\r\n") != NULL || entry->name[0] == '\0') {
            fprintf(stderr, "error: the test at %s:%d has a name that is empty or spans lines\n",
                    entry->file, entry->line);
            ok = 0;
        }
    }
    return ok;
}

static int cuno_test_open(void) {
#ifdef _WIN32
    const wchar_t *path = _wgetenv(L"CUNO_TEST_RESULT");
    cuno_test_out = path != NULL && path[0] != L'\0' ? _wfopen(path, L"wb") : stderr;
#else
    const char *path = getenv("CUNO_TEST_RESULT");
    cuno_test_out = path != NULL && path[0] != '\0' ? fopen(path, "wb") : stderr;
#endif
    if (cuno_test_out == NULL) {
        fputs("error: cannot write the result file named by CUNO_TEST_RESULT\n", stderr);
        return 0;
    }
    return 1;
}

static int cuno_test_main(int argc, char **argv) {
    const cuno_test_entry *entry;
    if (!cuno_test_check_names()) {
        return 1;
    }
    if (argc == 2 && strcmp(argv[1], "--list") == 0) {
        if (!cuno_test_open()) {
            return 2;
        }
        for (entry = cuno_test_head; entry != NULL; entry = entry->next) {
            fputs("test", cuno_test_out);
            cuno_test_field(entry->name);
            cuno_test_field(entry->file);
            cuno_test_number(entry->line);
            cuno_test_field(entry->from_macro ? "macro" : "modern");
            cuno_test_end_record();
        }
        return 0;
    }
    if (argc != 3 || strcmp(argv[1], "--exact") != 0) {
        fputs("usage: --list | --exact <name>\n"
              "This is a test executable built by `cuno test`, which runs it.\n",
              stderr);
        return 2;
    }
    /* The first registration: cuno has already refused a name registered
     * at two different places. */
    for (entry = cuno_test_head; entry != NULL; entry = entry->next) {
        if (strcmp(entry->name, argv[2]) == 0) {
            break;
        }
    }
    if (entry == NULL) {
        fprintf(stderr, "error: no test named `%s`\n", argv[2]);
        return 2;
    }
    if (!cuno_test_open()) {
        return 2;
    }
    entry->run(entry->context);
    fputs("done", cuno_test_out);
    cuno_test_end_record();
    return cuno_test_failed ? 1 : 0;
}

#ifdef _WIN32
/* The arguments as UTF-16, converted to UTF-8, the encoding test names are
 * compiled in: char arguments are in the code page, which may not hold
 * them. */
int wmain(int argc, wchar_t **wargv) {
    char **argv = (char **)calloc((size_t)argc + 1, sizeof *argv);
    int i;
    if (argv == NULL) {
        fputs("error: out of memory reading the arguments\n", stderr);
        return 2;
    }
    for (i = 0; i < argc; i++) {
        int size = WideCharToMultiByte(CP_UTF8, 0, wargv[i], -1, NULL, 0, NULL, NULL);
        argv[i] = (char *)malloc(size > 0 ? (size_t)size : 1);
        if (argv[i] == NULL) {
            fputs("error: out of memory reading the arguments\n", stderr);
            return 2;
        }
        argv[i][0] = '\0';
        if (size > 0) {
            WideCharToMultiByte(CP_UTF8, 0, wargv[i], -1, argv[i], size, NULL, NULL);
        }
    }
    return cuno_test_main(argc, argv);
}
#else
int main(int argc, char **argv) { return cuno_test_main(argc, argv); }
#endif
