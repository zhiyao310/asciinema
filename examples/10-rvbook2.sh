# GCC and LLVM compiler test for RISC-V Book 2.
# Run this scenario after the laptop has booted and the terminal login is ready.
# RuyiSDK: 0.50.0; gnu-ruyisdk: 0.20260625.0; llvm-ruyisdk: 22.1.8-ruyi.20260625.

#$ expect \$
ssh tiaoban
#$ expect \$
ssh rv2
#$ expect (?i)password:
debian
#$ expect \$
cat /proc/cpuinfo
#$ expect \$
#$ snapshot device-cpuinfo
cat /proc/device-tree/model
#$ expect \$
#$ snapshot device-model
ruyi venv -t gnu-ruyisdk manual venv-gnu-ruyisdk-riscv-book-2
#$ expect \$
. venv-gnu-ruyisdk-riscv-book-2/bin/ruyi-activate
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
riscv64-ruyisdk-linux-gnu-gcc hello.c -o hello-gcc && ./hello-gcc
#$ expect Hello, World!
#$ snapshot gcc-hello
#$ expect \$
git clone https://github.com/eembc/coremark
#$ expect \$
cd coremark
#$ expect \$
make CC=riscv64-ruyisdk-linux-gnu-gcc XCFLAGS="-mcpu=xt-c920" compile
#$ expect \$
mv coremark.exe coremark-gcc && ./coremark-gcc
#$ expect CoreMark
#$ snapshot gcc-coremark
#$ expect \$
cd .. && ruyi-deactivate
#$ expect \$

ruyi venv -t llvm-ruyisdk manual --sysroot-from gnu-ruyisdk venv-llvm-ruyisdk-riscv-book-2
#$ expect \$
. venv-llvm-ruyisdk-riscv-book-2/bin/ruyi-activate
#$ expect \$
clang -v
#$ expect \$
clang hello.c -o hello-llvm && ./hello-llvm
#$ expect Hello, World!
#$ snapshot llvm-hello
#$ expect \$
cd coremark && make clean
#$ expect \$
make CC=clang XCFLAGS="-march=rv64imafdcv_zicntr_zicsr_zifencei_zihpm_zba_zbb_zbs_sscofpmf_svpbmt" compile
#$ expect \$
mv coremark.exe coremark-llvm && ./coremark-llvm
#$ expect CoreMark
#$ snapshot llvm-coremark
#$ expect \$
cd .. && ruyi-deactivate
#$ expect \$

#$ send \n
#$ sendcharacter ~~.
#$ expect \$
exit 0
#$ expect \$
