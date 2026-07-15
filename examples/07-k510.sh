# GCC and LLVM compiler test for Canaan K510 CRB-V1.2 KIT.
#
# This is the one host-side scenario in the set. The K510 test document builds
# on an x86 host and then transfers the binaries to the board. 
# 1.Ensure the board is connected via USB-UART and you can access it with:
# sudo minicom -D /dev/ttyUSB0 -b 115200
# 2.Optionally set K510_HTTP_PORT if you want a custom HTTP port (default: 8000)
# 3.K510_SERVER_IP defaults to the address used in the test document.
# RuyiSDK: 0.50.0; gnu-ruyisdk: 0.20260625.0; llvm-ruyisdk: 22.1.8-ruyi.20260625.

#$ expect \$
sudo minicom -D /dev/ttyUSB0 -b 115200
#$ expect \$
cat /proc/cpuinfo
#$ expect \$
#$ snapshot device-cpuinfo
cat /proc/device-tree/model
#$ expect \$
#$ snapshot device-model
#$ expect \$
exit
#$ expect \$
ruyi venv -t gnu-ruyisdk generic gcc-env
#$ expect \$
. gcc-env/bin/ruyi-activate
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
riscv64-ruyisdk-linux-gnu-gcc -static -march=rv64imafdc hello.c -o hello-gcc
#$ expect \$
riscv64-ruyisdk-linux-gnu-objcopy --remove-section=.riscv.attributes hello-gcc hello-gcc.tmp && mv hello-gcc.tmp hello-gcc
#$ expect \$
git clone https://github.com/eembc/coremark
#$ expect \$
cd coremark
#$ expect \$
make CC=riscv64-ruyisdk-linux-gnu-gcc XCFLAGS="-static -march=rv64imafd" compile
#$ expect \$
mv coremark.exe coremark-gcc
#$ expect \$
cd .. && ruyi-deactivate
#$ expect \$

# Start the host-side file server before the board-side download steps.
K510_SERVER_IP="${K510_SERVER_IP:-10.13.21.160}"; K510_HTTP_PORT="${K510_HTTP_PORT:-8000}"; K510_BOARD_HOST="${K510_BOARD_HOST:?Set K510_BOARD_HOST, for example root@192.168.1.50}"; python3 -m http.server "$K510_HTTP_PORT" >/tmp/k510-compiler-test-http.log 2>&1 & HTTP_PID=$!; trap 'kill "$HTTP_PID" 2>/dev/null' EXIT
#$ expect \$
sudo minicom -D /dev/ttyUSB0 -b 115200 
#$ expect \$
wget http://${K510_SERVER_IP}:${K510_HTTP_PORT}/hello-gcc -O /root/hello-gcc && wget http://${K510_SERVER_IP}:${K510_HTTP_PORT}/coremark-gcc -O /root/coremark-gcc
#$ expect \$
chmod +x /root/hello-gcc && /root/hello-gcc
#$ expect Hello, World!
#$ snapshot gcc-hello
#$ expect \$
chmod +x /root/coremark-gcc && /root/coremark-gcc
#$ expect CoreMark
#$ snapshot gcc-coremark
#$ expect \$
exit
#$ expect \$

ruyi venv -t llvm-ruyisdk generic --sysroot-from gnu-ruyisdk llvm-env
#$ expect \$
. llvm-env/bin/ruyi-activate
#$ expect \$
clang -v
#$ expect \$
clang -static --target=riscv64-linux-gnu -march=rv64imafdc hello.c -o hello-llvm
#$ expect \$
OBJCOPY="${K510_OBJCOPY:-$HOME/tes/k510_buildroot/k510_crb_lp3_v1_2_defconfig/host/bin/riscv64-buildroot-linux-gnu-objcopy}"; [ -x "$OBJCOPY" ] || OBJCOPY=/opt/riscv64-lp64d--glibc--stable-2025.08-1/bin/riscv64-linux-objcopy; "$OBJCOPY" --remove-section=.riscv.attributes hello-llvm hello-llvm.tmp && mv hello-llvm.tmp hello_llvm
#$ expect \$
cd coremark && make clean
#$ expect \$
make CC=clang XCFLAGS="-static --target=riscv64-linux-gnu -march=rv64imafdc -Iposix" compile
#$ expect \$
mv coremark.exe coremark-llvm
#$ expect \$
OBJCOPY="${K510_OBJCOPY:-$HOME/tes/k510_buildroot/k510_crb_lp3_v1_2_defconfig/host/bin/riscv64-buildroot-linux-gnu-objcopy}"; [ -x "$OBJCOPY" ] || OBJCOPY=/opt/riscv64-lp64d--glibc--stable-2025.08-1/bin/riscv64-linux-objcopy; "$OBJCOPY" --remove-section=.riscv.attributes coremark-llvm coremark-llvm.tmp && mv coremark-llvm.tmp coremark_llvm
#$ expect \$
cd .. && ruyi-deactivate
#$ expect \$

# Start the host-side file server before the board-side download steps.
K510_SERVER_IP="${K510_SERVER_IP:-10.13.21.160}"; K510_HTTP_PORT="${K510_HTTP_PORT:-8000}"; K510_BOARD_HOST="${K510_BOARD_HOST:?Set K510_BOARD_HOST, for example root@192.168.1.50}"; python3 -m http.server "$K510_HTTP_PORT" >/tmp/k510-compiler-test-http.log 2>&1 & HTTP_PID=$!; trap 'kill "$HTTP_PID" 2>/dev/null' EXIT
#$ expect \$
sudo minicom -D /dev/ttyUSB0 -b 115200 
#$ expect \$
wget http://${K510_SERVER_IP}:${K510_HTTP_PORT}/hello_llvm -O /root/hello_llvm && wget http://${K510_SERVER_IP}:${K510_HTTP_PORT}/coremark_llvm -O /root/coremark_llvm
#$ expect \$
chmod +x /root/hello_llvm && /root/hello_llvm
#$ expect Hello, World!
#$ snapshot llvm-hello
#$ expect \$
chmod +x /root/coremark_llvm && /root/coremark_llvm
#$ expect CoreMark
#$ snapshot llvm-coremark
#$ expect \$
exit
#$ expect \$
kill "$HTTP_PID"
#$ expect \$
