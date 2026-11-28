#include <cstdio>

// Each define says whether its layer reached the program.
static void report(const char* name, bool defined) {
    std::printf("%s: %s\n", name, defined ? "yes" : "no");
}

int main() {
#ifdef FROM_BUILD
    report("FROM_BUILD", true);
#else
    report("FROM_BUILD", false);
#endif
#ifdef FROM_WINDOWS
    report("FROM_WINDOWS", true);
#else
    report("FROM_WINDOWS", false);
#endif
#ifdef FROM_LINUX
    report("FROM_LINUX", true);
#else
    report("FROM_LINUX", false);
#endif
#ifdef FROM_DEV
    report("FROM_DEV", true);
#else
    report("FROM_DEV", false);
#endif
#ifdef FROM_RELEASE
    report("FROM_RELEASE", true);
#else
    report("FROM_RELEASE", false);
#endif
}
