#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# crates 下载抖动兜底（本地与 CI 同口径）：esp 门的构建走镜像源，共享出口偶发
# "transfer too slow / transferred 0 bytes"（2026-11-15 实测：一轮 20 分钟的 esp 门
# 白跑在 winnow 的下载上）。cargo 自带重试默认 3 次、低速阈值 10B/30s 太紧，这里放宽；
# 依赖源与锁文件都不动。
export CARGO_NET_RETRY="${CARGO_NET_RETRY:-10}"
export CARGO_HTTP_TIMEOUT="${CARGO_HTTP_TIMEOUT:-120}"
export CARGO_HTTP_LOW_SPEED_LIMIT="${CARGO_HTTP_LOW_SPEED_LIMIT:-1}"
# ESP-IDF std 构建链冒烟门（0.0.5 增补轮四 批W1）。check.sh ④ 显式排除 tools/espidf-std-hello：
# xtensa-esp32-espidf 目标编译需 esp fork 工具链（cargo +esp）+ build-std + esp-idf-sys
# （ESP-IDF v5.2.2 本地克隆 + ldproxy + 乐鑫 LLVM fork esp-clang——上游 libclang 无 Xtensa
# 后端，bindgen 会报 unknown target triple 'xtensa'），宿主工具链 check 必红，本脚本单独覆盖。
# 纪律：工具链缺位 = 本地 SKIP / CI 即红（tools/ci/lib/skip.sh）；
#       工具链在位但编译失败 = FAIL；编译退出 0 但 ELF 不在树 = FAIL。
set -uo pipefail
root="$(git rev-parse --show-toplevel 2>/dev/null)" || {
  echo "ERROR: git rev-parse --show-toplevel 失败——当前目录不在 git 仓库内。" >&2
  exit 1
}
cd "$root/tools/espidf-std-hello" || { echo "ERROR: 无法进入 tools/espidf-std-hello" >&2; exit 1; }
# 产物定位共享单点：target-dir 由仓根 .cargo/config.toml 移出源码区，不得写死
# crate 内 target/（否则编译成功也会被判"ELF 不在树"）。
. "$root/tools/ci/lib/cargo-artifact.sh"
. "$root/tools/ci/lib/skip.sh"

export PATH="$HOME/.cargo/bin:$PATH"

# 前置（工具链/目标/IDF/esp-clang/镜像/venv）走公共实现：四条 esp 门共用一份
# （2026-11-27 批128 收敛；安装方式见 tools/espidf-std-hello/README.md）。
. "$root/tools/ci/lib/esp-env.sh"
rr3_esp_preflight xtensa-esp32-espidf "espidf std 冒烟门"

# elf 判据：debug 构建（冒烟只证构建链通，无时序敏感性，同 release 判据不适用）
elf="$(rr3_cargo_artifact xtensa-esp32-espidf debug espidf-std-hello)" || {
  echo "FAIL  无法解析 cargo target_directory（metadata 不可用）" >&2
  exit 1
}
if cargo +esp build --locked \
     --config 'source.crates-io.replace-with="rsproxy-sparse"' \
     --config 'source.rsproxy-sparse.registry="sparse+https://rsproxy.cn/index/"'; then
  if [ -f "$elf" ]; then
    echo "PASS  espidf-std-hello xtensa-esp32-espidf 构建: $elf"
    exit 0
  else
    echo "FAIL  编译退出 0 但 ELF 不在树: $elf" >&2
    exit 1
  fi
else
  echo "FAIL  espidf-std-hello xtensa-esp32-espidf 构建失败" >&2
  exit 1
fi
