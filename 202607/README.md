# RISC-V 开发板编译测试

本目录保存 GCC/LLVM 开发板编译测试用例和测试结果。完整测试步骤见
[board-compiler-test-cases.md](board-compiler-test-cases.md)，对应的录制脚本位于仓库根目录的
`examples/`。

Word 版测试用例文档：[10种开发板+编译器测试用例.docx](10种开发板+编译器测试用例.docx)

`board-compiler-test-cases_media/` 保存每个开发板的录制结果：

- `.cast`：终端录制文件
- `.gif`：终端录制的 GIF 预览
- `.mp4`：终端录制的视频
- `snapshots/`：执行过程中截取的预期结果图片

## 开发板、脚本与测试环境照片

| 用例 | 开发板 | 脚本 | 测试环境照片 |
| --- | --- | --- | --- |
| 1 | ESWIN EBC7700 | `examples/01-ebc7700.sh` | — |
| 2 | HiFive Premier P550 | `examples/02-p550.sh` | — |
| 3 | ESWIN EBC7702 | `examples/03-ebc7702.sh` | — |
| 4 | SG2044 EVB | `examples/04-sg2044.sh` | — |
| 5 | A210 SODIMM V2 | `examples/05-a210-v2.sh` | [照片](boards_test_env_photos/5.a210-v2.jpg) |
| 6 | VisionFive 2 Lite | `examples/06-vf2-lite.sh` | — |
| 7 | Canaan K510 CRB-V1.2 KIT | `examples/07-k510.sh` | — |
| 8 | Milk-V Megrez | `examples/08-megrez.sh` | — |
| 9 | RISC-V Book | `examples/09-rvbook.sh` | [照片 1](boards_test_env_photos/9.ruyibook.jpg)、[照片 2](<boards_test_env_photos/9.ruyibook(2).jpg>) |
| 10 | RISC-V Book 2 | `examples/10-rvbook2.sh` | [照片 1](boards_test_env_photos/10.rvbook2.jpg)、[照片 2](<boards_test_env_photos/10.rvbook2(2).jpg>) |
| 11 | SpacemiT K3 Pico-ITX | `examples/11-k3.sh` | — |

## 运行

先确认开发板已经启动并登录，网络正常，且 RuyiSDK、GCC/LLVM 工具链和 CoreMark
等测试环境已经按测试文档准备好。脚本不执行系统依赖、Ruyi 或工具链的安装步骤。

从仓库根目录运行单个用例，例如：

```sh
cd /asciinema

asciinema expect examples/05-a210-v2.sh \
  --output-dir 202607/board-compiler-test-cases_media/5.a210-v2 \
  --timeout 300 \
  --monitor
```

如果全局 `asciinema` 不是当前源码构建的版本，可更新全局程序：

```sh
cargo install --path . --locked --force
```

也可以直接使用本地构建版本：

```sh
cargo build --locked
./target/debug/asciinema expect examples/05-a210-v2.sh \
  --output-dir 202607/board-compiler-test-cases_media/5.a210-v2 \
  --timeout 300 \
  --monitor
```

还原测试环境时，在仓库根目录执行：

```sh
rm -rf coremark hello.c hello-gcc hello-llvm ruyi-0.50.0.riscv64 venv-*
```


## 结果目录与图片命名

结果统一保存到：

```text
202607/board-compiler-test-cases_media/序号.开发板名/
```

例如：

```text
202607/board-compiler-test-cases_media/5.a210-v2/
├── 05-a210-v2.cast
├── 05-a210-v2.gif
├── 05-a210-v2.mp4
└── snapshots/
    ├── device-cpuinfo.png
    ├── device-model.png
    ├── gcc-hello.png
    ├── gcc-coremark.png
    ├── llvm-hello.png
    └── llvm-coremark.png
```

当前已保存用例 1–10 的录制结果；用例 11（SpacemiT K3 Pico-ITX）目前只有测试脚本，尚未收录录制媒体。

`snapshots/` 内的图片只使用快照名称，不带 `01-`、`02-` 等序号。重新生成前，
如果目录中还保留旧的带序号图片，应先清理旧的生成结果。

## 特殊脚本

- A210 脚本会通过 `ssh tiaoban` 进入跳板机，再通过 `ssh a210` 进入开发板。
  运行前需确保这两个 SSH 主机别名可用。
- A210 使用 `~~.` 将 SSH 转义序列传递给内层 SSH，再使用 `exit 0` 正常退出跳板机。
- RISC-V Book 脚本依次通过 `ssh tiaoban`、`ssh rv1` 进入开发板，RISC-V Book 2
  脚本依次通过 `ssh tiaoban`、`ssh rv2` 进入开发板；两个脚本的开发板登录密码均为
  `debian`。
- K510 是主机交叉编译、HTTP 传输到开发板的流程，需要按脚本注释配置
  `K510_BOARD_HOST`；必要时配置 `K510_SERVER_IP`、`K510_HTTP_PORT` 和
  `K510_OBJCOPY`。

## 检查脚本

```sh
bash -n examples/*.sh
cargo test --locked expect::tests
git diff --check
```
