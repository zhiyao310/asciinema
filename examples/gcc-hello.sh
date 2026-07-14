#$ expect \$

#$ snapshot before-create-source
cat > hello.c <<'EOF'
#include <stdio.h>

int main(void) {
    puts("Hello, expect!");
    return 0;
}
EOF
#$ expect \$
cat hello.c
#$ expect \$
#$ snapshot after-create-source

#$ snapshot before-compile
gcc -Wall -Wextra -std=c11 hello.c -o hello
#$ expect \$
#$ snapshot after-compile

#$ snapshot before-run
./hello
#$ expect Hello, expect!
#$ expect \$
#$ snapshot after-run
