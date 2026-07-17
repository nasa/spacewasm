#include <stdio.h>

int main(int argc, char *argv[]) {
    printf("%d", argc);
    for (int i=0; i<argc; i++) {
        printf(" %s", argv[i]);
    }
    printf("\n");
    return 0;
}
