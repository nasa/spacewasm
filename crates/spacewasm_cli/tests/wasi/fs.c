#include <stdio.h>

int main() {
    FILE *f;

    f = fopen("dummyfile", "r");
    if (f == NULL) {
        fprintf(stderr, "error: could not open dummyfile\n");
        return 1;
    }
    char content[100];
    fgets(content, 100, f);
    printf("%s", content);
    fclose(f);

    return 0;
}