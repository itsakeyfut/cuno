#include <cstdio>

#include "text/words.hpp"

int main() {
    auto words = split("one comfortable tool");
    std::printf("%zu words, the last is %s\n", words.size(), words.back().c_str());
}
