#include "checksum.h"

unsigned checksum(const char* text) {
    unsigned sum = 0;
    for (; *text; ++text) {
        sum += (unsigned char)*text;
    }
    return sum;
}
