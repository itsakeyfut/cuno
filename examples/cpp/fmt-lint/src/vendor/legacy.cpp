// Vendored code, kept as its authors wrote it: not formatted, and using 0
// for a null pointer, which [lint] deny would make an error were it linted.
#include "legacy.hpp"
int legacy_answer()
{
    int *unused = 0;
    if (unused == 0) return 42;
    return 0;
}
