#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# ESP-IDF 哨兵构建门（0.0.7 批D1）。check.sh ④/⑥ 显式排除 tools/espidf-sentinel：
# xtensa-esp32-espidf 目标编译需 esp fork 工具链（cargo +esp）+ build-std + esp-idf-sys
# （ESP-IDF v5.2.2 本地克隆 + ldproxy + 乐鑫 LLVM fork esp-clang），宿主工具链 check
# 必红，本脚本单独覆盖。SKIP 纪律同 espidf-std-check.sh：工具链缺位 = 本地 SKIP / CI 即红（tools/ci/lib/skip.sh）；工具链在位但编译失败 = FAIL；编译退出 0 但产物不在树 = FAIL。
# 双证之①（编译面）在本门执法：rr-core + rr-ipc + 哨兵在 xtensa-esp32-espidf
# 目标可编译；双证之②（板上真跑）走 README runbook，不属 CI。
# 伴生对手件 harness 已落 tools/rr3-sentinel-harness（独立宿主 crate，不落哨兵目录
# 内的原因见其 Cargo.toml 注记），由主门禁 ④/⑥ 与 CI lint 按普通独立工程全覆盖，
# 不在本门重复执跑。
set -uo pipefail
root="$(git rev-parse --show-toplevel 2>/dev/null)" || {
  echo "ERROR: git rev-parse --show-toplevel 失败——当前目录不在 git 仓库内。" >&2
  exit 1
}
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TERM_COLOR=never
fail=0

# ---- 甲面：xtensa 哨兵（工具链缺位 = 本地整门 SKIP / CI 即红，同 espidf-std-check.sh 口径）----
cd "$root/tools/espidf-sentinel" || { echo "ERROR: 无法进入 tools/espidf-sentinel" >&2; exit 1; }
# 产物定位共享单点：target-dir 由仓根 .cargo/config.toml 移出源码区，不得写死
# crate 内 target/（否则编译成功也会被判"ELF 不在树"）。
. "$root/tools/ci/lib/cargo-artifact.sh"
. "$root/tools/ci/lib/cargo-net.sh"
. "$root/tools/ci/lib/skip.sh"

# 前置（工具链/目标/IDF/esp-clang/镜像/venv）走公共实现：四条 esp 门共用一份
# （2026-11-27 批128 收敛；安装方式见 tools/espidf-sentinel/README.md）。
. "$root/tools/ci/lib/esp-env.sh"
rr3_esp_preflight xtensa-esp32-espidf "espidf 哨兵门"

elf="$(rr3_cargo_artifact xtensa-esp32-espidf debug espidf-sentinel)" || {
  echo "FAIL  无法解析 cargo target_directory（metadata 不可用）" >&2
  exit 1
}
if cargo +esp build --locked \
     --config 'source.crates-io.replace-with="rsproxy-sparse"' \
     --config 'source.rsproxy-sparse.registry="sparse+https://rsproxy.cn/index/"'; then
  if [ -f "$elf" ]; then
    echo "PASS  甲面 espidf-sentinel xtensa-esp32-espidf 构建（双证之①编译面）: $elf"
  else
    echo "FAIL  编译退出 0 但 ELF 不在树: $elf" >&2
    fail=1
  fi
else
  echo "FAIL  espidf-sentinel xtensa-esp32-espidf 构建失败" >&2
  fail=1
fi

exit "$fail"
