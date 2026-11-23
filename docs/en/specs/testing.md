<!-- translated-from: docs/ja/specs/testing.md @ blob 4087abd184bfdef9c2cf9490e7d5c166c802881d -->

# Testing

## Policy: A + B

| | Description | Intended for |
|---|---|---|
| **A. cuno's own tests** | A lightweight mechanism provided by cuno | Default for new projects |
| **B. Existing frameworks** | Use GTest / Catch2 / doctest etc. as they are; cuno unifies discovery, execution, and output | Users with existing assets |

- Selected with `[test] framework` in `cuno.toml` ([manifest.md](manifest.md)).
- The MVP covers A only.

## Background (research)

- There are few complaints about how test frameworks are written. The main complaint is compile time (including GoogleTest / Catch2 alone is said to cost several hundred ms or more per file, while doctest costs a few tens of ms or less; the figures are mainly those published by doctest).
- More than one third of C++ developers do not write unit tests, and 17% write them without using a framework (JetBrains, "The State of C++ 2025").
- From the above, we concluded that the pain point is not how tests are written but the "heaviness of adoption" (introducing a framework, integrating it into the build, registering tests).

## Requirements for A

- **Zero setup**: cuno's test header is usable right after `cuno new`.
- **Both places to write tests are allowed**. Users can choose whichever they prefer.
  - **In-source tests**: Tests can be written inside implementation files, like Rust's `#[cfg(test)]`. They are active only in test builds and are removed from normal / release builds.
  - **The `tests/` directory**: Tests can be written in files separate from the implementation.
