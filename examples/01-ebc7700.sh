# GCC and LLVM compiler test for ESWIN EBC7700.
# Run this scenario after the board has booted and the terminal login is ready.
# RuyiSDK: 0.50.0; gnu-ruyisdk: 0.20260625.0; llvm-ruyisdk: 22.1.8-ruyi.20260625.

#$ expect \$
cat /proc/cpuinfo
#$ expect \$
#$ snapshot device-cpuinfo
cat /proc/device-tree/model
#$ expect \$
#$ snapshot device-model
ruyi venv -t gnu-ruyisdk manual venv-gnu-ruyisdk
#$ expect \$
. venv-gnu-ruyisdk/bin/ruyi-activate
#$ expect \$
riscv64-ruyisdk-linux-gnu-gcc -v
#$ expect \$
cat > hello.c <<'EOF'
#include <stdio.h>

int main() {
    printf("Hello, World!\n");
    return 0;
}
EOF
#$ expect \$
riscv64-ruyisdk-linux-gnu-gcc hello.c -o hello-gcc
#$ expect \$
./hello-gcc
#$ expect Hello, World!
#$ snapshot gcc-hello
#$ expect \$
git clone https://github.com/eembc/coremark
#$ expect \$
cd coremark
#$ expect \$
make CC=riscv64-ruyisdk-linux-gnu-gcc XCFLAGS="-march=rv64gc_zba_zbb" compile
#$ expect \$
mv coremark.exe coremark-gcc && ./coremark-gcc
#$ expect CoreMark
#$ snapshot gcc-coremark
#$ expect \$
cd .. && ruyi-deactivate
#$ expect \$

ruyi venv -t llvm-ruyisdk manual --sysroot-from gnu-ruyisdk venv-llvm-ruyisdk
#$ expect \$
. venv-llvm-ruyisdk/bin/ruyi-activate
#$ expect \$
clang -v
#$ expect \$
clang hello.c -o hello-llvm
#$ expect \$
./hello-llvm
#$ expect Hello, World!
#$ snapshot llvm-hello
#$ expect \$
cd coremark && make clean
#$ expect \$
make CC=clang XCFLAGS="-march=rv64gc_zba_zbb" compile
#$ expect \$
mv coremark.exe coremark-llvm && ./coremark-llvm
#$ expect CoreMark
#$ snapshot llvm-coremark
#$ expect \$
cd .. && ruyi-deactivate
#$ expect \$
