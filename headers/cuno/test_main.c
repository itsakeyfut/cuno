/*
 * The runtime of cuno's tests: the registry both entries (cuno/test.h and
 * cuno/test.hpp) register into, and the `main` of every test executable.
 * cuno compiles this file into each test build; it is not included.
 * See docs/en/specs/testing.md.
 *
 * Command line (the contract with cuno):
 *
 *   --list          print every test's name, one per line, and exit 0
 *   --exact <name>  run that test and exit 0 when it passed or was skipped,
 *                   1 when it failed
 *
 * The result file. A test's outcome is written to the file named by the
 * environment variable CUNO_TEST_RESULT (to stderr when it is not set), one
 * event per line, fields separated by a tab, with backslash, tab, CR and LF
 * in a field written as \\, \t, \r and \n:
 *
 *   fail <file> <line> <message>   a check failed (the test goes on unless
 *                                  it was a require)
 *   info <text>                    supplementary information
 *   skip <reason>                  the test was skipped; nothing follows
 *   done                           the test ended; nothing follows
 *
 * A test passed when the file ends with `done` and holds no `fail`. A file
 * that ends any other way means the test did not finish (a crash, a call to
 * exit), which cuno reports as a failure. Each event is flushed as it is
 * written, so what came before a crash is kept.
 */

#if defined(_MSC_VER) && !defined(_CRT_SECURE_NO_WARNINGS)
#define _CRT_SECURE_NO_WARNINGS
#endif

#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "test.h"

typedef struct cuno_test_entry {
    const char *name;
    const char *file;
    int line;
    void (*run)(void *);
    void *context;
    int macro;
    struct cuno_test_entry *next;
} cuno_test_entry;

/* Zero-initialised, so registering works from any static initialiser,
 * whatever order translation units are initialised in. */
static cuno_test_entry *cuno_test_head;
static cuno_test_entry *cuno_test_tail;
static FILE *cuno_test_out;
static int cuno_test_failed;

static void cuno_test_add(const char *name, const char *file, int line,
                          void (*run)(void *), void *context, int macro) {
    cuno_test_entry *entry = (cuno_test_entry *)calloc(1, sizeof *entry);
    if (entry == NULL) {
        fputs("cuno: out of memory registering tests\n", stderr);
        exit(1);
    }
    entry->name = name;
    entry->file = file;
    entry->line = line;
    entry->run = run;
    entry->context = context;
    entry->macro = macro;
    if (cuno_test_tail == NULL) {
        cuno_test_head = entry;
    } else {
        cuno_test_tail->next = entry;
    }
    cuno_test_tail = entry;
}

void cuno_test_register(const char *name, const char *file, int line,
                        void (*run)(void *), void *context) {
    cuno_test_add(name, file, line, run, context, 0);
}

void cuno_test_register_macro(const char *name, const char *file, int line,
                              void (*run)(void *)) {
    cuno_test_add(name, file, line, run, NULL, 1);
}

