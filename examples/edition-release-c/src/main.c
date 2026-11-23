#include <stdio.h>

#ifdef EXAMPLE_RELEASE
#define PROFILE "release"
#else
#define PROFILE "dev"
#endif

int main(void) {
    static_assert(sizeof(int) >= 2, "C23 static_assert needs no header");
    printf("C23, %s profile\n", PROFILE);
    return 0;
}
