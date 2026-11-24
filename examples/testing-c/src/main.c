#include <stdio.h>

#include "stats.h"

int main(void) {
    int values[] = {1, 2, 4};
    printf("sum %d, mean %d\n", sum(values, 3), mean(values, 3));
    return 0;
}
