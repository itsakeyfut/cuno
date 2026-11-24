#pragma once

#include <string>
#include <string_view>

// `text` without the spaces at either end.
std::string trim(std::string_view text);
