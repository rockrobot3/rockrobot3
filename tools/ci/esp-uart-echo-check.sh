#!/usr/bin/env bash
# ESP32 echo 固件编译门（0.0.7 批C）。check.sh ④ 特意排除 tools/esp32-uart-echo：
# xtensa-esp32-none-elf 目标编译需 esp fork 工具链（cargo +esp）+ Xtensa esp-gcc
# （链接走 ldproxy，rust-lld 吃不下 exception.x 的 location counter 回退），宿主
# 工具链 check 必红，本脚本单独覆盖该工程。
# 纪律：工具链缺位 = SKIP（退出 0 并明说，同 ipct003 arm64 容器口径）；
#       工具链在位但编译失败 = FAIL；编译退出 0 但 ELF 不在树 = FAIL。
set -uo pipefail
root="$(git rev-parse --show-toplevel 2>/dev/null)" || {
  echo "ERROR: git rev-parse --show-toplevel 失败——当前目录不在 git 仓库内。" >&2
  exit 1
}
cd "$root/tools/esp32-uart-echo" || { echo "ERROR: 无法进入 tools/esp32-uart-echo" >&2; exit 1; }

export PATH="$HOME/.cargo/bin:$PATH"

# esp fork 工具链（rustup alias esp → esp-1.90.0.0，0.0.7 批A'）
if ! rustup toolchain list 2>/dev/null | grep -q '^esp'; then
  echo "SKIP  esp 工具链未安装（rustup toolchain list 无 'esp'）——固件编译门不执行，安装方式见 0.0.7 方案批A'"
  exit 0
fi

# Xtensa esp-gcc：默认落位 ~/.espressif/xtensa-esp-elf（0.0.7 批C），shell 已有则不动
if ! command -v xtensa-esp32-elf-gcc >/dev/null 2>&1; then
  if [ -x "$HOME/.espressif/xtensa-esp-elf/bin/xtensa-esp32-elf-gcc" ]; then
    export PATH="$HOME/.espressif/xtensa-esp-elf/bin:$PATH"
  else
    echo "SKIP  Xtensa esp-gcc 未安装（PATH 及 ~/.espressif/xtensa-esp-elf/bin 均无 xtensa-esp32-elf-gcc）——固件编译门不执行，安装方式见 0.0.7 方案批C"
    exit 0
  fi
fi
if ! command -v ldproxy >/dev/null 2>&1; then
  echo "SKIP  ldproxy 未安装（cargo install ldproxy）——固件编译门不执行"
  exit 0
fi

# release 构建：esp-hal 明示 dev profile 可能慢一个数量级并引发时序问题——夹具的
# 50ms 帧内静默超时与 115200 持续流（87µs/字节）对时序敏感，板级验证用 release。
elf="target/xtensa-esp32-none-elf/release/esp32-uart-echo"
# 仓库源不硬编码镜像（他人环境各异），rsproxy 注入随本机
if cargo +esp build --release \
     --config 'source.crates-io.replace-with="rsproxy-sparse"' \
     --config 'source.rsproxy-sparse.registry="sparse+https://rsproxy.cn/index/"'; then
  if [ -f "$elf" ]; then
    echo "PASS  esp32-uart-echo xtensa 构建: $elf"
    exit 0
  else
    echo "FAIL  编译退出 0 但 ELF 不在树: $elf" >&2
    exit 1
  fi
else
  echo "FAIL  esp32-uart-echo xtensa 构建失败" >&2
  exit 1
fi
