#include <stdio.h>

#ifdef EXAMPLE_RELEASE
#define PROFILE "release"
#else
#define PROFILE "dev"
#endif

int main(void) {
    /* typeof is C23: under C17 none of the three compilers accepts it. */
    typeof(sizeof(int)) size = sizeof(int);
    printf("C23, %s profile, int is %zu bytes\n", PROFILE, size);
    return 0;
}