static void cuno_test_field(const char *text) {
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

static void cuno_test_end_event(void) {
    fputc('\n', cuno_test_out);
    fflush(cuno_test_out);
}

void cuno_test_fail(const char *file, int line, const char *message, int fatal) {
    char number[16];
    cuno_test_failed = 1;
    fputs("fail\t", cuno_test_out);
    cuno_test_field(file);
    sprintf(number, "%d", line);
    fputc('\t', cuno_test_out);
    fputs(number, cuno_test_out);
    fputc('\t', cuno_test_out);
    cuno_test_field(message);
    cuno_test_end_event();
    if (fatal) {
        fputs("done", cuno_test_out);
        cuno_test_end_event();
        exit(1);
    }
}

void cuno_test_info(const char *text) {
    fputs("info\t", cuno_test_out);
    cuno_test_field(text);
    cuno_test_end_event();
}

void cuno_test_skip(const char *reason) {
    fputs("skip\t", cuno_test_out);
    cuno_test_field(reason);
    cuno_test_end_event();
    exit(0);
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

/* -1, 0 or 1 as a is below, equal to or above b, comparing the values
 * themselves: a negative signed value is below every unsigned one, which C's
 * own conversions would get wrong. */
static int cuno_test_order(cuno_test_value a, cuno_test_value b) {
    if (a.kind == CUNO_TEST_FLOAT || b.kind == CUNO_TEST_FLOAT) {
        long double x = a.kind == CUNO_TEST_FLOAT ? a.as.f
                        : a.kind == CUNO_TEST_SIGNED ? (long double)a.as.i
                                                     : (long double)a.as.u;
        long double y = b.kind == CUNO_TEST_FLOAT ? b.as.f
                        : b.kind == CUNO_TEST_SIGNED ? (long double)b.as.i
                                                     : (long double)b.as.u;
        return x < y ? -1 : x > y ? 1 : 0;
    }
    if (a.kind == CUNO_TEST_POINTER || b.kind == CUNO_TEST_POINTER) {
        const char *x = a.kind == CUNO_TEST_POINTER ? (const char *)a.as.p
                                                    : (const char *)(size_t)a.as.u;
        const char *y = b.kind == CUNO_TEST_POINTER ? (const char *)b.as.p
                                                    : (const char *)(size_t)b.as.u;
        return x < y ? -1 : x > y ? 1 : 0;
    }
    if (a.kind == CUNO_TEST_SIGNED && b.kind == CUNO_TEST_SIGNED) {
        return a.as.i < b.as.i ? -1 : a.as.i > b.as.i ? 1 : 0;
    }
    if (a.kind == CUNO_TEST_SIGNED && a.as.i < 0) {
        return -1;
    }
    if (b.kind == CUNO_TEST_SIGNED && b.as.i < 0) {
        return 1;
    }
    {
        unsigned long long x = a.kind == CUNO_TEST_SIGNED ? (unsigned long long)a.as.i : a.as.u;
        unsigned long long y = b.kind == CUNO_TEST_SIGNED ? (unsigned long long)b.as.i : b.as.u;
        return x < y ? -1 : x > y ? 1 : 0;
    }
}

static const char *const cuno_test_op_names[] = {"eq", "ne", "lt", "le", "gt", "ge"};

static int cuno_test_holds(int op, int order) {
    switch (op) {
    case CUNO_TEST_EQ: return order == 0;
    case CUNO_TEST_NE: return order != 0;
    case CUNO_TEST_LT: return order < 0;
    case CUNO_TEST_LE: return order <= 0;
    case CUNO_TEST_GT: return order > 0;
    default: return order >= 0;
    }
}

void cuno_test_values_failed(const char *file, int line, const char *what,
                             const char *left, const char *right, int fatal) {
    char message[1024];
    snprintf(message, sizeof message, "%s failed\n  left:  %s\n  right: %s", what, left,
             right);
    cuno_test_fail(file, line, message, fatal);
}

void cuno_test_compare(int op, cuno_test_value a, cuno_test_value b, const char *expression,
                       const char *file, int line, int fatal) {
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
             fatal ? (expected ? "require" : "require_false") : (expected ? "check" : "check_false"),
             expression);
    cuno_test_fail(file, line, message, fatal);
}

/* Tests are told apart by name and location (testing.md, "Common rules"):
 * the same name at the same place is one test registered by several
 * translation units, and the same name at another place is an error. */
static int cuno_test_same_place(const cuno_test_entry *a, const cuno_test_entry *b) {
    return a->line == b->line && strcmp(a->file, b->file) == 0;
}

static int cuno_test_check_registry(void) {
    const cuno_test_entry *a;
    const cuno_test_entry *b;
    int ok = 1;
    for (a = cuno_test_head; a != NULL; a = a->next) {
        if (strpbrk(a->name, "\r\n") != NULL || a->name[0] == '\0') {
            fprintf(stderr, "error: the test at %s:%d has a name that is empty or spans lines\n",
                    a->file, a->line);
            ok = 0;
        }
        for (b = a->next; b != NULL; b = b->next) {
            if (strcmp(a->name, b->name) != 0) {
                continue;
            }
            if (!cuno_test_same_place(a, b)) {
                fprintf(stderr, "error: two tests are named `%s`: %s:%d and %s:%d\n", a->name,
                        a->file, a->line, b->file, b->line);
                ok = 0;
            } else if (a->macro || b->macro) {
                /* Several translation units compiled the same cuno_test: it
                 * is in a header, which the macro API does not support. */
                fprintf(stderr,
                        "error: the test `%s` at %s:%d is compiled more than once\n"
                        "help: cuno_test does not support tests in headers; move it into a "
                        "source file, or use cuno/test.hpp\n",
                        a->name, a->file, a->line);
                ok = 0;
            }
        }
    }
    return ok;
}

/* Whether `entry` is the first registration of its test. */
static int cuno_test_first(const cuno_test_entry *entry) {
    const cuno_test_entry *e;
    for (e = cuno_test_head; e != entry; e = e->next) {
        if (strcmp(e->name, entry->name) == 0 && cuno_test_same_place(e, entry)) {
            return 0;
        }
    }
    return 1;
}

int main(int argc, char **argv) {
    const cuno_test_entry *entry;
    const char *path;
    if (!cuno_test_check_registry()) {
        return 1;
    }
    if (argc == 2 && strcmp(argv[1], "--list") == 0) {
        for (entry = cuno_test_head; entry != NULL; entry = entry->next) {
            if (cuno_test_first(entry)) {
                puts(entry->name);
            }
        }
        return 0;
    }
    if (argc != 3 || strcmp(argv[1], "--exact") != 0) {
        fputs("usage: --list | --exact <name>\n"
              "This is a test executable built by `cuno test`, which runs it.\n",
              stderr);
        return 2;
    }
    for (entry = cuno_test_head; entry != NULL; entry = entry->next) {
        if (strcmp(entry->name, argv[2]) == 0) {
            break;
        }
    }
    if (entry == NULL) {
        fprintf(stderr, "error: no test named `%s`\n", argv[2]);
        return 2;
    }
    path = getenv("CUNO_TEST_RESULT");
    cuno_test_out = path != NULL && path[0] != '\0' ? fopen(path, "w") : stderr;
    if (cuno_test_out == NULL) {
        fprintf(stderr, "error: cannot write the result file `%s`\n", path);
        return 2;
    }
    entry->run(entry->context);
    fputs("done", cuno_test_out);
    cuno_test_end_event();
    return cuno_test_failed ? 1 : 0;
}
