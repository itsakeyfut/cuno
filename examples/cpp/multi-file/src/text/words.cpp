#include "text/words.hpp"

#include <sstream>

std::vector<std::string> split(const std::string& text) {
    std::istringstream in(text);
    std::vector<std::string> words;
    for (std::string word; in >> word;) {
        words.push_back(word);
    }
    return words;
}
