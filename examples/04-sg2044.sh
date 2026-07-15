# GCC and LLVM compiler test for the SG2044 EVB.
# Run this scenario after the board has booted and the terminal login is ready.
# RuyiSDK: 0.50.0; gnu-ruyisdk: 0.20260625.0; llvm-ruyisdk: 22.1.8-ruyi.20260625.

#$ expect \$
cat /proc/cpuinfo
#$ expect \$
#$ snapshot device-cpuinfo
cat /proc/device-tree/model
#$ expect \$
#$ snapshot device-model
ruyi venv -t gnu-ruyisdk manual venv-gnu-ruyisdk-sg2044
#$ expect \$
. venv-gnu-ruyisdk-sg2044/bin/ruyi-activate
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
make CC=riscv64-ruyisdk-linux-gnu-gcc XCFLAGS="-mcpu=xt-c920v2" compile
#$ expect \$
mv coremark.exe coremark-gcc && ./coremark-gcc
#$ expect CoreMark
#$ snapshot gcc-coremark
#$ expect \$
cd .. && ruyi-deactivate
#$ expect \$

ruyi venv -t llvm-ruyisdk manual --sysroot-from gnu-ruyisdk venv-llvm-ruyisdk-sg2044
#$ expect \$
. venv-llvm-ruyisdk-sg2044/bin/ruyi-activate
#$ expect \$
clang -v
#$ expect \$
clang hello.c -o hello-llvm && ./hello-llvm
#$ expect Hello, World!
#$ snapshot llvm-hello
#$ expect \$
cd coremark && make clean
#$ expect \$
make CC=clang XCFLAGS="-march=rv64imafdcv_zicbom_zicbop_zicboz_ziccrse_zicntr_zicond_zicsr_zifencei_zihintntl_zihintpause_zihpm_zaamo_zalrsc_zawrs_zfa_zfbfmin_zfh_zfhmin_zca_zcb_zcd_zba_zbb_zbc_zbs_zve32f_zve32x_zve64d_zve64f_zve64x_zvfbfmin_zvfbfwma_zvfh_zvfhmin_sscofpmf_sstc_svinval_svnapot_svpbmt" compile
#$ expect \$
mv coremark.exe coremark-llvm && ./coremark-llvm
#$ expect CoreMark
#$ snapshot llvm-coremark
#$ expect \$
cd .. && ruyi-deactivate
#$ expect \$
