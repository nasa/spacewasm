#include <stdio.h>

int main() {
    FILE *f;

    f = fopen("dummyfile", "r");
    char content[100];
    fgets(content, 100, f);
    printf("%s", content);
    fclose(f);

    return 0;
}