- **Lightweight**: Keep the compile-time increase from the include on par with doctest (a few tens of ms or less per file).
- **Modern style**: Instead of the traditional style of lining up uppercase macros, use no macros in C++20 and a lowercase style in C (see below).
- `cuno test` handles discovery, building, execution, and result output.
- **The test headers are bundled with and distributed as part of cuno itself** (just as Rust's test harness is bundled with the Rust toolchain).

### Recommended split and the `cuno new` sample

While both styles are allowed, we recommend choosing where to write tests based on the kind of test (the same approach as Rust), and show this in the sample.

| Kind of test | Where to write it | Reason |
|---|---|---|
| Unit tests (checking internal functions in detail) | Same file as the implementation | Fixed together with the implementation, so tests are less likely to go stale. Functions not visible from outside (such as C `static` functions) can also be tested |
| Integration tests (checking the public usage from outside) | `tests/` | Verifies from the same standpoint as users. Less likely to break when internals are reworked |

- The `cuno new` sample: put a small function with tests in `src/calc.cpp` (`src/calc.c` for C), and call that function from `src/main.cpp` (`src/main.c` for C).
  - The file that has `main` (the entry file) is not included in test builds, so tests are written outside the entry file ("Source collection and linking" in [project-layout.md](project-layout.md)).
  - The sample also conveys the habit of "write logic and tests in the package body, and keep `main` thin".
- Usage of `tests/` is shown in the sample when library projects are supported (Later). Even in the MVP, integration tests written in `tests/` work (they are linked with the package body).

## Writing tests: two entry points

The mechanism for running tests and displaying results is shared. Only the entry point for writing tests differs by language and standard.

| Entry point | Header | Target | Form |
|---|---|---|---|
| Modern API | `cuno/test.hpp` | C++20 and later | No macros. Namespace `cuno::test` |
| Macro API | `cuno/test.h` | C (C11 and later) and C++17 | Lowercase macros. Prefix `cuno_` |

### Design rationale

- cuno's primary target is C++ (default C++20). Its public face is a modern style without macros.
- In C++20, tests can be written without macros thanks to lambdas, `std::source_location` (obtaining the call site), and inline variables (boost-ext/ut is a prior example).
- In C, macros are indispensable for registering tests and obtaining line numbers. However, the C standard `assert` is also a lowercase macro, so lowercase macros are natural in C.
- Uppercase macro names are tedious to type (heavy use of the Shift key). We standardize on lowercase.
- Making short lowercase names (`check`, `test`, etc.) into macros would collide with users' identifiers, so macros always carry the `cuno_` prefix. The modern API avoids collisions with a namespace.

### Modern API (C++20 and later)

```cpp
int add(int a, int b) { return a + b; }

#ifdef CUNO_TEST
#include <cuno/test.hpp>
static cuno::test::suite tests = [] {
    using namespace cuno::test;

    test("add works", [] {
        expect(eq(add(1, 2), 3));     // continues even on failure
        require(eq(add(0, 0), 0));    // aborts this test on failure
    });
};
#endif
```

- In-source tests are enclosed in `#ifdef CUNO_TEST` (the same shape as Rust's `#[cfg(test)] mod tests { ... }`). Files in `tests/` do not need to be enclosed.
- `suite` registers the tests written inside it at program startup.

| API | Purpose |
|---|---|
| `suite` | A group of tests. When placed as a static variable, the tests inside are registered automatically |
| `test(name, function)` | Defines one test. The name is a string |
| `expect(condition)` | Checks a condition. Continues even on failure |
| `require(condition)` | Checks a condition. Aborts the test on failure |
| `eq` / `ne` / `lt` / `le` / `gt` / `ge(a, b)` | Comparisons. Display the values of both sides on failure |
| `approx(a, b, tolerance)` | Floating-point comparison |
| `throws<ExceptionType>(function)` / `nothrow(function)` | Whether an exception is thrown (only in builds with exceptions enabled) |
| `info(...)` | Supplementary information displayed together with a failure |
| `skip(reason)` | Marks the test as skipped |

- A boolean can also be passed directly, as in `expect(add(1, 2) == 3)`, but in that case the values cannot be displayed on failure. To see the values, use `eq` etc.

### Macro API (C / C++17)

```c
#include <cuno/test.h>

int add(int a, int b) { return a + b; }

cuno_test(add_works) {
    cuno_check_eq(add(1, 2), 3);     /* continues even on failure */
    cuno_require_eq(add(0, 0), 0);   /* aborts this test on failure */
}
```

- `cuno_test` etc. are automatically disabled in builds where `CUNO_TEST` is not defined. Test-only helper functions can be enclosed in `#ifdef CUNO_TEST`.

| Macro | Purpose |
|---|---|
| `cuno_test(name)` | Defines one test. The name is an identifier |
| `cuno_check(expr)` / `cuno_require(expr)` | Whether the expression is true. check continues, require aborts |
| `cuno_check_false(expr)` / `cuno_require_false(expr)` | Whether the expression is false |
| `cuno_check_eq` / `_ne` / `_lt` / `_le` / `_gt` / `_ge(a, b)` | Comparisons. Display the values of both sides on failure (`cuno_require_` versions also exist) |
| `cuno_check_approx(a, b, tolerance)` | Floating-point comparison |
| `cuno_info(...)` | Supplementary information displayed together with a failure |
| `cuno_skip(reason)` | Marks the test as skipped |

### Common rules

- **Enabling define**: In test builds, cuno defines `CUNO_TEST`.
- **Warnings in normal builds**: No warnings caused by tests (unused functions, etc.) are emitted.
- **Builds with exceptions disabled**: Game development often builds with exceptions disabled. Aborting a test via `require` / `cuno_require_*` must work even with exceptions disabled.
  - How: each test runs in its own process (see "Execution model" below), so when `require` fails, the test writes its result and ends that process. Neither exceptions nor `longjmp` are used.
- **Registration**: Tests are registered by static initialization at program startup (the modern API through a static `suite` variable, the macro API through a function placed in the area that lists functions called at program startup (`.CRT$XCU` under the MSVC ABI)). cuno does not read sources to find tests.
  - Reason: reading sources can miss tests that go through macros or `#if`. A missed test would never run, and the run would still be reported as a success.
- **Bundled runtime**: The registry and `main` live in a C source bundled with the headers (`cuno/test_main.c`), which is linked into every executable of a test build. Both entry points register through this C interface.
- **Mixed languages**: Each file uses the entry point for its own language. A `.c` file in a C++ package uses `cuno/test.h`; a `.cpp` file in a C package uses the entry point that fits its standard (`cuno/test.hpp` for C++20 and later). Tests written either way register with the same runtime and run in the same test executable.
- **Test identity**: Determined by the pair of "name" and "source location (file + line)". cuno makes this decision, using the location of each registration that the test executable reports in its list.
  - Multiple registrations from the same location (when a header is included by multiple translation units) are merged into one.
  - Whether two locations are in the same file is decided by the file system, not by how the path is spelled (same as [project-layout.md](project-layout.md)). The same header included under different spellings, such as `"util.h"` and `"../src/util.h"`, is the same location.
  - If a test with the same name exists at a different location, it is an error as a duplicate.
- **Tests in headers**: The modern API registers them only once via the identity rule above. The macro API does not support tests in headers; when multiple registrations from the same location are detected, it reports an error with guidance.
  - With MSVC in C, the name of a macro-API test becomes a single program-wide name (symbol), so two tests with the same name in different places, and tests in headers, become link errors rather than cuno's guidance.
    - Reason: this keeps the linker from discarding the registration functions. If they were discarded, an optimized build (`lto = true`) would run no tests at all and still be reported as a success.
- **Not provided (for now)**: Subcases (doctest's SUBCASE) and a dedicated fixture mechanism. Shared logic is written as ordinary functions.

## Execution

- `cuno test [name filter]`.
- **Unit of test executables**: In-source tests are combined into one executable per package. One executable per file **directly under** `tests/` (same as cargo).
  - Subdirectories of `tests/` are treated as places for helper code shared among tests (they do not become test executables themselves, and are linked into each test executable directly under `tests/`).
- The working directory during test execution is the **project root** (same as `cuno run`).
- Tests run in parallel. The degree of parallelism is set with the common option `-j` (default is the number of CPU cores).
- Both standard output and standard error during tests are captured, and shown only for failed tests. `--nocapture` always shows them.
- Tests have no timeout (same as cargo). Users interrupt tests that hang.
- If even one test fails, the exit code is non-zero.

### Execution model: one process per test

- cuno launches the test executable **as a separate process for each test**. Parallel execution is per process, and output is captured per process.
- Reasons:
  - A process has only one standard output, so the output of tests running in parallel within the same process cannot be separated. Separating processes makes it possible to have both "parallel execution" and "show output only for failed tests".
  - Even if one test crashes (invalid memory access, etc.), the other tests continue and only that test is reported as failed. This is especially important in C/C++.
- Cost: slower by the cost of process startup (a few ms to a few tens of ms each on Windows). In case this becomes a problem at the scale of thousands of tests, room is left to add a "run everything in one process" mode later.

### Contract between cuno and test executables

| Argument | Behavior |
|---|---|
| `--list` | Reports the registered tests, each registration with its name, file, line and entry point (modern API / macro API) |
| `--exact <name>` | Runs only the specified test |

- Results are returned via the exit code (`0` = success, anything else = failure).
- The list of tests and each test's result (passed, failed or skipped and why, the location and values of a failed check, and `info` notes) are written, in a format interpreted only by cuno, to the file that cuno names in the environment variable `CUNO_TEST_RESULT`. Standard output and standard error belong to the user: cuno captures and displays them but does not interpret their contents.
  - Reason: so that no string in the user's output can be mistaken for a result. If the list were written to standard output, a single library that prints something at program startup would be enough to corrupt test names and keep those tests from running.
- A test that ends without writing the result file (a crash, a call to `exit`, etc.) fails, even if its exit code is 0.
- A test that calls `skip` after a failed check fails (failure takes precedence over skip).
- Test names and the `CUNO_TEST_RESULT` path are passed independently of the code page, on Windows too (Japanese names and paths can be used as they are).

### Output

Follows cargo.

```
running 3 tests
test add works ... ok
test parse rejects empty ... FAILED
test slow path ... skipped

failures:
---- parse rejects empty ----
src/parser.cpp:42: eq(result, 0) failed
  left:  -1
  right: 0

test result: FAILED. 1 passed; 1 failed; 1 skipped; finished in 0.02s
```

## Later (outside the MVP)

- **Benchmarks**: Bundle a header for `cuno bench` (like tests, with two entry points: the modern API and the macro API).
- **Sanitizers**: Specified as in `cuno test --sanitize address`. The available kinds depend on the compiler (MSVC supports only AddressSanitizer; Clang also supports UndefinedBehaviorSanitizer etc.). Unsupported combinations produce a clear error.
- **Coverage**: `cuno test --coverage` produces an HTML report (uses llvm-cov with Clang).

## Out of scope

- No mocking features (use an external library if needed).

## Requirements for B

- cuno handles setup (fetching and building the framework) and execution.

## Open questions

- Benchmark API
- How to achieve coverage with MSVC
