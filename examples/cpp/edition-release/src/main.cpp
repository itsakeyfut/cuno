#include <cstdio>
#include <utility>

#ifdef EXAMPLE_RELEASE
constexpr const char* profile = "release";
#else
constexpr const char* profile = "dev";
#endif

enum class Level : int { Low = 1, High = 2 };

int main() {
    // std::to_underlying is C++23.
    std::printf("C++23 level %d, %s profile\n", std::to_underlying(Level::High), profile);
}
