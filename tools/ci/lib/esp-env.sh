#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# esp 专用门公共环境（机制单点）：工具链/目标/IDF/esp-clang 前置、镜像与 venv 兼容、
# clippy -D warnings 腿。两条 esp 门（classic / S3）只保留各自的构建腿——
# 此前 65 行前置判据是两份复制体（2026-11-27 批123 收敛；复制体的直接后果是
# S3 门漏掉了 classic 已有的 clippy 腿，构建面绿而告警面零覆盖）。
#
# 用法（调用方已 `cd` 到固件工程目录并 source skip.sh / cargo-artifact.sh）：
#   . "$root/tools/ci/lib/esp-env.sh"
#   rr3_esp_preflight <target> <门名> [mcu]
#   rr3_esp_clippy <门名> <日志> [cargo clippy 选项…]
#
# 依赖：rr3_skip（skip.sh）、rr3_esp_clang_lib（cargo-artifact.sh）。

# 前置四查：工具链 / rust-src / 目标三元组 / ldproxy。任一缺位走 rr3_skip
# （本地 SKIP、CI 即红）——缺的是门禁覆盖，不是环境噪音。
rr3_esp_toolchain() {
  local target="$1" gate="$2" sysroot
  if ! rustup toolchain list 2>/dev/null | grep -q '^esp'; then
    rr3_skip "esp 工具链未安装（rustup toolchain list 无 'esp'）——${gate}不执行"
  fi
  sysroot="$(rustc +esp --print sysroot 2>/dev/null)" || {
    rr3_skip "esp 工具链 rustc 不可用（rustc +esp 失败）——${gate}不执行"
  }
  if [ ! -d "${sysroot}/lib/rustlib/src/rust" ]; then
    rr3_skip "esp 工具链缺 rust-src 源码树（build-std 依赖）——${gate}不执行"
  fi
  # 目标由 esp fork rustc 内置 target spec 提供（build-std 从 rust-src 编译，
  # 自定义工具链不会出现 lib/rustlib/<target> 目录，故查 target-list）。
  if ! rustc +esp --print target-list 2>/dev/null | grep -qx "${target}"; then
    rr3_skip "esp 工具链不认 ${target} 目标——${gate}不执行"
  fi
  if ! command -v ldproxy >/dev/null 2>&1; then
    rr3_skip "ldproxy 未安装（cargo install ldproxy）——${gate}不执行"
  fi
}

# IDF 面：本地克隆 + esp-clang（bindgen 需 Xtensa 后端）。
rr3_esp_idf() {
  local gate="$1"
  if [ ! -f "$HOME/.espressif/frameworks/esp-idf-v5.2.2/tools/idf.py" ]; then
    rr3_skip "ESP-IDF v5.2.2 本地克隆缺位——${gate}不执行"
  fi
  export IDF_PATH="$HOME/.espressif/frameworks/esp-idf-v5.2.2"
  if ! rr3_esp_clang_lib >/dev/null; then
    rr3_skip "esp-clang 缺位（~/.espup/esp-clang 无 libclang.dylib/.so）——${gate}不执行"
  fi
}

# 兼容入口：工具链 + IDF 两面都要的门（esp-idf std 档）用它。
rr3_esp_ready() {
  rr3_esp_toolchain "$1" "$2"
  rr3_esp_idf "$2"
}

# 构建环境：镜像注入（仓库源不硬编码镜像）+ pip 超时 + venv dist-info 兼容
# （pip≥24 规范化目录名，py3.9 importlib.metadata 按 dir stem 严格匹配点号名 →
# idf-component-manager 依赖检查误报 not found → CMake Python 检查退 255）。
rr3_esp_env() {
  export IDF_GITHUB_ASSETS=dl.espressif.com/github_assets
  export PIP_INDEX_URL="${PIP_INDEX_URL:-https://mirrors.cloud.tencent.com/pypi/simple}"
  export PIP_DEFAULT_TIMEOUT=60
  local sp d
  sp=".embuild/espressif/python_env/idf5.2_py3.9_env/lib/python3.9/site-packages"
  for d in "${sp}"/ruamel_yaml-*.dist-info; do
    if [ -d "$d" ]; then
      mv "$d" "${d/ruamel_yaml-/ruamel.yaml-}"
      echo "PASS  venv dist-info 兼容改名: ${d##*/} → ruamel.yaml-*"
    fi
  done
}

# 前置总入口：ready → env → 可选芯片档 MCU（覆盖 .cargo/config.toml [env]，
# 外部显式值优先，故只在调用方指定时 export）。
rr3_esp_preflight() {
  local target="$1" gate="$2" mcu="${3:-}"
  rr3_esp_ready "${target}" "${gate}"
  rr3_esp_env
  if [ -n "${mcu}" ]; then
    export MCU="${mcu}"
  fi
}

# clippy 腿（现有机制，不是新门）：esp fork 工具链自带 clippy，以固件最全特性档
# all-targets 跑 -D warnings。测试目标在 panic=abort 下必须带 -Zpanic_abort_tests
# （与单测编译同口径，否则 clippy 直接拒编测试目标）；RUSTFLAGS 会覆盖 config 的
# target rustflags，故整条重写。
rr3_esp_clippy() {
  local gate="$1" log="$2"
  shift 2
  local rf="--cfg espidf_time64 -Zunstable-options -Cpanic=immediate-abort -Zpanic_abort_tests"
  if ! cargo +esp clippy --version >/tmp/rr3-esp-clippy-ver 2>&1; then
    echo "FAIL  esp clippy 不可用（esp 工具链缺 clippy 组件）: $(tail -1 /tmp/rr3-esp-clippy-ver)" >&2
    return 1
  fi
  if RUSTFLAGS="${rf}" cargo +esp clippy --locked --all-targets "$@" \
       --config 'source.crates-io.replace-with="rsproxy-sparse"' \
       --config 'source.rsproxy-sparse.registry="sparse+https://rsproxy.cn/index/"' \
       -- -D warnings >"${log}" 2>&1; then
    echo "PASS  ${gate} clippy -D warnings（all-targets）"
    return 0
  fi
  local errs
  errs=$(grep -A1 -E '^error' "${log}" | head -12 | tr '\n' '|' | cut -c1-900)
  echo "FAIL  ${gate} clippy -D warnings: ${errs}" >&2
  tail -3 "${log}" >&2
  return 1
}
