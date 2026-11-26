#include <cstdio>

int main() {
    const char* names[] = {"none", "msvc", "clang", "clang-cl"};
    std::printf("built by %s\n", names[BUILT_BY]);
}
