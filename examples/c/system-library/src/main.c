#include <stdio.h>

#include <windows.h>

#include <shlwapi.h>

int main(void) {
    printf("%s\n", PathFindExtensionA("notes.txt"));
    return 0;
}
