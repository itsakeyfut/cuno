#include <stdio.h>

#include "shapes.h"

int main(void) {
    printf("square 3: %d, rectangle 2x5: %d\n", square_area(3), rectangle_area(2, 5));
    return 0;
}
